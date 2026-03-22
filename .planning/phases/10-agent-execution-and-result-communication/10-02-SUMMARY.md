---
phase: 10-agent-execution-and-result-communication
plan: "02"
subsystem: agents
tags: [agents, executor, spawn-agent, tool, streaming, approval-hook]
dependency_graph:
  requires: ["10-01"]
  provides: ["spawn_agent_task", "SpawnAgentTool", "effective_tool_filter"]
  affects: ["src/agents/executor.rs", "src/tools/spawn_agent.rs", "src/tools/mod.rs", "src/ai/streaming.rs", "src/approval/hook.rs", "src/chat/component.rs"]
tech_stack:
  added: []
  patterns: ["effective_tool_filter via ApprovalHook", "AgentHandleMap for cancellation infrastructure", "all-tools-registered + hook-filtered agent pattern"]
key_files:
  created:
    - src/tools/spawn_agent.rs
  modified:
    - src/agents/executor.rs
    - src/tools/mod.rs
    - src/ai/streaming.rs
    - src/approval/hook.rs
    - src/chat/component.rs
decisions:
  - "Register all tools unconditionally on agent builder; enforce effective_tools filter via ApprovalHook rather than conditional .tool() calls (avoids Rig type-level builder constraint)"
  - "spawn_agent_task returns (JoinHandle, Arc<Mutex<String>>) tuple for Phase 11 cancellation infrastructure"
  - "Agents use fresh disconnected approval channel so they don't block parent chat on approvals"
  - "Agent sessions saved to SQLite as child sessions (fire-and-forget) via spawn_blocking"
  - "Chat struct gets default empty agent_registry and agent_handles; full wiring deferred to Plan 03"
metrics:
  duration: "~10min"
  completed: "2026-03-22"
  tasks: 2
  files: 6
---

# Phase 10 Plan 02: Agent Execution Engine and SpawnAgentTool Summary

Implemented the core agent execution engine (`spawn_agent_task`) and `SpawnAgentTool` that allows the parent AI to spawn background agents.

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Implement spawn_agent_task in executor.rs | bfb0432 | src/agents/executor.rs, src/approval/hook.rs, src/chat/component.rs |
| 2 | Create SpawnAgentTool and register in parent streaming bridge | 7695f23 | src/tools/spawn_agent.rs, src/tools/mod.rs, src/ai/streaming.rs |

## What Was Built

**src/agents/executor.rs:** Added `spawn_agent_task(provider, params) -> (JoinHandle<()>, Arc<Mutex<String>>)`. The function spawns an async tokio task that:
- Creates a child SQLite session via `create_child_session` (fire-and-forget)
- Builds a Rig agent with all tools registered (tool restriction via hook filter, not selective registration)
- Wraps execution in `tokio::timeout` with `max_time_minutes * 60` seconds
- Accumulates assistant text in a shared `Arc<Mutex<String>>` partial output buffer
- Sends `Action::AgentCompleted(result)` on completion (or timeout)
- Returns `(JoinHandle, Arc<Mutex<String>>)` for Phase 11 cancellation infrastructure

Added `ALL_TOOL_NAMES` constant listing the 9 standard tools (excludes `spawn_agent` and `save_memory`).

**src/approval/hook.rs:** Added `effective_tool_filter: Option<Vec<String>>` field to `ApprovalHook`. When `Some`, any tool not in the list is auto-skipped before the normal approval logic. Added `deny_rules()` accessor. Updated all callers to pass `None` (parent chat preserves existing behavior).

**src/tools/spawn_agent.rs:** Created `SpawnAgentTool` implementing `rig::tool::Tool`:
- Looks up agent in registry, returns error if not found
- Computes effective tools using allowlist-first semantics (D-06)
- Resolves model alias to provider-specific model ID
- Inherits approval mode from parent if agent doesn't specify
- Calls `spawn_agent_task` and stores `(JoinHandle, partial_output_buffer)` in `AgentHandleMap`
- Returns immediate confirmation string: `"Agent '{name}' spawned successfully. Model: {model} | Max turns: {n} | Timeout: {m}min"`

**src/ai/streaming.rs:** Registered `SpawnAgentTool` on both Bedrock and OpenRouter agent builders. Added new `StreamChatParams` fields: `agent_registry`, `agent_handles`, `parent_session_id`, `approval_tx`, `parent_approval_mode`.

**src/chat/component.rs:** Added `agent_registry: Arc<AgentRegistry>` and `agent_handles: AgentHandleMap` fields (initialized with defaults; full wiring in Plan 03). Updated `StreamChatParams` construction to populate new fields.

## Decisions Made

1. **All-tools-registered + hook-filtered pattern**: Rig 0.32's AgentBuilder uses type-level generics — each `.tool()` call changes the type, making `if` statements impossible. Solution: register all tools unconditionally, use `ApprovalHook::effective_tool_filter` to deny tools not in the agent's effective list. This is a clean reuse of existing hook infrastructure.

2. **Fresh disconnected approval channel for agents**: Spawned agents create a fresh `unbounded_channel` for approvals that isn't connected to the TUI. This prevents background agents from blocking the parent chat waiting for user approval. The tool filter on the hook enforces effective_tools restrictions without interactive approval.

3. **Cancellation infrastructure (D-16)**: Phase 10 provides `(JoinHandle, Arc<Mutex<String>>)` infrastructure. Phase 11 provides the `/agents cancel` command that calls `abort()` and reads the partial buffer.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Rig type-level builder prevents conditional tool registration**
- **Found during:** Task 1 implementation
- **Issue:** Plan described using `if effective.contains(...)` conditional builder calls, but Rig's AgentBuilder uses generic types — each `.tool()` changes the concrete type, making if-branches impossible at the type level
- **Fix:** Adopted the recommended approach from the plan itself: register all tools unconditionally and use `ApprovalHook::effective_tool_filter` to skip disallowed tools at call time
- **Files modified:** src/agents/executor.rs, src/approval/hook.rs
- **Commit:** bfb0432

**2. [Rule 2 - Missing] Chat::new() needed default values for new streaming fields**
- **Found during:** Task 2 — `StreamChatParams` added fields that `Chat::new()` didn't populate
- **Fix:** Added `agent_registry` (default empty) and `agent_handles` (empty map) fields to `Chat` struct, wired into `StreamChatParams`. Plan 03 will replace defaults with real loaded registry.
- **Files modified:** src/chat/component.rs
- **Commit:** 7695f23

## Known Stubs

- `Chat::agent_registry` is initialized as an empty `AgentRegistry::default()` — no agents will be available via `spawn_agent` until Plan 03 wires the real registry from `load_agents()`
- `Chat::agent_handles` is initialized as an empty HashMap — this is correct behavior (map is populated at runtime as agents spawn)

## Self-Check: PASSED

Files exist:
- src/agents/executor.rs: FOUND (spawn_agent_task implemented)
- src/tools/spawn_agent.rs: FOUND
- src/tools/mod.rs: FOUND (SpawnAgentTool re-exported)
- src/ai/streaming.rs: FOUND (SpawnAgentTool registered)
- src/approval/hook.rs: FOUND (effective_tool_filter added)

Commits exist:
- bfb0432: FOUND (feat(10-02): implement spawn_agent_task in executor.rs)
- 7695f23: FOUND (feat(10-02): create SpawnAgentTool and register in parent streaming bridge)
