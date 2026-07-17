// Test lane: default

use crate::patch::get_dotted_json_path;
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use toml::Value as TomlValue;
use toml_edit::{DocumentMut as TomlEditDocument, Table as TomlEditTable};

pub const DEFAULT_CONFIG_SOURCE_ID: &str = "config";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiModel {
    pub sources: Vec<ConfigUiSource>,
    pub tabs: Vec<String>,
    /// Optional host-selected tab for generic diagnostics and status rows.
    pub operational_tab: Option<String>,
    pub tab_list_tables: BTreeMap<String, ConfigUiListTable>,
    pub fields: Vec<ConfigUiField>,
    /// Host-recommended fields, matched by source id and field path.
    ///
    /// `None` recommends every field, so [`crate::ConfigUiApp`] starts in
    /// [`ConfigUiSettingsView::All`] because there is no Overview/All distinction. `Some`
    /// recommends exactly its identities; explicit, invalid, externally managed, and
    /// fields named by exact diagnostics remain visible in Overview when omitted.
    pub recommended_fields: Option<Vec<ConfigUiFieldId>>,
    pub file_actions: Vec<ConfigUiFileAction>,
    pub sidecars: Vec<ConfigUiSidecar>,
    pub native_config_statuses: Vec<ConfigUiNativeStatus>,
    pub diagnostics: Vec<ConfigUiDiagnostic>,
    pub theme_switcher: Option<ConfigUiThemeSwitcher>,
}

/// Stable field identity used by recommendations and field-scoped diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConfigUiFieldId {
    /// Host-supplied config source id.
    pub source_id: String,
    /// Stable field path within the source.
    pub path: String,
}

impl ConfigUiFieldId {
    /// Creates a source/path field identity.
    pub fn new(source_id: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            source_id: source_id.into(),
            path: path.into(),
        }
    }
}

/// Field visibility used outside active search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigUiSettingsView {
    /// Host recommendations plus fields requiring attention.
    Overview,
    /// The complete field inventory supplied by the host.
    All,
}

