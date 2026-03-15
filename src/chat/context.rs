//! Context state tracking for token usage and context window management.
//!
//! Provides `ContextState` for monitoring token budget consumption,
//! threshold detection for compression triggers, and context window
//! size lookup for different model providers.

use ratatui::style::Color;

/// Tracks context window usage and compression state.
#[derive(Debug, Clone)]
pub struct ContextState {
    /// Total context window size in tokens.
    pub context_window: u64,
    /// Tokens currently used in the context.
    pub tokens_used: u64,
    /// Whether compression is currently in progress.
    pub compressing: bool,
    /// Number of messages since last compression.
    pub messages_since_compression: usize,
}

impl ContextState {
    /// Create a new context state with the given window size.
    #[must_use]
    pub fn new(context_window: u64) -> Self {
        Self {
            context_window,
            tokens_used: 0,
            compressing: false,
            messages_since_compression: 0,
        }
    }

    /// Calculate the usage ratio (0.0 to 1.0).
    ///
    /// Returns 0.0 if `context_window` is 0 to avoid division by zero.
    #[must_use]
    pub fn usage_ratio(&self) -> f64 {
        if self.context_window == 0 {
            return 0.0;
        }
        #[allow(clippy::cast_precision_loss)]
        let ratio = self.tokens_used as f64 / self.context_window as f64;
        ratio.clamp(0.0, 1.0)
    }

    /// Whether proactive compression should be triggered.
    ///
    /// True when usage is 70-84%, not currently compressing, and at least
    /// 2 messages since last compression.
    #[must_use]
    pub fn needs_proactive_compression(&self) -> bool {
        let ratio = self.usage_ratio();
        (0.70..0.85).contains(&ratio) && !self.compressing && self.messages_since_compression >= 2
    }

    /// Whether enforced compression should be triggered.
    ///
    /// True when usage is 85%+, not currently compressing, and at least
    /// 2 messages since last compression.
    #[must_use]
    pub fn needs_enforced_compression(&self) -> bool {
        let ratio = self.usage_ratio();
        ratio >= 0.85 && !self.compressing && self.messages_since_compression >= 2
    }

    /// Update token usage from the latest API response.
    ///
    /// `input_tokens` is the API-reported input token count from the latest
    /// response, which represents the actual context size sent to the model.
    pub fn update_tokens(&mut self, input_tokens: u64) {
        self.tokens_used = input_tokens;
        self.messages_since_compression += 1;
    }

    /// Reset state after a compression operation completes.
    ///
    /// Sets the new token count, clears the compressing flag, and resets
    /// the messages-since-compression cooldown counter.
    pub fn reset_after_compression(&mut self, new_tokens_used: u64) {
        self.tokens_used = new_tokens_used;
        self.compressing = false;
        self.messages_since_compression = 0;
    }

    /// Get the color for the current usage zone.
    ///
    /// Green (0-69%), Yellow (70-84%), Red (85%+).
    #[must_use]
    pub fn color_zone(&self) -> Color {
        let ratio = self.usage_ratio();
        if ratio >= 0.85 {
            Color::Red
        } else if ratio >= 0.70 {
            Color::Yellow
        } else {
            Color::Green
        }
    }
}

/// Format a token count for compact sidebar display.
///
/// Returns "156k" for >= 1000, "500" for < 1000.
/// Uses integer division for the "k" suffix.
#[must_use]
pub fn format_token_count(tokens: u64) -> String {
    if tokens >= 1000 {
        format!("{}k", tokens / 1000)
    } else {
        tokens.to_string()
    }
}

/// Look up the context window size for a Bedrock model.
///
/// Uses hardcoded values based on model family:
/// - Claude models: 200,000 tokens
/// - Llama models: 128,000 tokens
/// - Mistral models: 32,000 tokens
/// - Unknown: 128,000 tokens (with warning logged via `tracing`)
#[must_use]
pub fn bedrock_context_window(model_id: &str) -> u64 {
    let id = model_id.to_lowercase();
    if id.contains("claude") {
        200_000
    } else if id.contains("llama") {
        128_000
    } else if id.contains("mistral") {
        32_000
    } else {
        tracing::warn!(
            "Unknown Bedrock model '{}', using 128k context window fallback",
            model_id
        );
        128_000
    }
}

