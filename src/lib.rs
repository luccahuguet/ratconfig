//! Reusable Ratatui config editor boundary.
//!
//! Applications own loading, validation, persistence, and post-save apply
//! behavior. Ratconfig owns project-agnostic model, editor, rendering, text
//! patching, and migration semantics.

pub mod contract;
#[cfg(feature = "crossterm-runner")]
pub mod crossterm_runner;
pub mod editor;
pub mod migration;
pub mod model;
pub mod patch;
#[cfg(feature = "ui")]
pub mod render;
#[cfg(test)]
mod test_support;
pub mod toml_adapter;

pub use contract::*;
#[cfg(feature = "crossterm-runner")]
pub use crossterm_runner::*;
pub use editor::*;
#[cfg(test)]
pub(crate) use model::ConfigUiFieldState;
pub use model::{
    ConfigUiApplyStatus, ConfigUiCapability, ConfigUiChoice, ConfigUiContractField,
    ConfigUiDiagnostic, ConfigUiDiagnosticScope, ConfigUiField, ConfigUiFieldId,
    ConfigUiFieldMetadata, ConfigUiFieldSnapshot, ConfigUiFieldSpec, ConfigUiFileAction,
    ConfigUiListColumn, ConfigUiListTable, ConfigUiMetadata, ConfigUiModel, ConfigUiNativeStatus,
    ConfigUiOverride, ConfigUiResolvedValue, ConfigUiSchemaField, ConfigUiSettingsView,
    ConfigUiSidecar, ConfigUiSource, ConfigUiTextEncoding, ConfigUiTheme, ConfigUiThemeMapping,
    ConfigUiThemeSwitcher, ConfigUiTomlDocumentRows, ConfigUiTomlDocumentSpec,
    DEFAULT_CONFIG_SOURCE_ID,
};
pub use model::{
    UiRowRef, build_toml_document_fields, collect_config_ui_schema_fields,
    config_contract_fields_from_toml, config_ui_metadata_from_toml, effective_string_config,
    effective_string_list_config, get_json_path, render_json_edit_value, render_json_value,
    schema_tabs, string_list_values_from_json, tab_index, toml_value_to_json,
};
#[cfg(feature = "ui")]
pub use render::*;
