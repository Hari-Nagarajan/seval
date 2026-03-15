# Contributing to seval

Thanks for your interest in contributing! Here's how to get started.

## Contributor License Agreement

Before your first pull request can be merged, you must sign the [Contributor License Agreement](CLA.md). This is handled automatically — when you open a PR, the CLA Assistant bot will comment with a link to sign. You only need to do this once.

The CLA ensures that contributions can be properly licensed and that the project can evolve its licensing as needed.

## Getting Started

1. Fork the repo and create a branch from `main`
2. Enable the git hooks: `git config core.hooksPath .githooks`
3. Make your changes
4. Run `cargo build` and `cargo test` to verify
5. Run `cargo clippy` and fix any warnings
6. Open a pull request

The pre-push hook runs `cargo fmt --check`, `cargo clippy`, and `cargo test` automatically before each push.

## Code Style

- Follow existing patterns in the codebase
- Run `cargo fmt` before committing
- Keep changes focused — one feature or fix per PR

## Reporting Issues

Open a GitHub issue with:
- What you expected to happen
- What actually happened
- Steps to reproduce
- Your OS and Rust version

## Security Issues

If you find a security vulnerability, please see [SECURITY.md](SECURITY.md) for reporting instructions.
