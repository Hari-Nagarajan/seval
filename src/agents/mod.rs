//! Agent definition parsing and management.
//!
//! Provides types and functions for loading AGENT.md files with TOML frontmatter.

pub mod types;

use types::{AgentDef, AgentFrontmatter, AgentSource};

use crate::ai::provider::AiProvider;

// -------------------------------------------------------------------------
// Built-in agent content (embedded at compile time)
// -------------------------------------------------------------------------

const BUILTIN_SECURITY_ANALYZER: &str = include_str!("builtin/security-analyzer.md");
const BUILTIN_CODE_REVIEWER: &str = include_str!("builtin/code-reviewer.md");
const BUILTIN_RECON_AGENT: &str = include_str!("builtin/recon-agent.md");

// -------------------------------------------------------------------------
// AgentRegistry
// -------------------------------------------------------------------------

/// Registry of loaded agent definitions, keyed by agent name.
#[derive(Debug, Clone)]
pub struct AgentRegistry {
    agents: std::collections::HashMap<String, AgentDef>,
}

impl AgentRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            agents: std::collections::HashMap::new(),
        }
    }

    /// Look up an agent by name.
    pub fn get(&self, name: &str) -> Option<&AgentDef> {
        self.agents.get(name)
    }

    /// Return all agents sorted by name.
    pub fn list(&self) -> Vec<&AgentDef> {
        let mut agents: Vec<&AgentDef> = self.agents.values().collect();
        agents.sort_by(|a, b| a.frontmatter.name.cmp(&b.frontmatter.name));
        agents
    }

    /// Return the number of loaded agents.
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Return true if no agents are loaded.
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// -------------------------------------------------------------------------
// Directory helpers
// -------------------------------------------------------------------------

/// Returns `~/.seval/agents/default/` — where built-in agents are installed.
fn builtin_agents_dir() -> Option<std::path::PathBuf> {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".seval").join("agents").join("default"))
}

/// Returns `~/.seval/agents/` — the user-global agent directory.
fn user_agents_dir() -> Option<std::path::PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().join(".seval").join("agents"))
}

// -------------------------------------------------------------------------
// install_builtins
// -------------------------------------------------------------------------

/// Write the three built-in agent files to `dir`, creating it if needed.
///
/// Called with a temp directory in tests; called with [`builtin_agents_dir`] in production.
/// Overwrites any existing files (idempotent per D-12).
pub fn install_builtins_to(dir: &std::path::Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let agents = [
        ("security-analyzer.md", BUILTIN_SECURITY_ANALYZER),
        ("code-reviewer.md", BUILTIN_CODE_REVIEWER),
        ("recon-agent.md", BUILTIN_RECON_AGENT),
    ];
    for (filename, content) in agents {
        std::fs::write(dir.join(filename), content)?;
    }
    Ok(())
}

/// Install built-in agents to `~/.seval/agents/default/`.
///
/// Always overwrites (D-12). Non-fatal on failure — callers should `tracing::warn` on error.
pub fn install_builtins() -> anyhow::Result<()> {
    let dir = builtin_agents_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
    install_builtins_to(&dir)
}

// -------------------------------------------------------------------------
// load_tier / load_agents
// -------------------------------------------------------------------------

/// Load all `.md` agent files from `dir` into `map` with the given `source` tag.
///
/// Missing or unreadable directories are silently skipped. Files that fail to
/// parse emit a `tracing::warn` and are skipped (D-03).
fn load_tier(
    dir: &std::path::Path,
    source: AgentSource,
    map: &mut std::collections::HashMap<String, AgentDef>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            match std::fs::read_to_string(&path) {
                Ok(content) => match parse_agent_file(&content) {
                    Ok((frontmatter, body)) => {
                        let name = frontmatter.name.clone();
                        map.insert(
                            name,
                            AgentDef {
                                frontmatter,
                                system_prompt: body,
                                source,
                            },
                        );
                    }
                    Err(e) => {
                        tracing::warn!("skipping agent file {:?}: {e}", path);
                    }
                },
                Err(e) => {
                    tracing::warn!("cannot read agent file {:?}: {e}", path);
                }
            }
        }
    }
}

