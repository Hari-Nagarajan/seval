# Roadmap: Rust Security CLI

## Overview

This roadmap delivers a Rust-based AI security CLI that replaces SEVAL-CLI as a daily driver. The 8 phases of v1.0 progressed from foundational architecture through a working AI chat, to a full dashboard TUI with tools, context management, and session persistence. The 3 phases of v2.0 layer a full sub-agent system on top: agent definitions load from a layered directory hierarchy, agents execute asynchronously with isolated context and filtered tools, and the sidebar plus slash commands give the user full visibility and control.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

### v1.0 Foundation (Complete)

- [x] **Phase 1: Foundation** - Event-driven async architecture, terminal safety, single binary scaffold
- [x] **Phase 2: Configuration** - Config system, init wizard, project-local settings
- [x] **Phase 3: Streaming Chat** - Multi-provider AI chat with streaming, markdown rendering, token tracking
- [x] **Phase 4: Basic TUI** - Split-pane dashboard layout with input area, status bar, and scrolling
- [x] **Phase 5: Tool Execution** - Built-in tools: shell, file ops, grep, glob, ls, web fetch/search (completed 2026-03-14)
- [x] **Phase 6: Tool Approval and Agentic Loop** - Permission modes, approval workflow, agentic tool loop, deny rules (completed 2026-03-14)
- [x] **Phase 7: Context Management** - Token tracking, context compression, sidebar context and session displays (completed 2026-03-14)
- [x] **Phase 8: Session and Memory** - Save/resume sessions, SEVAL-CLI format compatibility, persistent memory (completed 2026-03-15)

### v2.0 Sub-Agent System (Active)

- [x] **Phase 9: Agent Definitions and Loading** - AGENT.md format, layered directory loading, three built-in agents (completed 2026-03-21)
- [ ] **Phase 10: Agent Execution and Result Communication** - spawn_agent tool, async execution, result delivery, child sessions
- [ ] **Phase 11: Agent UI and Management Commands** - Sidebar live display, /agents slash command suite

## Phase Details

### Phase 1: Foundation
**Goal**: Application has a working async event loop, compiles to a single binary, and handles crashes/signals gracefully
**Depends on**: Nothing (first phase)
**Requirements**: INFRA-01, INFRA-02, INFRA-03, INFRA-04, INFRA-05, INFRA-06
**Success Criteria** (what must be TRUE):
  1. Running `cargo build --release` produces a single binary with no runtime dependencies
  2. Application starts and displays a basic terminal screen in under 200ms
  3. The event loop processes input without blocking (typing remains responsive during async operations)
  4. Killing the process with Ctrl+C or SIGTERM restores the terminal to its original state
  5. A forced panic restores the terminal to its original state and prints a useful error message
**Plans**: 3 plans

Plans:
- [x] 01-01-PLAN.md — Project scaffold, dependencies, logging, panic hook
- [x] 01-02-PLAN.md — Event loop, Component trait, signal handling
- [x] 01-03-PLAN.md — Welcome screen, startup validation, terminal size check

### Phase 2: Configuration
**Goal**: Users can configure the application on first run and customize settings per-project
**Depends on**: Phase 1
**Requirements**: ONBD-01, ONBD-02, ONBD-03, ONBD-04, ONBD-05
**Success Criteria** (what must be TRUE):
  1. Running the app for the first time launches an interactive wizard that sets up AWS profile, region, and model
  2. The wizard also configures tool approval mode and deny rules
  3. Running with CLI flags (e.g. --profile, --region) skips the wizard and applies settings directly
  4. Configuration is stored in ~/.seval/config.toml (global) and .seval/config.toml (project-local), with project settings overriding global
**Plans**: 2 plans

Plans:
- [x] 02-01-PLAN.md — Config types, load/save/merge, CLI flags, and tests
- [x] 02-02-PLAN.md — Interactive TUI wizard and app startup wiring

### Phase 3: Streaming Chat
**Goal**: Users can have a multi-turn AI conversation with streaming responses, markdown rendering, and reliable credential management
**Depends on**: Phase 1, Phase 2
**Requirements**: CHAT-01, CHAT-02, CHAT-03, CHAT-08, CHAT-09, CHAT-10, CHAT-11, CHAT-12
**Success Criteria** (what must be TRUE):
  1. User can type a message and see tokens stream in real time at a smooth 30fps render rate
  2. Conversation maintains multi-turn history (user can reference earlier messages and AI remembers context)
  3. AI responses render as formatted markdown with syntax-highlighted code blocks for Python, YAML, JSON, shell, Go, C, and Rust
  4. Token usage is tracked per message from API response metadata
  5. Auth errors are handled gracefully with inline error messages without losing conversation state
**Plans**: 3 plans

Plans:
- [x] 03-01-PLAN.md — Core AI types, provider abstraction, message model, slash commands
- [x] 03-02-PLAN.md — Markdown-to-Ratatui renderer and chat input area
- [x] 03-03-PLAN.md — Chat component, app wiring, streaming integration, end-to-end verification

