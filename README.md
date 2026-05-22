# Yazelix Ratconfig

Yazelix Ratconfig is a reusable Rust crate for building Ratatui config editors over JSONC-backed settings

It is extracted from Yazelix, but it is project-agnostic: applications provide their own config schema, default values, validation, file writes, and post-save apply behavior

## Scope

- generic config document and field model
- navigation, edit state, bool toggles, single-select, and multiselect controls
- generic Ratatui rendering
- comment-preserving JSONC patch primitives
- deterministic migration operations

Yazelix-specific behavior stays out of this repository, including Home Manager ownership, Zellij/Yazi policy, generated runtime refreshes, and Yazelix command names

## Status

The reusable model, editor, renderer, JSONC patcher, and migration primitives live in this crate. Host applications still own their own schema loading, validation, persistence policy, and post-save apply behavior
