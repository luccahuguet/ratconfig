# Ratconfig

Ratconfig is a reusable Rust crate for building Ratatui config editors over TOML-backed settings

It is extracted from Yazelix, but it is project-agnostic: applications provide their own config schema, default values, validation, file writes, and post-save apply behavior

![Yazelix config UI powered by ratconfig](assets/screenshots/yazelix_config_ui.png)

Example host integration in Yazelix: ratconfig owns the reusable tabs, rows, edit state, details pane, diagnostics, and rendering while the host supplies product-specific settings metadata and save/apply policy

## What It Owns

- generic config document and field model
- tabs, visible rows, search, selection, notices, and edit state
- optional host-supplied list table profiles for structured field tabs
- staged bool toggles, scalar editing, single-select, multiselect, and default reset controls
- host-routed file action rows and structured-field source shortcuts for native config files
- built-in dark/light UI palettes and optional model-driven theme switching
- generic Ratatui rendering for the model
- optional host-supplied rich detail rendering callbacks
- comment-preserving TOML set/unset patch primitives
- deterministic migration operations: rename, delete, add default, and narrow value transform
- deterministic config contracts that record joined state, replay safe versioned changes, and report manual blockers when automation is not safe

## What The Host Owns

- loading defaults and user config
- deciding which fields exist and how they are grouped
- validation and diagnostics
- file IO and atomic writes
- native config file creation and editor launching for file action rows
- mapping ratconfig errors into application-specific errors
- applying saved settings to a live runtime
- any product-specific detail text, commands, keybindings, or ownership policy
- deciding where contract state is stored and when migrated text is written atomically

Yazelix-specific behavior stays out of this repository, including Home Manager ownership, Zellij/Yazi policy, generated runtime refreshes, and Yazelix command names

## Minimal Shape

```rust
use std::path::PathBuf;
use ratconfig::{
    ConfigUiApplyStatus, ConfigUiEditBehavior, ConfigUiField, ConfigUiModel,
    ConfigUiPathOwner, ConfigUiSource, ConfigUiValueState,
    DEFAULT_CONFIG_SOURCE_ID,
    toml_adapter::{TomlPatchError, set_toml_value_text},
};

fn model() -> ConfigUiModel {
    ConfigUiModel {
        sources: vec![ConfigUiSource {
            id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
            tab: "general".to_string(),
            label: "Settings".to_string(),
            path: PathBuf::from("settings.toml"),
            exists: true,
            owner: ConfigUiPathOwner::User,
            read_only: false,
        }],
        tabs: vec!["general".to_string()],
        tab_list_tables: std::collections::BTreeMap::new(),
        fields: vec![ConfigUiField {
            source_id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
            path: "core.debug".to_string(),
            display_label: String::new(),
            section_label: String::new(),
            list_cells: Vec::new(),
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
        file_actions: Vec::new(),
        sidecars: Vec::new(),
        native_config_statuses: Vec::new(),
        diagnostics: Vec::new(),
        theme_switcher: None,
    }
}

fn patch_toml() -> Result<String, TomlPatchError> {
    let outcome = set_toml_value_text(
        "[core]\ndebug = false\n",
        "core.debug",
        &serde_json::json!(true),
    )?;
    Ok(outcome.text)
}
```

Host applications build the model from their own schema and config files, then use ratconfig editor/rendering helpers inside their terminal event loop. After an edit, the host validates and writes the patched text, reloads the model, and applies any live runtime changes it owns

Populate `ConfigUiModel::sources` for host-owned config documents. Ratconfig uses that metadata only to render the selected tab's label, path, owner, and write mode; tabs without a matching source show neutral non-file-backed metadata. Hosts still own discovery, loading, writes, creation policy, and validation

Use `ConfigUiField::display_label` when row and detail text should be friendlier than the stable field path. Ratconfig still uses `path` for edit intents and host write routing

Use `ConfigUiField::section_label` to place consecutive fields under host-defined, non-selectable headings within a tab. Ratconfig derives headings from the visible filtered rows, so empty sections disappear during search while selection and edit intents continue to address only real fields. Leave it empty to preserve the unsectioned layout