### Phase 4: Basic TUI
**Goal**: Users see a professional split-pane dashboard with a chat area, sidebar, input editor, and status bar
**Depends on**: Phase 3
**Requirements**: TUI-01, TUI-05, TUI-06, TUI-07, TUI-08
**Success Criteria** (what must be TRUE):
  1. Screen is split into a main chat pane and a sidebar, both rendering content
  2. Input area supports multi-line editing and pasting multi-line text
  3. Status bar displays current mode, model name, and keyboard shortcuts
  4. A loading/streaming indicator is visible while AI is generating a response
  5. User can scroll up and down through conversation history
**Plans**: 2 plans

Plans:
- [x] 04-01-PLAN.md — Dashboard layout with sidebar, status bar, and split-pane rendering
- [x] 04-02-PLAN.md — Bracketed paste support with line-ending normalization and TUI verification

### Phase 5: Tool Execution
**Goal**: AI can execute shell commands, manipulate files, search codebases, and fetch web content
**Depends on**: Phase 3
**Requirements**: TOOL-01, TOOL-02, TOOL-03, TOOL-04, TOOL-05, TOOL-06, TOOL-07, TOOL-08, TOOL-09, TOOL-10, TOOL-11
**Success Criteria** (what must be TRUE):
  1. AI can run shell commands and receive stdout, stderr, and exit code (with 30s timeout and 100KB output limit)
  2. AI can read files with line numbers, write new files, and make surgical diff-based edits to existing files
  3. AI can search file contents via regex grep and discover files via glob patterns
  4. AI can list directory contents with metadata
  5. AI can fetch web URLs (with HTML-to-text conversion) and perform web searches
**Plans**: 4 plans

Plans:
- [x] 05-01-PLAN.md — Tool framework, dependencies, Action variants, shell tool with timeout/truncation
- [x] 05-02-PLAN.md — File tools: read (line numbers), write (parent-dir safety), edit (search-and-replace)
- [x] 05-03-PLAN.md — Search tools: grep (regex + gitignore), glob (pattern matching), ls (directory listing)
- [x] 05-04-PLAN.md — Web tools (fetch, search), streaming bridge integration, chat rendering, system prompt

### Phase 6: Tool Approval and Agentic Loop
**Goal**: AI autonomously chains tool calls with user-controlled approval, completing multi-step tasks without manual intervention
**Depends on**: Phase 4, Phase 5
**Requirements**: TOOL-12, TOOL-13, TOOL-14, TOOL-15, TOOL-16, TOOL-17, TUI-03
**Success Criteria** (what must be TRUE):
  1. AI calls tools, receives results, and continues reasoning automatically (agentic loop completes multi-step tasks)
  2. Each tool request displays the tool name and arguments for user review before execution
  3. User can approve, deny, or "approve all of this type" per tool request
  4. Four permission modes work correctly: plan (read-only), default (ask for writes), auto-edit (auto-approve file edits), yolo (bypass all)
  5. Deny rules block known-dangerous command patterns and tool execution status is visible in the sidebar
**Plans**: 3 plans

Plans:
- [x] 06-01-PLAN.md — Approval module: types, PromptHook, permission modes, deny rules, display formatting
- [x] 06-02-PLAN.md — Streaming bridge multi-turn integration, chat approval UI with Y/N/A keys, Esc cancellation
- [x] 06-03-PLAN.md — Sidebar tool status display (spinner + history) and status bar turn counter

### Phase 7: Context Management
**Goal**: Users can see their token budget, and the system automatically compresses context to maintain conversation quality
**Depends on**: Phase 3, Phase 4
**Requirements**: CHAT-04, CHAT-05, CHAT-06, CHAT-07, TUI-02, TUI-04
**Success Criteria** (what must be TRUE):
  1. Sidebar displays a color-coded progress bar (green/yellow/red) showing current token usage against the context window
  2. Sidebar displays session info including model name and message count
  3. At 70% token capacity, proactive compression triggers and the user sees the context bar decrease
  4. At 85% token capacity, enforced compression triggers automatically
  5. Compression preserves tool results and key findings while summarizing older chat messages
**Plans**: 2 plans

Plans:
- [x] 07-01-PLAN.md — Context state tracking, context window discovery, sidebar context bar and session info
- [x] 07-02-PLAN.md — AI-powered compression pipeline with automatic threshold triggers

### Phase 8: Session and Memory
**Goal**: Users can save, resume, and manage sessions with SEVAL-CLI compatibility and persistent cross-session memory
**Depends on**: Phase 3, Phase 5
**Requirements**: SESS-01, SESS-02, SESS-03, SESS-04, SESS-05, SESS-06, SESS-07
**Success Criteria** (what must be TRUE):
  1. User can save the current session and later resume it with full conversation and tool history intact
  2. User can list all saved sessions and delete sessions they no longer need
  3. Session files are compatible with SEVAL-CLI (can read SEVAL-CLI sessions and write files SEVAL-CLI can read)
  4. Per-project persistent memory is loaded on session start, and key findings are auto-saved to the project memory directory
