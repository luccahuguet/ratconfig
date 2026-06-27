// Test lane: default

use crate::patch::get_dotted_json_path;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::path::PathBuf;
use toml::Value as TomlValue;

pub const DEFAULT_CONFIG_SOURCE_ID: &str = "config";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiModel {
    pub active_config_path: PathBuf,
    pub cursor_config_path: PathBuf,
    pub default_cursor_config_path: PathBuf,
    pub active_config_exists: bool,
    pub config_owner: ConfigUiPathOwner,
    pub config_read_only: bool,
    pub sources: Vec<ConfigUiSource>,
    pub tabs: Vec<String>,
    pub fields: Vec<ConfigUiField>,
    pub sidecars: Vec<ConfigUiSidecar>,
    pub native_config_statuses: Vec<ConfigUiNativeStatus>,
    pub diagnostics: Vec<ConfigUiDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigUiPathOwner {
    Default,
    HomeManager,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiSource {
    pub id: String,
    pub tab: String,
    pub label: String,
    pub path: PathBuf,
    pub exists: bool,
    pub owner: ConfigUiPathOwner,
    pub read_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiRowRef {
    Field(usize),
    Sidecar(usize),
    NativeStatus(usize),
    Diagnostic(usize),
}

pub fn visible_rows_for_tab_search(
    model: &ConfigUiModel,
    selected_tab: usize,
    search: &str,
) -> Vec<UiRowRef> {
    let tab = model
        .tabs
        .get(selected_tab)
        .map(String::as_str)
        .unwrap_or("general");
    let search = search.to_ascii_lowercase();
    if tab == "advanced" {
        return (0..model.diagnostics.len())
            .map(UiRowRef::Diagnostic)
            .chain((0..model.sidecars.len()).map(UiRowRef::Sidecar))
            .chain((0..model.native_config_statuses.len()).map(UiRowRef::NativeStatus))
            .filter(|row| row_matches_search(model, *row, &search))
            .collect();
    }

    (0..model.fields.len())
        .filter(|index| model.fields[*index].tab == tab)
        .map(UiRowRef::Field)
        .filter(|row| row_matches_search(model, *row, &search))
        .collect()
}

pub fn tab_index(tabs: &[String], tab: &str) -> usize {
    tabs.iter()
        .position(|candidate| candidate == tab)
        .unwrap_or(tabs.len())
}

pub fn selected_config_source(
    model: &ConfigUiModel,
    selected_tab: usize,
) -> Option<&ConfigUiSource> {
    let tab = model.tabs.get(selected_tab)?;
    if tab == "advanced" {
        return None;
    }
    model
        .sources
        .iter()
        .find(|source| source.tab == tab.as_str())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigUiValueState {
    Explicit,
    Defaulted,
    Unset,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiField {
    pub source_id: String,
    pub path: String,
    pub display_label: String,
    pub tab: String,
    pub kind: String,
    pub current_value: String,
    pub edit_value: String,
    pub default_value: String,
    pub state: ConfigUiValueState,
    pub description: String,
    pub allowed_values: Vec<String>,
    pub validation: String,
    pub rebuild_required: bool,
    pub apply_status: ConfigUiApplyStatus,
    pub edit_behavior: ConfigUiEditBehavior,
}

#[derive(Debug, Clone)]
pub struct ConfigUiFieldRowSpec<'a> {
    pub source_id: &'a str,
    pub path: &'a str,
    pub display_label: String,
    pub tab: &'a str,
    pub kind: &'a str,
    pub current: Option<&'a JsonValue>,
    pub default: Option<&'a JsonValue>,
    pub description: String,
    pub allowed_values: Vec<String>,
    pub validation: String,
    pub rebuild_required: bool,
    pub apply_status: ConfigUiApplyStatus,
    pub has_blocking_diagnostic: bool,
    pub edit_behavior: ConfigUiEditBehavior,
}

pub fn build_config_ui_field(spec: ConfigUiFieldRowSpec<'_>) -> ConfigUiField {
    let state = if spec.has_blocking_diagnostic {
        ConfigUiValueState::Invalid
    } else if spec.current.is_some() {
        ConfigUiValueState::Explicit
    } else if spec.default.is_some() {
        ConfigUiValueState::Defaulted
    } else {
        ConfigUiValueState::Unset
    };
    ConfigUiField {
        source_id: spec.source_id.to_string(),
        path: spec.path.to_string(),
        display_label: spec.display_label,
        tab: spec.tab.to_string(),
        kind: spec.kind.to_string(),
        current_value: spec
            .current
            .or(spec.default)
            .map(render_json_value)
            .unwrap_or_else(|| "not set".to_string()),
        edit_value: spec
            .current
            .or(spec.default)
            .map(render_json_edit_value)
            .unwrap_or_default(),
        default_value: spec
            .default
            .map(render_json_value)
            .unwrap_or_else(|| "no default".to_string()),
        state,
        description: spec.description,
        allowed_values: spec.allowed_values,
        validation: spec.validation,
        rebuild_required: spec.rebuild_required,
        apply_status: spec.apply_status,
        edit_behavior: spec.edit_behavior,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigUiEditBehavior {
    Default,
    FriendlyStringList,
    StructuredOnly { notice: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiApplyStatus {
    pub summary: String,
    pub label: String,
    pub detail: String,
    pub pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiSidecar {
    pub name: String,
    pub path: PathBuf,
    pub present: bool,
    pub owner: ConfigUiPathOwner,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiDiagnostic {
    pub path: String,
    pub status: String,
    pub headline: String,
    pub blocking: bool,
    pub detail_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiNativeStatus {
    pub surface: String,
    pub tool: String,
    pub description: String,
    pub status: String,
    pub label: String,
    pub severity: String,
    pub active_path: Option<String>,
    pub managed_path: Option<String>,
    pub native_paths: Vec<String>,
    pub generated_path: Option<String>,
    pub allowed_action: String,
    pub read_only_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigUiContractField {
    pub path: String,
    pub kind: String,
    pub default_value: Option<JsonValue>,
    pub validation: String,
    pub allowed_values: Vec<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub rebuild_required: bool,
    pub apply_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiFieldMetadata {
    pub tab: String,
    pub help: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiMetadata {
    pub tabs: Vec<String>,
    pub fields: BTreeMap<String, ConfigUiFieldMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiSchemaField {
    pub path: String,
    pub kind: String,
    pub allowed_values: Vec<String>,
}

pub fn config_contract_fields_from_toml(
    contract: &toml::Table,
) -> Result<BTreeMap<String, ConfigUiContractField>, String> {
    let fields_table = contract
        .get("fields")
        .and_then(TomlValue::as_table)
        .ok_or_else(|| "config contract is missing its fields table".to_string())?;

    let mut fields = BTreeMap::new();
    for (field_path, value) in fields_table {
        let table = value
            .as_table()
            .ok_or_else(|| format!("config contract field {field_path} must be a TOML table"))?;
        let kind = table
            .get("kind")
            .and_then(TomlValue::as_str)
            .unwrap_or("unknown")
            .to_string();
        let validation = table
            .get("validation")
            .and_then(TomlValue::as_str)
            .unwrap_or("")
            .to_string();
        let allowed_values = string_array(table.get("allowed_values"));
        let min = table.get("min").and_then(toml_number_as_f64);
        let max = table.get("max").and_then(toml_number_as_f64);
        let rebuild_required = table
            .get("rebuild_required")
            .and_then(TomlValue::as_bool)
            .unwrap_or(false);
        let apply_mode = required_toml_string(table, field_path, "apply_mode")?;
        let default_value = table.get("default").map(toml_value_to_json).transpose()?;
        fields.insert(
            field_path.clone(),
            ConfigUiContractField {
                path: field_path.clone(),
                kind,
                default_value,
                validation,
                allowed_values,
                min,
                max,
                rebuild_required,
                apply_mode,
            },
        );
    }

    Ok(fields)
}

pub fn config_ui_metadata_from_toml(metadata: &toml::Table) -> Result<ConfigUiMetadata, String> {
    let tabs = metadata
        .get("tab_order")
        .and_then(TomlValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(TomlValue::as_str)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if tabs.is_empty() {
        return Err("config UI metadata is missing tab_order".to_string());
    }

    let fields_table = metadata
        .get("fields")
        .and_then(TomlValue::as_table)
        .ok_or_else(|| "config UI metadata is missing its fields table".to_string())?;

    let mut fields = BTreeMap::new();
    for (field_path, value) in fields_table {
        let table = value
            .as_table()
            .ok_or_else(|| format!("config UI metadata field {field_path} must be a TOML table"))?;
        fields.insert(
            field_path.clone(),
            ConfigUiFieldMetadata {
                tab: required_toml_string(table, field_path, "tab")?,
                help: required_toml_string(table, field_path, "help")?,
            },
        );
    }

    Ok(ConfigUiMetadata { tabs, fields })
}

pub fn schema_tabs(
    schema: &JsonValue,
    schema_extension_key: &str,
    default_tabs: &[&str],
) -> Vec<String> {
    let mut tabs = schema
        .get(schema_extension_key)
        .and_then(|value| value.get("tabs"))
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_str)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if tabs.is_empty() {
        tabs = default_tabs.iter().map(|tab| (*tab).to_string()).collect();
    }
    if !tabs.iter().any(|tab| tab == "advanced") {
        tabs.push("advanced".to_string());
    }
    tabs
}

pub fn collect_config_ui_schema_fields(
    schema: &JsonValue,
    root_path: &str,
) -> Vec<ConfigUiSchemaField> {
    let mut fields = Vec::new();
    collect_schema_fields(schema, root_path, &mut fields);
    fields
}

fn collect_schema_fields(schema: &JsonValue, path: &str, out: &mut Vec<ConfigUiSchemaField>) {
    let kind = schema_type(schema);
    if kind == "object" {
        let Some(properties) = schema.get("properties").and_then(JsonValue::as_object) else {
            out.push(schema_field(schema, path, kind));
            return;
        };
        for (name, property) in properties {
            collect_schema_fields(property, &format!("{path}.{name}"), out);
        }
        return;
    }

    if kind == "array"
        && let Some(items) = schema.get("items")
        && items.get("type").and_then(JsonValue::as_str) == Some("object")
    {
        out.push(schema_field(schema, path, kind));
        return;
    }

    if kind == "array"
        && let Some(items) = schema.get("items")
        && items.get("type").and_then(JsonValue::as_str) == Some("string")
    {
        out.push(schema_field(schema, path, "string_list".to_string()));
        return;
    }

    out.push(schema_field(schema, path, kind));
}

fn schema_field(schema: &JsonValue, path: &str, kind: String) -> ConfigUiSchemaField {
    let allowed_values = if kind == "string_list" {
        schema
            .get("items")
            .map(schema_enum_values)
            .unwrap_or_default()
    } else {
        schema_enum_values(schema)
    };
    ConfigUiSchemaField {
        path: path.to_string(),
        kind,
        allowed_values,
    }
}

fn schema_type(schema: &JsonValue) -> String {
    schema
        .get("type")
        .and_then(JsonValue::as_str)
        .unwrap_or("unknown")
        .to_string()
}

fn schema_enum_values(schema: &JsonValue) -> Vec<String> {
    schema
        .get("enum")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn required_toml_string(
    table: &toml::Table,
    field_path: &str,
    key: &str,
) -> Result<String, String> {
    table
        .get(key)
        .and_then(TomlValue::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("field {field_path} is missing {key}"))
}

pub fn toml_value_to_json(value: &TomlValue) -> Result<JsonValue, String> {
    match value {
        TomlValue::String(value) => Ok(JsonValue::String(value.clone())),
        TomlValue::Integer(value) => Ok(JsonValue::Number((*value).into())),
        TomlValue::Float(value) => serde_json::Number::from_f64(*value)
            .map(JsonValue::Number)
            .ok_or_else(|| "TOML float must be finite".to_string()),
        TomlValue::Boolean(value) => Ok(JsonValue::Bool(*value)),
        TomlValue::Array(values) => values
            .iter()
            .map(toml_value_to_json)
            .collect::<Result<Vec<_>, _>>()
            .map(JsonValue::Array),
        TomlValue::Table(values) => values
            .iter()
            .map(|(key, value)| toml_value_to_json(value).map(|value| (key.clone(), value)))
            .collect::<Result<serde_json::Map<_, _>, _>>()
            .map(JsonValue::Object),
        TomlValue::Datetime(value) => Ok(JsonValue::String(value.to_string())),
    }
}

fn string_array(value: Option<&TomlValue>) -> Vec<String> {
    value
        .and_then(TomlValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(TomlValue::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn toml_number_as_f64(value: &TomlValue) -> Option<f64> {
    value
        .as_float()
        .or_else(|| value.as_integer().map(|value| value as f64))
}

pub fn owner_label(owner: ConfigUiPathOwner) -> &'static str {
    match owner {
        ConfigUiPathOwner::Default => "default",
        ConfigUiPathOwner::HomeManager => "home-manager",
        ConfigUiPathOwner::User => "user",
    }
}

fn row_matches_search(model: &ConfigUiModel, row: UiRowRef, search: &str) -> bool {
    match row {
        UiRowRef::Field(index) => {
            let field = &model.fields[index];
            search_matches(
                search,
                [
                    field.path.as_str(),
                    field.display_label.as_str(),
                    field.current_value.as_str(),
                    field.default_value.as_str(),
                    field.description.as_str(),
                ],
            )
        }
        UiRowRef::Sidecar(index) => {
            let sidecar = &model.sidecars[index];
            let path = sidecar.path.to_string_lossy();
            search_matches(
                search,
                [
                    sidecar.name.as_str(),
                    path.as_ref(),
                    owner_label(sidecar.owner),
                ],
            )
        }
        UiRowRef::Diagnostic(index) => {
            let diagnostic = &model.diagnostics[index];
            search_matches(
                search,
                [
                    diagnostic.path.as_str(),
                    diagnostic.status.as_str(),
                    diagnostic.headline.as_str(),
                ],
            )
        }
        UiRowRef::NativeStatus(index) => {
            let status = &model.native_config_statuses[index];
            search_matches(
                search,
                [
                    status.surface.as_str(),
                    status.tool.as_str(),
                    status.status.as_str(),
                    status.label.as_str(),
                    status.description.as_str(),
                ],
            )
        }
    }
}

fn search_matches<'a>(search: &str, candidates: impl IntoIterator<Item = &'a str>) -> bool {
    search.is_empty()
        || candidates
            .into_iter()
            .any(|candidate| candidate.to_ascii_lowercase().contains(search))
}

pub fn get_json_path<'a>(value: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    get_dotted_json_path(value, path)
}

pub fn effective_json_path<'a>(
    active: &'a JsonValue,
    default: &'a JsonValue,
    path: &str,
) -> Option<&'a JsonValue> {
    get_json_path(active, path).or_else(|| get_json_path(default, path))
}

pub fn effective_string_config(
    active: &JsonValue,
    default: &JsonValue,
    path: &str,
    fallback: &str,
) -> String {
    effective_json_path(active, default, path)
        .and_then(JsonValue::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}

pub fn effective_string_list_config(
    active: &JsonValue,
    default: &JsonValue,
    path: &str,
    fallback: &[&str],
) -> Vec<String> {
    let values = effective_json_path(active, default, path)
        .and_then(JsonValue::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(JsonValue::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if values.is_empty() {
        fallback.iter().map(|value| (*value).to_string()).collect()
    } else {
        values
    }
}

pub fn render_json_value(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::Number(value) => value.to_string(),
        JsonValue::String(value) => format!("{value:?}"),
        JsonValue::Array(values) => {
            if values.len() <= 4 {
                serde_json::to_string(values)
                    .unwrap_or_else(|_| format!("[{} items]", values.len()))
            } else {
                format!("[{} items]", values.len())
            }
        }
        JsonValue::Object(object) => format!("{{{} keys}}", object.len()),
    }
}

pub fn render_json_edit_value(value: &JsonValue) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| render_json_value(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status() -> ConfigUiApplyStatus {
        ConfigUiApplyStatus {
            summary: "after save".to_string(),
            label: "after save".to_string(),
            detail: "Host applies this after saving.".to_string(),
            pending: true,
        }
    }

    // Defends: source metadata is selected by host tab while operational tabs can keep the legacy fallback.
    #[test]
    fn selected_config_source_matches_selected_tab() {
        let source = |id: &str, tab: &str| ConfigUiSource {
            id: id.to_string(),
            tab: tab.to_string(),
            label: String::new(),
            path: PathBuf::new(),
            exists: true,
            owner: ConfigUiPathOwner::User,
            read_only: false,
        };
        let model = ConfigUiModel {
            active_config_path: PathBuf::from("/tmp/acme/settings.jsonc"),
            cursor_config_path: PathBuf::new(),
            default_cursor_config_path: PathBuf::new(),
            active_config_exists: true,
            config_owner: ConfigUiPathOwner::User,
            config_read_only: false,
            sources: vec![
                source("settings-source", "settings"),
                source("keys-source", "keys"),
                source("advanced-source", "advanced"),
            ],
            tabs: vec![
                "settings".to_string(),
                "keys".to_string(),
                "advanced".to_string(),
            ],
            fields: Vec::new(),
            sidecars: Vec::new(),
            native_config_statuses: Vec::new(),
            diagnostics: Vec::new(),
        };

        assert_eq!(
            selected_config_source(&model, 1).map(|source| source.id.as_str()),
            Some("keys-source")
        );
        assert!(selected_config_source(&model, 2).is_none());
    }

    fn spec<'a>(
        current: Option<&'a JsonValue>,
        default: Option<&'a JsonValue>,
        has_blocking_diagnostic: bool,
    ) -> ConfigUiFieldRowSpec<'a> {
        ConfigUiFieldRowSpec {
            source_id: DEFAULT_CONFIG_SOURCE_ID,
            path: "ui.theme",
            display_label: String::new(),
            tab: "general",
            kind: "string",
            current,
            default,
            description: "Theme name".to_string(),
            allowed_values: vec!["light".to_string(), "dark".to_string()],
            validation: "must be a known theme".to_string(),
            rebuild_required: false,
            apply_status: status(),
            has_blocking_diagnostic,
            edit_behavior: ConfigUiEditBehavior::Default,
        }
    }

    // Defends: schema tab extraction is reusable and always includes the advanced operational tab.
    #[test]
    fn schema_tabs_use_schema_order_or_fallback_with_advanced() {
        let schema = json!({
            "x-host-config": {
                "tabs": ["general", "editor"]
            }
        });
        assert_eq!(
            schema_tabs(&schema, "x-host-config", &["fallback"]),
            vec!["general", "editor", "advanced"]
        );

        assert_eq!(
            schema_tabs(
                &json!({}),
                "x-host-config",
                &["general", "workspace", "advanced"]
            ),
            vec!["general", "workspace", "advanced"]
        );
    }

    // Defends: schema traversal converts common JSON Schema leaves into ratconfig field specs.
    #[test]
    fn schema_field_collection_discovers_nested_scalars_arrays_and_enums() {
        let schema = json!({
            "type": "object",
            "properties": {
                "enabled": { "type": "boolean" },
                "theme": { "type": "string", "enum": ["light", "dark"] },
                "plugins": {
                    "type": "array",
                    "items": { "type": "string", "enum": ["git", "search"] }
                },
                "rules": {
                    "type": "array",
                    "items": { "type": "object" }
                }
            }
        });

        assert_eq!(
            collect_config_ui_schema_fields(&schema, "app"),
            vec![
                ConfigUiSchemaField {
                    path: "app.enabled".to_string(),
                    kind: "boolean".to_string(),
                    allowed_values: Vec::new(),
                },
                ConfigUiSchemaField {
                    path: "app.plugins".to_string(),
                    kind: "string_list".to_string(),
                    allowed_values: vec!["git".to_string(), "search".to_string()],
                },
                ConfigUiSchemaField {
                    path: "app.rules".to_string(),
                    kind: "array".to_string(),
                    allowed_values: Vec::new(),
                },
                ConfigUiSchemaField {
                    path: "app.theme".to_string(),
                    kind: "string".to_string(),
                    allowed_values: vec!["light".to_string(), "dark".to_string()],
                },
            ]
        );
    }

    // Defends: TOML contract and UI metadata decoding are generic ratconfig model behavior.
    #[test]
    fn toml_contract_and_metadata_decode_to_neutral_specs() {
        let contract = r#"
[fields."ui.theme"]
kind = "string"
default = "light"
validation = "known theme"
allowed_values = ["light", "dark"]
min = 1
max = 4
rebuild_required = true
apply_mode = "tab_session_restart"
"#
        .parse::<toml::Table>()
        .expect("contract toml");
        let fields = config_contract_fields_from_toml(&contract).expect("fields");
        let field = fields.get("ui.theme").expect("field");
        assert_eq!(field.kind, "string");
        assert_eq!(field.default_value, Some(json!("light")));
        assert_eq!(field.allowed_values, vec!["light", "dark"]);
        assert_eq!(field.min, Some(1.0));
        assert_eq!(field.max, Some(4.0));
        assert!(field.rebuild_required);
        assert_eq!(field.apply_mode, "tab_session_restart");

        let metadata = r#"
tab_order = ["general", "advanced"]

[fields."ui.theme"]
tab = "general"
help = "Theme name"
"#
        .parse::<toml::Table>()
        .expect("metadata toml");
        let metadata = config_ui_metadata_from_toml(&metadata).expect("metadata");
        assert_eq!(metadata.tabs, vec!["general", "advanced"]);
        assert_eq!(metadata.fields["ui.theme"].help, "Theme name");
    }

    // Defends: reusable field row construction marks explicit, defaulted, unset, and invalid states from host-provided values.
    #[test]
    fn field_row_builder_derives_neutral_value_state() {
        let current = json!("dark");
        let default = json!("light");

        let explicit = build_config_ui_field(spec(Some(&current), Some(&default), false));
        assert_eq!(explicit.state, ConfigUiValueState::Explicit);
        assert_eq!(explicit.current_value, "\"dark\"");
        assert_eq!(explicit.edit_value, "\"dark\"");
        assert_eq!(explicit.default_value, "\"light\"");

        let defaulted = build_config_ui_field(spec(None, Some(&default), false));
        assert_eq!(defaulted.state, ConfigUiValueState::Defaulted);
        assert_eq!(defaulted.current_value, "\"light\"");

        let unset = build_config_ui_field(spec(None, None, false));
        assert_eq!(unset.state, ConfigUiValueState::Unset);
        assert_eq!(unset.current_value, "not set");
        assert_eq!(unset.default_value, "no default");

        let invalid = build_config_ui_field(spec(Some(&current), Some(&default), true));
        assert_eq!(invalid.state, ConfigUiValueState::Invalid);
    }

    // Defends: host policy fields pass through unchanged while the generic builder renders JSON safely.
    #[test]
    fn field_row_builder_preserves_host_metadata() {
        let current = json!(["git", "search", "preview", "terminal", "theme"]);
        let field = build_config_ui_field(ConfigUiFieldRowSpec {
            source_id: "settings",
            path: "plugins.enabled",
            display_label: "Enabled plugins".to_string(),
            tab: "advanced",
            kind: "string_list",
            current: Some(&current),
            default: None,
            description: "Enabled plugin list".to_string(),
            allowed_values: vec!["git".to_string()],
            validation: "known plugins only".to_string(),
            rebuild_required: true,
            apply_status: status(),
            has_blocking_diagnostic: false,
            edit_behavior: ConfigUiEditBehavior::FriendlyStringList,
        });

        assert_eq!(field.source_id, "settings");
        assert_eq!(field.path, "plugins.enabled");
        assert_eq!(field.display_label, "Enabled plugins");
        assert_eq!(field.tab, "advanced");
        assert_eq!(field.current_value, "[5 items]");
        assert_eq!(
            field.edit_value,
            r#"["git","search","preview","terminal","theme"]"#
        );
        assert!(field.rebuild_required);
        assert_eq!(
            field.edit_behavior,
            ConfigUiEditBehavior::FriendlyStringList
        );
        assert_eq!(field.apply_status.summary, "after save");
    }
}
