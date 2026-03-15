//! AI-powered context compression.
//!
//! Provides conversation summarization to reduce token usage when the context
//! window approaches capacity. Splits messages into a batch to compress and
//! recent messages to keep verbatim, then uses the AI provider to generate
//! a concise summary.

use std::fmt::Write;
use std::sync::Arc;

use anyhow::{bail, Result};
use rig::client::CompletionClient;
use rig::completion::Prompt;
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;
use crate::ai::provider::AiProvider;
use crate::chat::message::{ChatMessage, Role};

/// Number of recent messages to keep verbatim (2 user-assistant exchanges).
const KEEP_RECENT: usize = 4;

/// Max tokens for proactive compression summary response.
const PROACTIVE_MAX_TOKENS: u64 = 4096;

/// Max tokens for enforced compression summary response.
const ENFORCED_MAX_TOKENS: u64 = 2048;

/// Proactive compression prompt (30-40% reduction target).
const PROACTIVE_COMPRESSION_PROMPT: &str = "\
You are a conversation summarizer. Summarize the following conversation history \
concisely while preserving:
1. All tool execution results and their outputs
2. Key decisions and findings
3. Important technical details and code snippets
4. The overall context and goals of the conversation

Omit: greetings, small talk, repeated explanations, verbose tool arguments.

Target length: Reduce the content to approximately 30-40% of its current size.

Format: Write a single coherent summary paragraph or bullet list. \
Do NOT include meta-commentary about the summarization process.";

/// Enforced compression prompt (20-25% reduction, aggressive).
const ENFORCED_COMPRESSION_PROMPT: &str = "\
You are a conversation summarizer performing aggressive compression. \
Summarize the following conversation to approximately 20-25% of its size.

MUST preserve:
- Tool results (what was found/executed)
- Key decisions and conclusions
- Critical code/config snippets

Aggressively omit everything else. Be extremely concise.";

/// Result of a compression operation.
#[derive(Debug, Clone)]
pub struct CompressionResult {
    /// The AI-generated summary text.
    pub summary: String,
    /// Number of messages that were compressed (removed).
    pub messages_removed: usize,
    /// Estimated token count of the summary (chars / 4).
    pub estimated_summary_tokens: u64,
}

/// Split messages into (compress, keep) batches.
///
/// The last `keep_recent` messages are kept verbatim. If there are not
/// enough messages to compress (i.e., total <= `keep_recent`), the compress
/// batch is empty.
#[must_use]
pub fn split_messages(
    messages: &[ChatMessage],
    keep_recent: usize,
) -> (Vec<&ChatMessage>, Vec<&ChatMessage>) {
    if messages.len() <= keep_recent {
        return (Vec::new(), messages.iter().collect());
    }
    let split_at = messages.len() - keep_recent;
    let compress_batch = messages[..split_at].iter().collect();
    let keep_batch = messages[split_at..].iter().collect();
    (compress_batch, keep_batch)
}

/// Build the summarization prompt from a batch of messages.
///
/// Uses the proactive or enforced compression system prompt, followed by
/// the formatted conversation history.
fn build_summarization_prompt(messages: &[&ChatMessage], aggressive: bool) -> String {
    let system = if aggressive {
        ENFORCED_COMPRESSION_PROMPT
    } else {
        PROACTIVE_COMPRESSION_PROMPT
    };

    let mut prompt = format!("{system}\n\n--- CONVERSATION ---\n");
    for msg in messages {
        let role_label = match msg.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::System => "System",
        };
        let _ = writeln!(prompt, "{role_label}: {}", msg.content);
    }
    prompt
}

/// Compress a conversation by summarizing older messages with AI.
///
/// Splits messages into a batch to compress and recent messages to keep,
/// builds a summarization prompt, and calls the AI provider for a summary.
///
/// # Errors
///
/// Returns an error if there are not enough messages to compress, or if
/// the AI call fails.
pub async fn compress_conversation(
    provider: &AiProvider,
    messages: &[ChatMessage],
    aggressive: bool,
) -> Result<CompressionResult> {
    let (to_compress, _to_keep) = split_messages(messages, KEEP_RECENT);

    if to_compress.is_empty() {
        bail!("Not enough messages to compress");
    }

    let prompt = build_summarization_prompt(&to_compress, aggressive);
    let max_tokens = if aggressive {
        ENFORCED_MAX_TOKENS
    } else {
        PROACTIVE_MAX_TOKENS
    };

    let summary: String = match provider {
        AiProvider::Bedrock { client, model } => {
            let agent = client
                .agent(model)
                .max_tokens(max_tokens)
                .build();
            agent.prompt(&prompt).await?
        }
        AiProvider::OpenRouter { client, model } => {
            let agent = client
                .agent(model)
                .max_tokens(max_tokens)
                .build();
            agent.prompt(&prompt).await?
        }
    };

    let messages_removed = to_compress.len();
    #[allow(clippy::cast_possible_truncation)]
    let estimated_summary_tokens = (summary.len() as u64) / 4;

    Ok(CompressionResult {
        summary,
        messages_removed,
        estimated_summary_tokens,
    })
}

