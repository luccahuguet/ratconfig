# Yazelix Ratconfig

Yazelix Ratconfig is a reusable Rust crate for building Ratatui config editors over JSONC-backed settings

It is extracted from Yazelix, but it is project-agnostic: applications provide their own config schema, default values, validation, file writes, and post-save apply behavior

## What It Owns

- generic config document and field model
- tabs, visible rows, search, selection, notices, and edit state
- bool toggles, scalar editing, single-select, and multiselect controls
- generic Ratatui rendering for the model
- optional host-supplied rich detail rendering callbacks
- comment-preserving JSONC set/unset patch primitives
- deterministic migration operations: rename, delete, add default, and narrow value transform

## What The Host Owns

- loading defaults and user config
- deciding which fields exist and how they are grouped
- validation and diagnostics
- file IO and atomic writes
- mapping ratconfig errors into application-specific errors
- applying saved settings to a live runtime
- any product-specific detail text, commands, keybindings, or ownership policy

Yazelix-specific behavior stays out of this repository, including Home Manager ownership, Zellij/Yazi policy, generated runtime refreshes, and Yazelix command names

## Minimal Shape

```rust
use std::path::PathBuf;
use yazelix_ratconfig::{
    ConfigUiApplyStatus, ConfigUiEditBehavior, ConfigUiField, ConfigUiModel,
    ConfigUiPathOwner, ConfigUiValueState,
    jsonc::{PatchError, set_jsonc_value_text},
};

fn model() -> ConfigUiModel {
    ConfigUiModel {
        active_config_path: PathBuf::from("settings.jsonc"),
        cursor_config_path: PathBuf::new(),
        default_cursor_config_path: PathBuf::new(),
        active_config_exists: true,
        config_owner: ConfigUiPathOwner::User,
        config_read_only: false,
        tabs: vec!["general".to_string()],
        fields: vec![ConfigUiField {
            path: "core.debug".to_string(),
            tab: "general".to_string(),
            kind: "bool".to_string(),
            current_value: "false".to_string(),
            edit_value: "false".to_string(),
            default_value: "false".to_string(),
            state: ConfigUiValueState::Explicit,
            description: "Enable debug logging".to_string(),
            allowed_values: Vec::new(),
            validation: "bool".to_string(),
            rebuild_required: false,
            apply_status: ConfigUiApplyStatus {
                summary: "after restart".to_string(),
                label: "restart".to_string(),
                detail: "Reload the application to apply this value".to_string(),
                pending: false,
            },
            edit_behavior: ConfigUiEditBehavior::Default,
        }],
        sidecars: Vec::new(),
        native_config_statuses: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn patch_jsonc() -> Result<String, PatchError> {
    let outcome = set_jsonc_value_text(
        r#"{ "core": { "debug": false } }"#,
        "core.debug",
        &serde_json::json!(true),
    )?;
    Ok(outcome.text)
}
```

Host applications build the model from their own schema and config files, then use ratconfig editor/rendering helpers inside their terminal event loop. After an edit, the host validates and writes the patched JSONC, reloads the model, and applies any live runtime changes it owns

## Status

The reusable model, editor, renderer, JSONC patcher, and migration primitives live in this crate. TOML support is not part of the first crate shape
