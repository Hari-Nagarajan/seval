//! AI provider abstraction.
//!
//! Wraps Rig's Bedrock and `OpenRouter` clients in a unified enum to avoid
//! complex generics threading throughout the codebase.

use anyhow::{Context, Result};
use aws_config::BehaviorVersion;
use rig::providers::openrouter;

use crate::config::AppConfig;
use crate::config::ProviderKind;

/// Default model for Bedrock.
const DEFAULT_BEDROCK_MODEL: &str = "us.anthropic.claude-sonnet-4-20250514-v1:0";

/// Default model for `OpenRouter` API.
const DEFAULT_OPENROUTER_MODEL: &str = "anthropic/claude-sonnet-4-6";

/// Unified AI provider abstracting over Bedrock and `OpenRouter` backends.
#[derive(Debug, Clone)]
pub enum AiProvider {
    /// AWS Bedrock client.
    Bedrock {
        /// The Rig Bedrock client.
        client: rig_bedrock::client::Client,
        /// Model identifier.
        model: String,
    },
    /// `OpenRouter` multi-model API client.
    OpenRouter {
        /// The Rig `OpenRouter` client.
        client: openrouter::Client,
        /// Model identifier (prefixed, e.g. "anthropic/claude-sonnet-4-6").
        model: String,
    },
}

impl AiProvider {
    /// Create a provider from the application configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the required credentials are missing for the active provider.
    pub async fn from_config(config: &AppConfig) -> Result<Self> {
        match config.provider.active {
            ProviderKind::Bedrock => {
                let bedrock = &config.bedrock;
                let access_key = bedrock
                    .access_key_id
                    .as_ref()
                    .context("Bedrock access key ID is required. Set it in ~/.seval/config.toml under [bedrock] access_key_id")?
                    .clone();
                let secret_key = bedrock
                    .secret_access_key
                    .as_ref()
                    .context("Bedrock secret access key is required. Set it in ~/.seval/config.toml under [bedrock] secret_access_key")?
                    .clone();
                let region = bedrock.region.as_deref().unwrap_or("us-east-1").to_string();

                let sdk_config = aws_config::defaults(BehaviorVersion::latest())
                    .credentials_provider(aws_sdk_bedrockruntime::config::Credentials::new(
                        access_key,
                        secret_key,
                        None,
                        None,
                        "seval-config",
                    ))
                    .region(aws_config::Region::new(region))
                    .load()
                    .await;

                let aws_client = aws_sdk_bedrockruntime::Client::new(&sdk_config);
                let client: rig_bedrock::client::Client = aws_client.into();

                let model = config
                    .provider
                    .model
                    .clone()
                    .unwrap_or_else(|| DEFAULT_BEDROCK_MODEL.to_string());
                Ok(Self::Bedrock { client, model })
            }
            ProviderKind::OpenRouter => {
                let api_key = config
                    .openrouter
                    .api_key
                    .as_ref()
                    .context("OpenRouter API key is required. Set it in ~/.seval/config.toml under [openrouter] api_key")?;
                let client = openrouter::Client::new(api_key)
                    .map_err(|e| anyhow::anyhow!("Failed to create OpenRouter client: {e}"))?;
                let model = config
                    .provider
                    .model
                    .clone()
                    .unwrap_or_else(|| DEFAULT_OPENROUTER_MODEL.to_string());
                Ok(Self::OpenRouter { client, model })
            }
        }
    }

    /// Get the model name in use.
    #[must_use]
    pub fn model_name(&self) -> &str {
        match self {
            Self::Bedrock { model, .. } | Self::OpenRouter { model, .. } => model,
        }
    }

    /// Get the provider name.
    #[must_use]
    pub fn provider_name(&self) -> &str {
        match self {
            Self::Bedrock { .. } => "bedrock",
            Self::OpenRouter { .. } => "openrouter",
        }
    }

    /// Get the context window size for the current model.
    ///
    /// For Bedrock, uses hardcoded lookup. For `OpenRouter`, queries the API
    /// with a 128k fallback on failure.
    pub async fn context_window_size(&self) -> u64 {
        match self {
            Self::Bedrock { model, .. } => crate::chat::context::bedrock_context_window(model),
            Self::OpenRouter { model, .. } => {
                match crate::chat::context::fetch_openrouter_context_length(model).await {
                    Ok(size) => size,
                    Err(e) => {
                        tracing::warn!(
                            "Failed to fetch OpenRouter context window: {e}, using 128k fallback"
                        );
                        128_000
                    }
                }
            }
        }
    }

    /// Update the model at runtime (e.g. via `/model` command).
    pub fn set_model(&mut self, new_model: String) {
        match self {
            Self::Bedrock { model, .. } | Self::OpenRouter { model, .. } => {
                *model = new_model;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AppConfig, AwsConfig, BedrockConfig, OpenRouterConfig, ProviderConfig, ToolsConfig,
    };

    fn make_config(
        kind: ProviderKind,
        bedrock_keys: Option<(&str, &str, &str)>,
        openrouter_key: Option<&str>,
    ) -> AppConfig {
        AppConfig {
            aws: AwsConfig::default(),
            tools: ToolsConfig::default(),
            provider: ProviderConfig {
                active: kind,
                model: None,
            },
            bedrock: BedrockConfig {
                access_key_id: bedrock_keys.map(|(k, _, _)| k.to_string()),
                secret_access_key: bedrock_keys.map(|(_, s, _)| s.to_string()),
                region: bedrock_keys.map(|(_, _, r)| r.to_string()),
            },
            openrouter: OpenRouterConfig {
                api_key: openrouter_key.map(String::from),
            },
            brave_api_key: None,
        }
    }

    #[tokio::test]
    async fn from_config_bedrock_with_keys_creates_provider() {
        let config = make_config(
            ProviderKind::Bedrock,
            Some(("AKIATEST", "secret123", "us-east-1")),
            None,
        );
        let provider = AiProvider::from_config(&config).await.unwrap();
        assert_eq!(provider.provider_name(), "bedrock");
        assert_eq!(provider.model_name(), DEFAULT_BEDROCK_MODEL);
    }

    #[tokio::test]
    async fn from_config_openrouter_with_key_creates_provider() {
        let config = make_config(ProviderKind::OpenRouter, None, Some("sk-or-test-key"));
        let provider = AiProvider::from_config(&config).await.unwrap();
        assert_eq!(provider.provider_name(), "openrouter");
        assert_eq!(provider.model_name(), DEFAULT_OPENROUTER_MODEL);
    }

    #[tokio::test]
    async fn from_config_bedrock_missing_keys_errors() {
        let config = make_config(ProviderKind::Bedrock, None, None);
        let err = AiProvider::from_config(&config).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Bedrock access key"),
            "unexpected error: {msg}"
        );
    }

    #[tokio::test]
    async fn from_config_openrouter_missing_key_errors() {
        let config = make_config(ProviderKind::OpenRouter, None, None);
        let err = AiProvider::from_config(&config).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("OpenRouter API key"),
            "unexpected error: {msg}"
        );
    }

    #[tokio::test]
    async fn set_model_updates_name() {
        let config = make_config(
            ProviderKind::Bedrock,
            Some(("AKIATEST", "secret123", "us-east-1")),
            None,
        );
        let mut provider = AiProvider::from_config(&config).await.unwrap();
        provider.set_model("claude-haiku".to_string());
        assert_eq!(provider.model_name(), "claude-haiku");
    }
}
