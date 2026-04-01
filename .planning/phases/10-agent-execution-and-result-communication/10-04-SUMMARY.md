---
phase: 10-agent-execution-and-result-communication
plan: "04"
subsystem: agents
tags: [rust, atomic, arc, turn-counter, approval-hook]

# Dependency graph
requires:
  - phase: 10-agent-execution-and-result-communication
    provides: ApprovalHook with turn_counter() returning Arc<AtomicUsize>

provides:
  - Accurate turns_completed in AgentResult for normal completion paths
  - Accurate turns_completed in AgentResult for timeout paths (bedrock and openrouter)

affects: [phase-11-cancellation, agent-result-display]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Clone Arc<AtomicUsize> before hook is consumed by .with_hook() to retain read access after stream ends"
    - "Pass turn_counter Arc down call chain to timeout branches that cannot see inside the stream"

key-files:
  created: []
  modified:
    - src/agents/executor.rs

key-decisions:
  - "Clone Arc in both run_agent_task (for timeout branches) and run_agent_stream (for normal completion) — two separate Arc clones needed since run_agent_stream is generic and unaware of the outer call context"
  - "Use fully-qualified Ordering import (std::sync::atomic::{AtomicUsize, Ordering}) to avoid name collision risk"

patterns-established:
  - "Arc clone before consumption pattern: call hook.turn_counter() before .with_hook(hook) to retain shared reference"

requirements-completed:
  - AGENTEXEC-04

# Metrics
duration: 5min
completed: 2026-04-01
---

# Phase 10 Plan 04: Turn Counter Fix Summary

**Arc<AtomicUsize> cloned before ApprovalHook consumption so turns_completed in AgentResult reflects actual tool-call turns, not hardcoded 0**

## Performance

- **Duration:** ~5 min
- **Started:** 2026-04-01T14:35:00Z
- **Completed:** 2026-04-01T14:40:00Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments

- Replaced hardcoded `0u32` turns_completed with actual ApprovalHook turn counter reads
- Normal completion path in `run_agent_stream` now reads `turn_counter.load(Ordering::Relaxed)`
- Timeout paths in `run_bedrock_agent` and `run_openrouter_agent` now report accumulated turn count at time of timeout
- Added `std::sync::atomic::{AtomicUsize, Ordering}` import to support the new reads

## Task Commits

Each task was committed atomically:

1. **Task 1: Clone turn counter Arc before hook consumption and read it after stream ends** - `e726a81` (fix)

**Plan metadata:** (docs commit to follow)

## Files Created/Modified

- `src/agents/executor.rs` - Added turn_counter cloning and reading in run_agent_task, run_bedrock_agent, run_openrouter_agent, and run_agent_stream

## Decisions Made

- Clone Arc in `run_agent_task` (line ~212) before passing hook to `run_bedrock_agent`/`run_openrouter_agent`, then pass that clone as a parameter to both functions for use in their timeout branches. This avoids deeper refactoring.
- Also clone Arc inside `run_agent_stream` (line ~457) before `.with_hook(hook)` for the normal completion path, since `run_agent_stream` is a generic function unaware of the outer call chain.
- Two Arc clones total: outer (for timeout) and inner (for normal completion). Both point to the same underlying AtomicUsize.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `turns_completed` is now accurate in all AgentResult instances
- Phase 11 cancellation can rely on accurate turn counts when reporting partial results
- No blockers

---
*Phase: 10-agent-execution-and-result-communication*
*Completed: 2026-04-01*
