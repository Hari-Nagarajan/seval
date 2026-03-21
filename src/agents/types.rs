//! Agent definition types.
//!
//! Defines the data structures for agent definitions parsed from AGENT.md files.

use serde::Deserialize;

use crate::config::ApprovalMode;

fn default_temperature() -> f64 {
    0.7
}

fn default_max_time_minutes() -> u32 {
    10
}

/// Frontmatter fields parsed from the TOML block in an AGENT.md file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentFrontmatter {
    /// Agent name (required).
    pub name: String,
    /// Short description of the agent's purpose.
    pub description: Option<String>,
    /// Model alias or full model ID (required).
    pub model: String,
    /// Sampling temperature clamped to [0.0, 1.0].
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    /// Maximum number of agentic loop turns (required).
    pub max_turns: u32,
    /// Maximum wall-clock time in minutes.
    #[serde(default = "default_max_time_minutes")]
    pub max_time_minutes: u32,
    /// Allowlist of tool names. If non-empty, only these tools are available.
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// Denylist of tool names. Used only when `allowed_tools` is empty.
    #[serde(default)]
    pub denied_tools: Vec<String>,
    /// Optional approval mode override for this agent.
    pub approval_mode: Option<ApprovalMode>,
}

/// A fully parsed agent definition combining frontmatter and system prompt.
#[derive(Debug, Clone)]
pub struct AgentDef {
    /// Parsed frontmatter fields.
    pub frontmatter: AgentFrontmatter,
    /// System prompt (the markdown body after the closing +++ delimiter).
    pub system_prompt: String,
    /// Where this agent definition was loaded from.
    pub source: AgentSource,
}

/// Origin of an agent definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSource {
    /// Bundled with the binary.
    BuiltIn,
    /// Loaded from `~/.seval/agents/`.
    UserGlobal,
    /// Loaded from `.seval/agents/` in the project directory.
    ProjectLocal,
}
