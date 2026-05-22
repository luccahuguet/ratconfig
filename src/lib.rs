//! Reusable Ratatui config editor for JSONC-backed settings.
//!
//! The crate owns generic config UI model, editor, rendering, JSONC patching,
//! and migration primitives. Applications provide their own config loading,
//! validation, writes, and post-save apply behavior.

pub mod editor;
pub mod jsonc;
pub mod migration;
pub mod model;
pub mod render;

pub use model::{ConfigField, ConfigModel, ValueState};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::EditIntent;

    #[test]
    fn public_shape_can_represent_project_agnostic_settings() {
        let model = ConfigModel {
            tabs: vec!["general".to_string()],
            fields: vec![ConfigField {
                path: "server.enabled".to_string(),
                tab: "general".to_string(),
                kind: "bool".to_string(),
                value: "false".to_string(),
                state: ValueState::Explicit,
            }],
        };

        assert_eq!(model.fields[0].path, "server.enabled");
        assert_eq!(
            EditIntent::Unset {
                path: model.fields[0].path.clone(),
            }
            .path(),
            "server.enabled"
        );
    }
}
