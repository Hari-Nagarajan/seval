# Seval CLI

<p align="center">
  <a href="https://github.com/Hari-Nagarajan/seval/actions/workflows/ci.yml"><img src="https://github.com/Hari-Nagarajan/seval/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/Hari-Nagarajan/seval/actions/workflows/codeql.yml"><img src="https://github.com/Hari-Nagarajan/seval/actions/workflows/codeql.yml/badge.svg" alt="CodeQL"></a>
  <a href="https://crates.io/crates/seval"><img src="https://img.shields.io/crates/v/seval.svg" alt="crates.io"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-blue.svg" alt="License: AGPL v3"></a>
  <a href="https://blog.rust-lang.org/2025/06/26/Rust-1.91.0.html"><img src="https://img.shields.io/badge/MSRV-1.91-orange?logo=rust" alt="MSRV"></a>
  <a href="https://github.com/Hari-Nagarajan/seval/actions/workflows/ci.yml"><img src="https://img.shields.io/badge/deps-audited-green?logo=rust" alt="dependency audit"></a>
  <a href="CONTRIBUTING.md"><img src="https://img.shields.io/badge/PRs-welcome-brightgreen.svg" alt="PRs welcome"></a>
  <a href="https://cla-assistant.io/Hari-Nagarajan/seval"><img src="https://cla-assistant.io/readme/badge/Hari-Nagarajan/seval" alt="CLA assistant"></a>
</p>

<p align="center">
  <a href="https://discord.gg/c8zQwH4qvp"><img src="assets/discord.svg" alt="Discord" height="32"></a>
  <br>
  <a href="https://discord.gg/c8zQwH4qvp">Join the community</a>
</p>

Your offensive security assistant in the terminal. seval pairs any LLM with 10 built-in tools (shell access, file I/O, web recon) and lets it work autonomously through a real-time split-pane TUI. Point it at a pentest engagement, red team exercise, or your own infrastructure audit and let it handle the tedious work: enumeration, vulnerability discovery, exploit development, and reporting. You set the guardrails, it does the grinding.

---

## Install

**Homebrew** (macOS/Linux, no Rust required):

```sh
brew install Hari-Nagarajan/tap/seval
```

**Cargo** (requires Rust 1.91+):

```sh
cargo install seval
```

Then run the setup wizard:

```sh
seval init
```

> Requires either [AWS Bedrock](https://aws.amazon.com/bedrock/) credentials or an [OpenRouter](https://openrouter.ai/) API key.

Or build from source:

```sh
git clone https://github.com/Hari-Nagarajan/seval.git
cd seval && cargo build --release
```

---

## What It Does

**seval** gives you an AI assistant in your terminal that can actually do things. Ask it to investigate a target, and it will run commands, read files, search the web, and chain multiple tools together -- all while you watch in real time.

### Agentic Tool Execution

The AI has 10 built-in tools it can invoke autonomously in multi-turn loops:

`shell` | `read` | `write` | `edit` | `grep` | `glob` | `ls` | `web_fetch` | `web_search` | `save_memory`

You control what it's allowed to do via **approval modes**:

| Mode | What the AI Can Do |
|---|---|
| `plan` | Read-only recon -- no tools executed |
| `default` | Auto-approves reads, asks before writes or shell |
| `auto-edit` | Auto-approves reads and file edits, asks for shell |
| `yolo` | Full autonomy -- everything auto-approved |

```sh
seval --approval-mode yolo
```

### Split-Pane TUI

The interface shows the conversation on the left and tool activity on the right, with a status bar tracking the model, token usage, and session info.

### Multi-Provider Support

Connect to any LLM through **AWS Bedrock** or **OpenRouter**. Switch models mid-conversation with `/model`.

Supported model families include Claude, GPT, Llama, Gemini, Mistral, DeepSeek, and more.

### Session Persistence

Every conversation is saved to a local SQLite database. Resume previous sessions, export them as JSON, or import sessions from other machines.

### Project Memories

The AI can save important findings with `save_memory`. Memories are scoped to the current project directory and automatically loaded into future sessions -- so the AI remembers what it learned.

### Context Compression

When the conversation approaches the model's context limit, seval automatically summarizes older messages to free up space. A color-coded bar in the sidebar shows context usage at a glance.

---

## CLI Reference

```
seval [OPTIONS] [COMMAND]

Commands:
  init                         Interactive setup wizard

Options:
  --profile <PROFILE>          AWS profile name
  --region <REGION>            AWS region
  --model <MODEL>              Model ID
  --approval-mode <MODE>       plan | default | auto-edit | yolo
  --config <PATH>              Path to config file
  -h, --help                   Print help
  -V, --version                Print version
```

### Slash Commands

| Command | Description |
|---|---|
| `/model [name]` | Switch model (or show current) |
| `/sessions` | List saved sessions |
| `/sessions resume <id>` | Resume a session |
| `/sessions delete <id>` | Delete a session |
| `/import <path>` | Import session from JSON |
| `/export [id]` | Export session to JSON |
| `/memory` | List project memories |
| `/memory delete <id>` | Delete a memory |
| `/clear` | Clear conversation |
| `/help` | Show help |
| `/quit` | Quit |

---

## Documentation

See the **[Wiki](https://github.com/Hari-Nagarajan/seval/wiki)** for full documentation:

- **[Installation](https://github.com/Hari-Nagarajan/seval/wiki/Installation)** -- prerequisites, building, setup
- **[Usage](https://github.com/Hari-Nagarajan/seval/wiki/Usage)** -- keybindings, tools, approval modes, sessions
- **[Configuration](https://github.com/Hari-Nagarajan/seval/wiki/Configuration)** -- providers, config files, deny rules, custom system prompts
- **[Architecture](https://github.com/Hari-Nagarajan/seval/wiki/Architecture)** -- codebase structure and design patterns

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines. PRs welcome.

## License

[GNU Affero General Public License v3.0](LICENSE)
