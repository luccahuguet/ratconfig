// Test lane: default

use super::{
    AppliedContractChange, ConfigContract, ContractApplyOutcome, ContractError,
    ContractJoinOutcome, ContractState, append_applied_change_ids, contract_state_to_json,
    new_joined_state, plan_from_changes, planned_change_refs, read_contract_state_value,
    validate_contract,
};
use crate::patch::{PatchOutcome, get_dotted_json_path_parts};
use crate::toml_adapter::{
    TomlPatchError, apply_toml_migrations_text, parse_toml_value, set_toml_value_text,
    split_toml_path,
};
use serde_json::Value as JsonValue;

impl From<TomlPatchError> for ContractError {
    fn from(error: TomlPatchError) -> Self {
        Self::TomlPatch(error)
    }
}

pub fn apply_toml_contract_text(
    raw: &str,
    contract: &ConfigContract,
    from_version: u64,
) -> Result<ContractApplyOutcome, ContractError> {
    let changes = planned_change_refs(contract, from_version)?;
    let plan = plan_from_changes(from_version, contract.current_version, &changes);
    if plan.requires_manual_action() {
        return Err(ContractError::ManualRequired { plan });
    }

    let mut text = raw.to_string();
    let mut applied_changes = Vec::new();
    for change in changes {
        let outcome = apply_toml_migrations_text(&text, &change.operations).map_err(|error| {
            ContractError::TomlMigration {
                change_id: change.id.clone(),
                error,
            }
        })?;
        text = outcome.text;
        applied_changes.push(AppliedContractChange {
            id: change.id.clone(),
            from_version: change.from_version,
            to_version: change.to_version,
            mutations: outcome.mutations,
        });
    }

    Ok(ContractApplyOutcome {
        text,
        from_version,
        to_version: contract.current_version,
        applied_changes,
    })
}

pub fn read_toml_contract_state_text(
    raw: &str,
    state_path: &str,
) -> Result<Option<ContractState>, ContractError> {
    let value = parse_toml_value(raw)?;
    read_toml_contract_state(&value, state_path)
}

pub fn read_toml_contract_state(
    value: &JsonValue,
    state_path: &str,
) -> Result<Option<ContractState>, ContractError> {
    let parts = split_toml_path(state_path)?;
    get_dotted_json_path_parts(value, &parts)
        .map(|state_value| read_contract_state_value(state_value, state_path))
        .transpose()
}

pub fn write_toml_contract_state_text(
    raw: &str,
    state_path: &str,
    state: &ContractState,
) -> Result<PatchOutcome, ContractError> {
    set_toml_value_text(raw, state_path, &contract_state_to_json(state)).map_err(Into::into)
}

pub fn join_toml_contract_text(
    raw: &str,
    contract: &ConfigContract,
    state_path: &str,
) -> Result<ContractJoinOutcome, ContractError> {
    if read_toml_contract_state_text(raw, state_path)?.is_some() {
        return reconcile_joined_toml_contract_text(raw, contract, state_path);
    }
    validate_contract(contract)?;
    write_toml_joined_state(raw, contract, state_path, Vec::new())
}

pub fn join_toml_contract_text_from_version(
    raw: &str,
    contract: &ConfigContract,
    state_path: &str,
    from_version: u64,
) -> Result<ContractJoinOutcome, ContractError> {
    if read_toml_contract_state_text(raw, state_path)?.is_some() {
        return reconcile_joined_toml_contract_text(raw, contract, state_path);
    }
    let applied = apply_toml_contract_text(raw, contract, from_version)?;
    let text = applied.text;
    write_toml_joined_state(&text, contract, state_path, applied.applied_changes)
}

pub fn reconcile_joined_toml_contract_text(
    raw: &str,
    contract: &ConfigContract,
    state_path: &str,
) -> Result<ContractJoinOutcome, ContractError> {
    let Some(previous_state) = read_toml_contract_state_text(raw, state_path)? else {
        return Err(ContractError::NotJoined {
            state_path: state_path.to_string(),
        });
    };
    if previous_state.contract_id != contract.id {
        return Err(ContractError::ContractMismatch {
            expected: contract.id.clone(),
            found: previous_state.contract_id,
        });
    }

    let applied = apply_toml_contract_text(raw, contract, previous_state.version)?;
    let mut state = previous_state;
    state.version = contract.current_version;
    append_applied_change_ids(&mut state, &applied.applied_changes);
    let state_patch = write_toml_contract_state_text(&applied.text, state_path, &state)?;
    Ok(ContractJoinOutcome {
        text: state_patch.text,
        state,
        applied_changes: applied.applied_changes,
        state_mutation: state_patch.mutation,
    })
}

