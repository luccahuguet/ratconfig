// Test lane: default

//! Deterministic config contract reconciliation.
//!
//! The low-level migration modules own individual text edits. This module
//! layers a linear, versioned contract over those edits so host applications can
//! record that a config has joined a contract and then reconcile future contract
//! changes automatically when every step is safe.

pub mod jsonc;
pub mod toml;

pub use jsonc::*;
pub use toml::*;

use crate::jsonc::PatchError;
use crate::migration::{MigrationError, MigrationMutation, MigrationOp};
use crate::patch::PatchMutation;
use crate::toml_adapter::{TomlMigrationError, TomlPatchError};
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use std::collections::BTreeSet;

pub const CONTRACT_STATE_SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Clone)]
pub struct ConfigContract {
    pub id: String,
    pub baseline_version: u64,
    pub current_version: u64,
    pub changes: Vec<ContractChange>,
}

#[derive(Debug, Clone)]
pub struct ContractChange {
    pub id: String,
    pub from_version: u64,
    pub to_version: u64,
    pub operations: Vec<MigrationOp>,
    pub manual_steps: Vec<ManualMigrationStep>,
}

impl ContractChange {
    pub fn automatic(
        id: impl Into<String>,
        from_version: u64,
        to_version: u64,
        operations: Vec<MigrationOp>,
    ) -> Self {
        Self {
            id: id.into(),
            from_version,
            to_version,
            operations,
            manual_steps: Vec::new(),
        }
    }

