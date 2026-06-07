# Agent Guidelines

Shared Yazelix agent workflow and release policy live in the main repo:

- https://github.com/luccahuguet/yazelix/blob/main/AGENTS.md
- In sibling local checkouts, read `../yazelix/AGENTS.md` first

Only Yazelix Ratconfig-specific guidance belongs here.

## Local Scope

- This repo owns project-agnostic Ratatui config editor primitives.
- Host applications own schemas, validation, file IO, apply policy, and product-specific text.
- Keep Yazelix-specific Home Manager, Zellij, Yazi, runtime-refresh, and command-name policy out of this crate.

## Local Commands

- `cargo fmt --all -- --check`
- `cargo test`

## Integration Notes

Main Yazelix consumes this crate through a pinned git dependency and owns its JSONC settings adapter.