**Plans**: 3 plans

Plans:
- [x] 08-01-PLAN.md — SQLite database layer, session persistence, auto-save, /sessions command
- [x] 08-02-PLAN.md — Persistent memory system with save_memory tool and /memory command
- [x] 08-03-PLAN.md — SEVAL-CLI import/export compatibility

### Phase 9: Agent Definitions and Loading
**Goal**: Users can define agents as AGENT.md files and have three built-in agents available out of the box
**Depends on**: Phase 8
**Requirements**: AGENTDEF-01, AGENTDEF-02, AGENTBI-01, AGENTBI-02
**Success Criteria** (what must be TRUE):
  1. User can create an AGENT.md file with TOML frontmatter (name, model, max_turns, allowed_tools, etc.) and the markdown body becomes the agent's system prompt
  2. Agent definitions load from three tiers (~/.seval/agents/default/, ~/.seval/agents/, .seval/agents/) with project-local overriding user-global overriding built-in
  3. The three built-in agents (security-analyzer, code-reviewer, recon-agent) are installed to ~/.seval/agents/default/ on first run and appear in agent listings
  4. User can copy a built-in agent file as a starting template for a custom agent
**Plans**: 2 plans

Plans:
- [x] 09-01-PLAN.md — Agent types, TOML frontmatter parser, tool filtering, built-in agent content files
- [x] 09-02-PLAN.md — Three-tier directory loading, AgentRegistry, install_builtins, App startup wiring

### Phase 10: Agent Execution and Result Communication
**Goal**: The AI can spawn agents that run asynchronously with isolated context, and results flow back into the parent conversation
**Depends on**: Phase 9
**Requirements**: AGENTEXEC-01, AGENTEXEC-02, AGENTEXEC-03, AGENTEXEC-04, AGENTEXEC-05, AGENTEXEC-06, AGENTRES-01, AGENTRES-02, AGENTRES-03
**Success Criteria** (what must be TRUE):
  1. AI can invoke the spawn_agent tool with an agent name and task; the tool returns immediately while the agent runs in the background
  2. Spawned agent executes with its own isolated message history, system prompt, filtered tool set, max_turns cap, and max_time_minutes timeout
  3. When an agent completes (or times out), the result appears as a formatted system message in the parent chat and is injected into the parent AI's context for the next turn
  4. Agent conversation is stored in SQLite as a child session linked to the parent via parent_session_id
  5. Nested agent spawning is prevented — the spawn_agent tool is not registered within any agent's tool set
**Plans**: 3 plans

Plans:
- [ ] 10-01-PLAN.md — Agent execution types (AgentResult, AgentStatus, AgentExecParams), Action::AgentCompleted, SQLite migration 2
- [ ] 10-02-PLAN.md — spawn_agent_task executor, SpawnAgentTool, filtered tool registration, streaming bridge integration
- [ ] 10-03-PLAN.md — Chat wiring: AgentCompleted handler, pending result queue, rig_history injection, end-to-end verification

### Phase 11: Agent UI and Management Commands
**Goal**: Users have full visibility into running agents and can manage them through a complete slash command suite
**Depends on**: Phase 10
**Requirements**: AGENTUI-01, AGENTUI-02, AGENTCMD-01, AGENTCMD-02, AGENTCMD-03, AGENTCMD-04
**Success Criteria** (what must be TRUE):
  1. Sidebar shows each running agent with a spinner, name, and live turn progress (e.g., [|] security-analyzer turn 3/25)
  2. Sidebar shows recently completed agents with elapsed time (e.g., security-analyzer done (45s))
  3. Running /agents lists all available agents with their name, description, and source directory
  4. Running /agents info <name> shows the full agent configuration including model, tools, max_turns, and a system prompt preview
  5. Running /agents create <name> scaffolds a new AGENT.md template in .seval/agents/<name>/
**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> ... -> 8 -> 9 -> 10 -> 11

### v1.0 Foundation (Complete)

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Foundation | 3/3 | Complete | 2026-03-14 |
| 2. Configuration | 2/2 | Complete | 2026-03-14 |
| 3. Streaming Chat | 3/3 | Complete | 2026-03-14 |
| 4. Basic TUI | 2/2 | Complete | 2026-03-14 |
| 5. Tool Execution | 4/4 | Complete | 2026-03-14 |
| 6. Tool Approval and Agentic Loop | 3/3 | Complete | 2026-03-14 |
| 7. Context Management | 2/2 | Complete | 2026-03-14 |
| 8. Session and Memory | 3/3 | Complete | 2026-03-15 |

### v2.0 Sub-Agent System (Active)

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 9. Agent Definitions and Loading | 2/2 | Complete   | 2026-03-21 |
| 10. Agent Execution and Result Communication | 0/3 | Not started | - |
| 11. Agent UI and Management Commands | 0/? | Not started | - |

---
*Roadmap created: 2026-03-14*
*v2.0 phases added: 2026-03-21*
