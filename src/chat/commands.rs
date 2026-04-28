//! Slash command parsing.
//!
//! Parses user input that begins with `/` into structured commands.

use serde::{Deserialize, Serialize};

/// A slash command entered by the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlashCommand {
    /// Switch AI model. Argument is the model name, if provided.
    Model(Option<String>),
    /// Switch AI provider: `/provider [bedrock|openrouter|chatgpt]`.
    Provider(Option<String>),
    /// Show help text listing available commands.
    Help,
    /// Clear conversation history.
    Clear,
    /// Quit the application.
    Quit,
    /// Session management: `/sessions [list|resume <id>|delete <id>]`.
    Sessions(Option<String>),
    /// Memory management: `/memory [delete <id>]`.
    Memory(Option<String>),
    /// Import a SEVAL-CLI session: `/import <path>`.
    Import(String),
    /// Export a session to SEVAL-CLI format: `/export [session_id]`.
    Export(Option<String>),
    /// Agent management: `/agents [info <name>|status|cancel <name>|create <name>]`.
    Agents(Option<String>),
    /// Unrecognized command.
    Unknown(String),
}

impl SlashCommand {
    /// Parse user input into a slash command.
    ///
    /// Returns `None` if the input is not a slash command (doesn't start with `/`).
    #[must_use]
    pub fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        if !trimmed.starts_with('/') {
            return None;
        }
        let mut parts = trimmed[1..].splitn(2, ' ');
        let cmd = parts.next()?;
        let arg = parts
            .next()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        Some(match cmd {
            "model" => Self::Model(arg),
            "provider" => Self::Provider(arg),
            "help" => Self::Help,
            "clear" => Self::Clear,
            "sessions" => Self::Sessions(arg),
            "memory" => Self::Memory(arg),
            "import" => {
                if let Some(path) = arg {
                    Self::Import(path)
                } else {
                    Self::Unknown("import (missing path argument)".to_string())
                }
            }
            "export" => Self::Export(arg),
            "agents" => Self::Agents(arg),
            "quit" | "q" => Self::Quit,
            other => Self::Unknown(other.to_string()),
        })
    }

    /// Returns help text listing all available commands.
    #[must_use]
    pub fn help_text() -> &'static str {
        "\
Available commands:
  /provider [name] - Switch provider (bedrock, openrouter, chatgpt)
  /model [name]  - Switch AI model (show current if no name given)
  /sessions      - List saved sessions
  /sessions resume <id> - Resume a saved session
  /sessions delete <id> - Delete a saved session
  /import <path> - Import a SEVAL-CLI session JSON file
  /export [id]   - Export session to SEVAL-CLI JSON (current if no id)
  /memory        - List project memories
  /memory delete <id> - Delete a memory entry
  /agents        - List available agents
  /agents info <name>   - Show agent configuration
  /agents status        - Show running/completed agents
  /agents cancel <name> - Cancel a running agent
  /agents create <name> - Create new agent template
  /help          - Show this help message
  /clear         - Clear conversation history
  /quit or /q    - Quit the application"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_model_with_arg() {
        assert_eq!(
            SlashCommand::parse("/model claude-sonnet"),
            Some(SlashCommand::Model(Some("claude-sonnet".to_string())))
        );
    }

    #[test]
    fn parse_model_no_arg() {
        assert_eq!(
            SlashCommand::parse("/model"),
            Some(SlashCommand::Model(None))
        );
    }

    #[test]
    fn parse_help() {
        assert_eq!(SlashCommand::parse("/help"), Some(SlashCommand::Help));
    }

    #[test]
    fn parse_clear() {
        assert_eq!(SlashCommand::parse("/clear"), Some(SlashCommand::Clear));
    }

    #[test]
    fn parse_quit() {
        assert_eq!(SlashCommand::parse("/quit"), Some(SlashCommand::Quit));
    }

    #[test]
    fn parse_q_alias() {
        assert_eq!(SlashCommand::parse("/q"), Some(SlashCommand::Quit));
    }

    #[test]
    fn parse_unknown_command() {
        assert_eq!(
            SlashCommand::parse("/unknown"),
            Some(SlashCommand::Unknown("unknown".to_string()))
        );
    }

    #[test]
    fn parse_not_a_command() {
        assert_eq!(SlashCommand::parse("hello"), None);
    }

    #[test]
    fn parse_empty_string() {
        assert_eq!(SlashCommand::parse(""), None);
    }

    #[test]
    fn parse_import_with_path() {
        assert_eq!(
            SlashCommand::parse("/import /path/to/file.json"),
            Some(SlashCommand::Import("/path/to/file.json".to_string()))
        );
    }

    #[test]
    fn parse_import_no_path_is_unknown() {
        match SlashCommand::parse("/import") {
            Some(SlashCommand::Unknown(_)) => {} // expected
            other => panic!("Expected Unknown for /import without path, got {other:?}"),
        }
    }

    #[test]
    fn parse_export_no_arg() {
        assert_eq!(
            SlashCommand::parse("/export"),
            Some(SlashCommand::Export(None))
        );
    }

    #[test]
    fn parse_export_with_session_id() {
        assert_eq!(
            SlashCommand::parse("/export abc123"),
            Some(SlashCommand::Export(Some("abc123".to_string())))
        );
    }

    #[test]
    fn parse_agents_no_arg() {
        assert_eq!(
            SlashCommand::parse("/agents"),
            Some(SlashCommand::Agents(None))
        );
    }

    #[test]
    fn parse_agents_with_subcommand() {
        assert_eq!(
            SlashCommand::parse("/agents info test-agent"),
            Some(SlashCommand::Agents(Some("info test-agent".to_string())))
        );
    }

    #[test]
    fn parse_agents_status() {
        assert_eq!(
            SlashCommand::parse("/agents status"),
            Some(SlashCommand::Agents(Some("status".to_string())))
        );
    }

    #[test]
    fn parse_provider_no_arg() {
        assert_eq!(
            SlashCommand::parse("/provider"),
            Some(SlashCommand::Provider(None))
        );
    }

    #[test]
    fn parse_provider_with_arg() {
        assert_eq!(
            SlashCommand::parse("/provider chatgpt"),
            Some(SlashCommand::Provider(Some("chatgpt".to_string())))
        );
    }

    #[test]
    fn help_text_is_not_empty() {
        let text = SlashCommand::help_text();
        assert!(text.contains("/provider"));
        assert!(text.contains("/model"));
        assert!(text.contains("/help"));
        assert!(text.contains("/clear"));
        assert!(text.contains("/quit"));
        assert!(text.contains("/import"));
        assert!(text.contains("/export"));
        assert!(text.contains("/memory"));
        assert!(text.contains("/agents"));
    }
}