/// Overview and total field counts for one tab.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ConfigUiFieldCounts {
    /// Recommended, explicit, invalid, externally managed, or named by an exact diagnostic.
    pub(crate) overview: usize,
    /// Every host-supplied field on the tab.
    pub(crate) total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiSource {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
    pub exists: bool,
    pub owner_label: Option<String>,
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
    pub field: ConfigUiFieldId,
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

/// Returns rows from the complete inventory for the selected tab and search.
///
/// Use [`visible_rows_for_tab_search_in_view`] when the caller needs Overview filtering.
#[cfg(any(feature = "ui", test))]
pub(crate) fn visible_rows_for_tab_search(
    model: &ConfigUiModel,
    selected_tab: usize,
    search: &str,
) -> Vec<UiRowRef> {
    visible_rows_for_tab_search_in_view(model, selected_tab, search, ConfigUiSettingsView::All)
}

/// Returns rows for one settings view.
///
/// A non-empty search spans the complete host inventory even when `view` is
/// [`ConfigUiSettingsView::Overview`]. Clearing the search restores the caller's chosen view.
/// Host-routed operational rows and file actions remain available in both views.
pub(crate) fn visible_rows_for_tab_search_in_view(
    model: &ConfigUiModel,
    selected_tab: usize,
    search: &str,
    view: ConfigUiSettingsView,
) -> Vec<UiRowRef> {
    let tab = model.tabs[selected_tab].as_str();
    let search = search.to_ascii_lowercase();
    if model.operational_tab.as_deref() == Some(tab) {
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
        .filter(|index| {
            view == ConfigUiSettingsView::All
                || !search.is_empty()
                || field_is_visible_in_overview(model, &model.fields[*index])
        })
        .map(UiRowRef::Field)
        .chain(file_action_rows_for_tab(model, tab))
        .filter(|row| row_matches_search(model, *row, &search))
        .collect()
}

/// Counts Overview and total fields on the selected tab.
///
/// Overview includes recommendations and fields requiring attention. The host-selected
/// operational tab has zero field counts.
pub(crate) fn field_counts_for_tab(
    model: &ConfigUiModel,
    selected_tab: usize,
) -> ConfigUiFieldCounts {
    let tab = model.tabs[selected_tab].as_str();
    if model.operational_tab.as_deref() == Some(tab) {
        return ConfigUiFieldCounts::default();
    }
    let fields = model.fields.iter().filter(|field| field.tab == tab);
    fields.fold(ConfigUiFieldCounts::default(), |mut counts, field| {
        counts.total += 1;
        counts.overview += usize::from(field_is_visible_in_overview(model, field));
        counts
    })
}

fn field_is_visible_in_overview(model: &ConfigUiModel, field: &ConfigUiField) -> bool {
    matches!(
        snapshot_field_state(field),
        ConfigUiFieldState::Explicit | ConfigUiFieldState::Invalid
    ) || field.snapshot.external_manager.is_some()
        || model.diagnostics.iter().any(|diagnostic| {
            matches!(
                &diagnostic.scope,
                ConfigUiDiagnosticScope::Field(identity) if field.matches_id(identity)
            )
        })
        || model.recommended_fields.as_ref().is_none_or(|recommended| {
            recommended
                .iter()
                .any(|identity| field.matches_id(identity))
        })
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

pub(crate) fn config_ui_theme_for_model(
    model: &ConfigUiModel,
    fallback: ConfigUiTheme,
) -> ConfigUiTheme {
    match &model.theme_switcher {
        None => ConfigUiTheme::Dark,
        Some(switcher) => switcher.resolve(&model.fields).unwrap_or(fallback),
    }
}

impl ConfigUiThemeSwitcher {
    fn resolve(&self, fields: &[ConfigUiField]) -> Option<ConfigUiTheme> {
        let field = fields.iter().find(|field| field.matches_id(&self.field))?;
        self.theme_for_value(&field.snapshot.effective.as_ref()?.value)
    }

    pub fn theme_for_value(&self, value: &JsonValue) -> Option<ConfigUiTheme> {
        self.mappings
            .iter()
            .find(|mapping| mapping.value == *value)
            .map(|mapping| mapping.theme)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigUiFieldState {
    Explicit,
    Inherited,
    Absent,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiFieldSnapshot {
    pub intent: ConfigUiOverride,
    pub effective: Option<ConfigUiResolvedValue>,
    pub baseline: Option<ConfigUiResolvedValue>,
    pub external_manager: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigUiOverride {
    Absent,
    Explicit(JsonValue),
    Invalid { input: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiResolvedValue {
    pub value: JsonValue,
    pub origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiChoice {
    pub value: JsonValue,
    pub label: Option<String>,
}

impl ConfigUiChoice {
    pub fn new(value: JsonValue) -> Self {
        Self { value, label: None }
    }

    pub(crate) fn display_label(&self) -> String {
        self.label.clone().unwrap_or_else(|| match &self.value {
            JsonValue::String(value) => value.clone(),
            value => render_json_edit_value(value),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigUiTextEncoding {
    String,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigUiCapability {
    ReadOnly {
        reason: String,
        file_action_id: Option<String>,
    },
    FreeText {
        encoding: ConfigUiTextEncoding,
    },
    Toggle {
        off: ConfigUiChoice,
        on: ConfigUiChoice,
    },
    Choice {
        choices: Vec<ConfigUiChoice>,
    },
    MultiChoice {
        choices: Vec<ConfigUiChoice>,
        ordered: bool,
    },
}

impl ConfigUiResolvedValue {
    pub fn new(value: JsonValue) -> Self {
        Self {
            value,
            origin: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiField {
    pub source_id: String,
    pub path: String,
    pub display_label: String,
    pub section_label: String,
    pub list_cells: Vec<String>,
    pub tab: String,
    /// Optional display-only type text. This never authorizes editing.
    pub type_label: Option<String>,
    pub snapshot: ConfigUiFieldSnapshot,
    pub description: String,
    pub validation: String,
    pub rebuild_required: bool,
    pub apply_status: ConfigUiApplyStatus,
    pub capability: ConfigUiCapability,
    pub can_unset: bool,
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
    pub fn id(&self) -> ConfigUiFieldId {
        ConfigUiFieldId::new(&self.source_id, &self.path)
    }

    pub(crate) fn matches_id(&self, identity: &ConfigUiFieldId) -> bool {
        self.source_id == identity.source_id && self.path == identity.path
    }

    pub fn has_baseline_value(&self) -> bool {
        self.snapshot.baseline.is_some()
    }
}

pub(crate) const UNSET_CONFIG_VALUE_LABEL: &str = "not set";

pub(crate) fn field_current_json_value(field: &ConfigUiField) -> Option<&JsonValue> {
    match (&field.snapshot.intent, &field.snapshot.effective) {
        (ConfigUiOverride::Invalid { .. }, _) => None,
        (_, Some(resolved)) => Some(&resolved.value),
        (ConfigUiOverride::Explicit(value), None) => Some(value),
        (ConfigUiOverride::Absent, None) => None,
    }
}

pub(crate) fn field_current_value(field: &ConfigUiField) -> String {
    match &field.snapshot.intent {
        ConfigUiOverride::Invalid { input } => input.clone(),
        ConfigUiOverride::Explicit(_) | ConfigUiOverride::Absent => field_current_json_value(field)
            .map(|value| render_field_value(field, value))
            .unwrap_or_else(|| UNSET_CONFIG_VALUE_LABEL.to_string()),
    }
}

pub(crate) fn field_baseline_value(field: &ConfigUiField) -> Option<String> {
    field
        .snapshot
        .baseline
        .as_ref()
        .map(|resolved| render_field_edit_value(field, &resolved.value))
}

fn render_field_value(field: &ConfigUiField, value: &JsonValue) -> String {
    match value {
        JsonValue::String(value) if field.type_label.as_deref() != Some("string") => value.clone(),
        _ => render_json_value(value),
    }
}

fn render_field_edit_value(field: &ConfigUiField, value: &JsonValue) -> String {
    match value {
        JsonValue::String(value) if field.type_label.as_deref() != Some("string") => value.clone(),
        _ => render_json_edit_value(value),
    }
}

#[derive(Debug, Clone)]
pub struct ConfigUiFieldSpec {
    pub source_id: String,
    pub path: String,
    pub display_label: String,
    pub section_label: String,
    pub list_cells: Vec<String>,
    pub tab: String,
    pub description: String,
    pub validation: String,
    pub rebuild_required: bool,
    pub apply_status: ConfigUiApplyStatus,
    pub capability: ConfigUiCapability,
    pub can_unset: bool,
}

impl ConfigUiFieldSpec {
    pub fn new(
        source_id: impl Into<String>,
        path: impl Into<String>,
        tab: impl Into<String>,
        description: impl Into<String>,
        capability: ConfigUiCapability,
        validation: impl Into<String>,
        apply_status: ConfigUiApplyStatus,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            path: path.into(),
            display_label: String::new(),
            section_label: String::new(),
            list_cells: Vec::new(),
            tab: tab.into(),
            description: description.into(),
            validation: validation.into(),
            rebuild_required: false,
            apply_status,
            capability,
            can_unset: false,
        }
    }

    pub fn build(
        self,
        type_label: impl Into<String>,
        current: Option<&JsonValue>,
        default: Option<&JsonValue>,
    ) -> ConfigUiField {
        let effective = current.or(default).cloned().map(ConfigUiResolvedValue::new);
        let baseline = default.cloned().map(ConfigUiResolvedValue::new);
        let intent = current
            .cloned()
            .map_or(ConfigUiOverride::Absent, ConfigUiOverride::Explicit);
        ConfigUiField {
            source_id: self.source_id,
            path: self.path,
            display_label: self.display_label,
            section_label: self.section_label,
            list_cells: self.list_cells,
            tab: self.tab,
            type_label: Some(type_label.into()),
            snapshot: ConfigUiFieldSnapshot {
                intent,
                effective,
                baseline,
                external_manager: None,
            },
            description: self.description,
            validation: self.validation,
            rebuild_required: self.rebuild_required,
            apply_status: self.apply_status,
            capability: self.capability,
            can_unset: self.can_unset,
        }
    }
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
    pub owner_label: Option<String>,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiDiagnostic {
    pub path: String,
    pub status: String,
    pub headline: String,
    pub blocking: bool,
    /// Host-declared routing for this diagnostic.
    pub scope: ConfigUiDiagnosticScope,
    pub detail_lines: Vec<String>,
}

/// Host-declared routing target for a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigUiDiagnosticScope {
    /// Every field in the model.
    Global,
    /// Every field from one config source.
    Source { source_id: String },
    /// One exact source/path identity.
    Field(ConfigUiFieldId),
}

impl ConfigUiModel {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let tabs = nonblank_unique("tab", self.tabs.iter().map(String::as_str))?;
        if tabs.is_empty() {
            return Err("model must define at least one tab".to_string());
        }
        let operational_tab = self.operational_tab.as_deref();
        if operational_tab.is_some_and(|tab| !tabs.contains(tab)) {
            return Err("operational_tab must name a declared tab".to_string());
        }

        let sources = nonblank_unique(
            "source id",
            self.sources.iter().map(|source| source.id.as_str()),
        )?;
        for source in &self.sources {
            require_nonblank("source label", &source.label)?;
            validate_optional_label("source owner_label", source.owner_label.as_deref())?;
        }

        let mut field_ids = BTreeSet::new();
        for field in &self.fields {
            require_nonblank("field source_id", &field.source_id)?;
            require_nonblank("field path", &field.path)?;
            if !tabs.contains(field.tab.as_str()) {
                return Err(format!(
                    "field {} uses unknown tab {}",
                    field.path, field.tab
                ));
            }
            if operational_tab == Some(field.tab.as_str()) {
                return Err(format!(
                    "operational tab {} cannot contain fields",
                    field.tab
                ));
            }
            let source = self
                .sources
                .iter()
                .find(|source| source.id == field.source_id)
                .ok_or_else(|| {
                    format!(
                        "field {} references missing source {}",
                        field.path, field.source_id
                    )
                })?;
            if !source.exists && !matches!(field.snapshot.intent, ConfigUiOverride::Absent) {
                return Err(format!(
                    "field {} has an explicit or invalid override from absent source {}",
                    field.path, field.source_id
                ));
            }
            if !field_ids.insert(field.id()) {
                return Err(format!(
                    "duplicate field identity ({}, {})",
                    field.source_id, field.path
                ));
            }
            validate_optional_label("field type_label", field.type_label.as_deref())?;
            validate_snapshot(field)?;
            validate_capability(field, source, &self.file_actions)?;
        }

        let mut action_ids = BTreeSet::new();
        for action in &self.file_actions {
            require_nonblank("file action source_id", &action.source_id)?;
            require_nonblank("file action action_id", &action.action_id)?;
            require_nonblank("file action label", &action.label)?;
            validate_optional_label(
                "file action disabled_reason",
                action.disabled_reason.as_deref(),
            )?;
            if !tabs.contains(action.tab.as_str()) {
                return Err(format!(
                    "file action {} uses unknown tab {}",
                    action.action_id, action.tab
                ));
            }
            if !action_ids.insert((action.source_id.as_str(), action.action_id.as_str())) {
                return Err(format!(
                    "duplicate file action identity ({}, {})",
                    action.source_id, action.action_id
                ));
            }
        }

        for tab in self.tab_list_tables.keys() {
            if !tabs.contains(tab.as_str()) {
                return Err(format!("list table uses unknown tab {tab}"));
            }
            if operational_tab == Some(tab.as_str()) {
                return Err(format!("operational tab {tab} cannot use a list table"));
            }
        }

        let has_operational_rows = !self.sidecars.is_empty()
            || !self.native_config_statuses.is_empty()
            || self.diagnostics.iter().any(|diagnostic| {
                matches!(
                    diagnostic.scope,
                    ConfigUiDiagnosticScope::Global | ConfigUiDiagnosticScope::Source { .. }
                )
            });
        if has_operational_rows && operational_tab.is_none() {
            return Err(
                "source/global diagnostics, sidecars, and native statuses require operational_tab"
                    .to_string(),
            );
        }
        for sidecar in &self.sidecars {
            require_nonblank("sidecar name", &sidecar.name)?;
            validate_optional_label("sidecar owner_label", sidecar.owner_label.as_deref())?;
        }
        for diagnostic in &self.diagnostics {
            match &diagnostic.scope {
                ConfigUiDiagnosticScope::Global => {}
                ConfigUiDiagnosticScope::Source { source_id } => {
                    require_nonblank("diagnostic source_id", source_id)?;
                    if !sources.contains(source_id.as_str()) {
                        return Err(format!("diagnostic references missing source {source_id}"));
                    }
                }
                ConfigUiDiagnosticScope::Field(identity) => {
                    require_nonblank("diagnostic field source_id", &identity.source_id)?;
                    require_nonblank("diagnostic field path", &identity.path)?;
                    if !field_ids.contains(identity) {
                        return Err(format!(
                            "diagnostic references missing field ({}, {})",
                            identity.source_id, identity.path
                        ));
                    }
                }
            }
        }

        if let Some(recommended_fields) = &self.recommended_fields {
            let mut unique = BTreeSet::new();
            for identity in recommended_fields {
                if !field_ids.contains(identity) {
                    return Err(format!(
                        "recommended fields reference missing field ({}, {})",
                        identity.source_id, identity.path
                    ));
                }
                if !unique.insert(identity) {
                    return Err(format!(
                        "duplicate recommended field ({}, {})",
                        identity.source_id, identity.path
                    ));
                }
            }
        }

        if let Some(switcher) = &self.theme_switcher {
            if !field_ids.contains(&switcher.field) {
                return Err(format!(
                    "theme switcher references missing field ({}, {})",
                    switcher.field.source_id, switcher.field.path
                ));
            }
            if switcher.mappings.is_empty() {
                return Err("theme switcher must define at least one mapping".to_string());
            }
            if switcher
                .mappings
                .iter()
                .enumerate()
                .any(|(index, mapping)| {
                    switcher.mappings[..index]
                        .iter()
                        .any(|previous| previous.value == mapping.value)
                })
            {
                return Err("theme switcher mapping values must be unique".to_string());
            }
        }
        Ok(())
    }

    #[cfg(any(feature = "ui", test))]
    pub(crate) fn field_state(&self, field: &ConfigUiField) -> ConfigUiFieldState {
        if self.diagnostics.iter().any(|diagnostic| {
            diagnostic.blocking
                && match &diagnostic.scope {
                    ConfigUiDiagnosticScope::Global => true,
                    ConfigUiDiagnosticScope::Source { source_id } => {
                        field.source_id == source_id.as_str()
                    }
                    ConfigUiDiagnosticScope::Field(identity) => field.matches_id(identity),
                }
        }) {
            return ConfigUiFieldState::Invalid;
        }
        snapshot_field_state(field)
    }
}

pub(crate) fn snapshot_field_state(field: &ConfigUiField) -> ConfigUiFieldState {
    match &field.snapshot.intent {
        ConfigUiOverride::Explicit(_) => ConfigUiFieldState::Explicit,
        ConfigUiOverride::Absent if field.snapshot.effective.is_some() => {
            ConfigUiFieldState::Inherited
        }
        ConfigUiOverride::Absent => ConfigUiFieldState::Absent,
        ConfigUiOverride::Invalid { .. } => ConfigUiFieldState::Invalid,
    }
}

#[cfg(test)]
pub(crate) fn set_field_state_for_test(field: &mut ConfigUiField, state: ConfigUiFieldState) {
    let value = field
        .snapshot
        .effective
        .as_ref()
        .or(field.snapshot.baseline.as_ref())
        .map(|resolved| resolved.value.clone())
        .or_else(|| match &field.snapshot.intent {
            ConfigUiOverride::Explicit(value) => Some(value.clone()),
            ConfigUiOverride::Absent | ConfigUiOverride::Invalid { .. } => None,
        })
        .unwrap_or(JsonValue::Null);
    match state {
        ConfigUiFieldState::Explicit => {
            field.snapshot.intent = ConfigUiOverride::Explicit(value.clone());
            field.snapshot.effective = Some(ConfigUiResolvedValue::new(value));
        }
        ConfigUiFieldState::Inherited => {
            let resolved = ConfigUiResolvedValue::new(value);
            field.snapshot.intent = ConfigUiOverride::Absent;
            field.snapshot.effective = Some(resolved.clone());
            field.snapshot.baseline = Some(resolved);
        }
        ConfigUiFieldState::Absent => {
            field.snapshot.intent = ConfigUiOverride::Absent;
            field.snapshot.effective = None;
            field.snapshot.baseline = None;
        }
        ConfigUiFieldState::Invalid => {
            field.snapshot.intent = ConfigUiOverride::Invalid {
                input: render_field_edit_value(field, &value),
            };
        }
    }
}

fn nonblank_unique<'a>(
    label: &str,
    values: impl IntoIterator<Item = &'a str>,
) -> Result<BTreeSet<&'a str>, String> {
    let mut unique = BTreeSet::new();
    for value in values {
        require_nonblank(label, value)?;
        if !unique.insert(value) {
            return Err(format!("duplicate {label} {value}"));
        }
    }
    Ok(unique)
}

fn require_nonblank(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} must not be blank"))
    } else {
        Ok(())
    }
}

fn validate_optional_label(label: &str, value: Option<&str>) -> Result<(), String> {
    if let Some(value) = value {
        require_nonblank(label, value)?;
    }
    Ok(())
}

fn validate_snapshot(field: &ConfigUiField) -> Result<(), String> {
    validate_optional_label(
        "field external_manager",
        field.snapshot.external_manager.as_deref(),
    )?;
    for resolved in [
        field.snapshot.effective.as_ref(),
        field.snapshot.baseline.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        validate_optional_label("resolved value origin", resolved.origin.as_deref())?;
    }
    if matches!(field.snapshot.intent, ConfigUiOverride::Absent)
        && field.snapshot.effective != field.snapshot.baseline
    {
        return Err(format!(
            "absent field {} must have identical effective and baseline resolutions or neither",
            field.path
        ));
    }
    Ok(())
}

fn validate_capability(
    field: &ConfigUiField,
    source: &ConfigUiSource,
    file_actions: &[ConfigUiFileAction],
) -> Result<(), String> {
    if source.read_only && !matches!(field.capability, ConfigUiCapability::ReadOnly { .. }) {
        return Err(format!(
            "field {} has a mutating capability on read-only source {}",
            field.path, field.source_id
        ));
    }
    match &field.capability {
        ConfigUiCapability::ReadOnly {
            reason,
            file_action_id,
        } => {
            require_nonblank("read-only reason", reason)?;
            if let Some(action_id) = file_action_id {
                require_nonblank("read-only file_action_id", action_id)?;
                if file_actions
                    .iter()
                    .filter(|action| {
                        action.source_id == field.source_id && action.action_id == *action_id
                    })
                    .count()
                    != 1
                {
                    return Err(format!(
                        "field {} read-only action ({}, {}) must resolve exactly once",
                        field.path, field.source_id, action_id
                    ));
                }
            }
        }
        ConfigUiCapability::FreeText { .. } => {}
        ConfigUiCapability::Toggle { off, on } => {
            if off.value == on.value {
                return Err(format!(
                    "field {} toggle values must be distinct",
                    field.path
                ));
            }
            validate_choices(field, [off, on])?;
        }
        ConfigUiCapability::Choice { choices }
        | ConfigUiCapability::MultiChoice { choices, .. } => {
            validate_choices(field, choices)?;
        }
    }
    Ok(())
}

fn validate_choices<'a>(
    field: &ConfigUiField,
    choices: impl IntoIterator<Item = &'a ConfigUiChoice>,
) -> Result<(), String> {
    let choices = choices.into_iter().collect::<Vec<_>>();
    if choices.is_empty() {
        return Err(format!(
            "field {} choice capability must define at least one choice",
            field.path
        ));
    }
    let mut labels = BTreeSet::new();
    for (index, choice) in choices.iter().enumerate() {
        let label = choice.display_label();
        require_nonblank("choice label", &label)?;
        if choices[..index]
            .iter()
            .any(|previous| previous.value == choice.value)
        {
            return Err(format!("field {} choice values must be unique", field.path));
        }
        if !labels.insert(label) {
            return Err(format!("field {} choice labels must be unique", field.path));
        }
    }
    Ok(())
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
    let path_is_patchable = toml_document_path_is_patchable(current.as_table(), &entry.segments);
    let observed = entry
        .current
        .as_ref()
        .or(entry.default.as_ref())
        .expect("TOML entries are observed in at least one document");
    let type_label = toml_document_type_label(observed);
    let intent = entry
        .current
        .as_ref()
        .map_or(ConfigUiOverride::Absent, |value| {
            ConfigUiOverride::Explicit(toml_document_snapshot_value(value))
        });
    let state = if entry.current.is_some() {
        ConfigUiFieldState::Explicit
    } else {
        ConfigUiFieldState::Inherited
    };
    let current_value = toml_document_render_value(observed);
    let rendered_default = entry.default.as_ref().map(toml_document_render_value);
    let default_cell = rendered_default.unwrap_or_else(|| "-".to_string());
    let baseline = entry
        .default
        .as_ref()
        .map(toml_document_snapshot_value)
        .map(ConfigUiResolvedValue::new);
    let validation = if spec.validation.trim().is_empty() {
        "read-only inferred TOML value".to_string()
    } else {
        spec.validation.to_string()
    };

    Ok(ConfigUiField {
        source_id: spec.source_id.to_string(),
        path: display_path.clone(),
        display_label: display_path.clone(),
        section_label: spec.section_label.to_string(),
        list_cells: vec![
            toml_document_parent_label(&entry.segments),
            toml_document_key_label(&entry.segments, type_label),
            type_label.to_string(),
            field_state_label(state).to_string(),
            current_value,
            default_cell,
        ],
        tab: spec.tab.to_string(),
        type_label: Some(type_label.to_string()),
        snapshot: ConfigUiFieldSnapshot {
            intent,
            effective: Some(ConfigUiResolvedValue::new(toml_document_snapshot_value(
                observed,
            ))),
            baseline,
            external_manager: None,
        },
        description: toml_document_description(&display_path, type_label, path_is_patchable),
        validation,
        rebuild_required: spec.rebuild_required,
        apply_status: spec.apply_status.clone(),
        capability: ConfigUiCapability::ReadOnly {
            reason: "No editor capability was declared for this inferred TOML row.".to_string(),
            file_action_id: None,
        },
        can_unset: false,
    })
}

fn toml_document_path_is_patchable(root: &TomlEditTable, segments: &[String]) -> bool {
    if segments.is_empty()
        || !segments
            .iter()
            .all(|segment| is_toml_document_bare_key(segment))
    {
        return false;
    }
    toml_document_parent_path_is_patchable(root, segments)
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

fn toml_document_string_list(value: &TomlValue) -> bool {
    matches!(value, TomlValue::Array(values) if !values.is_empty() && values.iter().all(|value| matches!(value, TomlValue::String(_))))
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
        TomlValue::Datetime(value) => value.to_string(),
        _ => toml_value_to_json(value)
            .map(|value| render_json_edit_value(&value))
            .unwrap_or_else(|_| value.to_string()),
    }
}

fn toml_document_snapshot_value(value: &TomlValue) -> JsonValue {
    toml_value_to_json(value)
        .unwrap_or_else(|_| JsonValue::String(toml_document_render_value(value)))
}

fn toml_document_description(
    display_path: &str,
    type_label: &str,
    path_is_patchable: bool,
) -> String {
    if path_is_patchable {
        format!(
            "Generic TOML {type_label} value at {display_path}. Inferred rows are inspection-only until the host declares an editor capability."
        )
    } else {
        format!(
            "Generic TOML {type_label} value at {display_path}. This path cannot be represented as a safe dotted TOML patch path; edit the source file directly."
        )
    }
}

pub(crate) fn field_state_label(state: ConfigUiFieldState) -> &'static str {
    match state {
        ConfigUiFieldState::Explicit => "explicit",
        ConfigUiFieldState::Inherited => "default",
        ConfigUiFieldState::Absent => "unset",
        ConfigUiFieldState::Invalid => "invalid",
    }
}

fn row_matches_search(model: &ConfigUiModel, row: UiRowRef, search: &str) -> bool {
    match row {
        UiRowRef::Field(index) => {
            let field = &model.fields[index];
            let current = field_current_value(field);
            let baseline = field_baseline_value(field).unwrap_or_default();
            search_matches(
                search,
                [
                    field.path.as_str(),
                    field.display_label.as_str(),
                    field.section_label.as_str(),
                    current.as_str(),
                    baseline.as_str(),
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
                    sidecar.owner_label.as_deref().unwrap_or_default(),
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

pub fn render_json_value(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::Number(value) => value.to_string(),
        JsonValue::String(_) => render_json_edit_value(value),
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
    use crate::test_support::{apply_status, field, field_with_source, model_with_fields};
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

    // Defends: source identity is independent of tabs and remains unique across one shared tab.
    #[test]
    fn source_ids_are_unique_without_tab_ownership() {
        let mut model = model_with_fields(vec![
            field_with_source("settings-source", "ui.theme", "string", "dark", &[]),
            field_with_source("keys-source", "keys.leader", "string", "space", &[]),
        ]);
        assert!(model.validate().is_ok());

        model.sources[0].id = model.sources[1].id.clone();
        assert!(
            model
                .validate()
                .unwrap_err()
                .contains("duplicate source id")
        );
    }

    // Defends: sparse snapshots preserve override intent independently from resolved values and
    // reject only the contradictory inherited shapes that a host cannot explain.
    #[test]
    fn sparse_snapshots_preserve_intent_resolution_and_invalid_input() {
        fn resolved(value: JsonValue, origin: &str) -> ConfigUiResolvedValue {
            ConfigUiResolvedValue {
                value,
                origin: Some(origin.to_string()),
            }
        }

        let mut inherited = field("inherited", "string", r#""base""#, &[]);
        inherited.snapshot.intent = ConfigUiOverride::Absent;
        inherited.snapshot.effective = Some(resolved(json!("base"), "defaults"));
        inherited.snapshot.baseline = inherited.snapshot.effective.clone();
        let inherited_model = model_with_fields(vec![inherited.clone()]);
        assert!(inherited_model.validate().is_ok());
        assert_eq!(
            snapshot_field_state(&inherited),
            ConfigUiFieldState::Inherited
        );

        let mut absent = inherited.clone();
        absent.snapshot.effective = None;
        absent.snapshot.baseline = None;
        assert!(model_with_fields(vec![absent]).validate().is_ok());

        let mut contradictory = inherited.clone();
        contradictory.snapshot.baseline = Some(resolved(json!("other"), "defaults"));
        assert!(
            model_with_fields(vec![contradictory])
                .validate()
                .unwrap_err()
                .contains("identical effective and baseline")
        );
        let mut partial = inherited.clone();
        partial.snapshot.baseline = None;
        assert!(model_with_fields(vec![partial]).validate().is_err());

        let mut explicit = field("explicit", "string", r#""pinned""#, &[]);
        explicit.snapshot.baseline = Some(resolved(json!("pinned"), "old defaults"));
        assert!(model_with_fields(vec![explicit.clone()]).validate().is_ok());
        assert_eq!(
            snapshot_field_state(&explicit),
            ConfigUiFieldState::Explicit
        );
        explicit.snapshot.baseline = Some(resolved(json!("changed"), "new defaults"));
        assert!(model_with_fields(vec![explicit.clone()]).validate().is_ok());
        assert_eq!(
            snapshot_field_state(&explicit),
            ConfigUiFieldState::Explicit
        );
        assert_eq!(field_current_value(&explicit), r#""pinned""#);

        let mut invalid = field("invalid", "number", "1", &[]);
        invalid.snapshot.intent = ConfigUiOverride::Invalid {
            input: "not-a-number".to_string(),
        };
        invalid.snapshot.effective = Some(resolved(json!(4), "runtime"));
        invalid.snapshot.baseline = Some(resolved(json!(2), "defaults"));
        invalid.snapshot.external_manager = Some("system policy".to_string());
        assert!(model_with_fields(vec![invalid.clone()]).validate().is_ok());
        assert_eq!(snapshot_field_state(&invalid), ConfigUiFieldState::Invalid);
        assert_eq!(field_current_value(&invalid), "not-a-number");
        assert_eq!(field_baseline_value(&invalid).as_deref(), Some("2"));

        let mut independent = field("independent", "string", r#""intent""#, &[]);
        independent.snapshot.effective = Some(resolved(json!("effective"), "runtime"));
        independent.snapshot.baseline = Some(resolved(json!("baseline"), "defaults"));
        assert!(model_with_fields(vec![independent]).validate().is_ok());
    }

    // Defends: all model relationships are checked at ingestion while standalone actions and
    // exact-field diagnostics remain usable without fabricated sources or an operational tab.
    #[test]
    fn model_validation_rejects_unreachable_and_ambiguous_relationships() {
        fn invalid(model: ConfigUiModel, expected: &str) {
            let error = model.validate().expect_err("model should be invalid");
            assert!(
                error.contains(expected),
                "expected {error:?} to contain {expected:?}"
            );
        }

        fn action(source_id: &str, action_id: &str, tab: &str) -> ConfigUiFileAction {
            ConfigUiFileAction {
                source_id: source_id.to_string(),
                action_id: action_id.to_string(),
                tab: tab.to_string(),
                label: "Open file".to_string(),
                description: String::new(),
                path: PathBuf::from("settings.toml"),
                exists: true,
                read_only: false,
                create_if_missing: false,
                disabled_reason: None,
            }
        }

        let base = model_with_fields(vec![field("known", "string", r#""value""#, &[])]);

        let mut model = base.clone();
        model.tabs.clear();
        invalid(model, "at least one tab");

        let mut model = base.clone();
        model.tabs[0] = " ".to_string();
        invalid(model, "tab must not be blank");

        let mut model = base.clone();
        model.tabs.push("general".to_string());
        invalid(model, "duplicate tab");

        let mut model = base.clone();
        model.sources[0].id.clear();
        invalid(model, "source id must not be blank");

        let mut model = base.clone();
        model.fields[0].source_id.clear();
        invalid(model, "field source_id must not be blank");

        let mut model = base.clone();
        model.fields[0].path = "\t".to_string();
        invalid(model, "field path must not be blank");

        let mut model = base.clone();
        model.fields.push(model.fields[0].clone());
        invalid(model, "duplicate field identity");

        let mut model = base.clone();
        model.fields[0].source_id = "missing".to_string();
        invalid(model, "references missing source");

        let mut model = base.clone();
        model.sources[0].exists = false;
        invalid(model, "explicit or invalid override from absent source");

        let mut model = base.clone();
        model.sources[0].exists = false;
        model.fields[0].snapshot.intent = ConfigUiOverride::Absent;
        model.fields[0].snapshot.effective = model.fields[0].snapshot.baseline.clone();
        assert!(
            model.validate().is_ok(),
            "an absent override may target a source the host can create"
        );

        let mut model = base.clone();
        model.fields[0].tab = "missing".to_string();
        invalid(model, "uses unknown tab");

        let mut model = base.clone();
        model.file_actions = vec![action("standalone", "open", "general")];
        assert!(
            model.validate().is_ok(),
            "actions do not require a ConfigUiSource"
        );
        model
            .file_actions
            .push(action("standalone", "open", "general"));
        invalid(model, "duplicate file action identity");

        let invalid_action = |action, expected| {
            let mut model = base.clone();
            model.file_actions = vec![action];
            invalid(model, expected);
        };
        invalid_action(action("standalone", "open", "missing"), "uses unknown tab");
        invalid_action(
            action("", "open", "general"),
            "file action source_id must not be blank",
        );
        invalid_action(
            action("standalone", " ", "general"),
            "file action action_id must not be blank",
        );

        let mut blank_label = action("standalone", "open", "general");
        blank_label.label.clear();
        invalid_action(blank_label, "file action label must not be blank");

        let mut blank_reason = action("standalone", "open", "general");
        blank_reason.disabled_reason = Some(" ".to_string());
        invalid_action(
            blank_reason,
            "file action disabled_reason must not be blank",
        );

        let mut model = base.clone();
        model.operational_tab = Some("missing".to_string());
        invalid(model, "operational_tab must name a declared tab");

        let mut model = base.clone();
        model.tabs.push("operations".to_string());
        model.operational_tab = Some("operations".to_string());
        model.fields[0].tab = "operations".to_string();
        invalid(model, "cannot contain fields");

        let mut model = base.clone();
        model.tabs.push("operations".to_string());
        model.operational_tab = Some("operations".to_string());
        model.tab_list_tables.insert(
            "operations".to_string(),
            ConfigUiListTable {
                columns: Vec::new(),
            },
        );
        invalid(model, "cannot use a list table");

        let mut model = base.clone();
        model.tab_list_tables.insert(
            "missing".to_string(),
            ConfigUiListTable {
                columns: Vec::new(),
            },
        );
        invalid(model, "list table uses unknown tab");

        let field_id = base.fields[0].id();
        let diagnostic = |scope| ConfigUiDiagnostic {
            path: "host.check".to_string(),
            status: "invalid".to_string(),
            headline: "Host diagnostic".to_string(),
            blocking: true,
            scope,
            detail_lines: Vec::new(),
        };
        let mut model = base.clone();
        model
            .diagnostics
            .push(diagnostic(ConfigUiDiagnosticScope::Field(field_id)));
        assert!(
            model.validate().is_ok(),
            "exact-field diagnostics need no operations tab"
        );

        let mut model = base.clone();
        model
            .diagnostics
            .push(diagnostic(ConfigUiDiagnosticScope::Global));
        invalid(model, "require operational_tab");

        let mut model = base.clone();
        model
            .diagnostics
            .push(diagnostic(ConfigUiDiagnosticScope::Source {
                source_id: "missing".to_string(),
            }));
        model.tabs.push("operations".to_string());
        model.operational_tab = Some("operations".to_string());
        invalid(model, "diagnostic references missing source");

        let mut model = base.clone();
        model
            .diagnostics
            .push(diagnostic(ConfigUiDiagnosticScope::Field(
                ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "missing"),
            )));
        invalid(model, "diagnostic references missing field");

        let mut model = base.clone();
        model.recommended_fields = Some(vec![ConfigUiFieldId::new(
            DEFAULT_CONFIG_SOURCE_ID,
            "missing",
        )]);
        invalid(model, "recommended fields reference missing field");

        let mut model = base.clone();
        model.recommended_fields = Some(vec![model.fields[0].id(), model.fields[0].id()]);
        invalid(model, "duplicate recommended field");

        let mut model = base.clone();
        model.theme_switcher = Some(ConfigUiThemeSwitcher {
            field: ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "missing"),
            mappings: vec![ConfigUiThemeMapping {
                value: json!("dark"),
                theme: ConfigUiTheme::Dark,
            }],
        });
        invalid(model, "theme switcher references missing field");

        let mut model = base.clone();
        model.theme_switcher = Some(ConfigUiThemeSwitcher {
            field: model.fields[0].id(),
            mappings: Vec::new(),
        });
        invalid(model, "must define at least one mapping");

        let mut model = base;
        model.theme_switcher = Some(ConfigUiThemeSwitcher {
            field: model.fields[0].id(),
            mappings: vec![
                ConfigUiThemeMapping {
                    value: json!("same"),
                    theme: ConfigUiTheme::Dark,
                },
                ConfigUiThemeMapping {
                    value: json!("same"),
                    theme: ConfigUiTheme::Light,
                },
            ],
        });
        invalid(model, "mapping values must be unique");
    }

    // Defends: capability declarations are complete, unambiguous, and cannot grant mutation on a
    // source that the host marked read-only.
    #[test]
    fn model_validation_rejects_invalid_capability_authority_and_shapes() {
        fn invalid(model: ConfigUiModel, expected: &str) {
            let error = model.validate().expect_err("model should be invalid");
            assert!(
                error.contains(expected),
                "expected {error:?} to contain {expected:?}"
            );
        }

        fn choice(value: JsonValue, label: Option<&str>) -> ConfigUiChoice {
            ConfigUiChoice {
                value,
                label: label.map(str::to_string),
            }
        }

        let base = model_with_fields(vec![field("known", "string", r#""value""#, &[])]);

        let mut model = base.clone();
        model.sources[0].read_only = true;
        invalid(model, "mutating capability on read-only source");

        let mut model = base.clone();
        model.fields[0].capability = ConfigUiCapability::ReadOnly {
            reason: " ".to_string(),
            file_action_id: None,
        };
        invalid(model, "read-only reason must not be blank");

        let mut model = base.clone();
        model.fields[0].capability = ConfigUiCapability::ReadOnly {
            reason: "Use the source file.".to_string(),
            file_action_id: Some(" ".to_string()),
        };
        invalid(model, "read-only file_action_id must not be blank");

        let mut model = base.clone();
        model.fields[0].capability = ConfigUiCapability::ReadOnly {
            reason: "Use the source file.".to_string(),
            file_action_id: Some("open".to_string()),
        };
        invalid(model, "must resolve exactly once");

        let mut model = base.clone();
        model.fields[0].capability = ConfigUiCapability::Toggle {
            off: ConfigUiChoice::new(json!("same")),
            on: ConfigUiChoice::new(json!("same")),
        };
        invalid(model, "toggle values must be distinct");

        let mut model = base.clone();
        model.fields[0].capability = ConfigUiCapability::Choice {
            choices: Vec::new(),
        };
        invalid(model, "must define at least one choice");

        let mut model = base.clone();
        model.fields[0].capability = ConfigUiCapability::Choice {
            choices: vec![choice(json!(1), Some("one")), choice(json!(1), Some("uno"))],
        };
        invalid(model, "choice values must be unique");

        let mut model = base.clone();
        model.fields[0].capability = ConfigUiCapability::Choice {
            choices: vec![
                choice(json!(1), Some("same")),
                choice(json!(2), Some("same")),
            ],
        };
        invalid(model, "choice labels must be unique");

        let mut model = base;
        model.fields[0].capability = ConfigUiCapability::Choice {
            choices: vec![choice(json!(1), Some("\t"))],
        };
        invalid(model, "choice label must not be blank");
    }

    // Defends: optional provenance and ownership labels are either absent or meaningful.
    #[test]
    fn model_validation_rejects_blank_optional_labels() {
        let base = model_with_fields(vec![field("known", "string", r#""value""#, &[])]);
        let invalid = |model: ConfigUiModel, expected: &str| {
            assert!(model.validate().unwrap_err().contains(expected));
        };

        let mut model = base.clone();
        model.sources[0].owner_label = Some("  ".to_string());
        invalid(model, "source owner_label");

        let mut model = base.clone();
        model.fields[0].snapshot.external_manager = Some(String::new());
        invalid(model, "field external_manager");

        let mut model = base.clone();
        model.fields[0]
            .snapshot
            .effective
            .as_mut()
            .expect("effective")
            .origin = Some("\t".to_string());
        invalid(model, "resolved value origin");

        let mut model = base;
        model.tabs.push("operations".to_string());
        model.operational_tab = Some("operations".to_string());
        model.sidecars.push(ConfigUiSidecar {
            name: "sidecar".to_string(),
            path: PathBuf::from("sidecar.toml"),
            present: true,
            owner_label: Some(" ".to_string()),
            read_only: false,
        });
        invalid(model, "sidecar owner_label");
    }

    fn spec() -> ConfigUiFieldSpec {
        ConfigUiFieldSpec::new(
            DEFAULT_CONFIG_SOURCE_ID,
            "ui.theme",
            "general",
            "Theme name",
            ConfigUiCapability::Choice {
                choices: vec![
                    ConfigUiChoice::new(json!("light")),
                    ConfigUiChoice::new(json!("dark")),
                ],
            },
            "must be a known theme",
            status(),
        )
    }

    // Defends: schema tab extraction preserves only host/schema-declared tabs.
    #[test]
    fn schema_tabs_use_schema_order_or_fallback_without_injection() {
        let schema = json!({
            "x-host-config": {
                "tabs": ["general", "editor"]
            }
        });
        assert_eq!(
            schema_tabs(&schema, "x-host-config", &["fallback"]),
            vec!["general", "editor"]
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

    // Defends: reusable field rows derive state while keeping scalar value channels valid JSON.
    #[test]
    fn field_row_builder_derives_state_and_json_scalar_values() {
        let current = json!("dark");
        let default = json!("light");

        let explicit = spec().build("string", Some(&current), Some(&default));
        assert_eq!(
            snapshot_field_state(&explicit),
            ConfigUiFieldState::Explicit
        );
        assert_eq!(field_current_value(&explicit), "\"dark\"");
        assert_eq!(
            field_baseline_value(&explicit).as_deref(),
            Some("\"light\"")
        );
        assert!(explicit.has_baseline_value());

        let defaulted = spec().build("string", None, Some(&default));
        assert_eq!(
            snapshot_field_state(&defaulted),
            ConfigUiFieldState::Inherited
        );
        assert_eq!(field_current_value(&defaulted), "\"light\"");
        assert!(defaulted.has_baseline_value());

        let unset = spec().build("string", None, None);
        assert_eq!(snapshot_field_state(&unset), ConfigUiFieldState::Absent);
        assert_eq!(field_current_value(&unset), "not set");
        assert_eq!(field_baseline_value(&unset), None);
        assert!(!unset.has_baseline_value());

        let control_value = json!("\0");
        let control_field = spec().build("string", Some(&control_value), Some(&control_value));
        for rendered in [
            field_current_value(&control_field),
            field_baseline_value(&control_field).expect("baseline"),
        ] {
            assert_eq!(
                serde_json::from_str::<JsonValue>(&rendered).expect("valid JSON value"),
                control_value
            );
        }
    }

    // Defends: host policy fields pass through unchanged while the generic builder renders JSON safely.
    #[test]
    fn field_row_builder_preserves_host_metadata() {
        let current = json!(["git", "search", "preview", "terminal", "theme"]);
        let field = ConfigUiFieldSpec {
            display_label: "Enabled plugins".to_string(),
            section_label: "Plugins".to_string(),
            list_cells: vec!["plugins".to_string(), "5 enabled".to_string()],
            rebuild_required: true,
            ..ConfigUiFieldSpec::new(
                "settings",
                "plugins.enabled",
                "advanced",
                "Enabled plugin list",
                ConfigUiCapability::MultiChoice {
                    choices: ["git", "search", "preview", "terminal", "theme"]
                        .into_iter()
                        .map(|value| ConfigUiChoice::new(json!(value)))
                        .collect(),
                    ordered: false,
                },
                "known plugins only",
                status(),
            )
        }
        .build("string_list", Some(&current), None);

        assert_eq!(field.source_id, "settings");
        assert_eq!(field.path, "plugins.enabled");
        assert_eq!(field.display_label, "Enabled plugins");
        assert_eq!(field.section_label, "Plugins");
        assert_eq!(field.list_cells, vec!["plugins", "5 enabled"]);
        assert_eq!(field.tab, "advanced");
        assert_eq!(field_current_value(&field), "[5 items]");
        assert_eq!(field.snapshot.intent, ConfigUiOverride::Explicit(current));
        assert!(field.rebuild_required);
        assert!(matches!(
            field.capability,
            ConfigUiCapability::MultiChoice { ordered: false, .. }
        ));
        assert_eq!(field.apply_status.summary, "after save");
    }

    // Defends: hosts can build exact-value multichoice fields without a parallel kind contract.
    #[test]
    fn field_spec_builds_ordered_multichoice() {
        let field = ConfigUiFieldSpec {
            display_label: "Enabled widgets".to_string(),
            section_label: "Widgets".to_string(),
            list_cells: vec!["widgets".to_string(), "2 selected".to_string()],
            rebuild_required: true,
            ..ConfigUiFieldSpec::new(
                "settings",
                "widgets.enabled",
                "widgets",
                "Enabled widget ids",
                ConfigUiCapability::MultiChoice {
                    choices: ["clock", "status", "mode"]
                        .into_iter()
                        .map(|value| ConfigUiChoice::new(json!(value)))
                        .collect(),
                    ordered: true,
                },
                "known widget ids only",
                status(),
            )
        }
        .build(
            "string list",
            Some(&json!(["status", "clock"])),
            Some(&json!(["clock"])),
        );

        assert_eq!(field.source_id, "settings");
        assert_eq!(field.path, "widgets.enabled");
        assert_eq!(field.display_label, "Enabled widgets");
        assert_eq!(field.section_label, "Widgets");
        assert_eq!(field.list_cells, vec!["widgets", "2 selected"]);
        assert_eq!(field.tab, "widgets");
        assert_eq!(field.type_label.as_deref(), Some("string list"));
        assert_eq!(field_current_value(&field), r#"["status","clock"]"#);
        assert_eq!(
            field_baseline_value(&field).as_deref(),
            Some(r#"["clock"]"#)
        );
        assert_eq!(snapshot_field_state(&field), ConfigUiFieldState::Explicit);
        assert!(matches!(
            field.capability,
            ConfigUiCapability::MultiChoice { ordered: true, .. }
        ));
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
            vec![
                "",
                "[editor]",
                "table",
                "explicit",
                r#"{"cursor-shape":{"insert":"bar"},"line-number":"relative","plugins":["git","theme"],"rulers":[80,100]}"#,
                r#"{"cursor-shape":{"normal":"block"},"line-number":"absolute","plugins":["git"],"true-color":true}"#,
            ]
        );
        assert!(matches!(
            table.capability,
            ConfigUiCapability::ReadOnly { .. }
        ));

        let line_number = toml_field(&rows, "editor.line-number");
        assert_eq!(line_number.type_label.as_deref(), Some("string"));
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
        assert_eq!(field_current_value(line_number), "\"relative\"");
        assert_eq!(
            field_baseline_value(line_number).as_deref(),
            Some("\"absolute\"")
        );
        assert!(matches!(
            line_number.capability,
            ConfigUiCapability::ReadOnly { .. }
        ));
    }

    // Defends: default TOML documents can supply defaulted rows without a host schema.
    #[test]
    fn toml_document_helper_marks_defaulted_and_structured_rows() {
        let rows = toml_document_rows(
            r#"
[editor]
rulers = [80, 100]
limits = [inf, -inf, nan]
limit = inf
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
        assert_eq!(
            snapshot_field_state(line_number),
            ConfigUiFieldState::Inherited
        );
        assert_eq!(field_current_value(line_number), "\"relative\"");
        assert_eq!(
            field_baseline_value(line_number).as_deref(),
            Some("\"relative\"")
        );
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
        assert_eq!(rulers.type_label.as_deref(), Some("array"));
        assert_eq!(field_current_value(rulers), "[80,100]");
        assert_eq!(field_baseline_value(rulers).as_deref(), Some("[80]"));
        assert_eq!(rulers.list_cells[5], "[80]");
        assert_eq!(rulers.validation, "read-only inferred TOML value");
        assert!(matches!(
            rulers.capability,
            ConfigUiCapability::ReadOnly { .. }
        ));

        let limits = toml_field(&rows, "editor.limits");
        assert_eq!(field_current_value(limits), "[inf, -inf, nan]");

        let limit = toml_field(&rows, "editor.limit");
        assert_eq!(field_current_value(limit), "inf");
        assert!(matches!(
            limit.capability,
            ConfigUiCapability::ReadOnly { .. }
        ));
    }

    // Defends: host validation text remains display metadata without authorizing inferred rows.
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
        assert_eq!(rulers.validation, "host validates before writing");
        assert!(matches!(
            rulers.capability,
            ConfigUiCapability::ReadOnly { .. }
        ));
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
        assert_eq!(field_baseline_value(field).as_deref(), Some("\"default\""));
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
            field.capability,
            ConfigUiCapability::ReadOnly { .. }
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
        assert_eq!(name.type_label.as_deref(), Some("string"));
        assert_eq!(field_current_value(name), "\"ratconfig\"");
        assert_eq!(field_baseline_value(name), None);
        assert_eq!(name.validation, "read-only inferred TOML value");
        assert!(matches!(
            name.capability,
            ConfigUiCapability::ReadOnly { .. }
        ));
    }

    // Defends: inferred TOML syntax remains display-only, including string lists.
    #[test]
    fn toml_document_helper_infers_only_non_empty_string_lists() {
        let rows = toml_document_rows(
            r#"
[shell]
plugins = ["git", "status"]
empty = []
"#,
            None,
        );
        let plugins = toml_field(&rows, "shell.plugins");

        assert_eq!(plugins.type_label.as_deref(), Some("string list"));
        assert_eq!(field_current_value(plugins), r#"["git","status"]"#);
        assert!(matches!(
            plugins.capability,
            ConfigUiCapability::ReadOnly { .. }
        ));

        let empty = toml_field(&rows, "shell.empty");
        assert_eq!(empty.type_label.as_deref(), Some("array"));
        assert!(matches!(
            empty.capability,
            ConfigUiCapability::ReadOnly { .. }
        ));
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

        let mut model = ConfigUiModel {
            sources: Vec::new(),
            tabs: vec!["general".to_string(), "advanced".to_string()],
            operational_tab: Some("advanced".to_string()),
            tab_list_tables: BTreeMap::new(),
            fields: vec![spec().build("string", None, None)],
            recommended_fields: None,
            file_actions: vec![
                file_action("general", "Prompt config"),
                file_action("advanced", "Native logs"),
            ],
            sidecars: Vec::new(),
            native_config_statuses: Vec::new(),
            diagnostics: Vec::new(),
            theme_switcher: None,
        };
        let mut hidden_advanced_field = model.fields[0].clone();
        hidden_advanced_field.tab = "advanced".to_string();
        model.fields.push(hidden_advanced_field);

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
        assert_eq!(
            field_counts_for_tab(&model, 1),
            ConfigUiFieldCounts::default()
        );
        model.recommended_fields = Some(Vec::new());
        assert_eq!(
            visible_rows_for_tab_search_in_view(&model, 0, "", ConfigUiSettingsView::Overview),
            vec![UiRowRef::FileAction(0)]
        );
    }

    // Defends: recommendations focus Overview without hiding values requiring attention.
    #[test]
    fn recommended_fields_filters_defaults_but_keeps_active_values_and_all_scope_search() {
        let mut recommended = field("core.default", "string", r#""core""#, &[]);
        set_field_state_for_test(&mut recommended, ConfigUiFieldState::Inherited);
        let mut hidden = field("hidden.default", "string", r#""hidden""#, &[]);
        set_field_state_for_test(&mut hidden, ConfigUiFieldState::Inherited);
        let explicit = field("hidden.explicit", "string", r#""set""#, &[]);
        let mut invalid = field("hidden.invalid", "string", r#""broken""#, &[]);
        set_field_state_for_test(&mut invalid, ConfigUiFieldState::Invalid);
        let mut managed = field("hidden.managed", "string", r#""managed""#, &[]);
        set_field_state_for_test(&mut managed, ConfigUiFieldState::Inherited);
        managed.snapshot.external_manager = Some("host manager".to_string());
        let mut model = model_with_fields(vec![recommended, hidden, explicit, invalid, managed]);
        model.recommended_fields = Some(vec![ConfigUiFieldId::new(
            DEFAULT_CONFIG_SOURCE_ID,
            "core.default",
        )]);
        let visible = |search, view| visible_rows_for_tab_search_in_view(&model, 0, search, view);

        assert_eq!(
            visible("", ConfigUiSettingsView::Overview),
            vec![
                UiRowRef::Field(0),
                UiRowRef::Field(2),
                UiRowRef::Field(3),
                UiRowRef::Field(4),
            ]
        );
        assert_eq!(
            visible("", ConfigUiSettingsView::All),
            visible_rows_for_tab_search(&model, 0, "")
        );
        assert_eq!(
            visible("hidden.default", ConfigUiSettingsView::Overview),
            vec![UiRowRef::Field(1)]
        );
        assert_eq!(
            field_counts_for_tab(&model, 0),
            ConfigUiFieldCounts {
                overview: 4,
                total: 5,
            }
        );
    }

    // Defends: host-declared diagnostic scope invalidates only matching fields, while opaque
    // nonblocking diagnostics stay visible without changing known field state.
    #[test]
    fn scoped_diagnostics_derive_field_state_without_cross_source_leaks() {
        use crate::{ConfigUiApp, ConfigUiIntent, ConfigUiKey};
        use ConfigUiFieldState::{Inherited, Invalid};

        let mut source_a_one = field_with_source("source-a", "known.one", "string", "one", &[]);
        let mut source_a_two = field_with_source("source-a", "known.two", "string", "two", &[]);
        let mut source_b_one = field_with_source("source-b", "known.one", "string", "one", &[]);
        for field in [&mut source_a_one, &mut source_a_two, &mut source_b_one] {
            set_field_state_for_test(field, Inherited);
        }
        let mut model = model_with_fields(vec![source_a_one, source_a_two, source_b_one]);
        model.tabs.push("advanced".to_string());
        model.operational_tab = Some("advanced".to_string());
        model.recommended_fields = Some(Vec::new());
        let diagnostic = |blocking, scope| ConfigUiDiagnostic {
            path: "opaque.native".to_string(),
            status: if blocking { "invalid" } else { "preserved" }.to_string(),
            headline: "Native entry diagnostic".to_string(),
            blocking,
            scope,
            detail_lines: Vec::new(),
        };
        model.diagnostics.push(diagnostic(
            false,
            ConfigUiDiagnosticScope::Field(ConfigUiFieldId::new("source-a", "known.one")),
        ));
        let states = |model: &ConfigUiModel| {
            model
                .fields
                .iter()
                .map(|field| model.field_state(field))
                .collect::<Vec<_>>()
        };

        assert_eq!(states(&model), vec![Inherited; 3]);
        assert_eq!(
            visible_rows_for_tab_search_in_view(&model, 0, "", ConfigUiSettingsView::Overview),
            vec![UiRowRef::Field(0)],
            "nonblocking exact-field diagnostics still belong in Overview"
        );
        assert_eq!(
            visible_rows_for_tab_search(&model, 1, ""),
            vec![UiRowRef::Diagnostic(0)]
        );
        let mut no_operational_tab = model.clone();
        no_operational_tab.operational_tab = None;
        assert_eq!(
            ConfigUiApp::new(no_operational_tab).visible_rows(),
            vec![UiRowRef::Field(0)],
            "exact-field diagnostics do not require an operational tab"
        );
        let mut app = ConfigUiApp::new(model.clone());
        app.settings_view = ConfigUiSettingsView::All;
        assert_eq!(app.handle_key(ConfigUiKey::Enter), ConfigUiIntent::None);
        assert!(app.edit().is_some());

        model.diagnostics.push(diagnostic(
            true,
            ConfigUiDiagnosticScope::Field(ConfigUiFieldId::new("source-a", "known.one")),
        ));
        assert_eq!(states(&model), vec![Invalid, Inherited, Inherited]);
        assert_eq!(
            visible_rows_for_tab_search_in_view(&model, 0, "", ConfigUiSettingsView::Overview),
            vec![UiRowRef::Field(0)]
        );

        model.diagnostics[1].scope = ConfigUiDiagnosticScope::Source {
            source_id: "source-a".to_string(),
        };
        assert_eq!(states(&model), vec![Invalid, Invalid, Inherited]);
        assert_eq!(
            visible_rows_for_tab_search_in_view(&model, 0, "", ConfigUiSettingsView::Overview),
            vec![UiRowRef::Field(0)],
            "source blockers must not pull additional fields into Overview"
        );

        model.diagnostics[1].scope = ConfigUiDiagnosticScope::Global;
        assert_eq!(states(&model), vec![Invalid; 3]);
        assert_eq!(
            visible_rows_for_tab_search_in_view(&model, 0, "", ConfigUiSettingsView::Overview),
            vec![UiRowRef::Field(0)],
            "global blockers must remain operational rather than expanding Overview"
        );
    }

    // Defends: omitting recommendations treats every declared field as Overview, including on empty tabs.
    #[test]
    fn absent_recommendations_treat_every_field_as_overview() {
        let mut one = field("one", "string", r#""one""#, &[]);
        set_field_state_for_test(&mut one, ConfigUiFieldState::Inherited);
        let mut two = field("two", "string", r#""two""#, &[]);
        set_field_state_for_test(&mut two, ConfigUiFieldState::Absent);
        let mut model = model_with_fields(vec![one, two]);
        model.tabs.push("empty".to_string());

        assert_eq!(
            visible_rows_for_tab_search_in_view(&model, 0, "", ConfigUiSettingsView::Overview),
            visible_rows_for_tab_search(&model, 0, "")
        );
        assert_eq!(
            field_counts_for_tab(&model, 0),
            ConfigUiFieldCounts {
                overview: 2,
                total: 2
            }
        );
        assert_eq!(
            field_counts_for_tab(&model, 1),
            ConfigUiFieldCounts {
                overview: 0,
                total: 0
            }
        );
        assert_eq!(
            crate::ConfigUiApp::new(model.clone()).settings_view(),
            ConfigUiSettingsView::All,
            "None recommends every field, so there is no focused distinction"
        );

        model.recommended_fields = Some(Vec::new());
        assert!(
            visible_rows_for_tab_search_in_view(&model, 0, "", ConfigUiSettingsView::Overview)
                .is_empty(),
            "an empty recommendation set is attention-only"
        );
        assert_eq!(
            crate::ConfigUiApp::new(model.clone()).settings_view(),
            ConfigUiSettingsView::Overview
        );

        let mut active_only = model_with_fields(vec![field("explicit", "bool", "true", &[])]);
        active_only.recommended_fields = Some(Vec::new());
        assert_eq!(
            crate::ConfigUiApp::new(active_only).settings_view(),
            ConfigUiSettingsView::All,
            "Some(empty) starts in All when attention-only and All contain the same fields"
        );
    }

    // Defends: generated TOML rows can be classified by stable identity without reconstructing fields.
    #[test]
    fn recommended_fields_classifies_generated_toml_fields_by_source_and_path() {
        let rows = toml_document_rows(
            "",
            Some(
                r#"
[ui]
theme = "dark"
font = "mono"
"#,
            ),
        );
        let mut same_path_other_source = toml_field(&rows, "ui.theme").clone();
        same_path_other_source.source_id = "other".to_string();
        let mut model = model_with_fields(rows.fields);
        model.fields.push(same_path_other_source);
        model.tabs = vec!["native".to_string()];
        model.recommended_fields = Some(vec![ConfigUiFieldId::new("native", "ui.theme")]);

        let visible_paths =
            visible_rows_for_tab_search_in_view(&model, 0, "", ConfigUiSettingsView::Overview)
                .into_iter()
                .filter_map(|row| match row {
                    UiRowRef::Field(index) => Some(model.fields[index].path.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>();

        assert_eq!(visible_paths, vec!["ui.theme"]);
        assert_eq!(
            field_counts_for_tab(&model, 0),
            ConfigUiFieldCounts {
                overview: 1,
                total: 4
            }
        );
    }
}
