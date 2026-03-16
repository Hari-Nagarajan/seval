# GitHub Copilot Instructions for seval

## Project Overview

seval is a Rust TUI chat application (terminal AI assistant) built with ratatui. It uses an async runtime (tokio), SQLite for session persistence, and supports multiple AI providers (AWS Bedrock, OpenRouter).

## Project Structure

```
src/
├── ai/          # AI provider integration, streaming, compression, system prompts
├── approval/    # Tool approval flow (display, hooks, policies)
├── chat/        # Chat UI component, input, markdown rendering, syntax highlighting
├── config/      # Global and project configuration (TOML)
├── session/     # SQLite database, import/export, memory tool
├── tools/       # Built-in tools (edit, glob, grep, ls, read, shell, web_fetch, write)
├── tui/         # Terminal UI (sidebar, wizard)
├── action.rs    # Application action/event enum
├── app.rs       # Application entry point
├── cli.rs       # CLI argument parsing
├── colors.rs    # Color definitions
├── errors.rs    # Error handling and panic hook
├── logging.rs   # Tracing setup with file appender
├── lib.rs       # Library root
└── main.rs      # Binary entry point
```

## Code Style and Conventions

### Rust Standards

- **Edition**: Rust 2024
- **Lints**: `clippy::all` and `clippy::pedantic` are set to `deny`. Code must pass both with zero warnings.
- **Formatting**: `rustfmt` with project config in `rustfmt.toml`. Run `cargo fmt` before committing.
- **Allowed pedantic exceptions**: `module_name_repetitions`, `missing_errors_doc`, `must_use_candidate`

### Patterns

- Prefer `anyhow::Result` for error handling in application code.
- Use `tokio::task::spawn_blocking` for database operations and other blocking work.
- Channel-based communication: actions flow through `UnboundedSender<Action>` for UI updates.
- Session IDs are UUIDs — always truncate to 8 chars in user-facing messages (never log full session IDs).
- Write unit tests in the same file as the code being tested, not in separate test files.

### What to Avoid

- **No over-engineering**: Don't add abstractions, feature flags, or configurability beyond what's needed.
- **No unnecessary comments**: Only add comments where logic isn't self-evident. Don't add doc comments to unchanged code.
- **No secrets in logs**: Session IDs, API keys, and credentials must never appear in full in log output or user messages.

## Commit Messages

This project enforces [Conventional Commits v1.0.0](https://www.conventionalcommits.org/en/v1.0.0/). A `commit-msg` hook validates this automatically.

Format: `<type>[optional scope]: <description>`

### Types and When to Use Them

| Type | When to use | Example |
|------|-------------|---------|
| `feat` | New user-facing functionality or capability | `feat: add session export command` |
| `fix` | Bug fix that corrects wrong behavior | `fix(chat): prevent panic on empty session ID` |
| `docs` | Documentation only — README, comments, CONTRIBUTING, etc. | `docs: add copilot instructions` |
| `style` | Code formatting, whitespace, semicolons — no logic change | `style: apply rustfmt to chat module` |
| `refactor` | Code restructuring that doesn't change behavior or fix a bug | `refactor(tools): extract common truncation helper` |
| `perf` | Performance improvement with no functional change | `perf: cache compiled syntax highlighters` |
| `test` | Adding or updating tests only — no production code change | `test(session): add round-trip export test` |
| `build` | Build system, dependencies, Cargo.toml changes | `build: update ratatui 0.29 to 0.30` |
| `ci` | CI/CD config — workflows, hooks, GitHub Actions | `ci: add commit-msg hook enforcing conventional commits` |
| `chore` | Maintenance that doesn't fit above — license files, .gitignore, tooling config | `chore: add CDLA-Permissive-2.0 to deny.toml` |
| `revert` | Reverts a previous commit | `revert: undo session export feature` |

### Choosing the Right Type

- If a commit fixes a bug **and** adds a test for it, use `fix` (the test supports the fix).
- If a commit updates a dependency to fix a security advisory, use `build` (not `fix`, unless it fixes broken behavior in seval itself).
- If a commit changes both production code and tests, use the type that matches the production change.
- If a commit touches CI config **and** adds a git hook, use `ci`.
- Use `chore` as a last resort — most changes fit a more specific type.

### Scopes

Scopes are optional but encouraged when the change is confined to a specific module:
`ai`, `chat`, `config`, `session`, `tools`, `tui`, `approval`

### Breaking Changes

Append `!` after the type/scope to signal a breaking change: `refactor(config)!: rename provider field`

Use the commit body to explain what breaks and how to migrate.

### Commit Body

The first line should be concise (under 72 chars). Use the body for:
- Why the change was made (not just what changed — the diff shows that)
- Context that helps someone reading the changelog
- Breaking change migration instructions

## Pull Request Guidelines

- Keep PRs small and focused — one feature or fix per PR.
- Split refactoring from behavioral changes into separate PRs.
- Run `cargo fmt`, `cargo clippy`, and `cargo test` before pushing (the pre-push hook does this automatically).
- Ensure all 297+ existing tests continue to pass.
- New functionality must include tests.

## Code Review Priorities

When reviewing PRs, focus on:

1. **Security**: No cleartext logging of sensitive data (session IDs, credentials). No command injection, XSS, or SQL injection. Follow OWASP top 10.
2. **Correctness**: Does the code do what it claims? Are edge cases handled?
3. **Simplicity**: Is this the simplest solution? Could it be done with less code?
4. **Consistency**: Does it follow existing patterns in the codebase?
5. **Tests**: Is new code tested? Are existing tests preserved?

### Red Flags

- Changes to lint config (`clippy.toml`, `rustfmt.toml`, `Cargo.toml [lints]`) without discussion.
- Changes to CI workflows without clear justification.
- Large PRs (500+ lines) that mix refactoring with new features.
- Verbose, overly-commented, or non-idiomatic Rust code.
- Removal of existing tests without justification.
- Dependencies with incompatible licenses (project is AGPL-3.0-only).

## Configuration Files

Do not modify these without prior discussion:
- `clippy.toml`, `rustfmt.toml` — affects all contributors
- `.github/workflows/` — CI/CD pipeline
- `deny.toml` — dependency license and advisory policy
- `Cargo.toml` lint sections — project-wide lint policy