The first nine tabs display `(1)` through `(9)` shortcuts; pressing the matching digit selects that tab in normal mode while search and edit modes continue accepting digits as input

Populate `ConfigUiModel::tab_list_tables` and matching `ConfigUiField::list_cells` when a tab should render a structured display table instead of the default `takes effect | setting | value` field list. This is presentation-only data; Ratconfig does not parse labels, values, paths, or host-specific concepts to build those cells

Fields with defaults expose a reset-to-default action that emits `ConfigUiIntent::UnsetField`. Hosts decide whether that means unsetting text, writing a default, validation, persistence, reloads, and apply behavior. Use `NO_CONFIG_DEFAULT_VALUE_LABEL` for manually constructed fields that have no default; builder helpers set it automatically

Populate `ConfigUiModel::theme_switcher` when a committed field value should select a built-in Ratconfig theme. The switcher names one source id, one field path, and `serde_json::Value` mappings to `ConfigUiTheme::Dark` or `ConfigUiTheme::Light`; Ratconfig resolves the initial theme from model fields and switches after `ConfigUiApp::finish_successful_set_field_by_path()` or `ConfigUiApp::finish_successful_unset_field_by_path()` confirms a successful write of that source/path after any host reload. Failed host validation/writeback should not call those methods, so staged edits stay active and the theme does not change

## String-List Choices

Use `ConfigUiFieldSpec::build_string_list` for string-list settings whose values must come from a host-defined allowed set. The same field spec builds ordinary JSON-backed rows with `build`, so presentation and policy options have one shared construction surface. `ConfigUiEditBehavior::Default` keeps edited values in allowed-value order; `ConfigUiEditBehavior::OrderedStringList` preserves selected-value order and enables reorder controls in the picker

```rust
use ratconfig::{
    ConfigUiApplyStatus, ConfigUiEditBehavior, ConfigUiField, ConfigUiFieldSpec,
    toml_adapter::{TomlPatchError, set_toml_value_text},
};
use serde_json::Value;

fn sections_field() -> Result<ConfigUiField, String> {
    ConfigUiFieldSpec {
        display_label: "Layout sections".to_string(),
        section_label: "Visible content".to_string(),
        edit_behavior: ConfigUiEditBehavior::OrderedStringList,
        ..ConfigUiFieldSpec::new(
            "settings",
            "layout.sections",
            "layout",
            "Choose visible layout sections",
            vec![
                "left".to_string(),
                "center".to_string(),
                "right".to_string(),
            ],
            "known layout section ids only",
            ConfigUiApplyStatus {
                summary: "after save".to_string(),
                label: "after save".to_string(),
                detail: "Reload the application to apply this value".to_string(),
                pending: true,
            },
        )
    }
    .build_string_list(
        Some(vec!["left".to_string(), "center".to_string()]),
        Some(vec!["center".to_string()]),
    )
}

fn patch_sections_toml(raw: &str, value: &Value) -> Result<String, TomlPatchError> {
    let outcome = set_toml_value_text(raw, "layout.sections", value)?;
    Ok(outcome.text)
}
```

`ConfigUiIntent::SetField` supplies the edited `serde_json::Value`; the host validates that value against its own schema, calls the TOML patcher or its own writer, writes atomically, reloads the model, and applies any runtime policy it owns

## Arbitrary TOML Documents

Use `build_toml_document_fields` when a host-owned TOML file should be inspectable without declaring every field in a schema. The helper parses the current TOML text, optionally parses default TOML text, and returns ordinary `ConfigUiField` rows plus a `ConfigUiListTable` profile for the tab

