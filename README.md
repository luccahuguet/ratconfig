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

## Initial Status

This repository starts as the child crate shell for the extraction. The reusable implementation is moved from the Yazelix main repository in focused follow-up commits
