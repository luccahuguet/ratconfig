// Test lane: default

use crate::migration::{MigrationMutation, MigrationOp, MigrationOutcome};
use crate::model::toml_value_to_json;
use crate::patch::{
    PatchMutation, PatchOutcome, dotted_paths_overlap, get_dotted_json_path, split_dotted_path,
};
use serde_json::Value as JsonValue;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TomlPatchError {
    InvalidToml { source: String },
    InvalidPath { path: String },
    RewriteRequired { path: String, detail: String },
    UnsupportedValue { path: String, detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TomlMigrationError {
    Patch(TomlPatchError),
    DestinationExists { from: String, to: String },
    OverlappingPaths { from: String, to: String },
    TransformFailed { path: String, message: String },
}

impl From<TomlPatchError> for TomlMigrationError {
    fn from(error: TomlPatchError) -> Self {
        Self::Patch(error)
    }
}

pub fn set_toml_value_text(
    raw: &str,
    path: &str,
    value: &JsonValue,
) -> Result<PatchOutcome, TomlPatchError> {
    let parts = split_path(path)?;
    let replacement = toml_value_from_json(value, path)?;
    let mut document = parse_document(raw)?;
    let parent = parent_table_or_create(document.as_table_mut(), &parts, path)?;
    let leaf = parts.last().expect("split path guarantees a leaf");
    let mutation = if parent.contains_key(leaf) {
        PatchMutation::Replaced
    } else {
        PatchMutation::Inserted
    };
    parent.insert(leaf, Item::Value(replacement));
    let text = document.to_string();
    let mutation = if text == raw {
        PatchMutation::Unchanged
    } else {
        mutation
    };
    validate_toml(&text)?;
    Ok(PatchOutcome { text, mutation })
}

pub fn unset_toml_value_text(raw: &str, path: &str) -> Result<PatchOutcome, TomlPatchError> {
    let parts = split_path(path)?;
    let mut document = parse_document(raw)?;
    let unchanged = || PatchOutcome {
        text: raw.to_string(),
        mutation: PatchMutation::Unchanged,
    };
    let Some(parent) = parent_table_if_present(document.as_table_mut(), &parts, path)? else {
        return Ok(unchanged());
    };
    let leaf = parts.last().expect("split path guarantees a leaf");
    if parent.remove(leaf).is_none() {
        return Ok(unchanged());
    }
    let text = document.to_string();
    validate_toml(&text)?;
    Ok(PatchOutcome {
        text,
        mutation: PatchMutation::Removed,
    })
}

pub fn parse_toml_value(raw: &str) -> Result<JsonValue, TomlPatchError> {
    let table =
        ::toml::from_str::<::toml::Table>(raw).map_err(|source| TomlPatchError::InvalidToml {
            source: source.to_string(),
        })?;
    toml_value_to_json(&::toml::Value::Table(table)).map_err(|source| TomlPatchError::InvalidToml {
        source: source.to_string(),
    })
}

pub fn get_toml_path<'a>(value: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    get_dotted_json_path(value, path)
}

pub fn apply_toml_migrations_text(
    raw: &str,
    operations: &[MigrationOp],
) -> Result<MigrationOutcome, TomlMigrationError> {
    let mut text = raw.to_string();
    let mut mutations = Vec::new();
    for operation in operations {
        match operation {
            MigrationOp::Rename { from, to } => {
                split_path(from)?;
                split_path(to)?;
                if dotted_paths_overlap(from, to) {
                    return Err(TomlMigrationError::OverlappingPaths {
                        from: from.clone(),
                        to: to.clone(),
                    });
                }
                let value_tree = parse_toml_value(&text)?;
                let Some(value) = get_toml_path(&value_tree, from).cloned() else {
                    continue;
                };
                if get_toml_path(&value_tree, to).is_some() {
                    return Err(TomlMigrationError::DestinationExists {
                        from: from.clone(),
                        to: to.clone(),
                    });
                }
                let set = set_toml_value_text(&text, to, &value)?;
                let unset = unset_toml_value_text(&set.text, from)?;
                if set.changed() || unset.changed() {
                    mutations.push(MigrationMutation::Renamed {
                        from: from.clone(),
                        to: to.clone(),
                    });
                }
                text = unset.text;
            }
            MigrationOp::Delete { path } => {
                let outcome = unset_toml_value_text(&text, path)?;
                if outcome.mutation == PatchMutation::Removed {
                    mutations.push(MigrationMutation::Deleted { path: path.clone() });
                }
                text = outcome.text;
            }
            MigrationOp::AddDefault { path, value } => {
                let value_tree = parse_toml_value(&text)?;
                if get_toml_path(&value_tree, path).is_none() {
                    let outcome = set_toml_value_text(&text, path, value)?;
                    if outcome.changed() {
                        mutations.push(MigrationMutation::AddedDefault { path: path.clone() });
                    }
                    text = outcome.text;
                }
            }
            MigrationOp::Transform { path, transform } => {
                split_path(path)?;
                let value_tree = parse_toml_value(&text)?;
                let Some(value) = get_toml_path(&value_tree, path) else {
                    continue;
                };
                let Some(next) =
                    transform(value).map_err(|message| TomlMigrationError::TransformFailed {
                        path: path.clone(),
                        message,
                    })?
                else {
                    continue;
                };
                if &next == value {
                    continue;
                }
                let outcome = set_toml_value_text(&text, path, &next)?;
                if outcome.changed() {
                    mutations.push(MigrationMutation::Transformed { path: path.clone() });
                }
                text = outcome.text;
            }
        }
    }
    Ok(MigrationOutcome { text, mutations })
}

pub fn apply_toml_defaults_text(
    raw: &str,
    defaults: &[(&str, JsonValue)],
) -> Result<MigrationOutcome, TomlMigrationError> {
    let operations = defaults
        .iter()
        .map(|(path, value)| MigrationOp::AddDefault {
            path: (*path).to_string(),
            value: value.clone(),
        })
        .collect::<Vec<_>>();
    apply_toml_migrations_text(raw, &operations)
}

fn parse_document(raw: &str) -> Result<DocumentMut, TomlPatchError> {
    raw.parse::<DocumentMut>()
        .map_err(|source| TomlPatchError::InvalidToml {
            source: source.to_string(),
        })
}

fn validate_toml(raw: &str) -> Result<(), TomlPatchError> {
    parse_toml_value(raw).map(|_| ())
}

fn split_path(path: &str) -> Result<Vec<String>, TomlPatchError> {
    split_dotted_path(path).ok_or_else(|| TomlPatchError::InvalidPath {
        path: path.to_string(),
    })
}

fn parent_table_or_create<'a>(
    mut table: &'a mut Table,
    parts: &[String],
    path: &str,
) -> Result<&'a mut Table, TomlPatchError> {
    for part in &parts[..parts.len().saturating_sub(1)] {
        let item = table.entry(part).or_insert(Item::Table(Table::new()));
        if item.is_none() {
            *item = Item::Table(Table::new());
        }
        table = item.as_table_mut().ok_or_else(|| {
            rewrite_required(
                path,
                "A parent path exists but is not a TOML table, so ratconfig cannot patch through it safely.",
            )
        })?;
    }
    Ok(table)
}

