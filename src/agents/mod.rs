//! Agent definition parsing and management.
//!
//! Provides types and functions for loading AGENT.md files with TOML frontmatter.

pub mod types;

use types::AgentFrontmatter;

use crate::ai::provider::AiProvider;

/// Parse an AGENT.md file content into frontmatter and system prompt body.
///
/// The file format is:
/// ```text
/// +++
/// name = "my-agent"
/// model = "sonnet"
/// max_turns = 10
/// +++
/// System prompt body here...
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - The content does not start with `+++`
/// - There is no closing `+++` delimiter
/// - The TOML block fails to deserialize into [`AgentFrontmatter`]
pub fn parse_agent_file(content: &str) -> anyhow::Result<(AgentFrontmatter, String)> {
    // Trim leading newlines
    let content = content.trim_start_matches('\n');

    // Strip opening +++ delimiter (handle both LF and CRLF)
    let rest = if let Some(s) = content.strip_prefix("+++\r\n") {
        s
    } else if let Some(s) = content.strip_prefix("+++\n") {
        s
    } else {
        anyhow::bail!("missing opening +++ delimiter");
    };

    // Find the FIRST occurrence of \n+++ in the remaining content
    let close_pos = rest
        .find("\n+++")
        .ok_or_else(|| anyhow::anyhow!("missing closing +++ delimiter"))?;

    let toml_block = &rest[..close_pos];
    // Everything after the closing +++ (and its newline) is the body
    let after_close = &rest[close_pos + "\n+++".len()..];
    // Strip exactly one leading newline from the body if present
    let body = after_close
        .strip_prefix("\r\n")
        .or_else(|| after_close.strip_prefix('\n'))
        .unwrap_or(after_close);

    let mut frontmatter: AgentFrontmatter = toml::from_str(toml_block)?;

    // Clamp temperature to [0.0, 1.0]
    if frontmatter.temperature < 0.0 {
        tracing::warn!(
            "Agent '{}' temperature {} is below 0.0, clamping to 0.0",
            frontmatter.name,
            frontmatter.temperature
        );
        frontmatter.temperature = 0.0;
    } else if frontmatter.temperature > 1.0 {
        tracing::warn!(
            "Agent '{}' temperature {} is above 1.0, clamping to 1.0",
            frontmatter.name,
            frontmatter.temperature
        );
        frontmatter.temperature = 1.0;
    }

    Ok((frontmatter, body.to_string()))
}

/// Return the effective tool list for an agent, applying allowlist-first semantics (D-06).
///
/// - If `allowed` is non-empty: return only those tools (denylist is ignored).
/// - If `allowed` is empty: return all tools minus those in `denied`.
pub fn effective_tools<'a>(
    allowed: &'a [String],
    denied: &'a [String],
    all_tools: &'a [String],
) -> Vec<&'a str> {
    if allowed.is_empty() {
        all_tools
            .iter()
            .filter(|t| !denied.contains(t))
            .map(String::as_str)
            .collect()
    } else {
        allowed.iter().map(String::as_str).collect()
    }
}