```rust
use ratconfig::{
    ConfigUiApplyStatus, ConfigUiModel, ConfigUiTomlDocumentSpec,
    build_toml_document_fields,
    toml_adapter::{TomlPatchError, set_toml_value_text},
};
use serde_json::Value;

fn add_native_toml_rows(model: &mut ConfigUiModel, raw: &str, default_raw: &str) -> Result<(), String> {
    let document = build_toml_document_fields(ConfigUiTomlDocumentSpec {
        source_id: "helix-config",
        tab: "helix",
        section_label: "Editor settings",
        current_toml: raw,
        default_toml: Some(default_raw),
        validation: "host validates before writing",
        rebuild_required: false,
        apply_status: ConfigUiApplyStatus {
            summary: "after save".to_string(),
            label: "after save".to_string(),
            detail: "Reload the application to apply this file".to_string(),
            pending: true,
        },
    })?;
    model.tab_list_tables.insert("helix".to_string(), document.list_table);
    model.fields.extend(document.fields);
    Ok(())
}

fn patch_native_toml(raw: &str, path: &str, value: &Value) -> Result<String, TomlPatchError> {
    let outcome = set_toml_value_text(raw, path, value)?;
    Ok(outcome.text)
}
```

The generated rows include tables, scalar leaves, arrays, current/default state, deterministic table/key ordering, and compact previews of complete structured values. Strings, booleans, integers, finite floats, and non-empty string arrays use the normal editable field path when the TOML key path can be represented as dotted bare keys such as `editor.line-number` and the current document can be patched safely through that path

Complex tables, complex arrays, datetimes, quoted keys with dots, and other paths that cannot be safely represented as dotted patch paths are rendered as structured read-only rows. When exactly one file action has the same `source_id` as the selected structured field, `e` emits that action's existing `ConfigUiIntent::OpenFile` if it is available; unavailable, ambiguous, or missing ownership keeps the read-only behavior. Give each editable TOML document its own source id when its rows should open one exact file

Ratconfig still does not infer product labels, schema validation, file layering, atomic writes, reloads, or apply policy for arbitrary TOML documents

Populate `ConfigUiModel::file_actions` when the UI should show rows for host-owned native config files. Ratconfig renders label, path, state labels including `existing`, neutral `absent`, `read-only`, and `error`, plus the create-if-missing affordance, then emits `ConfigUiIntent::OpenFile` from the action row or the uniquely owned structured-field shortcut. Hosts still own file discovery, creation, editor launch, validation, reloads, and all file IO

While a text field is being edited, `Ctrl+e` emits `ConfigUiIntent::EditTextExternally`. The intent carries the field index, source id, path, and staged input buffer. Hosts can write that input to a temporary file, open the user's editor, read the edited text back, apply any host-owned newline or multiline policy, then call `ConfigUiApp::apply_external_text_edit`. Ratconfig does not spawn editors, create temporary files, or save automatically; `Enter` still emits `SetField` and `Esc` still cancels the staged edit

When using the optional crossterm runner, the callback is invoked while the runner's terminal session is active; hosts that launch a full-screen editor must own any terminal restore/re-entry policy themselves, or use the lower-level editor/render APIs and own the event loop

Hosts that want ratconfig to own the crossterm terminal setup, draw loop, event reads, and key conversion can enable the optional runner:

```toml
ratconfig = { git = "https://github.com/luccahuguet/ratconfig", tag = "v4.0.0", features = ["crossterm-runner"] }
```

```rust,no_run
use ratconfig::{ConfigUiApp, ConfigUiIntent, run_config_ui};
use serde_json::Value;

fn run_editor(mut app: ConfigUiApp) -> Result<(), Box<dyn std::error::Error>> {
    run_config_ui(&mut app, |app, intent| {
        match intent {
            ConfigUiIntent::BeginEdit { field_index, .. } => {
                app.begin_edit_field(field_index);
            }
            ConfigUiIntent::SetField { source_id, path, value, .. } => {
                host_validate_and_write(&source_id, &path, &value)?;
                app.finish_successful_set_field_by_path(&source_id, &path, &value);
            }
            ConfigUiIntent::UnsetField { source_id, path, .. } => {
                host_unset_and_reload(&source_id, &path)?;
                app.finish_successful_unset_field_by_path(&source_id, &path);
            }
            ConfigUiIntent::EditTextExternally { field_index, input, .. } => {
                let edited = host_edit_text_buffer(&input)?;
                if let Err(message) = app.apply_external_text_edit(field_index, edited) {
                    app.notice_error(message);
                }
            }
            ConfigUiIntent::OpenFile { path, create_if_missing, .. } => {
                host_open_file(&path, create_if_missing)?;
            }
            ConfigUiIntent::None | ConfigUiIntent::Exit => {}
        }
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}

fn host_validate_and_write(
    _source_id: &str,
    _path: &str,
    _value: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

fn host_unset_and_reload(
    _source_id: &str,
    _path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

fn host_edit_text_buffer(input: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(input.to_string())
}

fn host_open_file(
    _path: &std::path::Path,
    _create_if_missing: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
```