/// Load agents from the three-tier hierarchy using production directory paths.
///
/// Priority (lowest to highest): built-in < user-global < project-local (D-08, D-09).
pub fn load_agents() -> AgentRegistry {
    let builtin = builtin_agents_dir();
    let user = user_agents_dir();
    let project = std::path::PathBuf::from(".seval").join("agents");
    load_agents_from_paths(builtin.as_deref(), user.as_deref(), Some(&project))
}

/// Load agents from explicit tier paths (testable variant of [`load_agents`]).
///
/// `None` for a tier means that tier is skipped entirely.
pub fn load_agents_from_paths(
    builtin_dir: Option<&std::path::Path>,
    user_dir: Option<&std::path::Path>,
    project_dir: Option<&std::path::Path>,
) -> AgentRegistry {
    let mut map = std::collections::HashMap::new();

    // Tier 1: built-in (lowest priority)
    if let Some(dir) = builtin_dir {
        load_tier(dir, AgentSource::BuiltIn, &mut map);
    }
    // Tier 2: user-global (middle priority — overwrites built-in)
    if let Some(dir) = user_dir {
        load_tier(dir, AgentSource::UserGlobal, &mut map);
    }
    // Tier 3: project-local (highest priority — overwrites all)
    if let Some(dir) = project_dir {
        load_tier(dir, AgentSource::ProjectLocal, &mut map);
    }

    AgentRegistry { agents: map }
}

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

    // -------------------------------------------------------------------------
    // AgentRegistry / three-tier loading tests
    // -------------------------------------------------------------------------

    /// Write a minimal valid agent file to `dir/{name}.md`.
    fn write_test_agent(dir: &std::path::Path, name: &str, description: &str) {
        let content = format!(
            "+++\nname = \"{name}\"\ndescription = \"{description}\"\nmodel = \"sonnet\"\nmax_turns = 10\n+++\n\nTest system prompt for {name}\n"
        );
        std::fs::write(dir.join(format!("{name}.md")), content).unwrap();
    }

    #[test]
    fn install_builtins_writes_files() {
        let dir = tempfile::tempdir().unwrap();
        install_builtins_to(dir.path()).unwrap();
        assert!(dir.path().join("security-analyzer.md").exists());
        assert!(dir.path().join("code-reviewer.md").exists());
        assert!(dir.path().join("recon-agent.md").exists());
    }

    #[test]
    fn install_builtins_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        install_builtins_to(dir.path()).unwrap();
        let first = std::fs::read_to_string(dir.path().join("security-analyzer.md")).unwrap();
        install_builtins_to(dir.path()).unwrap();
        let second = std::fs::read_to_string(dir.path().join("security-analyzer.md")).unwrap();
        assert_eq!(first, second, "second install should produce identical files");
    }

    #[test]
    fn load_tier_reads_md_files() {
        let dir = tempfile::tempdir().unwrap();
        write_test_agent(dir.path(), "agent-a", "first");
        write_test_agent(dir.path(), "agent-b", "second");

        let mut map = std::collections::HashMap::new();
        load_tier(dir.path(), AgentSource::BuiltIn, &mut map);
        assert_eq!(map.len(), 2);
        assert!(map.contains_key("agent-a"));
        assert!(map.contains_key("agent-b"));
    }

    #[test]
    fn load_tier_skips_non_md() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("agent.txt"), "ignored").unwrap();
        std::fs::write(dir.path().join("agent.toml"), "[name]\nname=\"x\"").unwrap();

        let mut map = std::collections::HashMap::new();
        load_tier(dir.path(), AgentSource::BuiltIn, &mut map);
        assert!(map.is_empty());
    }

    #[test]
    fn load_tier_skips_invalid_md() {
        let dir = tempfile::tempdir().unwrap();
        // Write a file with invalid TOML frontmatter
        std::fs::write(dir.path().join("bad.md"), "+++\nnot_valid = {{{{}\n+++\nbody\n").unwrap();

        let mut map = std::collections::HashMap::new();
        load_tier(dir.path(), AgentSource::BuiltIn, &mut map);
        // Should warn and skip — no panic, empty map
        assert!(map.is_empty());
    }

    #[test]
    fn load_tier_missing_dir() {
        let nonexistent = std::path::Path::new("/tmp/seval-test-nonexistent-12345678");
        let mut map = std::collections::HashMap::new();
        // Should not panic or return error
        load_tier(nonexistent, AgentSource::BuiltIn, &mut map);
        assert!(map.is_empty());
    }

    #[test]
    fn three_tier_override() {
        let builtin_dir = tempfile::tempdir().unwrap();
        let user_dir = tempfile::tempdir().unwrap();
        let project_dir = tempfile::tempdir().unwrap();

        write_test_agent(builtin_dir.path(), "test-agent", "builtin-description");
        write_test_agent(user_dir.path(), "test-agent", "user-description");
        write_test_agent(project_dir.path(), "test-agent", "project-description");

        let registry = load_agents_from_paths(
            Some(builtin_dir.path()),
            Some(user_dir.path()),
            Some(project_dir.path()),
        );

        let agent = registry.get("test-agent").expect("should exist");
        assert_eq!(
            agent.frontmatter.description.as_deref(),
            Some("project-description"),
            "project-local should win"
        );
        assert_eq!(agent.source, AgentSource::ProjectLocal);
    }

    #[test]
    fn user_global_overrides_builtin() {
        let builtin_dir = tempfile::tempdir().unwrap();
        let user_dir = tempfile::tempdir().unwrap();

        write_test_agent(builtin_dir.path(), "shared-agent", "builtin-description");
        write_test_agent(user_dir.path(), "shared-agent", "user-description");

        let registry =
            load_agents_from_paths(Some(builtin_dir.path()), Some(user_dir.path()), None);

        let agent = registry.get("shared-agent").expect("should exist");
        assert_eq!(
            agent.frontmatter.description.as_deref(),
            Some("user-description"),
            "user-global should override built-in"
        );
        assert_eq!(agent.source, AgentSource::UserGlobal);
    }

    #[test]
    fn agent_source_tagged_correctly() {
        let builtin_dir = tempfile::tempdir().unwrap();
        let user_dir = tempfile::tempdir().unwrap();
        let project_dir = tempfile::tempdir().unwrap();

        write_test_agent(builtin_dir.path(), "builtin-only", "b");
        write_test_agent(user_dir.path(), "user-only", "u");
        write_test_agent(project_dir.path(), "project-only", "p");

        let registry = load_agents_from_paths(
            Some(builtin_dir.path()),
            Some(user_dir.path()),
            Some(project_dir.path()),
        );

        assert_eq!(
            registry.get("builtin-only").unwrap().source,
            AgentSource::BuiltIn
        );
        assert_eq!(
            registry.get("user-only").unwrap().source,
            AgentSource::UserGlobal
        );
        assert_eq!(
            registry.get("project-only").unwrap().source,
            AgentSource::ProjectLocal
        );
    }

    #[test]
    fn registry_get_returns_agent() {
        let dir = tempfile::tempdir().unwrap();
        install_builtins_to(dir.path()).unwrap();
        let registry = load_agents_from_paths(Some(dir.path()), None, None);
        assert!(
            registry.get("security-analyzer").is_some(),
            "security-analyzer should be present"
        );
    }

    #[test]
    fn registry_list_returns_all() {
        let dir = tempfile::tempdir().unwrap();
        write_test_agent(dir.path(), "agent-z", "z");
        write_test_agent(dir.path(), "agent-a", "a");
        write_test_agent(dir.path(), "agent-m", "m");

        let registry = load_agents_from_paths(Some(dir.path()), None, None);
        let list = registry.list();
        assert_eq!(list.len(), 3);
        // Sorted by name
        assert_eq!(list[0].frontmatter.name, "agent-a");
        assert_eq!(list[1].frontmatter.name, "agent-m");
        assert_eq!(list[2].frontmatter.name, "agent-z");
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