    pub fn manual(
        id: impl Into<String>,
        from_version: u64,
        to_version: u64,
        manual_steps: Vec<ManualMigrationStep>,
    ) -> Self {
        Self {
            id: id.into(),
            from_version,
            to_version,
            operations: Vec::new(),
            manual_steps,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualMigrationStep {
    pub id: String,
    pub path: String,
    pub reason: String,
    pub remediation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractState {
    pub contract_id: String,
    pub version: u64,
    pub applied_change_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractPlan {
    pub from_version: u64,
    pub to_version: u64,
    pub changes: Vec<ContractPlanChange>,
    pub manual_steps: Vec<ManualMigrationStep>,
}

impl ContractPlan {
    pub fn already_current(&self) -> bool {
        self.from_version == self.to_version && self.changes.is_empty()
    }

    pub fn requires_manual_action(&self) -> bool {
        !self.manual_steps.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractPlanChange {
    pub id: String,
    pub from_version: u64,
    pub to_version: u64,
    pub automatic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedContractChange {
    pub id: String,
    pub from_version: u64,
    pub to_version: u64,
    pub mutations: Vec<MigrationMutation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractApplyOutcome {
    pub text: String,
    pub from_version: u64,
    pub to_version: u64,
    pub applied_changes: Vec<AppliedContractChange>,
}

impl ContractApplyOutcome {
    pub fn changed(&self) -> bool {
        self.applied_changes
            .iter()
            .any(|change| !change.mutations.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractJoinOutcome {
    pub text: String,
    pub state: ContractState,
    pub applied_changes: Vec<AppliedContractChange>,
    pub state_mutation: PatchMutation,
}

impl ContractJoinOutcome {
    pub fn changed(&self) -> bool {
        self.state_mutation != PatchMutation::Unchanged
            || self
                .applied_changes
                .iter()
                .any(|change| !change.mutations.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    InvalidContract {
        detail: String,
    },
    InvalidState {
        state_path: String,
        detail: String,
    },
    NotJoined {
        state_path: String,
    },
    ContractMismatch {
        expected: String,
        found: String,
    },
    UnsupportedStateVersion {
        version: u64,
        baseline_version: u64,
        current_version: u64,
    },
    MissingMigration {
        from_version: u64,
        target_version: u64,
    },
    ManualRequired {
        plan: ContractPlan,
    },
    JsoncMigration {
        change_id: String,
        error: MigrationError,
    },
    JsoncPatch(PatchError),
    TomlMigration {
        change_id: String,
        error: TomlMigrationError,
    },
    TomlPatch(TomlPatchError),
}

pub fn plan_contract_migration(
    contract: &ConfigContract,
    from_version: u64,
) -> Result<ContractPlan, ContractError> {
    let changes = planned_change_refs(contract, from_version)?;
    Ok(plan_from_changes(
        from_version,
        contract.current_version,
        &changes,
    ))
}

fn apply_contract_with(
    raw: &str,
    contract: &ConfigContract,
    from_version: u64,
    mut apply_change: impl FnMut(
        &str,
        &ContractChange,
    ) -> Result<(String, Vec<MigrationMutation>), ContractError>,
) -> Result<ContractApplyOutcome, ContractError> {
    let changes = planned_change_refs(contract, from_version)?;
    let plan = plan_from_changes(from_version, contract.current_version, &changes);
    if plan.requires_manual_action() {
        return Err(ContractError::ManualRequired { plan });
    }

    let mut text = raw.to_string();
    let mut applied_changes = Vec::new();
    for change in changes {
        let (next_text, mutations) = apply_change(&text, change)?;
        text = next_text;
        applied_changes.push(AppliedContractChange {
            id: change.id.clone(),
            from_version: change.from_version,
            to_version: change.to_version,
            mutations,
        });
    }

    Ok(ContractApplyOutcome {
        text,
        from_version,
        to_version: contract.current_version,
        applied_changes,
    })
}

pub fn read_contract_state(
    value: &JsonValue,
    state_path: &str,
) -> Result<Option<ContractState>, ContractError> {
    read_contract_state_from_json(value, state_path)
}

pub fn read_contract_state_from_json(
    value: &JsonValue,
    state_path: &str,
) -> Result<Option<ContractState>, ContractError> {
    read_contract_state_from_value(value, state_path, jsonc::json_path)
}

fn read_contract_state_from_value(
    value: &JsonValue,
    state_path: &str,
    get_path: impl for<'a> Fn(&'a JsonValue, &str) -> Option<&'a JsonValue>,
) -> Result<Option<ContractState>, ContractError> {
    let Some(state_value) = get_path(value, state_path) else {
        return Ok(None);
    };
    let Some(state) = state_value.as_object() else {
        return Err(invalid_state(
            state_path,
            "contract state must be a JSON object",
        ));
    };
    let schema_version = required_u64(state, state_path, "schema_version")?;
    if schema_version != CONTRACT_STATE_SCHEMA_VERSION {
        return Err(invalid_state(
            state_path,
            format!(
                "unsupported contract state schema_version {schema_version}; supported version is {CONTRACT_STATE_SCHEMA_VERSION}"
            ),
        ));
    }
    Ok(Some(ContractState {
        contract_id: required_string(state, state_path, "contract_id")?,
        version: required_u64(state, state_path, "version")?,
        applied_change_ids: optional_string_array(state, state_path, "applied_change_ids")?,
    }))
}

fn planned_change_refs(
    contract: &ConfigContract,
    from_version: u64,
) -> Result<Vec<&ContractChange>, ContractError> {
    validate_contract(contract)?;
    if from_version < contract.baseline_version || from_version > contract.current_version {
        return Err(ContractError::UnsupportedStateVersion {
            version: from_version,
            baseline_version: contract.baseline_version,
            current_version: contract.current_version,
        });
    }

    let ordered = ordered_changes(contract);
    let mut version = from_version;
    let mut planned = Vec::new();
    while version < contract.current_version {
        let Some(change) = ordered
            .iter()
            .copied()
            .find(|change| change.from_version == version)
        else {
            return Err(ContractError::MissingMigration {
                from_version: version,
                target_version: contract.current_version,
            });
        };
        version = change.to_version;
        planned.push(change);
    }
    Ok(planned)
}

fn plan_from_changes(
    from_version: u64,
    to_version: u64,
    changes: &[&ContractChange],
) -> ContractPlan {
    let mut manual_steps = Vec::new();
    let changes = changes
        .iter()
        .map(|change| {
            manual_steps.extend(change.manual_steps.clone());
            ContractPlanChange {
                id: change.id.clone(),
                from_version: change.from_version,
                to_version: change.to_version,
                automatic: change.manual_steps.is_empty(),
            }
        })
        .collect();
    ContractPlan {
        from_version,
        to_version,
        changes,
        manual_steps,
    }
}

fn validate_contract(contract: &ConfigContract) -> Result<(), ContractError> {
    if contract.id.trim().is_empty() {
        return Err(invalid_contract("contract id must not be empty"));
    }
    if contract.baseline_version > contract.current_version {
        return Err(invalid_contract(
            "contract baseline_version must not be greater than current_version",
        ));
    }

    let mut seen_ids = BTreeSet::new();
    let mut cursor = contract.baseline_version;
    for change in ordered_changes(contract) {
        if change.id.trim().is_empty() {
            return Err(invalid_contract("contract change id must not be empty"));
        }
        if !seen_ids.insert(change.id.as_str()) {
            return Err(invalid_contract(format!(
                "contract change id {} is duplicated",
                change.id
            )));
        }
        if change.from_version != cursor {
            return Err(invalid_contract(format!(
                "contract changes must form one linear chain; expected a change from version {cursor}, found {} -> {}",
                change.from_version, change.to_version
            )));
        }
        if change.to_version <= change.from_version {
            return Err(invalid_contract(format!(
                "contract change {} must advance to a greater version",
                change.id
            )));
        }
        if !change.operations.is_empty() && !change.manual_steps.is_empty() {
            return Err(invalid_contract(format!(
                "contract change {} cannot mix automatic operations with manual steps",
                change.id
            )));
        }
        cursor = change.to_version;
    }
    if cursor != contract.current_version {
        return Err(invalid_contract(format!(
            "contract changes stop at version {cursor}, but current_version is {}",
            contract.current_version
        )));
    }
    Ok(())
}

fn ordered_changes(contract: &ConfigContract) -> Vec<&ContractChange> {
    let mut changes = contract.changes.iter().collect::<Vec<_>>();
    changes.sort_by_key(|change| (change.from_version, change.to_version, change.id.as_str()));
    changes
}

fn invalid_contract(detail: impl Into<String>) -> ContractError {
    ContractError::InvalidContract {
        detail: detail.into(),
    }
}

fn invalid_state(state_path: &str, detail: impl Into<String>) -> ContractError {
    ContractError::InvalidState {
        state_path: state_path.to_string(),
        detail: detail.into(),
    }
}

fn required_string(
    object: &JsonMap<String, JsonValue>,
    state_path: &str,
    key: &str,
) -> Result<String, ContractError> {
    object
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid_state(state_path, format!("{key} must be a non-empty string")))
}

fn required_u64(
    object: &JsonMap<String, JsonValue>,
    state_path: &str,
    key: &str,
) -> Result<u64, ContractError> {
    object
        .get(key)
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| invalid_state(state_path, format!("{key} must be an unsigned integer")))
}

fn optional_string_array(
    object: &JsonMap<String, JsonValue>,
    state_path: &str,
    key: &str,
) -> Result<Vec<String>, ContractError> {
    let Some(value) = object.get(key) else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err(invalid_state(
            state_path,
            format!("{key} must be an array of strings"),
        ));
    };
    let mut strings = Vec::new();
    for item in items {
        let Some(value) = item.as_str() else {
            return Err(invalid_state(
                state_path,
                format!("{key} must be an array of strings"),
            ));
        };
        strings.push(value.to_string());
    }
    Ok(strings)
}

fn contract_state_to_json(state: &ContractState) -> JsonValue {
    let mut object = JsonMap::new();
    object.insert(
        "schema_version".to_string(),
        JsonValue::Number(JsonNumber::from(CONTRACT_STATE_SCHEMA_VERSION)),
    );
    object.insert(
        "contract_id".to_string(),
        JsonValue::String(state.contract_id.clone()),
    );
    object.insert(
        "version".to_string(),
        JsonValue::Number(JsonNumber::from(state.version)),
    );
    object.insert(
        "applied_change_ids".to_string(),
        JsonValue::Array(
            state
                .applied_change_ids
                .iter()
                .cloned()
                .map(JsonValue::String)
                .collect(),
        ),
    );
    JsonValue::Object(object)
}

fn new_joined_state(
    contract: &ConfigContract,
    applied_changes: &[AppliedContractChange],
) -> ContractState {
    ContractState {
        contract_id: contract.id.clone(),
        version: contract.current_version,
        applied_change_ids: applied_changes
            .iter()
            .map(|change| change.id.clone())
            .collect(),
    }
}

fn append_applied_change_ids(state: &mut ContractState, changes: &[AppliedContractChange]) {
    for change in changes {
        if !state.applied_change_ids.contains(&change.id) {
            state.applied_change_ids.push(change.id.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonc::{get_json_path, parse_jsonc_value};
    use serde_json::json;

    fn full_to_compact(value: &JsonValue) -> Result<Option<JsonValue>, String> {
        match value.as_str() {
            Some("full") => Ok(Some(json!("compact"))),
            Some(_) => Ok(None),
            None => Err("expected a string".to_string()),
        }
    }

    fn contract_with_changes(changes: Vec<ContractChange>, current_version: u64) -> ConfigContract {
        ConfigContract {
            id: "demo".to_string(),
            baseline_version: 1,
            current_version,
            changes,
        }
    }

    // Defends: a contract is a linear versioned history whose automatic changes can be replayed deterministically.
    #[test]
    fn applies_linear_automatic_contract_changes() {
        let raw = r#"{
  // keep me
  "old": { "name": "ferox", "remove": true },
  "ui": { "mode": "full" }
}
"#;
        let contract = contract_with_changes(
            vec![
                ContractChange::automatic(
                    "move-name",
                    1,
                    2,
                    vec![
                        MigrationOp::Rename {
                            from: "old.name".to_string(),
                            to: "project.name".to_string(),
                        },
                        MigrationOp::Delete {
                            path: "old.remove".to_string(),
                        },
                    ],
                ),
                ContractChange::automatic(
                    "compact-mode",
                    2,
                    3,
                    vec![MigrationOp::Transform {
                        path: "ui.mode".to_string(),
                        transform: full_to_compact,
                    }],
                ),
            ],
            3,
        );

        let outcome = apply_jsonc_contract_text(raw, &contract, 1).expect("contract apply");

        assert_eq!(outcome.from_version, 1);
        assert_eq!(outcome.to_version, 3);
        assert_eq!(
            outcome
                .applied_changes
                .iter()
                .map(|change| change.id.as_str())
                .collect::<Vec<_>>(),
            vec!["move-name", "compact-mode"]
        );
        assert!(outcome.text.contains("// keep me"));
        assert!(!outcome.text.contains(r#""remove""#));
        let value = parse_jsonc_value(&outcome.text).expect("jsonc");
        assert_eq!(get_json_path(&value, "project.name"), Some(&json!("ferox")));
        assert_eq!(get_json_path(&value, "ui.mode"), Some(&json!("compact")));
    }

    // Defends: impossible migrations block the whole plan with actionable manual steps instead of partly rewriting config.
    #[test]
    fn manual_contract_change_blocks_automatic_application() {
        let contract = contract_with_changes(
            vec![ContractChange::manual(
                "split-theme",
                1,
                2,
                vec![ManualMigrationStep {
                    id: "choose-palette".to_string(),
                    path: "theme.palette".to_string(),
                    reason: "The old theme field can map to several new palettes.".to_string(),
                    remediation: "Pick one palette and set theme.palette explicitly.".to_string(),
                }],
            )],
            2,
        );

        let error = apply_jsonc_contract_text(r#"{ "theme": "dark" }"#, &contract, 1)
            .expect_err("manual migration required");

        assert_eq!(
            error,
            ContractError::ManualRequired {
                plan: ContractPlan {
                    from_version: 1,
                    to_version: 2,
                    changes: vec![ContractPlanChange {
                        id: "split-theme".to_string(),
                        from_version: 1,
                        to_version: 2,
                        automatic: false,
                    }],
                    manual_steps: vec![ManualMigrationStep {
                        id: "choose-palette".to_string(),
                        path: "theme.palette".to_string(),
                        reason: "The old theme field can map to several new palettes.".to_string(),
                        remediation: "Pick one palette and set theme.palette explicitly."
                            .to_string(),
                    }],
                },
            }
        );
    }

    // Defends: a joined config records contract state so future contract versions can be reconciled automatically.
    #[test]
    fn joined_contract_reconciles_future_versions_and_updates_state() {
        let raw = r#"{ "core": {} }"#;
        let v1 = contract_with_changes(Vec::new(), 1);
        let joined = join_jsonc_contract_text(raw, &v1, "ratconfig.contract").expect("join");

        assert_eq!(
            joined.state,
            ContractState {
                contract_id: "demo".to_string(),
                version: 1,
                applied_change_ids: Vec::new(),
            }
        );

        let v2 = contract_with_changes(
            vec![ContractChange::automatic(
                "add-debug",
                1,
                2,
                vec![MigrationOp::AddDefault {
                    path: "core.debug".to_string(),
                    value: json!(true),
                }],
            )],
            2,
        );
        let reconciled =
            reconcile_joined_jsonc_contract_text(&joined.text, &v2, "ratconfig.contract")
                .expect("reconcile");
        let value = parse_jsonc_value(&reconciled.text).expect("jsonc");

        assert_eq!(get_json_path(&value, "core.debug"), Some(&json!(true)));
        assert_eq!(
            read_contract_state(&value, "ratconfig.contract").expect("state"),
            Some(ContractState {
                contract_id: "demo".to_string(),
                version: 2,
                applied_change_ids: vec!["add-debug".to_string()],
            })
        );
    }

    // Defends: adopting a known older config can migrate first and then record the joined state in one returned text.
    #[test]
    fn join_from_known_version_applies_changes_before_recording_state() {
        let raw = r#"{ "legacy": { "shell": "zsh" } }"#;
        let contract = contract_with_changes(
            vec![ContractChange::automatic(
                "move-shell",
                1,
                2,
                vec![MigrationOp::Rename {
                    from: "legacy.shell".to_string(),
                    to: "shell.command".to_string(),
                }],
            )],
            2,
        );

        let joined = join_jsonc_contract_text_from_version(raw, &contract, "ratconfig.contract", 1)
            .expect("join from version");
        let value = parse_jsonc_value(&joined.text).expect("jsonc");

        assert_eq!(get_json_path(&value, "shell.command"), Some(&json!("zsh")));
        assert_eq!(joined.state.version, 2);
        assert_eq!(joined.state.applied_change_ids, vec!["move-shell"]);
    }

    // Defends: joined state is scoped to the host contract id rather than silently applying another project's migrations.
    #[test]
    fn reconcile_rejects_contract_id_mismatch() {
        let raw = r#"{
  "ratconfig": {
    "contract": {
      "schema_version": 1,
      "contract_id": "other",
      "version": 1,
      "applied_change_ids": []
    }
  }
}"#;
        let contract = contract_with_changes(Vec::new(), 1);

        let error = reconcile_joined_jsonc_contract_text(raw, &contract, "ratconfig.contract")
            .expect_err("mismatch");

        assert_eq!(
            error,
            ContractError::ContractMismatch {
                expected: "demo".to_string(),
                found: "other".to_string(),
            }
        );
    }

    // Defends: ratconfig refuses branchy or gapped histories because automatic reconciliation must be deterministic.
    #[test]
    fn contract_changes_must_form_one_linear_chain() {
        let contract = contract_with_changes(
            vec![ContractChange::automatic(
                "skip",
                2,
                3,
                vec![MigrationOp::Delete {
                    path: "old".to_string(),
                }],
            )],
            3,
        );

        let error = plan_contract_migration(&contract, 1).expect_err("invalid chain");

        assert!(matches!(error, ContractError::InvalidContract { .. }));
    }

    // Defends: a change is either automatic or manual; mixed changes would create unclear partial-apply semantics.
    #[test]
    fn contract_change_cannot_mix_operations_and_manual_steps() {
        let contract = contract_with_changes(
            vec![ContractChange {
                id: "ambiguous".to_string(),
                from_version: 1,
                to_version: 2,
                operations: vec![MigrationOp::Delete {
                    path: "old".to_string(),
                }],
                manual_steps: vec![ManualMigrationStep {
                    id: "review-old".to_string(),
                    path: "old".to_string(),
                    reason: "The old value needs review.".to_string(),
                    remediation: "Move the useful parts manually.".to_string(),
                }],
            }],
            2,
        );

        let error = plan_contract_migration(&contract, 1).expect_err("ambiguous change");

        assert!(matches!(error, ContractError::InvalidContract { .. }));
    }
}
