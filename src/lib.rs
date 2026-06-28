//! Reusable Ratatui config editor boundary.
//!
//! Applications own loading, validation, persistence, and post-save apply
//! behavior. Ratconfig owns project-agnostic model, editor, rendering, text
//! patching, and migration semantics.

pub mod contract;
#[cfg(feature = "crossterm-runner")]
pub mod crossterm_runner;
pub mod editor;
pub mod jsonc;
pub mod migration;
pub mod model;
pub mod patch;
#[cfg(feature = "ui")]
pub mod render;
pub mod toml_adapter;

pub use contract::*;
#[cfg(feature = "crossterm-runner")]
pub use crossterm_runner::*;
pub use editor::*;
pub use model::{
    ConfigUiApplyStatus, ConfigUiContractField, ConfigUiDiagnostic, ConfigUiEditBehavior,
    ConfigUiField, ConfigUiFieldMetadata, ConfigUiFieldRowSpec, ConfigUiFileAction,
    ConfigUiMetadata, ConfigUiModel, ConfigUiNativeStatus, ConfigUiPathOwner, ConfigUiSchemaField,
    ConfigUiSidecar, ConfigUiSource, ConfigUiValueState, DEFAULT_CONFIG_SOURCE_ID,
    NO_CONFIG_DEFAULT_VALUE_LABEL,
};
pub use model::{
    UiRowRef, build_config_ui_field, collect_config_ui_schema_fields,
    config_contract_fields_from_toml, config_ui_metadata_from_toml, effective_string_config,
    effective_string_list_config, get_json_path, owner_label, render_json_edit_value,
    render_json_value, schema_tabs, selected_config_source, tab_index, toml_value_to_json,
    visible_rows_for_tab_search,
};
#[cfg(feature = "ui")]
pub use render::*;
