use super::{
    AppliedContractChange, ConfigContract, ContractApplyOutcome, ContractError,
    ContractJoinOutcome, ContractState, append_applied_change_ids, apply_contract_with,
    contract_state_to_json, new_joined_state, validate_contract,
};
use crate::jsonc::{PatchError, PatchOutcome, parse_jsonc_value, set_jsonc_value_text};
use crate::migration::apply_migrations_text;
use serde_json::Value as JsonValue;

impl From<PatchError> for ContractError {
    fn from(error: PatchError) -> Self {
        Self::JsoncPatch(error)
    }
}

pub fn apply_jsonc_contract_text(
    raw: &str,
    contract: &ConfigContract,
    from_version: u64,
) -> Result<ContractApplyOutcome, ContractError> {
    apply_contract_with(raw, contract, from_version, |text, change| {
        apply_migrations_text(text, &change.operations)
            .map(|outcome| (outcome.text, outcome.mutations))
            .map_err(|error| ContractError::JsoncMigration {
                change_id: change.id.clone(),
                error,
            })
    })
}

pub fn read_jsonc_contract_state_text(
    raw: &str,
    state_path: &str,
) -> Result<Option<ContractState>, ContractError> {
    let value = parse_jsonc_value(raw)?;
    read_jsonc_contract_state(&value, state_path)
}

pub fn read_jsonc_contract_state(
    value: &JsonValue,
    state_path: &str,
) -> Result<Option<ContractState>, ContractError> {
    super::read_contract_state_from_json(value, state_path)
}

pub fn write_jsonc_contract_state_text(
    raw: &str,
    state_path: &str,
    state: &ContractState,
) -> Result<PatchOutcome, ContractError> {
    set_jsonc_value_text(raw, state_path, &contract_state_to_json(state)).map_err(Into::into)
}

pub fn join_jsonc_contract_text(
    raw: &str,
    contract: &ConfigContract,
    state_path: &str,
) -> Result<ContractJoinOutcome, ContractError> {
    if read_jsonc_contract_state_text(raw, state_path)?.is_some() {
        return reconcile_joined_jsonc_contract_text(raw, contract, state_path);
    }
    validate_contract(contract)?;
    write_jsonc_joined_state(raw, contract, state_path, Vec::new())
}

pub fn join_jsonc_contract_text_from_version(
    raw: &str,
    contract: &ConfigContract,
    state_path: &str,
    from_version: u64,
) -> Result<ContractJoinOutcome, ContractError> {
    if read_jsonc_contract_state_text(raw, state_path)?.is_some() {
        return reconcile_joined_jsonc_contract_text(raw, contract, state_path);
    }
    let applied = apply_jsonc_contract_text(raw, contract, from_version)?;
    let text = applied.text;
    write_jsonc_joined_state(&text, contract, state_path, applied.applied_changes)
}

pub fn reconcile_joined_jsonc_contract_text(
    raw: &str,
    contract: &ConfigContract,
    state_path: &str,
) -> Result<ContractJoinOutcome, ContractError> {
    let Some(previous_state) = read_jsonc_contract_state_text(raw, state_path)? else {
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

    let applied = apply_jsonc_contract_text(raw, contract, previous_state.version)?;
    let mut state = previous_state;
    state.version = contract.current_version;
    append_applied_change_ids(&mut state, &applied.applied_changes);
    let state_patch = write_jsonc_contract_state_text(&applied.text, state_path, &state)?;
    Ok(ContractJoinOutcome {
        text: state_patch.text,
        state,
        applied_changes: applied.applied_changes,
        state_mutation: state_patch.mutation,
    })
}

fn write_jsonc_joined_state(
    raw: &str,
    contract: &ConfigContract,
    state_path: &str,
    applied_changes: Vec<AppliedContractChange>,
) -> Result<ContractJoinOutcome, ContractError> {
    let state = new_joined_state(contract, &applied_changes);
    let state_patch = write_jsonc_contract_state_text(raw, state_path, &state)?;
    Ok(ContractJoinOutcome {
        text: state_patch.text,
        state,
        applied_changes,
        state_mutation: state_patch.mutation,
    })
}
