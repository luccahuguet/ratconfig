use crate::{
    ConfigUiApplyStatus, ConfigUiEditBehavior, ConfigUiField, ConfigUiModel, ConfigUiValueState,
    DEFAULT_CONFIG_SOURCE_ID,
};
use std::collections::BTreeMap;

pub(crate) fn apply_status(summary: &str, detail: &str) -> ConfigUiApplyStatus {
    ConfigUiApplyStatus {
        summary: summary.to_string(),
        label: summary.to_string(),
        detail: detail.to_string(),
        pending: true,
    }
}

pub(crate) fn after_save_status() -> ConfigUiApplyStatus {
    apply_status(
        "after save",
        "The host application applies this field after saving.",
    )
}

pub(crate) fn field(path: &str, kind: &str, value: &str, allowed: &[&str]) -> ConfigUiField {
    field_with_source(DEFAULT_CONFIG_SOURCE_ID, path, kind, value, allowed)
}

pub(crate) fn field_with_source(
    source_id: &str,
    path: &str,
    kind: &str,
    value: &str,
    allowed: &[&str],
) -> ConfigUiField {
    ConfigUiField {
        source_id: source_id.to_string(),
        path: path.to_string(),
        display_label: String::new(),
        section_label: String::new(),
        list_cells: Vec::new(),
        tab: "general".to_string(),
        kind: kind.to_string(),
        current_value: value.to_string(),
        edit_value: value.to_string(),
        default_value: value.to_string(),
        state: ConfigUiValueState::Explicit,
        description: String::new(),
        allowed_values: allowed.iter().map(|value| (*value).to_string()).collect(),
        validation: String::new(),
        rebuild_required: false,
        apply_status: after_save_status(),
        edit_behavior: ConfigUiEditBehavior::Default,
    }
}

pub(crate) fn model_with_fields(fields: Vec<ConfigUiField>) -> ConfigUiModel {
    ConfigUiModel {
        sources: Vec::new(),
        tabs: vec!["general".to_string()],
        tab_list_tables: BTreeMap::new(),
        fields,
        core_fields: None,
        file_actions: Vec::new(),
        sidecars: Vec::new(),
        native_config_statuses: Vec::new(),
        diagnostics: Vec::new(),
        theme_switcher: None,
    }
}
