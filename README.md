# seval

[![CI](https://github.com/Hari-Nagarajan/seval/actions/workflows/ci.yml/badge.svg)](https://github.com/Hari-Nagarajan/seval/actions/workflows/ci.yml)
[![CodeQL](https://github.com/Hari-Nagarajan/seval/actions/workflows/codeql.yml/badge.svg)](https://github.com/Hari-Nagarajan/seval/actions/workflows/codeql.yml)
[![License: AGPL v3](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-orange?logo=rust)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0.html)
[![dependency audit](https://img.shields.io/badge/deps-audited-green?logo=rust)](https://github.com/Hari-Nagarajan/seval/actions/workflows/ci.yml)
[![Discord](https://img.shields.io/discord/1350338014253748234?logo=discord&label=discord)](https://discord.gg/c8zQwH4qvp)
[![GitHub issues](https://img.shields.io/github/issues/Hari-Nagarajan/seval)](https://github.com/Hari-Nagarajan/seval/issues)
[![PRs welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)
[![CLA assistant](https://cla-assistant.io/readme/badge/Hari-Nagarajan/seval)](https://cla-assistant.io/Hari-Nagarajan/seval)

AI-powered security research CLI built in Rust. Features a split-pane TUI dashboard, agentic tool execution via any LLM (AWS Bedrock or OpenRouter), session persistence, and context compression.

## Features

- **Streaming chat** with any LLM through AWS Bedrock or OpenRouter (Claude, GPT, Llama, Gemini, etc.)
- **10 built-in tools** the AI can invoke autonomously (shell, file ops, search, web)
- **Split-pane TUI** powered by ratatui -- chat on the left, tool output on the right
- **Session management** with SQLite persistence, resume/export/import
- **Context compression** via tiktoken to stay within model limits
- **Approval modes** to control tool execution safety
- **Project memories** the AI can save and recall across sessions
- **Configurable deny rules** to block dangerous shell commands
- **Markdown rendering** with syntax highlighting in the terminal
- **System prompt override** via `~/.seval/system.md`

## Prerequisites

- **Rust toolchain** (edition 2024, so Rust 1.85+)
- **AWS credentials** with Bedrock access, *or* an OpenRouter API key
- **Brave Search API key** (optional, enables the `web_search` tool)

## Setup

Run `seval` for the first time or use `seval init` to launch the interactive configuration wizard. This creates `~/.seval/config.toml`.

To re-run the wizard and overwrite an existing config:

```sh
seval init --force
```

### Configuration files

| File | Purpose |
|---|---|
| `~/.seval/config.toml` | Global config (credentials, provider, model, approval mode, deny rules) |
| `.seval/config.toml` | Project-local overrides (approval mode, deny rules, AWS settings) |
| `~/.seval/system.md` | Custom system prompt (replaces the default) |

### Example global config

```toml
[provider]
active = "bedrock"   # or "open-router"

[bedrock]
access_key_id = "AKIA..."
secret_access_key = "..."
region = "us-east-1"

[openrouter]
api_key = "sk-or-..."

[tools]
approval_mode = "default"
max_turns = 25
deny_rules = [
  "rm -rf /",
  "rm -rf /*",
  "chmod 777 /",
  "mkfs.*",
  "> /dev/sd*",
  "dd if=* of=/dev/*",
]

brave_api_key = "BSA..."
```

## Build & Install

```sh
cargo build --release
# Binary at target/release/seval
```

Or run directly:

```sh
cargo run --release
```

## Usage

```
seval [OPTIONS] [COMMAND]

Commands:
  init  Initialize configuration (interactive wizard)

Options:
  --profile <PROFILE>          AWS profile name
  --region <REGION>            AWS region
  --model <MODEL>              Bedrock model ID
  --approval-mode <MODE>       Tool approval mode [plan|default|auto-edit|yolo]
  --config <PATH>              Path to configuration file
  -h, --help                   Print help
  -V, --version                Print version
```

### Approval modes

| Mode | Behavior |
|---|---|
| `plan` | Read-only, no tool execution |
| `default` | Ask before write operations |
| `auto-edit` | Auto-approve file edits, ask for shell commands |
| `yolo` | Approve everything automatically |

### Slash commands

Type these in the chat input:

| Command | Description |
|---|---|
| `/model [name]` | Switch AI model (show current if no name) |
| `/sessions` | List saved sessions |
| `/sessions resume <id>` | Resume a saved session |
| `/sessions delete <id>` | Delete a saved session |
| `/import <path>` | Import a session from JSON |
| `/export [id]` | Export session to JSON |
| `/memory` | List project memories |
| `/memory delete <id>` | Delete a memory entry |
| `/help` | Show help |
| `/clear` | Clear conversation history |
| `/quit` or `/q` | Quit |

## Tools

The AI has access to 10 built-in tools during agentic execution:

| Tool | Description |
|---|---|
| `shell` | Execute shell commands |
| `read` | Read file contents |
| `write` | Write/create files |
| `edit` | Apply targeted edits to files |
| `grep` | Search file contents with regex |
| `glob` | Find files by glob pattern |
| `ls` | List directory contents |
| `web_fetch` | Fetch and extract text from a URL |
| `web_search` | Search the web via Brave Search API |
| `save_memory` | Persist a memory for future sessions |

## TUI Layout

The interface is a split-pane terminal UI:

- **Left pane** -- Chat conversation with streaming markdown rendering
- **Right pane** -- Tool execution output and approval prompts
- **Bottom bar** -- Input area and status indicators (model, token count, session)

## License

This project is licensed under the [GNU Affero General Public License v3.0](LICENSE).
