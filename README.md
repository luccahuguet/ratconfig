# Ratconfig

Ratconfig is a reusable Rust crate for building Ratatui config editors over TOML-backed settings

It is extracted from Yazelix, but it is project-agnostic: applications provide their own config schema, default values, validation, file writes, and post-save apply behavior

![Yazelix config UI powered by ratconfig](assets/screenshots/yazelix_config_ui.png)

Example host integration in Yazelix: ratconfig owns the reusable tabs, rows, edit state, details pane, diagnostics, and rendering while the host supplies product-specific settings metadata and save/apply policy

## What It Owns

- generic config document and field model
- tabs, visible rows, search, selection, notices, and edit state
- Core and All visibility, per-tab counts, and search across a host-owned field inventory
- optional host-supplied list table profiles for structured field tabs
- capability-driven toggles, free text, single-select, multiselect, and default reset controls
- host-routed file action rows and exact field-to-action shortcuts for native config files
- built-in dark/light UI palettes and optional model-driven theme switching
- generic Ratatui rendering for the model
- optional host-supplied rich detail rendering callbacks
- comment-preserving TOML set/unset patch primitives
- deterministic migration operations: rename, delete, add default, and narrow value transform
- deterministic config contracts that record joined state, replay safe versioned changes, and report manual blockers when automation is not safe

## What The Host Owns

- loading defaults and user config
- supplying the complete field inventory, choosing the Core allowlist, and grouping fields
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
    ConfigUiApplyStatus, ConfigUiCapability, ConfigUiChoice, ConfigUiField,
    ConfigUiFieldSnapshot, ConfigUiModel, ConfigUiOverride, ConfigUiResolvedValue,
    ConfigUiSource, DEFAULT_CONFIG_SOURCE_ID,
    toml_adapter::{TomlPatchError, set_toml_value_text},
};
use serde_json::json;

