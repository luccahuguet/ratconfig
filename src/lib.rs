//! Reusable Ratatui config editor boundary.
//!
//! Applications own loading, validation, persistence, and post-save apply
//! behavior. Ratconfig owns project-agnostic model, editor, rendering, JSONC
//! patching, and migration semantics.

pub mod editor;
pub mod jsonc;
pub mod migration;
pub mod model;
pub mod render;

pub use editor::*;
pub use model::{
    ConfigUiApplyStatus, ConfigUiDiagnostic, ConfigUiEditBehavior, ConfigUiField, ConfigUiModel,
    ConfigUiNativeStatus, ConfigUiPathOwner, ConfigUiSidecar, ConfigUiValueState,
};
pub use model::{
    UiRowRef, effective_string_config, effective_string_list_config, get_json_path, owner_label,
    render_json_edit_value, render_json_value, tab_index, visible_rows_for_tab_search,
};
pub use render::*;
