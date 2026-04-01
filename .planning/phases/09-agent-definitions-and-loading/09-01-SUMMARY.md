---
phase: 09-agent-definitions-and-loading
plan: 01
subsystem: agents
tags: [agents, parsing, toml, types, rust]
dependency_graph:
  requires: []
  provides: [AgentFrontmatter, AgentDef, AgentSource, parse_agent_file, effective_tools, resolve_model_alias, builtin-agents]
  affects: [src/lib.rs]
tech_stack:
  added: []
  patterns: [toml-frontmatter-parsing, allowlist-first-tool-filtering, model-alias-resolution]
key_files:
  created:
    - src/agents/types.rs
    - src/agents/mod.rs
    - src/agents/builtin/security-analyzer.md
    - src/agents/builtin/code-reviewer.md
    - src/agents/builtin/recon-agent.md
  modified:
    - src/lib.rs
decisions:
  - "Used +++ delimiters (TOML-style) for agent frontmatter, reusing existing toml crate (no extra dep)"
  - "Built-in agent .md files created in src/agents/builtin/ and embedded via include_str! for single-binary distribution"
  - "effective_tools implements allowlist-first semantics: non-empty allowed_tools overrides denied_tools (D-06)"
  - "resolve_model_alias function provides short aliases (sonnet/haiku/opus) mapping to provider-specific IDs"
  - "Temperature clamped to [0.0, 1.0] with tracing::warn! on out-of-range values"
metrics:
  duration: 3min
  completed: "2026-03-21"
  tasks: 2
  files: 6
---

# Phase 09 Plan 01: Agent Definition Types and Parser Summary

Agent types, TOML frontmatter parser, tool filtering, model alias resolution, and three built-in security agent definitions implemented in a single-commit TDD execution.

## What Was Built

### `src/agents/types.rs`

- `AgentFrontmatter` struct: `#[serde(deny_unknown_fields)]` deserialization from TOML with required fields (`name`, `model`, `max_turns`) and defaults (`temperature=0.7`, `max_time_minutes=10`, empty tool lists, `approval_mode=None`)
- `AgentDef` struct: combines parsed frontmatter + system prompt body + source provenance
- `AgentSource` enum: `BuiltIn | UserGlobal | ProjectLocal`

### `src/agents/mod.rs`

- `parse_agent_file(content: &str) -> anyhow::Result<(AgentFrontmatter, String)>`: Parses TOML between `+++` delimiters, clamps temperature, returns frontmatter + body string
- `effective_tools(allowed, denied, all_tools) -> Vec<&str>`: Allowlist-first semantics — if `allowed` is non-empty, return only those; otherwise return `all_tools` minus `denied`
- `resolve_model_alias(alias, provider) -> String`: Maps `sonnet`/`haiku`/`opus` to Bedrock or OpenRouter model IDs; unknown aliases pass through unchanged
- 16 unit tests covering all parsing edge cases, filtering semantics, temperature clamping, and unknown field rejection

### Built-in Agent Files

Three AGENT.md files in `src/agents/builtin/` embedded via `include_str!`:

| Agent | Tools | Temperature | Max Turns |
|-------|-------|-------------|-----------|
| security-analyzer | shell, read, grep, glob, ls, web_search, web_fetch | 0.3 | 30 |
| code-reviewer | read, grep, glob, ls (read-only) | 0.2 | 25 |
| recon-agent | shell, read, grep, glob, ls, web_search, web_fetch, write | 0.5 | 20 |

Each file includes a detailed security-focused system prompt with methodology sections and structured output format.

## Deviations from Plan

### Implementation Note

Task 1 and Task 2 were committed together as a single commit because the `include_str!` macros referencing the built-in `.md` files are evaluated at compile time. The code in `mod.rs` (Task 1) won't compile unless the files in `builtin/` (Task 2) exist. This is a technical coupling — no functional deviation from the plan. All acceptance criteria for both tasks are met.

No other deviations.

## Key Decisions Made

1. **TOML frontmatter with `+++` delimiters** — Reuses existing `toml` crate, no extra dependency. Decision resolves Open Question #1 from STATE.md.
2. **Allowlist-first tool filtering (D-06)** — When `allowed_tools` is non-empty, `denied_tools` is completely ignored. This makes the security model deterministic.
3. **`include_str!` for built-in agents** — Files are embedded in the binary at compile time, supporting single-binary distribution. File-based templates remain user-copyable via the source.
4. **Temperature clamping with `tracing::warn!`** — Silent correction with a log warning, not an error. Allows forgiving parsing of slightly out-of-range values.

## Tests

| Test | Status |
|------|--------|
| parse_valid_agent | PASS |
| parse_minimal_agent | PASS |
| parse_missing_required_field | PASS |
| parse_missing_closing_delimiter | PASS |
| parse_missing_opening_delimiter | PASS |
| body_becomes_system_prompt | PASS |
| body_with_triple_plus_in_content | PASS |
| tool_filtering_allowlist | PASS |
| tool_filtering_denylist | PASS |
| tool_filtering_both_empty | PASS |
| tool_filtering_both_set | PASS |
| resolve_model_alias_bedrock | PASS |
| resolve_model_alias_openrouter | PASS |
| temperature_clamping | PASS |
| deny_unknown_fields | PASS |
| builtin_agents_parse | PASS |

**Total: 16/16 tests pass**

## Self-Check: PASSED

- `src/agents/types.rs` exists and contains `pub struct AgentFrontmatter`
- `src/agents/mod.rs` exists and contains `pub fn parse_agent_file`
- `src/agents/builtin/security-analyzer.md` exists with `name = "security-analyzer"`
- `src/agents/builtin/code-reviewer.md` exists with `name = "code-reviewer"`
- `src/agents/builtin/recon-agent.md` exists with `name = "recon-agent"`
- `src/lib.rs` contains `pub mod agents;`
- Commit `54d55fb` exists and is the current HEAD
- `cargo test -p seval agents::tests` exits 0 (16 passed)
- `cargo clippy -p seval -- -D warnings` exits 0
- `cargo build -p seval` exits 0
