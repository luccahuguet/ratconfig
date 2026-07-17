use crate::{
    ConfigUiApplyStatus, ConfigUiField, ConfigUiFieldSpec, ConfigUiModel, ConfigUiSource,
    DEFAULT_CONFIG_SOURCE_ID,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

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
    let value: serde_json::Value = serde_json::from_str(value)
        .unwrap_or_else(|_| serde_json::Value::String(value.to_string()));
    ConfigUiFieldSpec::new(
        source_id,
        path,
        "general",
        "",
        allowed.iter().map(|value| (*value).to_string()).collect(),
        "",
        after_save_status(),
    )
    .build(kind, Some(&value), Some(&value))
}

pub(crate) fn model_with_fields(fields: Vec<ConfigUiField>) -> ConfigUiModel {
    let sources = fields
        .iter()
        .map(|field| field.source_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|id| ConfigUiSource {
            label: id.clone(),
            path: PathBuf::from(format!("{id}.toml")),
            id,
            exists: true,
            owner_label: Some("test".to_string()),
            read_only: false,
        })
        .collect();
    ConfigUiModel {
        sources,
        tabs: vec!["general".to_string()],
        operational_tab: None,
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
