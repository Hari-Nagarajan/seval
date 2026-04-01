---
phase: 09-agent-definitions-and-loading
plan: 02
subsystem: agents
tags: [agent-registry, three-tier-loading, builtins, startup]
dependency_graph:
  requires: [09-01]
  provides: [AgentRegistry, install_builtins, load_agents, App::agent_registry()]
  affects: [src/agents/mod.rs, src/agents/types.rs, src/app.rs]
tech_stack:
  added: []
  patterns: [three-tier-override, fire-and-forget-error, include_str-embedding]
key_files:
  created: []
  modified:
    - src/agents/mod.rs
    - src/agents/types.rs
    - src/app.rs
decisions:
  - "AgentSource derives Copy (unit enum) — eliminates needless clone, satisfies clippy pedantic"
  - "load_tier silently skips missing dirs and warn+skips invalid files per D-03"
  - "Wizard mode App initializes with AgentRegistry::new() (no disk I/O at wizard startup)"
metrics:
  duration: 8min
  completed: "2026-03-21"
  tasks: 2
  files: 3
---

# Phase 9 Plan 02: Three-Tier Agent Loading and App Wiring Summary

**One-liner:** Three-tier agent registry loading (builtin < user-global < project-local) with embedded builtin installation and App startup wiring.

## What Was Built

### Task 1: AgentRegistry, install_builtins, load_agents (commit: 3adb3a3)

Added to `src/agents/mod.rs`:

- `AgentRegistry` struct with `get()`, `list()` (sorted by name), `len()`, `is_empty()`
- `install_builtins_to(dir)` — writes three embedded `.md` files, idempotent (D-12)
- `install_builtins()` — production wrapper using `~/.seval/agents/default/`
- `load_tier()` — scans a directory for `.md` files, warn+skips on parse error (D-03), silently skips missing dirs
- `load_agents()` — production entry point
- `load_agents_from_paths()` — testable variant with explicit tier paths
- Three `const` embeddings via `include_str!` for single-binary distribution

Also derived `Copy` on `AgentSource` in `src/agents/types.rs` (unit enum, no data).

11 new unit tests added covering: install idempotency, tier file scanning, non-md skip, invalid parse skip, missing dir skip, three-tier override, user-global override, source tagging, registry get/list.

### Task 2: Wire into App startup (commit: c7ebb2a)

Added to `src/app.rs`:

- `use crate::agents::AgentRegistry;` import
- `agent_registry: AgentRegistry` field on `App` struct
- `install_builtins()` call in `App::new()` with non-fatal warn on error
- `load_agents()` call in `App::new()` with `tracing::info` count log
- `agent_registry()` accessor method for Phase 10/11 downstream use
- Wizard mode `App` initializes with `AgentRegistry::new()` (no disk I/O)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] AgentSource needed Copy derive for clippy pedantic compliance**
- **Found during:** Task 1 clippy run
- **Issue:** `load_tier` takes `AgentSource` by value but uses `.clone()` inside; clippy pedantic flags `needless_pass_by_value` and `clone_on_copy`
- **Fix:** Added `Copy` to `#[derive(...)]` on `AgentSource` in `types.rs`; removed `.clone()` call; changed `map_or(false, ...)` to `is_some_and(...)`
- **Files modified:** `src/agents/types.rs`, `src/agents/mod.rs`
- **Commit:** 3adb3a3

## Verification

- `cargo test -p seval agents::tests` — 27 tests passed (Plan 01 + Plan 02)
- `cargo test -p seval` — 324 tests passed (full suite)
- `cargo clippy -p seval -- -D warnings` — clean
- `cargo build --release -p seval` — release build succeeded

## Self-Check: PASSED

- `src/agents/mod.rs` — contains `pub struct AgentRegistry`, `pub fn install_builtins_to`, `pub fn install_builtins`, `pub fn load_agents`, `pub fn load_agents_from_paths`, `fn load_tier`, all three `include_str!` constants, `tracing::warn!("skipping agent file`
- `src/app.rs` — contains `use crate::agents::AgentRegistry`, `agent_registry: AgentRegistry`, `crate::agents::install_builtins()`, `crate::agents::load_agents()`, `failed to install built-in agents`, `pub fn agent_registry(&self) -> &AgentRegistry`
- Commits exist: 3adb3a3 (Task 1), c7ebb2a (Task 2)