/// Spawn a background compression task.
///
/// On success, sends `Action::CompressionComplete` with the summary and stats.
/// On failure, sends `Action::Error` and logs via tracing.
pub fn spawn_compression_task(
    provider: Arc<AiProvider>,
    messages: Vec<ChatMessage>,
    aggressive: bool,
    tx: UnboundedSender<Action>,
) {
    tokio::spawn(async move {
        match compress_conversation(&provider, &messages, aggressive).await {
            Ok(result) => {
                // Estimate original tokens from compressed messages' char count.
                #[allow(clippy::cast_possible_truncation)]
                let original_tokens = messages
                    .iter()
                    .take(result.messages_removed)
                    .map(|m| m.content.len() as u64)
                    .sum::<u64>()
                    / 4;

                let _ = tx.send(Action::CompressionComplete {
                    original_tokens,
                    compressed_tokens: result.estimated_summary_tokens,
                    summary: result.summary,
                    messages_removed: result.messages_removed,
                });
            }
            Err(e) => {
                tracing::error!("Compression failed: {e}");
                let _ = tx.send(Action::Error(format!("Compression failed: {e}")));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::message::ChatMessage;

    fn make_msg(role: Role, content: &str) -> ChatMessage {
        ChatMessage::new(role, content)
    }

    #[test]
    fn split_messages_10_keeps_last_4() {
        let msgs: Vec<ChatMessage> = (0..10)
            .map(|i| make_msg(Role::User, &format!("msg {i}")))
            .collect();
        let (compress, keep) = split_messages(&msgs, 4);
        assert_eq!(compress.len(), 6);
        assert_eq!(keep.len(), 4);
        assert_eq!(keep[0].content, "msg 6");
        assert_eq!(keep[3].content, "msg 9");
    }

    #[test]
    fn split_messages_3_not_enough() {
        let msgs: Vec<ChatMessage> = (0..3)
            .map(|i| make_msg(Role::User, &format!("msg {i}")))
            .collect();
        let (compress, keep) = split_messages(&msgs, 4);
        assert!(compress.is_empty());
        assert_eq!(keep.len(), 3);
    }

    #[test]
    fn split_messages_5_keeps_last_4() {
        let msgs: Vec<ChatMessage> = (0..5)
            .map(|i| make_msg(Role::User, &format!("msg {i}")))
            .collect();
        let (compress, keep) = split_messages(&msgs, 4);
        assert_eq!(compress.len(), 1);
        assert_eq!(compress[0].content, "msg 0");
        assert_eq!(keep.len(), 4);
    }

    #[test]
    fn build_prompt_proactive_includes_prompt() {
        let msgs = [
            make_msg(Role::User, "hello"),
            make_msg(Role::Assistant, "hi there"),
        ];
        let refs: Vec<&ChatMessage> = msgs.iter().collect();
        let prompt = build_summarization_prompt(&refs, false);
        assert!(prompt.contains(PROACTIVE_COMPRESSION_PROMPT));
        assert!(prompt.contains("User: hello"));
        assert!(prompt.contains("Assistant: hi there"));
    }

    #[test]
    fn build_prompt_enforced_includes_prompt() {
        let msgs = [make_msg(Role::User, "test")];
        let refs: Vec<&ChatMessage> = msgs.iter().collect();
        let prompt = build_summarization_prompt(&refs, true);
        assert!(prompt.contains(ENFORCED_COMPRESSION_PROMPT));
    }

    #[test]
    fn build_prompt_system_role_formatted() {
        let msgs = [make_msg(Role::System, "system info")];
        let refs: Vec<&ChatMessage> = msgs.iter().collect();
        let prompt = build_summarization_prompt(&refs, false);
        assert!(prompt.contains("System: system info"));
    }

    #[test]
    fn build_prompt_tool_content_included_verbatim() {
        let msgs = [make_msg(
            Role::Assistant,
            "Tool: shell\n```tool\nls -la\n```\nResult here",
        )];
        let refs: Vec<&ChatMessage> = msgs.iter().collect();
        let prompt = build_summarization_prompt(&refs, false);
        assert!(prompt.contains("Tool: shell"));
        assert!(prompt.contains("```tool"));
    }

    #[test]
    fn split_messages_exactly_keep_recent() {
        let msgs: Vec<ChatMessage> = (0..4)
            .map(|i| make_msg(Role::User, &format!("msg {i}")))
            .collect();
        let (compress, keep) = split_messages(&msgs, 4);
        assert!(compress.is_empty());
        assert_eq!(keep.len(), 4);
    }
}
