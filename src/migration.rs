// Test lane: default

use crate::patch::{PatchMutation, PatchOutcome, dotted_paths_overlap};
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

pub(crate) trait MigrationErrorKind {
    fn destination_exists(from: &str, to: &str) -> Self;
    fn overlapping_paths(from: &str, to: &str) -> Self;
    fn transform_failed(path: &str, message: String) -> Self;
}

pub(crate) fn apply_migrations_text_with<Error: MigrationErrorKind>(
    raw: &str,
    operations: &[MigrationOp],
    parse_value: impl Fn(&str) -> Result<JsonValue, Error>,
    get_path: impl for<'a> Fn(&'a JsonValue, &str) -> Option<&'a JsonValue>,
    set_value: impl Fn(&str, &str, &JsonValue) -> Result<PatchOutcome, Error>,
    unset_value: impl Fn(&str, &str) -> Result<PatchOutcome, Error>,
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

pub(crate) fn defaults_to_add_default_ops(defaults: &[(&str, JsonValue)]) -> Vec<MigrationOp> {
    defaults
        .iter()
        .map(|(path, value)| MigrationOp::AddDefault {
            path: (*path).to_string(),
            value: value.clone(),
        })
        .collect()
}
