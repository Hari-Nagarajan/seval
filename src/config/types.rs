//! Configuration types for the Seval CLI.

use serde::{Deserialize, Serialize};

use super::defaults;

/// Tool approval mode controlling how Seval handles tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalMode {
    /// Read-only mode, no tool execution.
    Plan,
    /// Ask before write operations (default).
    #[default]
    Default,
    /// Auto-approve file edits, ask for shell commands.
    AutoEdit,
    /// Approve everything automatically.
    Yolo,
}

/// AWS-related configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AwsConfig {
    /// AWS profile name.
    pub profile: Option<String>,
    /// AWS region.
    pub region: Option<String>,
    /// Bedrock model ID.
    pub model: Option<String>,
}

/// Tools configuration with defaults.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolsConfig {
    /// Tool approval mode.
    #[serde(default)]
    pub approval_mode: ApprovalMode,
    /// Deny rules for dangerous commands.
    #[serde(default = "defaults::default_deny_rules")]
    pub deny_rules: Vec<String>,
    /// Maximum turns for the agentic loop.
    #[serde(default = "defaults::default_max_turns")]
    pub max_turns: usize,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            approval_mode: ApprovalMode::default(),
            deny_rules: defaults::default_deny_rules(),
            max_turns: defaults::default_max_turns(),
        }
    }
}

/// AI provider kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    /// AWS Bedrock (access key + secret key + region).
    #[default]
    Bedrock,
    /// `OpenRouter` multi-model API.
    OpenRouter,
    /// `ChatGPT` via Codex CLI auth tokens (`~/.codex/auth.json`).
    ChatGpt,
}

/// Provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ProviderConfig {
    /// Active provider.
    #[serde(default)]
    pub active: ProviderKind,
    /// Model override.
    pub model: Option<String>,
}

/// AWS Bedrock API configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BedrockConfig {
    /// AWS access key ID.
    pub access_key_id: Option<String>,
    /// AWS secret access key.
    pub secret_access_key: Option<String>,
    /// AWS region (e.g. us-east-1).
    pub region: Option<String>,
}

/// `OpenRouter` API configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct OpenRouterConfig {
    /// API key for `OpenRouter`.
    pub api_key: Option<String>,
}

/// Global configuration stored at ~/.seval/config.toml.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct GlobalConfig {
    /// AWS configuration.
    #[serde(default)]
    pub aws: AwsConfig,
    /// Tools configuration.
    #[serde(default)]
    pub tools: ToolsConfig,
    /// Provider configuration.
    #[serde(default)]
    pub provider: ProviderConfig,
    /// Bedrock configuration.
    #[serde(default)]
    pub bedrock: BedrockConfig,
    /// `OpenRouter` configuration.
    #[serde(default)]
    pub openrouter: OpenRouterConfig,
    /// Brave Search API key for web search tool.
    #[serde(default)]
    pub brave_api_key: Option<String>,
}

/// Project-local tools configuration (all fields optional for override).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ProjectToolsConfig {
    /// Override approval mode.
    pub approval_mode: Option<ApprovalMode>,
    /// Override deny rules (replaces, not appends).
    pub deny_rules: Option<Vec<String>>,
}

/// Project-local configuration stored at .seval/config.toml.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ProjectConfig {
    /// Override AWS configuration.
    pub aws: Option<AwsConfig>,
    /// Override tools configuration.
    pub tools: Option<ProjectToolsConfig>,
}

/// Merged runtime configuration used by the application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    /// AWS configuration.
    pub aws: AwsConfig,
    /// Tools configuration.
    pub tools: ToolsConfig,
    /// Provider configuration.
    pub provider: ProviderConfig,
    /// Bedrock configuration.
    pub bedrock: BedrockConfig,
    /// `OpenRouter` configuration.
    pub openrouter: OpenRouterConfig,
    /// Brave Search API key for web search tool.
    pub brave_api_key: Option<String>,
}
