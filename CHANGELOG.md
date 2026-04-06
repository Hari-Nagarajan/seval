# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.3] - 2026-04-06

### Added
- **Sub-Agent System** — AI can spawn specialized agents that run asynchronously with isolated context
- Agent definition format: AGENT.md files with TOML `+++` frontmatter (name, model, temperature, max_turns, allowed_tools, denied_tools, approval_mode)
- Three-tier agent loading: built-in (`~/.seval/agents/default/`) → user-global (`~/.seval/agents/`) → project-local (`.seval/agents/`)
- Three built-in agents: `security-analyzer` (vuln analysis / MITRE ATT&CK), `code-reviewer` (OWASP secure coding), `recon-agent` (reconnaissance / OSINT)
- `spawn_agent` tool for AI to launch agents with agent_name, task, and optional context
- Parallel agent execution via `tokio::spawn` with isolated message history and filtered tool sets
- Agent result delivery: immediate chat display + queued injection into AI context on next turn
- Agent child sessions stored in SQLite with `parent_session_id` linking to parent
- Sidebar live agent display with spinner, turn counter, and elapsed time
- `/agents` command: list all available agents with source tags (`[built-in]`/`[user]`/`[project]`)
- `/agents info <name>`: full agent configuration with 5-line system prompt preview
- `/agents status`: running agents with live turn progress and completed session log
- `/agents cancel <name>`: two-step confirmation, abort with partial result delivery
- `/agents create <name>`: scaffold new AGENT.md template to `~/.seval/agents/`
- Model alias resolution: short names (`sonnet`, `haiku`, `opus`) resolve to provider-specific model IDs
- Allowlist-first tool filtering: `allowed_tools` takes priority, `denied_tools` used only when allowlist is empty

### Changed
- `ApprovalHook` now supports `effective_tool_filter` for per-agent tool filtering
- `StreamChatParams` extended with agent-related fields (registry, handles, session ID, approval)
- SQLite schema migration 2: `parent_session_id` column on sessions table

### Fixed
- Agent turn counter now reads actual `Arc<AtomicUsize>` from `ApprovalHook` instead of hardcoded 0

### Security
- Updated `aws-lc-sys` 0.38.0 → 0.39.1 (RUSTSEC-2026-0044, RUSTSEC-2026-0048)
- Updated `rustls-webpki` 0.103.9 → 0.103.10 (RUSTSEC-2026-0049)

## [0.1.2] - 2026-03-16

### Added
- Published to [crates.io](https://crates.io/crates/seval)
- Homebrew tap (`brew install Hari-Nagarajan/tap/seval`)
- GitHub Wiki with full documentation
- CI workflow to auto-update Homebrew formula on release
- OSV-Scanner workflow for vulnerability scanning
- GitHub issue/PR templates
- Contributor Covenant Code of Conduct
- CLA integration

### Fixed
- Cleartext logging of session IDs (CWE-532)

### Changed
- Updated ratatui 0.29 to 0.30, fixing lru soundness advisory

## [0.1.0] - 2026-03-13

### Added
- Initial release
- Streaming chat with AWS Bedrock and OpenRouter providers
- 10 built-in tools: shell, read, write, edit, grep, glob, ls, web_fetch, web_search, save_memory
- Split-pane TUI with ratatui (chat left, tools right)
- Session persistence with SQLite
- Session import/export in JSON format
- Project memories scoped by working directory
- Context compression with proactive and enforced thresholds
- Interactive configuration wizard (`seval init`)
- Approval modes: plan, default, auto-edit, yolo
- Configurable deny rules for shell commands
- Markdown rendering with syntax highlighting
- Custom system prompt override via `~/.seval/system.md`
- Project-local config overrides (`.seval/config.toml`)

[0.1.3]: https://github.com/Hari-Nagarajan/seval/releases/tag/v0.1.3
[0.1.2]: https://github.com/Hari-Nagarajan/seval/releases/tag/v0.1.2
[0.1.0]: https://github.com/Hari-Nagarajan/seval/compare/afd2520...v0.1.2