fn parent_table_if_present<'a>(
    mut table: &'a mut Table,
    parts: &[String],
    path: &str,
) -> Result<Option<&'a mut Table>, TomlPatchError> {
    for part in &parts[..parts.len().saturating_sub(1)] {
        let Some(item) = table.get_mut(part) else {
            return Ok(None);
        };
        table = item.as_table_mut().ok_or_else(|| {
            rewrite_required(
                path,
                "A parent path exists but is not a TOML table, so ratconfig cannot remove through it safely.",
            )
        })?;
    }
    Ok(Some(table))
}

fn toml_value_from_json(value: &JsonValue, path: &str) -> Result<Value, TomlPatchError> {
    match value {
        JsonValue::Null => Err(unsupported_value(path, "TOML has no null value.")),
        JsonValue::Bool(value) => Ok(Value::from(*value)),
        JsonValue::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(Value::from(value))
            } else if let Some(value) = value.as_u64() {
                let value = i64::try_from(value).map_err(|_| {
                    unsupported_value(path, "TOML integers must fit into signed 64-bit values.")
                })?;
                Ok(Value::from(value))
            } else if let Some(value) = value.as_f64() {
                Ok(Value::from(value))
            } else {
                Err(unsupported_value(
                    path,
                    "The JSON number cannot be represented as a TOML number.",
                ))
            }
        }
        JsonValue::String(value) => Ok(Value::from(value.clone())),
        JsonValue::Array(values) => {
            let mut array = Array::new();
            for value in values {
                array.push(toml_value_from_json(value, path)?);
            }
            array.fmt();
            Ok(Value::Array(array))
        }
        JsonValue::Object(object) => {
            let mut table = InlineTable::new();
            for (key, value) in object {
                table.insert(key.clone(), toml_value_from_json(value, path)?);
            }
            table.fmt();
            Ok(Value::InlineTable(table))
        }
    }
}