fn model() -> ConfigUiModel {
    ConfigUiModel {
        sources: vec![ConfigUiSource {
            id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
            label: "Settings".to_string(),
            path: PathBuf::from("settings.toml"),
            exists: true,
            owner_label: Some("user".to_string()),
            read_only: false,
        }],
        tabs: vec!["general".to_string()],
        operational_tab: None,
        tab_list_tables: std::collections::BTreeMap::new(),
        fields: vec![ConfigUiField {
            source_id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
            path: "core.debug".to_string(),
            display_label: String::new(),
            section_label: String::new(),
            list_cells: Vec::new(),
            tab: "general".to_string(),
            type_label: Some("bool".to_string()),
            snapshot: ConfigUiFieldSnapshot {
                intent: ConfigUiOverride::Explicit(json!(false)),
                effective: Some(ConfigUiResolvedValue::new(json!(false))),
                baseline: Some(ConfigUiResolvedValue::new(json!(false))),
                external_manager: None,
            },
            description: "Enable debug logging".to_string(),
            validation: "bool".to_string(),
            rebuild_required: false,
            apply_status: ConfigUiApplyStatus {
                summary: "after restart".to_string(),
                label: "restart".to_string(),
                detail: "Reload the application to apply this value".to_string(),
                pending: false,
            },
            capability: ConfigUiCapability::Toggle {
                off: ConfigUiChoice::new(json!(false)),
                on: ConfigUiChoice::new(json!(true)),
            },
            can_unset: true,
        }],
        core_fields: None,
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

Construct the editor with `ConfigUiApp::try_new`. Model construction and both replacement methods validate tab routing, stable identities, source references, snapshots, capabilities, file actions, diagnostics, and theme mappings before the app changes. App state is read-only through narrow accessors; hosts report persistence or validation feedback with `notice_info` and `notice_error`

Populate `ConfigUiModel::sources` for host-owned config documents. Sources are unique by id and can back fields on several tabs. Ratconfig renders source metadata from the selected field, renders file-action metadata from the selected action, and uses neutral metadata when neither is selected. `owner_label` is optional display text and does not grant write authority. Hosts still own discovery, loading, writes, creation policy, and validation

Each field snapshot keeps four concerns separate: `intent` records whether the selected source contains an absent, explicit, or locally invalid override; `effective` records the resolved value in use; `baseline` records resolution without that override; and `external_manager` carries optional display provenance. Effective and baseline values can each include one optional `origin` label. An absent override requires effective and baseline to be identical or both unknown, while explicit and invalid snapshots may carry independent resolutions and require a present source document. Equality between an explicit value and its baseline remains explicit

Use `ConfigUiField::display_label` when row and detail text should be friendlier than the stable field path. Ratconfig routes field intents with the stable `ConfigUiFieldId` formed from `source_id` and `path`

Use `ConfigUiField::section_label` to place consecutive fields under host-defined, non-selectable headings within a tab. Ratconfig derives headings from the visible filtered rows, so empty sections disappear during search while selection and edit intents continue to address only real fields. Leave it empty to preserve the unsectioned layout

The first nine tabs display `(1)` through `(9)` shortcuts; pressing the matching digit selects that tab in normal mode while search and edit modes continue accepting digits as input

Populate `ConfigUiModel::tab_list_tables` and matching `ConfigUiField::list_cells` when a tab should render a structured display table instead of the default `takes effect | setting | value` field list. This is presentation-only data; Ratconfig does not parse labels, values, paths, or host-specific concepts to build those cells

Hosts do not choose widths for the default list. Ratconfig sizes status and setting from every row in the selected tab, then gives the remaining cells to value; search and selection leave column starts unchanged

`ConfigUiField::can_unset` declares host authorization to remove an override independently of editor capability and baseline knowledge. Ratconfig exposes that action only when the flag is true for `Explicit` or `Invalid` intent on a writable source, and emits `ConfigUiIntent::UnsetField`. A known baseline can preview the inherited result, while `snapshot.baseline: None` keeps that result unknown without blocking an authorized unset

Populate `ConfigUiModel::theme_switcher` when a committed field value should select a built-in Ratconfig theme. The switcher names one `ConfigUiFieldId` and maps exact `serde_json::Value` values to `ConfigUiTheme::Dark` or `ConfigUiTheme::Light`. `try_new` resolves the initial theme from the field's effective snapshot. After a successful host write and reload, call `replace_model_after_success(reloaded, &field_id)`; the validated replacement becomes committed truth and only a matching staged edit is cleared. Failed host validation or persistence should report a notice without replacing the model, which preserves the staged buffer

## Core And All Settings

Hosts place the complete known inventory in `ConfigUiModel::fields`. Set `core_fields` to a positive allowlist of stable source/path identities when the UI should open with a focused Core view:

```rust
use ratconfig::{
    ConfigUiFieldId, ConfigUiModel, DEFAULT_CONFIG_SOURCE_ID,
};

fn classify_core(model: &mut ConfigUiModel) {
    model.core_fields = Some(vec![
        ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "core.debug"),
        ConfigUiFieldId::new("editor", "theme"),
    ]);
}
```

Any field outside those identities is non-core while defaulted or unset. Ratconfig keeps explicit and invalid values in Core so active configuration and errors stay visible. `core_fields: None` treats every field as Core

`ConfigUiApp` starts in Core when the model contains a Core/All distinction. It starts in All when both views contain the same fields. Normal-mode `a` toggles the selected tab between Core and All when that distinction exists. A non-empty search spans All fields without changing the saved view. The settings heading reports Core and total counts, and file actions plus host-routed operational rows remain reachable in either view

Generated TOML rows use the same identity contract. Hosts can classify fields returned by `build_toml_document_fields` with `ConfigUiFieldId::new(source_id, path)` without reconstructing the rows

## Scoped Diagnostics

Hosts classify every diagnostic as blocking or nonblocking and scope it globally, to one source, or to one exact `ConfigUiFieldId`. An exact-field blocking diagnostic renders that field as invalid and keeps it visible in Core without changing its snapshot intent. Source/global blockers mark matching fields invalid without expanding Core, and nonblocking diagnostics remain informational. Exact-field diagnostics can exist without an operational tab, while source/global diagnostics, sidecars, and native-status rows require `operational_tab` to name one declared tab. That tab uses Ratconfig's generic status layout and cannot also contain fields or a list-table profile. No tab name is reserved, and `schema_tabs` returns only host/schema-declared tabs

Use a nonblocking diagnostic for an opaque native entry that the host can preserve safely. Use field scope for one known invalid setting, source scope when one document is unsafe as a whole, and global scope only when every field is affected:

```rust
use ratconfig::{
    ConfigUiDiagnostic, ConfigUiDiagnosticScope, ConfigUiFieldId, ConfigUiModel,
};

fn report_preserved_entry(model: &mut ConfigUiModel) {
    model.diagnostics.push(ConfigUiDiagnostic {
        path: "plugins.opaque-native-entry".to_string(),
        status: "preserved".to_string(),
        headline: "Unmodeled native entry is preserved".to_string(),
        blocking: false,
        scope: ConfigUiDiagnosticScope::Field(ConfigUiFieldId::new(
            "native-config",
            "plugins.opaque-native-entry",
        )),
        detail_lines: vec!["The host leaves this entry unchanged when saving.".to_string()],
    });
}

fn report_unsafe_source(model: &mut ConfigUiModel) {
    model.diagnostics.push(ConfigUiDiagnostic {
        path: "native-config".to_string(),
        status: "invalid".to_string(),
        headline: "The native document cannot be updated safely".to_string(),
        blocking: true,
        scope: ConfigUiDiagnosticScope::Source {
            source_id: "native-config".to_string(),
        },
        detail_lines: Vec::new(),
    });
}
```

Diagnostic scope routes state; it does not infer validity from `path`, parse a native format, preserve text, or authorize a write. Hosts remain responsible for deciding whether an opaque entry is valid, whether malformed input blocks a field or source, and whether a save is safe

## Field Capabilities

Every field declares one editor capability. Ratconfig never infers write authority from `type_label`, rendered syntax, a baseline, or schema metadata. `ReadOnly` supplies a nonblank reason and can name one exact `(source_id, action_id)` file action. `FreeText` chooses string or JSON encoding. `Toggle`, `Choice`, and `MultiChoice` carry exact host-approved JSON values with optional friendly labels. A mutating capability requires a writable declared source

`type_label` is optional display metadata only. It can describe a host type without affecting editor selection or authorization

`MultiChoice { ordered: false }` emits selected values in capability order. `ordered: true` preserves the staged order and enables picker reorder controls. Choice values and rendered labels must be unique, and the host still validates every emitted value before persistence

```rust
use ratconfig::{
    ConfigUiApplyStatus, ConfigUiCapability, ConfigUiChoice, ConfigUiField,
    ConfigUiFieldSpec,
    toml_adapter::{TomlPatchError, set_toml_value_text},
};
use serde_json::Value;

fn sections_field() -> ConfigUiField {
    ConfigUiFieldSpec {
        display_label: "Layout sections".to_string(),
        section_label: "Visible content".to_string(),
        ..ConfigUiFieldSpec::new(
            "settings",
            "layout.sections",
            "layout",
            "Choose visible layout sections",
            ConfigUiCapability::MultiChoice {
                choices: ["left", "center", "right"]
                    .into_iter()
                    .map(|value| ConfigUiChoice::new(serde_json::json!(value)))
                    .collect(),
                ordered: true,
            },
            "known layout section ids only",
            ConfigUiApplyStatus {
                summary: "after save".to_string(),
                label: "after save".to_string(),
                detail: "Reload the application to apply this value".to_string(),
                pending: true,
            },
        )
    }
    .build(
        "string list",
        Some(&serde_json::json!(["left", "center"])),
        Some(&serde_json::json!(["center"])),
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

The generated rows include tables, scalar leaves, arrays, sparse intent/effective/baseline snapshots, deterministic table/key ordering, and compact previews of complete structured values. Inferred rows are inspection-only because TOML syntax is not enough evidence to grant edit authority. A host can replace an inferred row's capability with one backed by its schema and validation policy

To route a read-only field to a source file, declare `ReadOnly { file_action_id: Some(action_id), .. }` and provide exactly one file action with the same source id and action id. Ratconfig does not guess from tab membership or from other actions owned by the source. Disabled actions remain unavailable

Ratconfig still does not infer product labels, schema validation, file layering, atomic writes, reloads, or apply policy for arbitrary TOML documents

Populate `ConfigUiModel::file_actions` when the UI should show rows for host-owned native config files. Action labels and supplied disabled reasons must be nonblank. Ratconfig renders label, path, state labels including `existing`, neutral `absent`, `read-only`, and `error`, plus the create-if-missing affordance, then emits `ConfigUiIntent::OpenFile` with stable source/action identity and the validated path/create payload. Hosts still own file discovery, creation, editor launch, validation, reloads, and all file IO

For a `FreeText` field, normal-mode `Enter` starts inline editing while `e` starts the same staged edit and immediately emits `ConfigUiIntent::EditTextExternally`. Inline editing supports grapheme-safe Left/Right movement, Home/End, Backspace/Delete, insertion and single-line paste at the cursor, and `Ctrl+u` to clear. `Ctrl+e` externalizes an edit already in progress. The external intent carries the stable field identity and exact staged input buffer. Hosts can write that input to a temporary file, open the user's editor, read the edited text back, apply any host-owned newline or multiline policy, then call `ConfigUiApp::apply_external_text_edit(&field, edited)`. Ratconfig does not spawn editors, create temporary files, or save automatically; `Enter` emits `SetField` and `Esc` cancels the staged edit

The optional crossterm runner enables bracketed paste and translates its key and paste events into Ratconfig's reducer vocabulary. Its callback is invoked while the runner's terminal session is active; hosts that launch a full-screen editor must own any terminal restore/re-entry policy themselves, or use the lower-level editor/render APIs and own the event loop

Hosts that want ratconfig to own the crossterm terminal setup, draw loop, event reads, and key conversion can enable the optional runner:

```toml
ratconfig = { git = "https://github.com/luccahuguet/ratconfig", branch = "main", features = ["crossterm-runner"] }
```

```rust,no_run
use ratconfig::{
    ConfigUiApp, ConfigUiIntent, ConfigUiModel, CrosstermRunnerError, run_config_ui,
};
use serde_json::Value;

fn run_editor(mut app: ConfigUiApp) -> Result<(), CrosstermRunnerError<std::io::Error>> {
    run_config_ui(&mut app, |app, intent| {
        match intent {
            ConfigUiIntent::SetField { field, value } => {
                host_validate_and_write(&field.source_id, &field.path, &value)?;
                let reloaded = host_reload_model()?;
                app.replace_model_after_success(reloaded, &field)
                    .map_err(std::io::Error::other)?;
            }
            ConfigUiIntent::UnsetField { field } => {
                host_unset(&field.source_id, &field.path)?;
                let reloaded = host_reload_model()?;
                app.replace_model_after_success(reloaded, &field)
                    .map_err(std::io::Error::other)?;
            }
            ConfigUiIntent::EditTextExternally { field, input } => {
                let edited = host_edit_text_buffer(&input)?;
                if let Err(message) = app.apply_external_text_edit(&field, edited) {
                    app.notice_error(message);
                }
            }
            ConfigUiIntent::OpenFile { path, create_if_missing, .. } => {
                host_open_file(&path, create_if_missing)?;
            }
            ConfigUiIntent::None | ConfigUiIntent::Exit => {}
        }
        Ok(())
    })
}

fn host_validate_and_write(
    _source_id: &str,
    _path: &str,
    _value: &Value,
) -> std::io::Result<()> {
    Ok(())
}

fn host_unset(
    _source_id: &str,
    _path: &str,
) -> std::io::Result<()> {
    Ok(())
}

fn host_reload_model() -> std::io::Result<ConfigUiModel> {
    unimplemented!("reload the host-owned config and build a fresh model")
}

fn host_edit_text_buffer(input: &str) -> std::io::Result<String> {
    Ok(input.to_string())
}

fn host_open_file(
    _path: &std::path::Path,
    _create_if_missing: bool,
) -> std::io::Result<()> {
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

Minor releases add to that contract without breaking existing hosts. Examples include a new helper or type, a new optional feature flag, or additive documented behavior that keeps existing intents and patch semantics valid

Major releases break or remove part of that contract. Examples include adding a required field to a directly constructed public struct; removing or renaming a public type, function, field, enum variant, or feature flag; changing `ConfigUiIntent` payload or reducer semantics in a way hosts can observe; changing TOML patch output semantics; changing migration/contract reconciliation rules; or raising the MSRV

Before cutting a release:

- inspect the commit range since the previous release and classify host-facing changes as patch, minor, or major
- update `Cargo.toml`, `Cargo.lock`, dependency examples, and release notes to the same crate version
- run `cargo fmt --all -- --check`
- run `cargo test`
- run feature checks when feature-gated behavior changes, such as `cargo test --no-default-features` and `cargo test --features crossterm-runner`
- tag the release as `vX.Y.Z` after the version commit is ready
- update downstream pinned-git consumers such as main Yazelix after the Ratconfig commit or tag is pushed

### 6.0.0 (unreleased)

- `ConfigUiFieldSnapshot` separates absent/explicit/invalid override intent from optional effective and baseline resolutions, provenance, and external-management labels; the parallel string value fields and sentinel defaults are absent from the model
- `ConfigUiCapability` is the sole editor-authorization surface for read-only, free-text, toggle, choice, and multichoice fields; `type_label` is display-only, and the intermediate `kind`, `allowed_values`, and `ConfigUiEditBehavior` field APIs are removed
- Field intents carry `ConfigUiFieldId`, `OpenFile` carries stable source/action identity with its validated path payload, edits start inside Ratconfig without `BeginEdit`, and external editor results are applied by field identity
- Normal-mode `u` emits `UnsetField` only for an `Explicit` or `Invalid` override with host-declared unset authority on a writable source; editor capability and baseline availability remain independent
- `ConfigUiApp::try_new`, `replace_model`, and `replace_model_after_success` validate complete models before mutation; app internals are read-only to hosts, reloads preserve stable selection and compatible edits by identity, and invalid replacements leave staged state untouched
- `ConfigUiSource` is unique by id and independent of tabs, selected rows drive source headers, and `operational_tab` replaces reserved-name routing for diagnostics and status rows
- `ConfigUiDiagnostic` adds required global/source/field scope, and effective field validity is derived from matching blocking diagnostics instead of a duplicate field-spec flag
- `ConfigUiEditState` adds a required grapheme-boundary cursor; `ConfigUiKey` adds Home, End, Delete, and owned paste input and is no longer `Copy`
- Free-form fields use `Enter` for cursor-aware single-line editing and normal-mode `e` for the host-owned external editor; choice controls remain native
- The outer-borderless body uses padded content, a single center divider, and an inset tab separator

### 5.0.0

- `ConfigUiModel` adds the required `core_fields` field, and direct `ConfigUiApp` literals add the required `settings_view` field; `ConfigUiApp::new` selects the initial view from the model
- Core includes allowlisted fields plus explicit and invalid configured values; All contains the host's complete inventory, non-empty search spans All without changing the saved view, and normal-mode `a` toggles views when the selected tab has a distinction
- TOML patching, migrations, and contract-state reads share dotted-path validation and normalization while preserving operation and contract-change error context
- Structured rows keep compact list previews while the details pane renders complete nested values and collapses equal defaults to `same as current`
- Default field columns allocate spare width to values and measure ASCII, CJK text, and joined emoji in terminal cells
- The light palette keeps semantic row colors readable; inactive tabs and pane headings use high-contrast text, and the borderless body uses padded content, a center gutter, and an inset tab separator

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
