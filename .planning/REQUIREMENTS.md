# Requirements: Rust Security CLI (seval)

**Defined:** 2026-03-21
**Core Value:** A fast, single-binary AI security CLI with a polished dashboard TUI that fully replaces SEVAL-CLI as a daily driver

## v1 Requirements (Completed — v1.0)

All 55 v1.0 requirements are complete. See traceability section for phase mapping.

### Core Infrastructure (Complete)
- [x] **INFRA-01** – **INFRA-06**: Single binary, fast startup, async event loop, crash/signal handling, file logging

### Onboarding (Complete)
- [x] **ONBD-01** – **ONBD-05**: Init wizard, CLI flags, global + project config

### AI Chat (Complete)
- [x] **CHAT-01** – **CHAT-12**: Streaming chat, multi-turn, markdown, syntax highlighting, context compression, credential refresh

### Tool System (Complete)
- [x] **TOOL-01** – **TOOL-17**: Shell, file ops, search, web, agentic loop, approval modes, deny rules

### TUI Dashboard (Complete)
- [x] **TUI-01** – **TUI-08**: Split-pane layout, sidebar, input area, status bar, scrolling

### Session Management (Complete)
- [x] **SESS-01** – **SESS-07**: Save/resume/list/delete, SEVAL-CLI compat, persistent memory

---

## v2 Requirements — Sub-Agent System

**Milestone:** v2.0 Sub-Agent System
**Defined:** 2026-03-21

### Agent Definitions

- [x] **AGENTDEF-01**: User can define an agent via AGENT.md with YAML frontmatter (name, model, temperature, max_turns, max_time_minutes, allowed_tools, denied_tools, approval_mode) — markdown body becomes the agent's system prompt
- [ ] **AGENTDEF-02**: Agent definitions load from three directory tiers — built-in (`~/.seval/agents/default/`), user-global (`~/.seval/agents/`), project-local (`.seval/agents/`) — with project-local overriding user-global overriding built-in

### Agent Execution

- [ ] **AGENTEXEC-01**: AI can invoke `spawn_agent` tool with agent_name, task, and optional context — receives immediate confirmation while agent runs asynchronously
- [ ] **AGENTEXEC-02**: Spawned agent runs via `tokio::spawn` with its own isolated `Vec<Message>` history (not the parent's conversation)
- [ ] **AGENTEXEC-03**: Spawned agent's tool set is filtered per `allowed_tools` / `denied_tools` from its definition
- [ ] **AGENTEXEC-04**: Spawned agent respects `max_turns` (turn counter) and `max_time_minutes` (tokio timeout), returning partial results on limit
- [ ] **AGENTEXEC-05**: User can cancel a running agent via `/agents cancel <name>`, terminating the task and returning partial results
- [ ] **AGENTEXEC-06**: Nested agent spawning is prevented — `spawn_agent` tool is not registered within agent tool sets

### Result Communication

- [ ] **AGENTRES-01**: Agent result is delivered to parent AI via `Action::AgentCompleted` and displayed as a formatted system message in chat
- [ ] **AGENTRES-02**: Agent output is injected into the parent's `rig_history` so the AI can reference findings on the next turn
- [ ] **AGENTRES-03**: Agent conversation is stored in SQLite as a child session with `parent_session_id` linking to parent

### Built-in Agents

- [x] **AGENTBI-01**: Three built-in agents ship with seval: `security-analyzer` (vuln analysis / MITRE ATT&CK), `code-reviewer` (OWASP secure coding), `recon-agent` (reconnaissance / OSINT)
- [ ] **AGENTBI-02**: Built-in agents are installed to `~/.seval/agents/default/` on first run and usable as templates

### Management Commands

- [ ] **AGENTCMD-01**: User can list all available agents with `/agents` (name, description, source directory)
- [ ] **AGENTCMD-02**: User can view agent details with `/agents info <name>` (model, tools, max_turns, system prompt preview)
- [ ] **AGENTCMD-03**: User can view running agents with `/agents status` (turn progress, elapsed time)
- [ ] **AGENTCMD-04**: User can scaffold a new agent template with `/agents create <name>`

### Sidebar Display

- [ ] **AGENTUI-01**: Sidebar displays running agents with spinner, name, and turn progress (e.g., `[|] security-analyzer turn 3/25`)
- [ ] **AGENTUI-02**: Sidebar displays recently completed agents with elapsed time (e.g., `security-analyzer done (45s)`)

## v3 Requirements

Deferred beyond v2.0. Tracked but not in current roadmap.

### Advanced Features

- **ADVF-01**: Knowledge Base integration (Bedrock KB attach/detach/query for RAG over private data)
- **ADVF-02**: Skills system (injectable .md prompt files with activation rules)
- **ADVF-04**: Report generation (extract findings by severity into markdown/HTML deliverables)
- **ADVF-05**: Vim keybindings for input area (modal normal/insert modes)
- **ADVF-06**: Theme/color customization from config
- **ADVF-07**: Scrollable chat with virtual scrolling for performance (thousands of messages)

### MCP Support

- **MCP-01**: Connect to external MCP servers via Model Context Protocol
- **MCP-02**: MCP server management (enable/disable/list)
- **MCP-03**: MCP tool discovery and execution
- **MCP-04**: OAuth support for credential-gated MCP servers

## Out of Scope

| Feature | Reason |
|---------|--------|
| Multi-provider AI (OpenAI, Anthropic direct) | Bedrock provides access to Claude/Mistral/Llama; scope explosion |
| IDE integration (VS Code, Zed) | CLI-first focus; different UX paradigm entirely |
| Web/mobile interface | Terminal tool stays terminal |
| Built-in vulnerability scanner | Integrate existing tools via shell/MCP; be the orchestrator |
| Nested agent spawning | Resource exhaustion risk; prevents infinite recursion |
| Agent-to-agent communication | Overly complex; parent-mediated results are sufficient |
| Agent marketplace | Premature; file-based agents provide extensibility |

## Traceability

### v2.0 Requirements

| Requirement | Phase | Status |
|-------------|-------|--------|
| AGENTDEF-01 | Phase 9 | Complete |
| AGENTDEF-02 | Phase 9 | Pending |
| AGENTEXEC-01 | Phase 10 | Pending |
| AGENTEXEC-02 | Phase 10 | Pending |
| AGENTEXEC-03 | Phase 10 | Pending |
| AGENTEXEC-04 | Phase 10 | Pending |
| AGENTEXEC-05 | Phase 10 | Pending |
| AGENTEXEC-06 | Phase 10 | Pending |
| AGENTRES-01 | Phase 10 | Pending |
| AGENTRES-02 | Phase 10 | Pending |
| AGENTRES-03 | Phase 10 | Pending |
| AGENTBI-01 | Phase 9 | Complete |
| AGENTBI-02 | Phase 9 | Pending |
| AGENTCMD-01 | Phase 11 | Pending |
| AGENTCMD-02 | Phase 11 | Pending |
| AGENTCMD-03 | Phase 11 | Pending |
| AGENTCMD-04 | Phase 11 | Pending |
| AGENTUI-01 | Phase 11 | Pending |
| AGENTUI-02 | Phase 11 | Pending |

**Coverage:**
- v2.0 requirements: 19 total
- Mapped to phases: 19
- Unmapped: 0 ✓

---
*Requirements defined: 2026-03-21*
*Last updated: 2026-03-21 after v2.0 milestone initialization*
