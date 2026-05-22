use crate::jsonc::{
    PatchError, PatchMutation, get_json_path, parse_jsonc_value, set_jsonc_value_text,
    unset_jsonc_value_text,
};
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
    TransformFailed { path: String, message: String },
}

impl From<PatchError> for MigrationError {
    fn from(error: PatchError) -> Self {
        Self::Patch(error)
    }
}

pub fn apply_migrations_text(
    raw: &str,
    operations: &[MigrationOp],
) -> Result<MigrationOutcome, MigrationError> {
    let mut text = raw.to_string();
    let mut mutations = Vec::new();
    for operation in operations {
        match operation {
            MigrationOp::Rename { from, to } => {
                text = rename_path(&text, from, to, &mut mutations)?;
            }
            MigrationOp::Delete { path } => {
                let outcome = unset_jsonc_value_text(&text, path)?;
                if outcome.mutation == PatchMutation::Removed {
                    mutations.push(MigrationMutation::Deleted { path: path.clone() });
                }
                text = outcome.text;
            }
            MigrationOp::AddDefault { path, value } => {
                let value_tree = parse_jsonc_value(&text)?;
                if get_json_path(&value_tree, path).is_none() {
                    let outcome = set_jsonc_value_text(&text, path, value)?;
                    if outcome.changed() {
                        mutations.push(MigrationMutation::AddedDefault { path: path.clone() });
                    }
                    text = outcome.text;
                }
            }
            MigrationOp::Transform { path, transform } => {
                text = transform_path(&text, path, *transform, &mut mutations)?;
            }
        }
    }
    Ok(MigrationOutcome { text, mutations })
}

fn rename_path(
    text: &str,
    from: &str,
    to: &str,
    mutations: &mut Vec<MigrationMutation>,
) -> Result<String, MigrationError> {
    let value_tree = parse_jsonc_value(text)?;
    let Some(value) = get_json_path(&value_tree, from).cloned() else {
        return Ok(text.to_string());
    };
    if get_json_path(&value_tree, to).is_some() {
        return Err(MigrationError::DestinationExists {
            from: from.to_string(),
            to: to.to_string(),
        });
    }
    let set = set_jsonc_value_text(text, to, &value)?;
    let unset = unset_jsonc_value_text(&set.text, from)?;
    if set.changed() || unset.changed() {
        mutations.push(MigrationMutation::Renamed {
            from: from.to_string(),
            to: to.to_string(),
        });
    }
    Ok(unset.text)
}

fn transform_path(
    text: &str,
    path: &str,
    transform: ValueTransform,
    mutations: &mut Vec<MigrationMutation>,
) -> Result<String, MigrationError> {
    let value_tree = parse_jsonc_value(text)?;
    let Some(value) = get_json_path(&value_tree, path) else {
        return Ok(text.to_string());
    };
    let Some(next) = transform(value).map_err(|message| MigrationError::TransformFailed {
        path: path.to_string(),
        message,
    })?
    else {
        return Ok(text.to_string());
    };
    if &next == value {
        return Ok(text.to_string());
    }
    let outcome = set_jsonc_value_text(text, path, &next)?;
    if outcome.changed() {
        mutations.push(MigrationMutation::Transformed {
            path: path.to_string(),
        });
    }
    Ok(outcome.text)
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
}
