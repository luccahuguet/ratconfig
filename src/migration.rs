// Test lane: default

use crate::jsonc::{
    PatchError, PatchMutation, PatchOutcome, get_json_path, parse_jsonc_value,
    set_jsonc_value_text, unset_jsonc_value_text,
};
use crate::patch::dotted_paths_overlap;
use serde_json::Value as JsonValue;

pub type ValueTransform = fn(&JsonValue) -> Result<Option<JsonValue>, String>;

#[derive(Debug, Clone)]
pub enum MigrationOp {
    Rename {
        from: String,
        to: String,
    },
    Delete {
        path: String,
    },
    AddDefault {
        path: String,
        value: JsonValue,
    },
    Transform {
        path: String,
        transform: ValueTransform,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationOutcome {
    pub text: String,
    pub mutations: Vec<MigrationMutation>,
}

impl MigrationOutcome {
    pub fn changed(&self) -> bool {
        !self.mutations.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationMutation {
    Renamed { from: String, to: String },
    Deleted { path: String },
    AddedDefault { path: String },
    Transformed { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationError {
    Patch(PatchError),
    DestinationExists { from: String, to: String },
    OverlappingPaths { from: String, to: String },
    TransformFailed { path: String, message: String },
}

impl From<PatchError> for MigrationError {
    fn from(error: PatchError) -> Self {
        Self::Patch(error)
    }
}

pub(crate) struct TextPatchOutcome {
    pub text: String,
    pub mutation: PatchMutation,
}

impl TextPatchOutcome {
    fn changed(&self) -> bool {
        self.mutation != PatchMutation::Unchanged
    }
}

impl From<PatchOutcome> for TextPatchOutcome {
    fn from(outcome: PatchOutcome) -> Self {
        Self {
            text: outcome.text,
            mutation: outcome.mutation,
        }
    }
}

pub(crate) trait MigrationErrorKind {
    fn destination_exists(from: &str, to: &str) -> Self;
    fn overlapping_paths(from: &str, to: &str) -> Self;
    fn transform_failed(path: &str, message: String) -> Self;
}

impl MigrationErrorKind for MigrationError {
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

pub fn apply_migrations_text(
    raw: &str,
    operations: &[MigrationOp],
) -> Result<MigrationOutcome, MigrationError> {
    apply_migrations_text_with(
        raw,
        operations,
        |text| parse_jsonc_value(text).map_err(MigrationError::from),
        get_json_path,
        |text, path, value| {
            set_jsonc_value_text(text, path, value)
                .map(TextPatchOutcome::from)
                .map_err(MigrationError::from)
        },
        |text, path| {
            unset_jsonc_value_text(text, path)
                .map(TextPatchOutcome::from)
                .map_err(MigrationError::from)
        },
    )
}

pub(crate) fn apply_migrations_text_with<Error: MigrationErrorKind>(
    raw: &str,
    operations: &[MigrationOp],
    parse_value: impl Fn(&str) -> Result<JsonValue, Error>,
    get_path: impl for<'a> Fn(&'a JsonValue, &str) -> Option<&'a JsonValue>,
    set_value: impl Fn(&str, &str, &JsonValue) -> Result<TextPatchOutcome, Error>,
    unset_value: impl Fn(&str, &str) -> Result<TextPatchOutcome, Error>,
) -> Result<MigrationOutcome, Error> {
    let mut text = raw.to_string();
    let mut mutations = Vec::new();
    for operation in operations {
        match operation {
            MigrationOp::Rename { from, to } => {
                if dotted_paths_overlap(from, to) {
                    return Err(Error::overlapping_paths(from, to));
                }
                let value_tree = parse_value(&text)?;
                let Some(value) = get_path(&value_tree, from).cloned() else {
                    continue;
                };
                if get_path(&value_tree, to).is_some() {
                    return Err(Error::destination_exists(from, to));
                }
                let set = set_value(&text, to, &value)?;
                let unset = unset_value(&set.text, from)?;
                if set.changed() || unset.changed() {
                    mutations.push(MigrationMutation::Renamed {
                        from: from.clone(),
                        to: to.clone(),
                    });
                }
                text = unset.text;
            }
            MigrationOp::Delete { path } => {
                let outcome = unset_value(&text, path)?;
                if outcome.mutation == PatchMutation::Removed {
                    mutations.push(MigrationMutation::Deleted { path: path.clone() });
                }
                text = outcome.text;
            }
            MigrationOp::AddDefault { path, value } => {
                let value_tree = parse_value(&text)?;
                if get_path(&value_tree, path).is_none() {
                    let outcome = set_value(&text, path, value)?;
                    if outcome.changed() {
                        mutations.push(MigrationMutation::AddedDefault { path: path.clone() });
                    }
                    text = outcome.text;
                }
            }
            MigrationOp::Transform { path, transform } => {
                let value_tree = parse_value(&text)?;
                let Some(value) = get_path(&value_tree, path) else {
                    continue;
                };
                let Some(next) =
                    transform(value).map_err(|message| Error::transform_failed(path, message))?
                else {
                    continue;
                };
                if &next == value {
                    continue;
                }
                let outcome = set_value(&text, path, &next)?;
                if outcome.changed() {
                    mutations.push(MigrationMutation::Transformed { path: path.clone() });
                }
                text = outcome.text;
            }
        }
    }
    Ok(MigrationOutcome { text, mutations })
}

pub fn apply_defaults_text(
    raw: &str,
    defaults: &[(&str, JsonValue)],
) -> Result<MigrationOutcome, MigrationError> {
    apply_migrations_text(raw, &defaults_to_add_default_ops(defaults))
}

pub(crate) fn defaults_to_add_default_ops(defaults: &[(&str, JsonValue)]) -> Vec<MigrationOp> {
    defaults
        .iter()
        .map(|(path, value)| MigrationOp::AddDefault {
            path: (*path).to_string(),
            value: value.clone(),
        })
        .collect()
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

    // Defends: JSONC migration operations run in order and preserve comments while recording semantic mutations.
    #[test]
    fn migrations_rename_delete_add_default_and_transform_jsonc() {
        let raw = r#"{
  // keep me
  "old": { "name": "ferox", "remove": true },
  "ui": { "mode": "full" }
}
"#;
        let outcome = apply_migrations_text(
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
        .expect("migrations");

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
        assert!(outcome.text.contains("// keep me"));
        assert!(!outcome.text.contains(r#""remove""#));
        let value = parse_jsonc_value(&outcome.text).expect("jsonc");
        assert_eq!(get_json_path(&value, "project.name"), Some(&json!("ferox")));
        assert_eq!(get_json_path(&value, "server.enabled"), Some(&json!(true)));
        assert_eq!(get_json_path(&value, "ui.mode"), Some(&json!("compact")));
    }

    // Defends: text-level JSONC default completion reuses add-default semantics without replacing existing values.
    #[test]
    fn defaults_text_inserts_only_missing_jsonc_values() {
        let raw = r#"{
  // keep me
  "core": { "debug": false }
}
"#;
        let defaults = [
            ("core.debug", json!(true)),
            ("open.log_level", json!("info")),
        ];
        let outcome = apply_defaults_text(raw, &defaults).expect("jsonc defaults");

        assert_eq!(
            outcome.mutations,
            vec![MigrationMutation::AddedDefault {
                path: "open.log_level".to_string()
            }]
        );
        assert!(outcome.text.contains("// keep me"));
        let value = parse_jsonc_value(&outcome.text).expect("jsonc");
        assert_eq!(get_json_path(&value, "core.debug"), Some(&json!(false)));
        assert_eq!(
            get_json_path(&value, "open.log_level"),
            Some(&json!("info"))
        );

        let unchanged = apply_defaults_text(&outcome.text, &defaults).expect("unchanged defaults");
        assert!(!unchanged.changed());
        assert_eq!(unchanged.text, outcome.text);
    }

    // Defends: renames fail before overwriting an existing JSONC destination path.
    #[test]
    fn rename_refuses_to_overwrite_destination() {
        let raw = r#"{ "old": "a", "new": "b" }"#;
        let error = apply_migrations_text(
            raw,
            &[MigrationOp::Rename {
                from: "old".to_string(),
                to: "new".to_string(),
            }],
        )
        .expect_err("collision");

        assert_eq!(
            error,
            MigrationError::DestinationExists {
                from: "old".to_string(),
                to: "new".to_string()
            }
        );
    }

    // Defends: parent-to-child renames are blocked before JSONC text can be rewritten destructively.
    #[test]
    fn rename_refuses_overlapping_paths() {
        let raw = r#"{ "old": { "enabled": true } }"#;
        let error = apply_migrations_text(
            raw,
            &[MigrationOp::Rename {
                from: "old".to_string(),
                to: "old.enabled_copy".to_string(),
            }],
        )
        .expect_err("overlap");

        assert_eq!(
            error,
            MigrationError::OverlappingPaths {
                from: "old".to_string(),
                to: "old.enabled_copy".to_string()
            }
        );
    }
}