fn write_toml_joined_state(
    raw: &str,
    contract: &ConfigContract,
    state_path: &str,
    applied_changes: Vec<AppliedContractChange>,
) -> Result<ContractJoinOutcome, ContractError> {
    let state = new_joined_state(contract, &applied_changes);
    let state_patch = write_toml_contract_state_text(raw, state_path, &state)?;
    Ok(ContractJoinOutcome {
        text: state_patch.text,
        state,
        applied_changes,
        state_mutation: state_patch.mutation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::MigrationOp;
    use crate::toml_adapter::get_toml_path;
    use serde_json::json;

    fn contract_with_changes(
        changes: Vec<super::super::ContractChange>,
        current_version: u64,
    ) -> ConfigContract {
        ConfigContract {
            id: "demo".to_string(),
            baseline_version: 1,
            current_version,
            changes,
        }
    }

    // Defends: TOML state reads target the same normalized path as state writes.
    #[test]
    fn toml_contract_state_reads_normalized_path() {
        let state = |contract_id| {
            json!({
                "schema_version": 1,
                "contract_id": contract_id,
                "version": 1,
            })
        };
        let value = json!({
            " ratconfig ": { " contract ": state("exact") },
            "ratconfig": { "contract": state("normalized") },
        });

        assert_eq!(
            read_toml_contract_state(&value, " ratconfig . contract ")
                .expect("normalized state path")
                .expect("contract state")
                .contract_id,
            "normalized"
        );
        assert!(matches!(
            read_toml_contract_state(
                &json!({ "ratconfig": { "contract": true } }),
                " ratconfig . contract "
            ),
            Err(ContractError::InvalidState { state_path, .. })
                if state_path == " ratconfig . contract "
        ));
    }

    // Defends: TOML configs can join the same semantic contract and later reconcile automatic changes.
    #[test]
    fn joined_toml_contract_reconciles_future_versions_and_updates_state() {
        let raw = r#"[core]
"#;
        let v1 = contract_with_changes(Vec::new(), 1);
        let joined = join_toml_contract_text(raw, &v1, "ratconfig.contract").expect("join");

        assert_eq!(
            joined.state,
            ContractState {
                contract_id: "demo".to_string(),
                version: 1,
                applied_change_ids: Vec::new(),
            }
        );

        let v2 = contract_with_changes(
            vec![super::super::ContractChange::automatic(
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
            reconcile_joined_toml_contract_text(&joined.text, &v2, "ratconfig.contract")
                .expect("reconcile");
        let value = parse_toml_value(&reconciled.text).expect("toml");

        assert_eq!(get_toml_path(&value, "core.debug"), Some(&json!(true)));
        assert_eq!(
            read_toml_contract_state(&value, "ratconfig.contract").expect("state"),
            Some(ContractState {
                contract_id: "demo".to_string(),
                version: 2,
                applied_change_ids: vec!["add-debug".to_string()],
            })
        );
    }

    // Defends: adopting an older TOML config applies automatic changes before recording joined state.
    #[test]
    fn join_toml_from_known_version_applies_changes_before_recording_state() {
        let raw = r#"[legacy]
shell = "zsh"
"#;
        let contract = contract_with_changes(
            vec![super::super::ContractChange::automatic(
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

        let joined = join_toml_contract_text_from_version(raw, &contract, "ratconfig.contract", 1)
            .expect("join from version");
        let value = parse_toml_value(&joined.text).expect("toml");

        assert_eq!(get_toml_path(&value, "shell.command"), Some(&json!("zsh")));
        assert_eq!(joined.state.version, 2);
        assert_eq!(joined.state.applied_change_ids, vec!["move-shell"]);
    }
}
