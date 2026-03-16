# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.1.2]: https://github.com/Hari-Nagarajan/seval/releases/tag/v0.1.2
[0.1.0]: https://github.com/Hari-Nagarajan/seval/compare/afd2520...v0.1.2
