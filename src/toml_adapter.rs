// Test lane: default

use crate::migration::{
    MigrationErrorKind, MigrationMutation, MigrationOp, MigrationOutcome, TextPatchOutcome,
    apply_migrations_text_with, defaults_to_add_default_ops,
};
use crate::model::toml_value_to_json;
use crate::patch::{PatchMutation, get_dotted_json_path, split_dotted_path};
use serde_json::Value as JsonValue;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TomlPatchOutcome {
    pub text: String,
    pub mutation: PatchMutation,
}

impl TomlPatchOutcome {
    pub fn changed(&self) -> bool {
        self.mutation != PatchMutation::Unchanged
    }
}

impl From<TomlPatchOutcome> for TextPatchOutcome {
    fn from(outcome: TomlPatchOutcome) -> Self {
        Self {
            text: outcome.text,
            mutation: outcome.mutation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TomlPatchError {
    InvalidToml { source: String },
    InvalidPath { path: String },
    RewriteRequired { path: String, detail: String },
    UnsupportedValue { path: String, detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TomlMigrationOutcome {
    pub text: String,
    pub mutations: Vec<MigrationMutation>,
}

impl TomlMigrationOutcome {
    pub fn changed(&self) -> bool {
        !self.mutations.is_empty()
    }
}

impl From<MigrationOutcome> for TomlMigrationOutcome {
    fn from(outcome: MigrationOutcome) -> Self {
        Self {
            text: outcome.text,
            mutations: outcome.mutations,
        }
    }
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

impl MigrationErrorKind for TomlMigrationError {
    fn destination_exists(from: &str, to: &str) -> Self {
        Self::DestinationExists {
            from: from.into(),
            to: to.into(),
        }
    }

    fn overlapping_paths(from: &str, to: &str) -> Self {
        Self::OverlappingPaths {
            from: from.into(),
            to: to.into(),
        }
    }

    fn transform_failed(path: &str, message: String) -> Self {
        Self::TransformFailed {
            path: path.into(),
            message,
        }
    }
}

pub fn set_toml_value_text(
    raw: &str,
    path: &str,
    value: &JsonValue,
) -> Result<TomlPatchOutcome, TomlPatchError> {
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
    Ok(TomlPatchOutcome { text, mutation })
}

pub fn unset_toml_value_text(raw: &str, path: &str) -> Result<TomlPatchOutcome, TomlPatchError> {
    let parts = split_path(path)?;
    let mut document = parse_document(raw)?;
    let Some(parent) = parent_table_if_present(document.as_table_mut(), &parts, path)? else {
        return Ok(TomlPatchOutcome {
            text: raw.to_string(),
            mutation: PatchMutation::Unchanged,
        });
    };
    let leaf = parts.last().expect("split path guarantees a leaf");
    if parent.remove(leaf).is_none() {
        return Ok(TomlPatchOutcome {
            text: raw.to_string(),
            mutation: PatchMutation::Unchanged,
        });
    }
    let text = document.to_string();
    validate_toml(&text)?;
    Ok(TomlPatchOutcome {
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
) -> Result<TomlMigrationOutcome, TomlMigrationError> {
    apply_migrations_text_with(
        raw,
        operations,
        |text| parse_toml_value(text).map_err(TomlMigrationError::from),
        get_toml_path,
        |text, path, value| {
            set_toml_value_text(text, path, value)
                .map(TextPatchOutcome::from)
                .map_err(TomlMigrationError::from)
        },
        |text, path| {
            unset_toml_value_text(text, path)
                .map(TextPatchOutcome::from)
                .map_err(TomlMigrationError::from)
        },
    )
    .map(TomlMigrationOutcome::from)
}

pub fn apply_toml_defaults_text(
    raw: &str,
    defaults: &[(&str, JsonValue)],
) -> Result<TomlMigrationOutcome, TomlMigrationError> {
    apply_toml_migrations_text(raw, &defaults_to_add_default_ops(defaults))
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
    table: &'a mut Table,
    parts: &[String],
    path: &str,
) -> Result<&'a mut Table, TomlPatchError> {
    if parts.len() <= 1 {
        return Ok(table);
    }
    let part = &parts[0];
    let item = table.entry(part).or_insert(Item::Table(Table::new()));
    if item.is_none() {
        *item = Item::Table(Table::new());
    }
    let child = item.as_table_mut().ok_or_else(|| {
        rewrite_required(
            path,
            "A parent path exists but is not a TOML table, so ratconfig cannot patch through it safely.",
        )
    })?;
    parent_table_or_create(child, &parts[1..], path)
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

    // Defends: TOML migration operations match the JSONC migration contract for safe changes.
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
}