fn unsupported_value(path: &str, detail: &str) -> TomlPatchError {
    TomlPatchError::UnsupportedValue {
        path: path.to_string(),
        detail: detail.to_string(),
    }
}

fn rewrite_required(path: &str, detail: &str) -> TomlPatchError {
    TomlPatchError::RewriteRequired {
        path: path.to_string(),
        detail: detail.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn full_to_compact(value: &JsonValue) -> Result<Option<JsonValue>, String> {
        match value.as_str() {
            Some("full") => Ok(Some(json!("compact"))),
            Some(_) => Ok(None),
            None => Err("expected a string".to_string()),
        }
    }

    fn reject_transform(_value: &JsonValue) -> Result<Option<JsonValue>, String> {
        Err("unsupported value".to_string())
    }

    // Defends: TOML edits preserve surrounding comments while materializing structured values.
    #[test]
    fn set_toml_value_supports_nested_arrays_and_inline_tables() {
        let raw = r#"# keep root comment

[zellij]
"#;

        let outcome = set_toml_value_text(
            raw,
            "zellij.custom_popups",
            &json!([
                {
                    "id": "btm",
                    "command": ["btm", "--basic"],
                    "keep_alive": true,
                    "geometry": { "width": 80, "height": 24 }
                }
            ]),
        )
        .expect("structured toml patch");
        let value = parse_toml_value(&outcome.text).expect("toml");

        assert_eq!(outcome.mutation, PatchMutation::Inserted);
        assert!(outcome.text.contains("# keep root comment"));
        assert_eq!(
            get_toml_path(&value, "zellij.custom_popups"),
            Some(&json!([
                {
                    "id": "btm",
                    "command": ["btm", "--basic"],
                    "keep_alive": true,
                    "geometry": { "width": 80, "height": 24 }
                }
            ]))
        );
    }

    // Defends: native TOML bare keys with hyphens remain patchable through ratconfig dotted paths.
    #[test]
    fn set_toml_value_supports_hyphenated_bare_keys() {
        let raw = r#"[editor]
line-number = "relative"
"#;

        let outcome = set_toml_value_text(raw, "editor.line-number", &json!("absolute"))
            .expect("hyphenated bare key patch");
        let value = parse_toml_value(&outcome.text).expect("toml");

        assert_eq!(outcome.mutation, PatchMutation::Replaced);
        assert_eq!(
            get_toml_path(&value, "editor.line-number"),
            Some(&json!("absolute"))
        );
    }

    // Defends: reads use the same dotted-path normalization as writes.
    #[test]
    fn toml_path_reads_match_patch_normalization() {
        let outcome =
            set_toml_value_text("", " ui . theme ", &json!("dark")).expect("normalized TOML path");
        let value = parse_toml_value(&outcome.text).expect("toml");

        assert_eq!(get_toml_path(&value, " ui . theme "), Some(&json!("dark")));
        assert_eq!(
            get_toml_path(
                &json!({ " ui ": { " theme ": "exact" }, "ui": { "theme": "normalized" } }),
                " ui . theme "
            ),
            Some(&json!("exact"))
        );
    }

    // Defends: TOML migration operations preserve the safe-change contract.
    #[test]
    fn toml_migrations_rename_delete_add_default_and_transform() {
        let raw = r#"# keep me

[old]
name = "ferox"
remove = true

[ui]
mode = "full"
"#;
        let outcome = apply_toml_migrations_text(
            raw,
            &[
                MigrationOp::Rename {
                    from: "old.name".to_string(),
                    to: "project.name".to_string(),
                },
                MigrationOp::Delete {
                    path: "old.remove".to_string(),
                },
                MigrationOp::AddDefault {
                    path: "server.enabled".to_string(),
                    value: json!(true),
                },
                MigrationOp::Transform {
                    path: "ui.mode".to_string(),
                    transform: full_to_compact,
                },
            ],
        )
        .expect("toml migrations");

        assert_eq!(
            outcome.mutations,
            vec![
                MigrationMutation::Renamed {
                    from: "old.name".to_string(),
                    to: "project.name".to_string()
                },
                MigrationMutation::Deleted {
                    path: "old.remove".to_string()
                },
                MigrationMutation::AddedDefault {
                    path: "server.enabled".to_string()
                },
                MigrationMutation::Transformed {
                    path: "ui.mode".to_string()
                },
            ]
        );
        assert!(outcome.text.contains("# keep me"));
        assert!(!outcome.text.contains("remove = true"));
        let value = parse_toml_value(&outcome.text).expect("toml");
        assert_eq!(get_toml_path(&value, "project.name"), Some(&json!("ferox")));
        assert_eq!(get_toml_path(&value, "server.enabled"), Some(&json!(true)));
        assert_eq!(get_toml_path(&value, "ui.mode"), Some(&json!("compact")));
    }

    // Defends: text-level TOML default completion preserves comments and existing host values.
    #[test]
    fn toml_defaults_text_inserts_only_missing_values() {
        let raw = r#"# keep me

[open]
log_level = "info"
"#;
        let defaults = [
            ("open.log_level", json!("debug")),
            ("core.enabled", json!(true)),
        ];
        let outcome = apply_toml_defaults_text(raw, &defaults).expect("toml defaults");

        assert_eq!(
            outcome.mutations,
            vec![MigrationMutation::AddedDefault {
                path: "core.enabled".to_string()
            }]
        );
        assert!(outcome.text.contains("# keep me"));
        let value = parse_toml_value(&outcome.text).expect("toml");
        assert_eq!(
            get_toml_path(&value, "open.log_level"),
            Some(&json!("info"))
        );
        assert_eq!(get_toml_path(&value, "core.enabled"), Some(&json!(true)));

        let unchanged =
            apply_toml_defaults_text(&outcome.text, &defaults).expect("unchanged defaults");
        assert!(!unchanged.changed());
        assert_eq!(unchanged.text, outcome.text);
    }

    // Defends: TOML adapter refuses null instead of inventing a lossy representation.
    #[test]
    fn toml_patch_rejects_json_null() {
        let error = set_toml_value_text("", "core.value", &JsonValue::Null).expect_err("null");

        assert_eq!(
            error,
            TomlPatchError::UnsupportedValue {
                path: "core.value".to_string(),
                detail: "TOML has no null value.".to_string(),
            }
        );
    }

    // Defends: parent-to-child renames are blocked before TOML text can be rewritten destructively.
    #[test]
    fn toml_rename_refuses_overlapping_paths() {
        let raw = r#"[old]
enabled = true
"#;
        let error = apply_toml_migrations_text(
            raw,
            &[MigrationOp::Rename {
                from: "old".to_string(),
                to: "old.enabled_copy".to_string(),
            }],
        )
        .expect_err("overlap");

        assert_eq!(
            error,
            TomlMigrationError::OverlappingPaths {
                from: "old".to_string(),
                to: "old.enabled_copy".to_string()
            }
        );
    }

    // Defends: migration errors preserve destination and transform context.
    #[test]
    fn toml_migrations_preserve_public_error_boundaries() {
        let raw = "old = \"a\"\nnew = \"b\"\n";
        assert!(matches!(
            apply_toml_migrations_text(
                raw,
                &[MigrationOp::Rename {
                    from: "old".to_string(),
                    to: "new".to_string(),
                }]
            ),
            Err(TomlMigrationError::DestinationExists { from, to })
                if from == "old" && to == "new"
        ));
        assert!(matches!(
            apply_toml_migrations_text(
                raw,
                &[MigrationOp::Transform {
                    path: "old".to_string(),
                    transform: reject_transform,
                }]
            ),
            Err(TomlMigrationError::TransformFailed { path, message })
                if path == "old" && message == "unsupported value"
        ));
    }

    // Defends: malformed migration paths fail even when no source value exists.
    #[test]
    fn toml_migrations_reject_invalid_absent_paths() {
        let operations = [
            MigrationOp::Rename {
                from: "missing".to_string(),
                to: "bad key".to_string(),
            },
            MigrationOp::Rename {
                from: "bad key".to_string(),
                to: "missing".to_string(),
            },
            MigrationOp::Transform {
                path: "bad key".to_string(),
                transform: reject_transform,
            },
        ];

        for operation in operations {
            assert!(matches!(
                apply_toml_migrations_text("", &[operation]),
                Err(TomlMigrationError::Patch(TomlPatchError::InvalidPath { path }))
                    if path == "bad key"
            ));
        }
    }
}