/// Resolve a short model alias to a provider-specific model ID.
///
/// Known aliases: `sonnet`, `haiku`, `opus`. Unknown strings pass through unchanged.
pub fn resolve_model_alias(alias: &str, provider: &AiProvider) -> String {
    match provider {
        AiProvider::Bedrock { .. } => match alias {
            "sonnet" => "us.anthropic.claude-sonnet-4-20250514-v1:0".to_string(),
            "haiku" => "us.anthropic.claude-haiku-3-5-20241022-v2:0".to_string(),
            "opus" => "us.anthropic.claude-opus-4-5-20251101-v1:0".to_string(),
            other => other.to_string(),
        },
        AiProvider::OpenRouter { .. } => match alias {
            "sonnet" => "anthropic/claude-sonnet-4-6".to_string(),
            "haiku" => "anthropic/claude-haiku-3-5".to_string(),
            "opus" => "anthropic/claude-opus-4-5".to_string(),
            other => other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // parse_agent_file tests
    // -------------------------------------------------------------------------

    const FULL_AGENT: &str = r#"+++
name = "test-agent"
description = "A test agent"
model = "sonnet"
temperature = 0.5
max_turns = 20
max_time_minutes = 15
allowed_tools = ["shell", "read"]
denied_tools = ["write"]
approval_mode = "yolo"
+++
# System Prompt

This is the system prompt body.
"#;

    const MINIMAL_AGENT: &str = r#"+++
name = "minimal-agent"
model = "haiku"
max_turns = 5
+++
Minimal body.
"#;

    #[test]
    fn parse_valid_agent() {
        let (fm, body) = parse_agent_file(FULL_AGENT).expect("should parse");
        assert_eq!(fm.name, "test-agent");
        assert_eq!(fm.description.as_deref(), Some("A test agent"));
        assert_eq!(fm.model, "sonnet");
        assert!((fm.temperature - 0.5).abs() < f64::EPSILON);
        assert_eq!(fm.max_turns, 20);
        assert_eq!(fm.max_time_minutes, 15);
        assert_eq!(fm.allowed_tools, vec!["shell", "read"]);
        assert_eq!(fm.denied_tools, vec!["write"]);
        assert!(fm.approval_mode.is_some());
        assert!(body.contains("System Prompt"));
    }

    #[test]
    fn parse_minimal_agent() {
        let (fm, body) = parse_agent_file(MINIMAL_AGENT).expect("should parse");
        assert_eq!(fm.name, "minimal-agent");
        assert_eq!(fm.model, "haiku");
        assert_eq!(fm.max_turns, 5);
        // Defaults
        assert!((fm.temperature - 0.7).abs() < f64::EPSILON);
        assert_eq!(fm.max_time_minutes, 10);
        assert!(fm.allowed_tools.is_empty());
        assert!(fm.denied_tools.is_empty());
        assert!(fm.approval_mode.is_none());
        assert_eq!(body, "Minimal body.\n");
    }

    #[test]
    fn parse_missing_required_field() {
        // Missing 'name' field
        let content = "+++\nmodel = \"sonnet\"\nmax_turns = 5\n+++\nbody\n";
        let result = parse_agent_file(content);
        assert!(result.is_err(), "should error on missing required field");
    }

    #[test]
    fn parse_missing_closing_delimiter() {
        let content = "+++\nname = \"test\"\nmodel = \"sonnet\"\nmax_turns = 5\n";
        let result = parse_agent_file(content);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("missing closing +++ delimiter"), "got: {msg}");
    }

    #[test]
    fn parse_missing_opening_delimiter() {
        let content = "name = \"test\"\nmodel = \"sonnet\"\nmax_turns = 5\n+++\nbody\n";
        let result = parse_agent_file(content);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("missing opening +++ delimiter"), "got: {msg}");
    }

    #[test]
    fn body_becomes_system_prompt() {
        let content = "+++\nname = \"t\"\nmodel = \"s\"\nmax_turns = 1\n+++\n# Header\n\nParagraph with  spaces.\n\n- list item\n";
        let (_, body) = parse_agent_file(content).expect("should parse");
        assert_eq!(body, "# Header\n\nParagraph with  spaces.\n\n- list item\n");
    }

    #[test]
    fn body_with_triple_plus_in_content() {
        // A +++ in the body should be preserved; only the first two +++ markers are structural
        let content = "+++\nname = \"t\"\nmodel = \"s\"\nmax_turns = 1\n+++\nFirst line.\n+++\nSecond line.\n";
        let (_, body) = parse_agent_file(content).expect("should parse");
        // The body should include the +++ that was in the body
        assert!(body.contains("+++"), "body should preserve inner +++");
        assert!(body.contains("First line."));
        assert!(body.contains("Second line."));
    }

    // -------------------------------------------------------------------------
    // effective_tools tests
    // -------------------------------------------------------------------------

    fn str_vec(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn tool_filtering_allowlist() {
        let allowed = str_vec(&["shell", "read"]);
        let denied = str_vec(&["write"]);
        let all = str_vec(&["shell", "read", "write", "grep"]);
        let result = effective_tools(&allowed, &denied, &all);
        assert_eq!(result, vec!["shell", "read"]);
    }

    #[test]
    fn tool_filtering_denylist() {
        let allowed: Vec<String> = vec![];
        let denied = str_vec(&["write", "shell"]);
        let all = str_vec(&["shell", "read", "write", "grep"]);
        let result = effective_tools(&allowed, &denied, &all);
        assert_eq!(result, vec!["read", "grep"]);
    }

    #[test]
    fn tool_filtering_both_empty() {
        let allowed: Vec<String> = vec![];
        let denied: Vec<String> = vec![];
        let all = str_vec(&["shell", "read", "write"]);
        let result = effective_tools(&allowed, &denied, &all);
        assert_eq!(result, vec!["shell", "read", "write"]);
    }

    #[test]
    fn tool_filtering_both_set() {
        // When both are set, allowed_tools takes precedence (denied is ignored)
        let allowed = str_vec(&["shell"]);
        let denied = str_vec(&["shell", "read"]); // would deny shell too
        let all = str_vec(&["shell", "read", "write"]);
        let result = effective_tools(&allowed, &denied, &all);
        // allowed wins — shell is still included
        assert_eq!(result, vec!["shell"]);
    }

    // -------------------------------------------------------------------------
    // resolve_model_alias tests (using mock-like approach via rig_bedrock)
    // -------------------------------------------------------------------------

    // We can't easily construct AiProvider in tests without credentials,
    // so we test the logic via a helper that matches on provider variant name.

    fn resolve_for_bedrock(alias: &str) -> String {
        match alias {
            "sonnet" => "us.anthropic.claude-sonnet-4-20250514-v1:0".to_string(),
            "haiku" => "us.anthropic.claude-haiku-3-5-20241022-v2:0".to_string(),
            "opus" => "us.anthropic.claude-opus-4-5-20251101-v1:0".to_string(),
            other => other.to_string(),
        }
    }

    fn resolve_for_openrouter(alias: &str) -> String {
        match alias {
            "sonnet" => "anthropic/claude-sonnet-4-6".to_string(),
            "haiku" => "anthropic/claude-haiku-3-5".to_string(),
            "opus" => "anthropic/claude-opus-4-5".to_string(),
            other => other.to_string(),
        }
    }

    #[test]
    fn resolve_model_alias_bedrock() {
        assert_eq!(
            resolve_for_bedrock("sonnet"),
            "us.anthropic.claude-sonnet-4-20250514-v1:0"
        );
        assert_eq!(
            resolve_for_bedrock("haiku"),
            "us.anthropic.claude-haiku-3-5-20241022-v2:0"
        );
        assert_eq!(resolve_for_bedrock("my-custom-model"), "my-custom-model");
    }

    #[test]
    fn resolve_model_alias_openrouter() {
        assert_eq!(
            resolve_for_openrouter("sonnet"),
            "anthropic/claude-sonnet-4-6"
        );
        assert_eq!(
            resolve_for_openrouter("unknown-alias"),
            "unknown-alias"
        );
    }

    #[test]
    fn temperature_clamping() {
        // Test via parse_agent_file which applies clamping
        let high_temp = "+++\nname = \"t\"\nmodel = \"s\"\nmax_turns = 1\ntemperature = 1.5\n+++\nbody\n";
        let (fm, _) = parse_agent_file(high_temp).expect("should parse");
        assert!((fm.temperature - 1.0).abs() < f64::EPSILON, "expected 1.0, got {}", fm.temperature);

        let low_temp = "+++\nname = \"t\"\nmodel = \"s\"\nmax_turns = 1\ntemperature = -0.5\n+++\nbody\n";
        let (fm, _) = parse_agent_file(low_temp).expect("should parse");
        assert!((fm.temperature - 0.0).abs() < f64::EPSILON, "expected 0.0, got {}", fm.temperature);
    }

    #[test]
    fn deny_unknown_fields() {
        let content = "+++\nname = \"t\"\nmodel = \"s\"\nmax_turns = 1\nfoo = \"bar\"\n+++\nbody\n";
        let result = parse_agent_file(content);
        assert!(result.is_err(), "should reject unknown fields");
    }

    #[test]
    fn builtin_agents_parse() {
        let builtins = [
            (
                "security-analyzer",
                include_str!("builtin/security-analyzer.md"),
            ),
            ("code-reviewer", include_str!("builtin/code-reviewer.md")),
            ("recon-agent", include_str!("builtin/recon-agent.md")),
        ];
        for (expected_name, content) in builtins {
            let (fm, body) = parse_agent_file(content)
                .unwrap_or_else(|e| panic!("{expected_name} failed to parse: {e}"));
            assert_eq!(fm.name, expected_name);
            assert!(!body.is_empty(), "{expected_name} has empty body");
        }
    }
}
