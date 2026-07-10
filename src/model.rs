// Test lane: default

use crate::patch::get_dotted_json_path;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::path::PathBuf;
use toml::Value as TomlValue;
use toml_edit::{DocumentMut as TomlEditDocument, Table as TomlEditTable};

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
    pub tab_list_tables: BTreeMap<String, ConfigUiListTable>,
    pub fields: Vec<ConfigUiField>,
    pub file_actions: Vec<ConfigUiFileAction>,
    pub sidecars: Vec<ConfigUiSidecar>,
    pub native_config_statuses: Vec<ConfigUiNativeStatus>,
    pub diagnostics: Vec<ConfigUiDiagnostic>,
    pub theme_switcher: Option<ConfigUiThemeSwitcher>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiListTable {
    pub columns: Vec<ConfigUiListColumn>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiListColumn {
    pub title: String,
    pub width: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigUiTheme {
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiThemeSwitcher {
    pub source_id: String,
    pub field_path: String,
    pub mappings: Vec<ConfigUiThemeMapping>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiThemeMapping {
    pub value: JsonValue,
    pub theme: ConfigUiTheme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiRowRef {
    Field(usize),
    FileAction(usize),
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
            .chain(file_action_rows_for_tab(model, tab))
            .chain((0..model.sidecars.len()).map(UiRowRef::Sidecar))
            .chain((0..model.native_config_statuses.len()).map(UiRowRef::NativeStatus))
            .filter(|row| row_matches_search(model, *row, &search))
            .collect();
    }

    (0..model.fields.len())
        .filter(|index| model.fields[*index].tab == tab)
        .map(UiRowRef::Field)
        .chain(file_action_rows_for_tab(model, tab))
        .filter(|row| row_matches_search(model, *row, &search))
        .collect()
}

fn file_action_rows_for_tab<'a>(
    model: &'a ConfigUiModel,
    tab: &'a str,
) -> impl Iterator<Item = UiRowRef> + 'a {
    (0..model.file_actions.len())
        .filter(move |index| model.file_actions[*index].tab == tab)
        .map(UiRowRef::FileAction)
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

pub(crate) fn config_ui_theme_from_model(model: &ConfigUiModel) -> ConfigUiTheme {
    model
        .theme_switcher
        .as_ref()
        .and_then(|switcher| switcher.resolve(&model.fields))
        .unwrap_or(ConfigUiTheme::Dark)
}

impl ConfigUiThemeSwitcher {
    pub fn resolve(&self, fields: &[ConfigUiField]) -> Option<ConfigUiTheme> {
        let field = fields
            .iter()
            .find(|field| field.source_id == self.source_id && field.path == self.field_path)?;
        let value = committed_field_value(field)?;
        self.theme_for_value(&value)
    }

    pub fn theme_for_value(&self, value: &JsonValue) -> Option<ConfigUiTheme> {
        self.mappings
            .iter()
            .find(|mapping| mapping.value == *value)
            .map(|mapping| mapping.theme)
    }
}

fn committed_field_value(field: &ConfigUiField) -> Option<JsonValue> {
    serde_json::from_str(&field.edit_value)
        .or_else(|_| serde_json::from_str(&field.current_value))
        .ok()
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
    pub section_label: String,
    pub list_cells: Vec<String>,
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

/// Host-owned config file action row.
///
/// Ratconfig renders this row and emits an `OpenFile` intent when activated.
/// Hosts own path discovery, creation, editor launching, validation, reloads,
/// and any file IO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiFileAction {
    pub source_id: String,
    pub action_id: String,
    pub tab: String,
    pub label: String,
    pub description: String,
    pub path: PathBuf,
    pub exists: bool,
    pub read_only: bool,
    pub create_if_missing: bool,
    pub disabled_reason: Option<String>,
}

impl ConfigUiField {
    pub fn has_default_value(&self) -> bool {
        self.default_value != NO_CONFIG_DEFAULT_VALUE_LABEL
    }
}

/// Display marker for manually constructed fields that do not have a default.
pub const NO_CONFIG_DEFAULT_VALUE_LABEL: &str = "no default";

#[derive(Debug, Clone)]
pub struct ConfigUiFieldRowSpec<'a> {
    pub source_id: &'a str,
    pub path: &'a str,
    pub display_label: String,
    pub section_label: String,
    pub list_cells: Vec<String>,
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

#[derive(Debug, Clone)]
pub struct ConfigUiStringListChoiceSpec {
    pub source_id: String,
    pub path: String,
    pub display_label: String,
    pub section_label: String,
    pub list_cells: Vec<String>,
    pub tab: String,
    pub current: Option<Vec<String>>,
    pub default: Option<Vec<String>>,
    pub description: String,
    pub allowed_values: Vec<String>,
    pub validation: String,
    pub rebuild_required: bool,
    pub apply_status: ConfigUiApplyStatus,
    pub has_blocking_diagnostic: bool,
    pub edit_behavior: ConfigUiEditBehavior,
}

#[derive(Debug, Clone)]
pub struct ConfigUiTomlDocumentSpec<'a> {
    pub source_id: &'a str,
    pub tab: &'a str,
    pub section_label: &'a str,
    pub current_toml: &'a str,
    pub default_toml: Option<&'a str>,
    pub validation: &'a str,
    pub rebuild_required: bool,
    pub apply_status: ConfigUiApplyStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiTomlDocumentRows {
    pub list_table: ConfigUiListTable,
    pub fields: Vec<ConfigUiField>,
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
        section_label: spec.section_label,
        list_cells: spec.list_cells,
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
            .unwrap_or_else(|| NO_CONFIG_DEFAULT_VALUE_LABEL.to_string()),
        state,
        description: spec.description,
        allowed_values: spec.allowed_values,
        validation: spec.validation,
        rebuild_required: spec.rebuild_required,
        apply_status: spec.apply_status,
        edit_behavior: spec.edit_behavior,
    }
}

pub fn build_string_list_choice_field(
    spec: ConfigUiStringListChoiceSpec,
) -> Result<ConfigUiField, String> {
    if spec.allowed_values.is_empty() {
        return Err(format!(
            "{} must define at least one allowed string-list value.",
            spec.path
        ));
    }
    for values in [spec.current.as_deref(), spec.default.as_deref()]
        .into_iter()
        .flatten()
    {
        for value in values {
            validate_string_choice_value(&spec.path, value, &spec.allowed_values)?;
        }
    }

    let current = spec.current.as_deref().map(string_list_values_json);
    let default = spec.default.as_deref().map(string_list_values_json);
    Ok(build_config_ui_field(ConfigUiFieldRowSpec {
        source_id: &spec.source_id,
        path: &spec.path,
        display_label: spec.display_label,
        section_label: spec.section_label,
        list_cells: spec.list_cells,
        tab: &spec.tab,
        kind: "string_list",
        current: current.as_ref(),
        default: default.as_ref(),
        description: spec.description,
        allowed_values: spec.allowed_values,
        validation: spec.validation,
        rebuild_required: spec.rebuild_required,
        apply_status: spec.apply_status,
        has_blocking_diagnostic: spec.has_blocking_diagnostic,
        edit_behavior: spec.edit_behavior,
    }))
}

fn toml_document_list_table() -> ConfigUiListTable {
    ConfigUiListTable {
        columns: [
            ("table", 24),
            ("key", 28),
            ("type", 12),
            ("state", 10),
            ("value", 28),
            ("default", 20),
        ]
        .into_iter()
        .map(|(title, width)| ConfigUiListColumn {
            title: title.to_string(),
            width,
        })
        .collect(),
    }
}

pub fn build_toml_document_fields(
    spec: ConfigUiTomlDocumentSpec<'_>,
) -> Result<ConfigUiTomlDocumentRows, String> {
    let current = parse_toml_document(spec.current_toml, "current TOML document")?;
    let current_edit = parse_toml_edit_document(spec.current_toml, "current TOML document")?;
    let default = spec
        .default_toml
        .map(|raw| parse_toml_document(raw, "default TOML document"))
        .transpose()?;
    let mut entries = BTreeMap::<String, TomlDocumentEntry>::new();
    collect_toml_document_entries(
        &current,
        Vec::new(),
        TomlDocumentSide::Current,
        &mut entries,
    );
    if let Some(default) = &default {
        collect_toml_document_entries(default, Vec::new(), TomlDocumentSide::Default, &mut entries);
    }

    let fields = entries
        .into_values()
        .map(|entry| toml_document_entry_field(&spec, &current_edit, entry))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ConfigUiTomlDocumentRows {
        list_table: toml_document_list_table(),
        fields,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigUiEditBehavior {
    Default,
    FriendlyStringList,
    OrderedStringList,
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

    let field_kind = if kind == "array"
        && schema
            .get("items")
            .and_then(|items| items.get("type"))
            .and_then(JsonValue::as_str)
            == Some("string")
    {
        "string_list"
    } else {
        kind
    };
    out.push(schema_field(schema, path, field_kind));
}

fn schema_field(schema: &JsonValue, path: &str, kind: &str) -> ConfigUiSchemaField {
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
        kind: kind.to_string(),
        allowed_values,
    }
}

fn schema_type(schema: &JsonValue) -> &str {
    schema
        .get("type")
        .and_then(JsonValue::as_str)
        .unwrap_or("unknown")
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

#[derive(Debug, Clone)]
struct TomlDocumentEntry {
    segments: Vec<String>,
    current: Option<TomlValue>,
    default: Option<TomlValue>,
}

#[derive(Debug, Clone, Copy)]
enum TomlDocumentSide {
    Current,
    Default,
}

fn parse_toml_document(raw: &str, label: &str) -> Result<TomlValue, String> {
    let table = raw
        .parse::<toml::Table>()
        .map_err(|source| format!("{label} is invalid TOML: {source}"))?;
    Ok(TomlValue::Table(table))
}

fn parse_toml_edit_document(raw: &str, label: &str) -> Result<TomlEditDocument, String> {
    raw.parse::<TomlEditDocument>()
        .map_err(|source| format!("{label} is invalid TOML: {source}"))
}

fn collect_toml_document_entries(
    value: &TomlValue,
    segments: Vec<String>,
    side: TomlDocumentSide,
    entries: &mut BTreeMap<String, TomlDocumentEntry>,
) {
    if !segments.is_empty() {
        let display_path = toml_document_display_path(&segments);
        let entry = entries
            .entry(display_path)
            .or_insert_with(|| TomlDocumentEntry {
                segments: segments.clone(),
                current: None,
                default: None,
            });
        match side {
            TomlDocumentSide::Current => entry.current = Some(value.clone()),
            TomlDocumentSide::Default => entry.default = Some(value.clone()),
        }
    }

    let TomlValue::Table(table) = value else {
        return;
    };
    for (key, child) in table {
        let mut child_segments = segments.clone();
        child_segments.push(key.clone());
        collect_toml_document_entries(child, child_segments, side, entries);
    }
}

fn toml_document_entry_field(
    spec: &ConfigUiTomlDocumentSpec<'_>,
    current: &TomlEditDocument,
    entry: TomlDocumentEntry,
) -> Result<ConfigUiField, String> {
    let display_path = toml_document_display_path(&entry.segments);
    let patch_path = toml_document_patch_path(current.as_table(), &entry.segments);
    let field_path = patch_path.as_deref().unwrap_or(&display_path).to_string();
    let effective = entry.current.as_ref().or(entry.default.as_ref());
    let kind = effective.map_or("unknown", toml_document_field_kind);
    let type_label = effective.map_or("unknown", toml_document_type_label);
    let editable = patch_path.is_some() && effective.is_some_and(toml_document_value_is_editable);
    let state = if entry.current.is_some() {
        ConfigUiValueState::Explicit
    } else if entry.default.is_some() {
        ConfigUiValueState::Defaulted
    } else {
        ConfigUiValueState::Unset
    };
    let current_value = effective
        .map(toml_document_render_value)
        .unwrap_or_else(|| "not set".to_string());
    let edit_value = if editable {
        effective
            .map(toml_value_to_json)
            .transpose()?
            .map(|value| render_json_edit_value(&value))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let default_value = match (&entry.default, editable) {
        (Some(value), true) => toml_document_render_value(value),
        _ => NO_CONFIG_DEFAULT_VALUE_LABEL.to_string(),
    };
    let default_cell = entry
        .default
        .as_ref()
        .map(toml_document_render_value)
        .unwrap_or_else(|| "-".to_string());
    let validation = if !editable || spec.validation.trim().is_empty() {
        toml_document_validation_label(type_label, editable).to_string()
    } else {
        spec.validation.to_string()
    };

    Ok(ConfigUiField {
        source_id: spec.source_id.to_string(),
        path: field_path,
        display_label: display_path.clone(),
        section_label: spec.section_label.to_string(),
        list_cells: vec![
            toml_document_parent_label(&entry.segments),
            toml_document_key_label(&entry.segments, type_label),
            type_label.to_string(),
            state_label_text(state).to_string(),
            current_value.clone(),
            default_cell,
        ],
        tab: spec.tab.to_string(),
        kind: kind.to_string(),
        current_value,
        edit_value,
        default_value,
        state,
        description: toml_document_description(
            &display_path,
            type_label,
            editable,
            patch_path.is_some(),
        ),
        allowed_values: Vec::new(),
        validation,
        rebuild_required: spec.rebuild_required,
        apply_status: spec.apply_status.clone(),
        edit_behavior: if editable {
            ConfigUiEditBehavior::Default
        } else {
            ConfigUiEditBehavior::StructuredOnly {
                notice: toml_document_read_only_notice(patch_path.is_some()).to_string(),
            }
        },
    })
}

fn toml_document_patch_path(root: &TomlEditTable, segments: &[String]) -> Option<String> {
    if segments.is_empty()
        || !segments
            .iter()
            .all(|segment| is_toml_document_bare_key(segment))
    {
        return None;
    }
    toml_document_parent_path_is_patchable(root, segments).then(|| segments.join("."))
}

fn is_toml_document_bare_key(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn toml_document_parent_path_is_patchable(root: &TomlEditTable, segments: &[String]) -> bool {
    let mut current = root;
    for segment in &segments[..segments.len().saturating_sub(1)] {
        let Some(next) = current.get(segment) else {
            return true;
        };
        let Some(next) = next.as_table() else {
            return false;
        };
        current = next;
    }
    true
}

fn toml_document_display_path(segments: &[String]) -> String {
    segments
        .iter()
        .map(|segment| {
            if is_toml_document_bare_key(segment) {
                segment.clone()
            } else {
                serde_json::to_string(segment).expect("strings serialize")
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn toml_document_parent_label(segments: &[String]) -> String {
    if segments.len() <= 1 {
        String::new()
    } else {
        toml_document_display_path(&segments[..segments.len() - 1])
    }
}

fn toml_document_key_label(segments: &[String], type_label: &str) -> String {
    let key = segments
        .last()
        .map(|segment| toml_document_display_path(std::slice::from_ref(segment)))
        .unwrap_or_default();
    if type_label == "table" {
        format!("[{key}]")
    } else {
        key
    }
}

fn toml_document_value_is_editable(value: &TomlValue) -> bool {
    matches!(
        value,
        TomlValue::String(_) | TomlValue::Integer(_) | TomlValue::Float(_) | TomlValue::Boolean(_)
    ) || toml_document_string_list(value)
}

fn toml_document_string_list(value: &TomlValue) -> bool {
    matches!(value, TomlValue::Array(values) if values.iter().all(|value| matches!(value, TomlValue::String(_))))
}

fn toml_document_field_kind(value: &TomlValue) -> &'static str {
    match value {
        TomlValue::String(_) => "string",
        TomlValue::Integer(_) => "int",
        TomlValue::Float(_) => "float",
        TomlValue::Boolean(_) => "bool",
        TomlValue::Array(_) if toml_document_string_list(value) => "string_list",
        TomlValue::Array(_) => "array",
        TomlValue::Table(_) => "object",
        TomlValue::Datetime(_) => "datetime",
    }
}

fn toml_document_type_label(value: &TomlValue) -> &'static str {
    match value {
        TomlValue::String(_) => "string",
        TomlValue::Integer(_) => "integer",
        TomlValue::Float(_) => "float",
        TomlValue::Boolean(_) => "boolean",
        TomlValue::Array(_) if toml_document_string_list(value) => "string list",
        TomlValue::Array(_) => "array",
        TomlValue::Table(_) => "table",
        TomlValue::Datetime(_) => "datetime",
    }
}

fn toml_document_render_value(value: &TomlValue) -> String {
    match value {
        TomlValue::Table(table) => format!("{{{} keys}}", table.len()),
        TomlValue::Array(values)
            if !values.is_empty()
                && values
                    .iter()
                    .all(|value| matches!(value, TomlValue::Table(_))) =>
        {
            format!("[{} tables]", values.len())
        }
        TomlValue::Array(values) if !toml_document_string_list(value) => {
            format!("[{} items]", values.len())
        }
        TomlValue::Datetime(value) => value.to_string(),
        _ => toml_value_to_json(value)
            .map(|value| render_json_value(&value))
            .unwrap_or_else(|source| format!("unsupported: {source}")),
    }
}

fn toml_document_validation_label(type_label: &str, editable: bool) -> &'static str {
    if editable {
        match type_label {
            "string list" => "TOML array of strings",
            "boolean" => "TOML boolean",
            "integer" => "TOML integer",
            "float" => "TOML float",
            "string" => "TOML string",
            _ => "TOML value",
        }
    } else {
        "read-only in generic TOML document view"
    }
}

fn toml_document_description(
    display_path: &str,
    type_label: &str,
    editable: bool,
    path_is_patchable: bool,
) -> String {
    if editable {
        return format!(
            "Generic TOML {type_label} value at {display_path}. Hosts validate, write, reload, and apply this source."
        );
    }
    if path_is_patchable {
        format!(
            "Generic TOML {type_label} value at {display_path}. Complex TOML values are shown for inspection; edit the source file for structured changes."
        )
    } else {
        format!(
            "Generic TOML {type_label} value at {display_path}. This path cannot be represented as a safe dotted TOML patch path; edit the source file directly."
        )
    }
}

fn toml_document_read_only_notice(path_is_patchable: bool) -> &'static str {
    if path_is_patchable {
        "Complex TOML values are read-only in this generic view; edit the source file directly."
    } else {
        "This TOML path cannot be edited safely through dotted path patching; edit the source file directly."
    }
}

fn state_label_text(state: ConfigUiValueState) -> &'static str {
    match state {
        ConfigUiValueState::Explicit => "explicit",
        ConfigUiValueState::Defaulted => "default",
        ConfigUiValueState::Unset => "unset",
        ConfigUiValueState::Invalid => "invalid",
    }
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
                    field.section_label.as_str(),
                    field.current_value.as_str(),
                    field.default_value.as_str(),
                    field.description.as_str(),
                ],
            )
        }
        UiRowRef::FileAction(index) => {
            let action = &model.file_actions[index];
            let path = action.path.to_string_lossy();
            search_matches(
                search,
                [
                    action.source_id.as_str(),
                    action.action_id.as_str(),
                    action.label.as_str(),
                    action.description.as_str(),
                    path.as_ref(),
                    action.disabled_reason.as_deref().unwrap_or_default(),
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

pub fn string_list_values_from_json(
    path: &str,
    value: &JsonValue,
    allowed_values: &[String],
) -> Result<Vec<String>, String> {
    let array = value
        .as_array()
        .ok_or_else(|| format!("{path} must be a JSON string array."))?;
    let mut strings = Vec::with_capacity(array.len());
    for value in array {
        let Some(value) = value.as_str() else {
            return Err(format!("{path} must contain only strings."));
        };
        validate_string_choice_value(path, value, allowed_values)?;
        strings.push(value.to_string());
    }
    Ok(strings)
}

pub(crate) fn validate_string_choice_value(
    path: &str,
    value: &str,
    allowed_values: &[String],
) -> Result<(), String> {
    if allowed_values.is_empty() || allowed_values.iter().any(|allowed| allowed == value) {
        return Ok(());
    }
    Err(format!(
        "{path} must be one of: {}.",
        allowed_values.join(", ")
    ))
}

fn string_list_values_json(values: &[String]) -> JsonValue {
    JsonValue::Array(values.iter().cloned().map(JsonValue::String).collect())
}

pub fn render_json_value(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::Number(value) => value.to_string(),
        JsonValue::String(value) => format!("{value:?}"),
        JsonValue::Array(values) if values.len() <= 4 => {
            serde_json::to_string(values).expect("serde_json::Value arrays serialize")
        }
        JsonValue::Array(values) => format!("[{} items]", values.len()),
        JsonValue::Object(object) => format!("{{{} keys}}", object.len()),
    }
}

pub fn render_json_edit_value(value: &JsonValue) -> String {
    serde_json::to_string(value).expect("serde_json::Value serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{apply_status, field, model_with_fields};
    use serde_json::json;

    fn status() -> ConfigUiApplyStatus {
        apply_status("after save", "Host applies this after saving.")
    }

    fn toml_document_rows(
        current_toml: &str,
        default_toml: Option<&str>,
    ) -> ConfigUiTomlDocumentRows {
        build_toml_document_fields(ConfigUiTomlDocumentSpec {
            source_id: "native",
            tab: "native",
            section_label: "",
            current_toml,
            default_toml,
            validation: "",
            rebuild_required: false,
            apply_status: status(),
        })
        .expect("toml document rows")
    }

    fn toml_field<'a>(rows: &'a ConfigUiTomlDocumentRows, path: &str) -> &'a ConfigUiField {
        rows.fields
            .iter()
            .find(|field| field.path == path)
            .unwrap_or_else(|| panic!("missing TOML document field {path}"))
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
            tab_list_tables: BTreeMap::new(),
            fields: Vec::new(),
            file_actions: Vec::new(),
            sidecars: Vec::new(),
            native_config_statuses: Vec::new(),
            diagnostics: Vec::new(),
            theme_switcher: None,
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
            section_label: String::new(),
            list_cells: Vec::new(),
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
        assert!(explicit.has_default_value());

        let defaulted = build_config_ui_field(spec(None, Some(&default), false));
        assert_eq!(defaulted.state, ConfigUiValueState::Defaulted);
        assert_eq!(defaulted.current_value, "\"light\"");
        assert!(defaulted.has_default_value());

        let unset = build_config_ui_field(spec(None, None, false));
        assert_eq!(unset.state, ConfigUiValueState::Unset);
        assert_eq!(unset.current_value, "not set");
        assert_eq!(unset.default_value, NO_CONFIG_DEFAULT_VALUE_LABEL);
        assert!(!unset.has_default_value());

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
            section_label: "Plugins".to_string(),
            list_cells: vec!["plugins".to_string(), "5 enabled".to_string()],
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
        assert_eq!(field.section_label, "Plugins");
        assert_eq!(field.list_cells, vec!["plugins", "5 enabled"]);
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

    // Defends: hosts can build allowed string-list choice fields without hand-assembling JSON row specs.
    #[test]
    fn string_list_choice_helper_builds_ordered_field() {
        let field = build_string_list_choice_field(ConfigUiStringListChoiceSpec {
            source_id: "settings".to_string(),
            path: "widgets.enabled".to_string(),
            display_label: "Enabled widgets".to_string(),
            section_label: "Widgets".to_string(),
            list_cells: vec!["widgets".to_string(), "2 selected".to_string()],
            tab: "widgets".to_string(),
            current: Some(vec!["status".to_string(), "clock".to_string()]),
            default: Some(vec!["clock".to_string()]),
            description: "Enabled widget ids".to_string(),
            allowed_values: vec![
                "clock".to_string(),
                "status".to_string(),
                "mode".to_string(),
            ],
            validation: "known widget ids only".to_string(),
            rebuild_required: true,
            apply_status: status(),
            has_blocking_diagnostic: false,
            edit_behavior: ConfigUiEditBehavior::Default,
        })
        .expect("valid string-list field");

        assert_eq!(field.source_id, "settings");
        assert_eq!(field.path, "widgets.enabled");
        assert_eq!(field.display_label, "Enabled widgets");
        assert_eq!(field.section_label, "Widgets");
        assert_eq!(field.list_cells, vec!["widgets", "2 selected"]);
        assert_eq!(field.tab, "widgets");
        assert_eq!(field.kind, "string_list");
        assert_eq!(field.current_value, r#"["status","clock"]"#);
        assert_eq!(field.edit_value, r#"["status","clock"]"#);
        assert_eq!(field.default_value, r#"["clock"]"#);
        assert_eq!(field.state, ConfigUiValueState::Explicit);
        assert_eq!(field.allowed_values, vec!["clock", "status", "mode"]);
        assert!(field.rebuild_required);
    }

    // Defends: string-list extraction keeps host order and reports generic validation errors.
    #[test]
    fn string_list_values_from_json_preserves_order_and_rejects_invalid_values() {
        let allowed = vec!["clock".to_string(), "status".to_string()];

        assert_eq!(
            string_list_values_from_json("widgets.enabled", &json!(["status", "clock"]), &allowed)
                .expect("valid list"),
            vec!["status", "clock"]
        );
        assert!(
            string_list_values_from_json("widgets.enabled", &json!("status"), &allowed)
                .expect_err("not an array")
                .contains("must be a JSON string array")
        );
        assert!(
            string_list_values_from_json("widgets.enabled", &json!(["status", 1]), &allowed)
                .expect_err("non-string")
                .contains("must contain only strings")
        );
        assert!(
            string_list_values_from_json("widgets.enabled", &json!(["unknown"]), &allowed)
                .expect_err("unknown value")
                .contains("must be one of: clock, status")
        );
    }

    // Defends: the choice helper fails fast when no choice ids are available.
    #[test]
    fn string_list_choice_helper_requires_allowed_values() {
        let error = build_string_list_choice_field(ConfigUiStringListChoiceSpec {
            source_id: "settings".to_string(),
            path: "widgets.enabled".to_string(),
            display_label: String::new(),
            section_label: String::new(),
            list_cells: Vec::new(),
            tab: "widgets".to_string(),
            current: None,
            default: None,
            description: String::new(),
            allowed_values: Vec::new(),
            validation: String::new(),
            rebuild_required: false,
            apply_status: status(),
            has_blocking_diagnostic: false,
            edit_behavior: ConfigUiEditBehavior::Default,
        })
        .expect_err("missing choices");

        assert!(error.contains("must define at least one allowed string-list value"));
    }

    // Defends: arbitrary TOML document rows are deterministic and expose table grouping without host-declared fields.
    #[test]
    fn toml_document_helper_builds_stable_grouped_rows() {
        let rows = toml_document_rows(
            r#"
theme = "dark"

[editor]
line-number = "relative"
plugins = ["git", "theme"]
rulers = [80, 100]

[editor.cursor-shape]
insert = "bar"
"#,
            Some(
                r#"
theme = "light"

[editor]
line-number = "absolute"
plugins = ["git"]
true-color = true

[editor.cursor-shape]
normal = "block"
"#,
            ),
        );

        assert_eq!(
            rows.fields
                .iter()
                .map(|field| field.path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "editor",
                "editor.cursor-shape",
                "editor.cursor-shape.insert",
                "editor.cursor-shape.normal",
                "editor.line-number",
                "editor.plugins",
                "editor.rulers",
                "editor.true-color",
                "theme",
            ]
        );
        assert_eq!(
            rows.list_table
                .columns
                .iter()
                .map(|column| column.title.as_str())
                .collect::<Vec<_>>(),
            vec!["table", "key", "type", "state", "value", "default"]
        );

        let table = &rows.fields[0];
        assert_eq!(
            table.list_cells,
            vec!["", "[editor]", "table", "explicit", "{4 keys}", "{4 keys}"]
        );
        assert_eq!(
            table.edit_behavior,
            ConfigUiEditBehavior::StructuredOnly {
                notice: "Complex TOML values are read-only in this generic view; edit the source file directly.".to_string(),
            }
        );

        let line_number = toml_field(&rows, "editor.line-number");
        assert_eq!(line_number.kind, "string");
        assert_eq!(
            line_number.list_cells,
            vec![
                "editor",
                "line-number",
                "string",
                "explicit",
                "\"relative\"",
                "\"absolute\""
            ]
        );
        assert_eq!(line_number.current_value, "\"relative\"");
        assert_eq!(line_number.edit_value, "\"relative\"");
        assert_eq!(line_number.default_value, "\"absolute\"");
        assert_eq!(line_number.edit_behavior, ConfigUiEditBehavior::Default);
    }

    // Defends: default TOML documents can supply defaulted rows without a host schema.
    #[test]
    fn toml_document_helper_marks_defaulted_and_unsupported_rows() {
        let rows = toml_document_rows(
            r#"
[editor]
rulers = [80, 100]
"#,
            Some(
                r#"
[editor]
line-number = "relative"
rulers = [80]
"#,
            ),
        );

        let line_number = toml_field(&rows, "editor.line-number");
        assert_eq!(line_number.state, ConfigUiValueState::Defaulted);
        assert_eq!(line_number.current_value, "\"relative\"");
        assert_eq!(line_number.default_value, "\"relative\"");
        assert_eq!(
            line_number.list_cells,
            vec![
                "editor",
                "line-number",
                "string",
                "default",
                "\"relative\"",
                "\"relative\""
            ]
        );

        let rulers = toml_field(&rows, "editor.rulers");
        assert_eq!(rulers.kind, "array");
        assert_eq!(rulers.current_value, "[2 items]");
        assert_eq!(rulers.default_value, NO_CONFIG_DEFAULT_VALUE_LABEL);
        assert_eq!(rulers.list_cells[5], "[1 items]");
        assert_eq!(rulers.validation, "read-only in generic TOML document view");
        assert!(matches!(
            rulers.edit_behavior,
            ConfigUiEditBehavior::StructuredOnly { .. }
        ));
    }

    // Defends: host validation text applies to editable TOML document rows without hiding read-only complex-row limits.
    #[test]
    fn toml_document_helper_preserves_read_only_validation_for_complex_rows() {
        let rows = build_toml_document_fields(ConfigUiTomlDocumentSpec {
            source_id: "native",
            tab: "native",
            section_label: "Editor",
            current_toml: r#"
[editor]
line-number = "relative"
rulers = [80, 100]
"#,
            default_toml: None,
            validation: "host validates before writing",
            rebuild_required: false,
            apply_status: status(),
        })
        .expect("toml document rows");

        let line_number = toml_field(&rows, "editor.line-number");
        assert_eq!(line_number.validation, "host validates before writing");

        let rulers = toml_field(&rows, "editor.rulers");
        assert_eq!(rulers.validation, "read-only in generic TOML document view");
    }

    // Defends: quoted or otherwise non-dotted TOML paths remain inspectable instead of becoming unsafe edit routes.
    #[test]
    fn toml_document_helper_renders_unpatchable_paths_as_read_only() {
        let rows = toml_document_rows(
            r#"
"weird.key" = "value"
"#,
            Some(
                r#"
"weird.key" = "default"
"#,
            ),
        );
        let field = rows.fields.first().expect("quoted key");

        assert_eq!(field.path, "\"weird.key\"");
        assert_eq!(field.display_label, "\"weird.key\"");
        assert_eq!(field.default_value, NO_CONFIG_DEFAULT_VALUE_LABEL);
        assert_eq!(
            field.list_cells,
            vec![
                "",
                "\"weird.key\"",
                "string",
                "explicit",
                "\"value\"",
                "\"default\""
            ]
        );
        assert!(matches!(
            field.edit_behavior,
            ConfigUiEditBehavior::StructuredOnly { .. }
        ));
    }

    // Defends: inline table children are not advertised as editable when the TOML patcher cannot patch through the parent.
    #[test]
    fn toml_document_helper_keeps_inline_table_children_read_only() {
        let rows = toml_document_rows(
            r#"
package = { name = "ratconfig", enabled = true }
"#,
            None,
        );

        let name = toml_field(&rows, "package.name");
        assert_eq!(name.kind, "string");
        assert_eq!(name.current_value, "\"ratconfig\"");
        assert_eq!(name.edit_value, "");
        assert_eq!(name.default_value, NO_CONFIG_DEFAULT_VALUE_LABEL);
        assert_eq!(name.validation, "read-only in generic TOML document view");
        assert_eq!(
            name.edit_behavior,
            ConfigUiEditBehavior::StructuredOnly {
                notice: "This TOML path cannot be edited safely through dotted path patching; edit the source file directly.".to_string(),
            }
        );
    }

    // Defends: simple arbitrary TOML string lists reuse the existing editable string-list field semantics.
    #[test]
    fn toml_document_helper_builds_editable_simple_string_lists() {
        let rows = toml_document_rows(
            r#"
[shell]
plugins = ["git", "status"]
"#,
            None,
        );
        let plugins = toml_field(&rows, "shell.plugins");

        assert_eq!(plugins.kind, "string_list");
        assert_eq!(plugins.current_value, r#"["git","status"]"#);
        assert_eq!(plugins.edit_value, r#"["git","status"]"#);
        assert_eq!(plugins.edit_behavior, ConfigUiEditBehavior::Default);
    }

    // Defends: host section labels remain presentation metadata while search preserves real field order and identity.
    #[test]
    fn section_labels_are_searchable_without_changing_visible_rows() {
        let mut runtime_enabled = field("runtime.enabled", "bool", "true", &[]);
        runtime_enabled.section_label = "Runtime".to_string();
        let mut runtime_shell = field("runtime.shell", "string", r#""nu""#, &[]);
        runtime_shell.section_label = "Runtime".to_string();
        let mut theme = field("ui.theme", "string", r#""dark""#, &[]);
        theme.section_label = "Appearance".to_string();
        let model = model_with_fields(vec![runtime_enabled, runtime_shell, theme]);

        assert_eq!(
            visible_rows_for_tab_search(&model, 0, ""),
            vec![UiRowRef::Field(0), UiRowRef::Field(1), UiRowRef::Field(2)]
        );
        assert_eq!(
            visible_rows_for_tab_search(&model, 0, "runtime"),
            vec![UiRowRef::Field(0), UiRowRef::Field(1)]
        );
        assert_eq!(
            visible_rows_for_tab_search(&model, 0, "appearance"),
            vec![UiRowRef::Field(2)]
        );
    }

    // Defends: host-owned file action rows join tab/search rows without becoming scalar settings.
    #[test]
    fn file_action_rows_are_visible_and_searchable_by_host_metadata() {
        fn file_action(tab: &str, label: &str) -> ConfigUiFileAction {
            ConfigUiFileAction {
                source_id: "native".to_string(),
                action_id: format!("open_{tab}"),
                tab: tab.to_string(),
                label: label.to_string(),
                description: format!("Open {label}"),
                path: PathBuf::from(format!("/home/alex/.config/acme/{tab}.toml")),
                exists: true,
                read_only: false,
                create_if_missing: false,
                disabled_reason: None,
            }
        }

        let model = ConfigUiModel {
            active_config_path: PathBuf::from("/tmp/acme/settings.jsonc"),
            cursor_config_path: PathBuf::new(),
            default_cursor_config_path: PathBuf::new(),
            active_config_exists: true,
            config_owner: ConfigUiPathOwner::User,
            config_read_only: false,
            sources: Vec::new(),
            tabs: vec!["general".to_string(), "advanced".to_string()],
            tab_list_tables: BTreeMap::new(),
            fields: vec![build_config_ui_field(spec(None, None, false))],
            file_actions: vec![
                file_action("general", "Prompt config"),
                file_action("advanced", "Native logs"),
            ],
            sidecars: Vec::new(),
            native_config_statuses: Vec::new(),
            diagnostics: Vec::new(),
            theme_switcher: None,
        };

        assert_eq!(
            visible_rows_for_tab_search(&model, 0, ""),
            vec![UiRowRef::Field(0), UiRowRef::FileAction(0)]
        );
        assert_eq!(
            visible_rows_for_tab_search(&model, 0, "prompt"),
            vec![UiRowRef::FileAction(0)]
        );
        assert_eq!(
            visible_rows_for_tab_search(&model, 1, ""),
            vec![UiRowRef::FileAction(1)]
        );
        assert_eq!(
            visible_rows_for_tab_search(&model, 1, "logs"),
            vec![UiRowRef::FileAction(1)]
        );
    }
}