/// Fetch the context window size for an `OpenRouter` model via API.
///
/// Tries the single-model endpoint first, then falls back to searching the
/// full models list (some models like `openrouter/hunter-alpha` are only
/// available there).
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the model is not found.
pub async fn fetch_openrouter_context_length(model_id: &str) -> anyhow::Result<u64> {
    let client = reqwest::Client::new();

    // Try single-model endpoint first.
    let encoded = urlencoding::encode(model_id);
    let url = format!("https://openrouter.ai/api/v1/models/{encoded}");
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await?;

    if resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        if let Some(ctx) = body
            .get("data")
            .and_then(|d| d.get("context_length"))
            .or_else(|| body.get("context_length"))
            .and_then(serde_json::Value::as_u64)
        {
            return Ok(ctx);
        }
    }

    // Fall back to the models list endpoint.
    let list_resp = client
        .get("https://openrouter.ai/api/v1/models")
        .header("Accept", "application/json")
        .send()
        .await?;

    if !list_resp.status().is_success() {
        anyhow::bail!("OpenRouter models list API returned {}", list_resp.status());
    }

    let list_body: serde_json::Value = list_resp.json().await?;
    let context_length = list_body
        .get("data")
        .and_then(|d| d.as_array())
        .and_then(|models| {
            models
                .iter()
                .find(|m| m.get("id").and_then(|v| v.as_str()) == Some(model_id))
        })
        .and_then(|m| m.get("context_length"))
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Could not find context_length for model '{model_id}' in OpenRouter response"
            )
        })?;

    Ok(context_length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_state_with_window_and_zero_used() {
        let state = ContextState::new(200_000);
        assert_eq!(state.context_window, 200_000);
        assert_eq!(state.tokens_used, 0);
        assert!(!state.compressing);
        assert_eq!(state.messages_since_compression, 0);
    }

    #[test]
    fn usage_ratio_zero_when_no_tokens_used() {
        let state = ContextState::new(200_000);
        assert!((state.usage_ratio() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn usage_ratio_half_when_half_used() {
        let mut state = ContextState::new(200_000);
        state.tokens_used = 100_000;
        assert!((state.usage_ratio() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn usage_ratio_clamps_to_one_when_exceeding_window() {
        let mut state = ContextState::new(200_000);
        state.tokens_used = 300_000;
        assert!((state.usage_ratio() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn usage_ratio_zero_when_window_is_zero() {
        let state = ContextState::new(0);
        assert!((state.usage_ratio() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn needs_proactive_compression_true_in_range() {
        let mut state = ContextState::new(200_000);
        state.tokens_used = 150_000; // 75%
        state.messages_since_compression = 3;
        assert!(state.needs_proactive_compression());
    }

    #[test]
    fn needs_proactive_compression_false_at_85_percent() {
        let mut state = ContextState::new(200_000);
        state.tokens_used = 170_000; // 85%
        state.messages_since_compression = 3;
        assert!(!state.needs_proactive_compression());
    }

    #[test]
    fn needs_enforced_compression_true_at_85_percent() {
        let mut state = ContextState::new(200_000);
        state.tokens_used = 170_000; // 85%
        state.messages_since_compression = 3;
        assert!(state.needs_enforced_compression());
    }

    #[test]
    fn compression_false_when_compressing() {
        let mut state = ContextState::new(200_000);
        state.tokens_used = 170_000; // 85%
        state.compressing = true;
        state.messages_since_compression = 3;
        assert!(!state.needs_proactive_compression());
        assert!(!state.needs_enforced_compression());
    }

    #[test]
    fn compression_false_when_too_few_messages() {
        let mut state = ContextState::new(200_000);
        state.tokens_used = 170_000; // 85%
        state.messages_since_compression = 1;
        assert!(!state.needs_proactive_compression());
        assert!(!state.needs_enforced_compression());
    }

    #[test]
    fn bedrock_context_window_claude_sonnet() {
        assert_eq!(
            bedrock_context_window("us.anthropic.claude-sonnet-4-20250514-v1:0"),
            200_000
        );
    }

    #[test]
    fn bedrock_context_window_claude_35_sonnet() {
        assert_eq!(
            bedrock_context_window("us.anthropic.claude-3-5-sonnet-20241022-v2:0"),
            200_000
        );
    }

    #[test]
    fn bedrock_context_window_llama() {
        assert_eq!(
            bedrock_context_window("meta.llama3-70b-instruct-v1:0"),
            128_000
        );
    }

    #[test]
    fn bedrock_context_window_unknown_fallback() {
        assert_eq!(bedrock_context_window("unknown-model"), 128_000);
    }

    #[test]
    fn format_token_count_large() {
        assert_eq!(format_token_count(156_000), "156k");
    }

    #[test]
    fn format_token_count_small() {
        assert_eq!(format_token_count(500), "500");
    }

    #[test]
    fn format_token_count_exactly_1000() {
        assert_eq!(format_token_count(1000), "1k");
    }

    #[test]
    fn format_token_count_1500() {
        assert_eq!(format_token_count(1500), "1k");
    }

    #[test]
    fn color_zone_green() {
        let mut state = ContextState::new(200_000);
        state.tokens_used = 100_000; // 50%
        assert_eq!(state.color_zone(), Color::Green);
    }

    #[test]
    fn color_zone_yellow() {
        let mut state = ContextState::new(200_000);
        state.tokens_used = 150_000; // 75%
        assert_eq!(state.color_zone(), Color::Yellow);
    }

    #[test]
    fn color_zone_red() {
        let mut state = ContextState::new(200_000);
        state.tokens_used = 180_000; // 90%
        assert_eq!(state.color_zone(), Color::Red);
    }

    #[test]
    fn update_tokens_sets_used_and_increments_messages() {
        let mut state = ContextState::new(200_000);
        state.update_tokens(50_000);
        assert_eq!(state.tokens_used, 50_000);
        assert_eq!(state.messages_since_compression, 1);
        state.update_tokens(75_000);
        assert_eq!(state.tokens_used, 75_000);
        assert_eq!(state.messages_since_compression, 2);
    }
}