Use `run_config_ui_with_details` when the host supplies richer detail lines. The callback still owns validation, file writes, model reloads, notices, and apply policy

## Deterministic Config Contracts

Ratconfig can also treat a config file as having "joined" a host-defined contract. The host gives ratconfig a linear version history, safe automatic operations, and explicit manual steps for changes that cannot be inferred without user intent.

```rust
use ratconfig::{
    ConfigContract, ContractChange, ManualMigrationStep,
    contract::{
        join_toml_contract_text_from_version, reconcile_joined_toml_contract_text,
    },
    migration::MigrationOp,
};

fn contract() -> ConfigContract {
    ConfigContract {
        id: "example-app".to_string(),
        baseline_version: 1,
        current_version: 3,
        changes: vec![
            ContractChange::automatic(
                "rename-debug",
                1,
                2,
                vec![MigrationOp::Rename {
                    from: "debug".to_string(),
                    to: "core.debug".to_string(),
                }],
            ),
            ContractChange::manual(
                "split-theme",
                2,
                3,
                vec![ManualMigrationStep {
                    id: "choose-theme-palette".to_string(),
                    path: "theme.palette".to_string(),
                    reason: "The old theme value can map to more than one palette".to_string(),
                    remediation: "Choose a palette and set theme.palette explicitly".to_string(),
                }],
            ),
        ],
    }
}

fn adopt_old_config(raw: &str) -> Result<String, ratconfig::ContractError> {
    let outcome = join_toml_contract_text_from_version(
        raw,
        &contract(),
        "ratconfig.contract",
        1,
    )?;
    Ok(outcome.text)
}

fn reconcile_existing_config(raw: &str) -> Result<String, ratconfig::ContractError> {
    let outcome = reconcile_joined_toml_contract_text(
        raw,
        &contract(),
        "ratconfig.contract",
    )?;
    Ok(outcome.text)
}
```

The rules are deliberately strict:

- contract changes form one linear chain from `baseline_version` to `current_version`
- each joined config records `contract_id`, current contract version, and applied change ids at a host-chosen path
- safe changes run in memory and return a complete new text for the host to validate and write atomically
- renames refuse existing destinations and overlapping paths
- manual changes stop the plan before any text is returned for writing
- mismatched contract ids, unsupported saved versions, branchy histories, and missing migrations fail clearly

Use `join_toml_contract_text` only for configs the host has already validated against the current contract. Use `join_toml_contract_text_from_version` when adopting an older known config version, so ratconfig applies each automatic change before recording the joined state.

Run default completion on the text returned by join or reconcile, then validate and write that completed text:

```rust
use ratconfig::toml_adapter::{TomlMigrationError, apply_toml_defaults_text};

fn complete_toml_defaults(raw: &str) -> Result<String, TomlMigrationError> {
    let outcome = apply_toml_defaults_text(
        raw,
        &[("open.log_level", serde_json::json!("info"))],
    )?;
    Ok(outcome.text)
}
```

Default completion returns complete patched text and mutation records; the host still chooses the defaults, validates the result, and writes atomically

The contract layer is project-agnostic. Each application defines its contract id, state path, default values, validation, and write policy.

## Why In-House

Ratconfig keeps the contract layer small and in-crate because the existing Rust ecosystem pieces solve adjacent problems rather than this complete workflow:

- `config` is useful for layered configuration reads, but it does not own durable user-file write-back, versioned migrations, or comment-preserving edits
- `jsonschema` is useful for validation, but validation only says whether a document matches a schema; it does not decide how to rename, delete, default, transform, or manually block a stale field
- `toml_edit` preserves comments, spaces, and item order while editing TOML, but it is a format-preserving TOML editor rather than a migration contract system

Ratconfig owns the semantic contract rules and uses `toml_edit` for comment-preserving storage edits. `ConfigContract`, `ContractChange`, and `ManualMigrationStep` stay independent from host schemas and write policy.

## Storage Format Position

TOML is Ratconfig's text adapter. Semantic values use `serde_json::Value`, so the editor model and migration operations stay independent from TOML's syntax.

The TOML adapter executes rename, delete, add-default, transform, join, reconcile, manual-blocker, and contract-id checks. It rejects JSON `null` because TOML has no null value, and parent paths must be TOML tables before Ratconfig patches through them.

## Versioning And Releases

Ratconfig's stable crate contract begins at `1.0.0`. The crate follows SemVer for changes to the public host-facing contract. Crate versions are separate from host config contract versions such as `ConfigContract::current_version`; hosts own those config contract numbers and may advance them independently of Ratconfig releases

The public Ratconfig contract includes:

- public Rust API exported by the crate and its documented modules
- default features, optional feature names, and feature-gated public API
- the MSRV declared by `rust-version` in `Cargo.toml`
- documented model, editor, intent, renderer, and theme-switching semantics
- TOML set/unset patch behavior
- migration and config contract semantics for join, reconcile, automatic changes, manual blockers, and contract state validation
- documented host integration responsibilities for schema loading, validation, file IO, atomic writes, editor launch, model reloads, and runtime apply policy

Patch releases preserve that contract. Examples include renderer bug fixes, clearer docs, internal refactors, test changes, and behavior-preserving cleanup

Minor releases add to that contract without breaking existing hosts. Examples include a new helper, an additive model field with a backwards-compatible default, a new optional feature flag, or additive documented behavior that keeps existing intents and patch semantics valid

Major releases break or remove part of that contract. Examples include removing or renaming a public type, function, field, enum variant, or feature flag; changing `ConfigUiIntent` payload or reducer semantics in a way hosts can observe; changing TOML patch output semantics; changing migration/contract reconciliation rules; or raising the MSRV

Before cutting a release:

- inspect the commit range since the previous release and classify host-facing changes as patch, minor, or major
- update `Cargo.toml`, `Cargo.lock`, dependency examples, and release notes to the same crate version
- run `cargo fmt --all -- --check`
- run `cargo test`
- run feature checks when feature-gated behavior changes, such as `cargo test --no-default-features` and `cargo test --features crossterm-runner`
- tag the release as `vX.Y.Z` after the version commit is ready
- update downstream pinned-git consumers such as main Yazelix after the Ratconfig commit or tag is pushed

### 4.0.0

- TOML is the only text adapter
- Ratconfig 4 removes the legacy JSONC APIs and `jsonc-parser` dependency
- Format-neutral migration operations, outcomes, and contract planning remain available through the TOML adapter

### 3.0.0

- `ConfigUiSource` is the only config-document metadata owner in `ConfigUiModel`
- `ConfigUiFieldSpec` replaces the duplicated ordinary and string-list field parameter bags
- Text patchers use one format-neutral `PatchOutcome`; TOML migrations return the shared `MigrationOutcome`
- Tabs without a matching source render neutral non-file-backed header metadata
- Legacy single-config and cursor-specific model fields are removed

### 2.0.0

- Boolean rows use `Space` to stage a value, `Enter` to save the staged value, and `Esc` to cancel it
- Normal-mode `Enter` on a boolean leaves the value unchanged and shows the staged-edit guidance
- Hosts can group consecutive fields under non-selectable section headings with `ConfigUiField::section_label` and `ConfigUiTomlDocumentSpec::section_label`

## Status

The reusable model, editor, renderer, TOML patcher, deterministic contract layer, default completion helpers, and migration primitives live in this crate
