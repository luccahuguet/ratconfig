use crate::{
    ConfigUiApplyStatus, ConfigUiCapability, ConfigUiChoice, ConfigUiField, ConfigUiFieldSpec,
    ConfigUiModel, ConfigUiSource, ConfigUiTextEncoding, DEFAULT_CONFIG_SOURCE_ID,
};
use serde_json::{Value as JsonValue, json};
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
    let value: JsonValue =
        serde_json::from_str(value).unwrap_or_else(|_| JsonValue::String(value.to_string()));
    let choices = allowed
        .iter()
        .map(|value| ConfigUiChoice::new(json!(value)))
        .collect();
    let capability = match kind {
        "bool" => ConfigUiCapability::Toggle {
            off: ConfigUiChoice::new(json!(false)),
            on: ConfigUiChoice::new(json!(true)),
        },
        "string" if !allowed.is_empty() => ConfigUiCapability::Choice { choices },
        "string_list" if !allowed.is_empty() => ConfigUiCapability::MultiChoice {
            choices,
            ordered: false,
        },
        "array" | "object" => ConfigUiCapability::ReadOnly {
            reason: "Structured test field is read-only.".to_string(),
            file_action_id: None,
        },
        "string" => ConfigUiCapability::FreeText {
            encoding: ConfigUiTextEncoding::String,
        },
        _ => ConfigUiCapability::FreeText {
            encoding: ConfigUiTextEncoding::Json,
        },
    };
    let mut spec = ConfigUiFieldSpec::new(
        source_id,
        path,
        "general",
        "",
        capability,
        "",
        after_save_status(),
    );
    spec.can_unset = true;
    spec.build(kind, Some(&value), Some(&value))
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
        recommended_fields: None,
        file_actions: Vec::new(),
        sidecars: Vec::new(),
        native_config_statuses: Vec::new(),
        diagnostics: Vec::new(),
        theme_switcher: None,
    }
}
