//! Streaming bridge between Rig and the action channel.
//!
//! Spawns a tokio task that consumes a Rig streaming response and sends
//! `Action` variants through the application's event channel. Registers
//! all 10 tools on the agent builder and forwards tool events. Supports
//! multi-turn agentic execution with an approval hook.

use std::path::PathBuf;
use std::sync::Arc;

use futures::StreamExt;
use rig::completion::{GetTokenUsage, Message};
use rig::prelude::*;
use rig::streaming::StreamingChat;
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;
use crate::ai::provider::AiProvider;
use crate::approval::ApprovalHook;
use crate::session::db::Database;
use crate::session::memory_tool::SaveMemoryTool;
use crate::tools::{
    EditTool, GlobTool, GrepTool, LsTool, ReadTool, ShellTool, WebFetchTool, WebSearchTool,
    WriteTool,
};

/// Parameters for spawning a streaming chat task.
pub struct StreamChatParams {
    /// Conversation history.
    pub history: Vec<Message>,
    /// User prompt text.
    pub prompt: String,
    /// System prompt for the agent.
    pub system_prompt: String,
    /// Action channel for sending events to the TUI.
    pub tx: UnboundedSender<Action>,
    /// Working directory for tool execution.
    pub working_dir: PathBuf,
    /// Brave Search API key (optional).
    pub brave_api_key: Option<String>,
    /// Maximum agentic turns.
    pub max_turns: usize,
    /// Approval hook for tool call gating.
    pub approval_hook: ApprovalHook,
    /// Shared database handle for memory tool (optional).
    pub db: Option<Arc<Database>>,
    /// Project path for memory tool scoping.
    pub project_path: String,
}

/// Spawn a streaming chat task that bridges Rig's stream to the action channel.
///
/// The spawned task:
/// 1. Builds an agent from the provider with the given system prompt and all tools
/// 2. Calls `stream_chat` with the prompt and history, chaining `multi_turn` and `with_hook`
/// 3. Forwards text chunks as `Action::StreamChunk`
/// 4. Forwards tool call and result events as `Action::ToolCallStart` / `Action::ToolResult`
/// 5. Sends `Action::StreamComplete` when the stream finishes
/// 6. Sends `Action::StreamError` on any error
///
/// Returns a `JoinHandle` so the caller can abort the task on cancellation.
pub fn spawn_streaming_chat(
    provider: &AiProvider,
    params: StreamChatParams,
) -> tokio::task::JoinHandle<()> {
    let StreamChatParams {
        history,
        prompt,
        system_prompt,
        tx,
        working_dir,
        brave_api_key,
        max_turns,
        approval_hook,
        db,
        project_path,
    } = params;

    // Build the SaveMemoryTool. If no database handle is provided, create an
    // in-memory fallback so the tool is always registered (saves silently fail).
    let memory_db = db.unwrap_or_else(|| {
        Arc::new(Database::open_in_memory().expect("in-memory DB for memory tool"))
    });
    let save_memory = SaveMemoryTool::new(memory_db, project_path);

    match provider {
        AiProvider::Bedrock { client, model } => {
            let agent = client
                .agent(model)
                .preamble(&system_prompt)
                .max_tokens(4096)
                .tool(ShellTool::new(working_dir.clone()))
                .tool(ReadTool)
                .tool(WriteTool)
                .tool(EditTool)
                .tool(GrepTool::new(working_dir.clone()))
                .tool(GlobTool::new(working_dir))
                .tool(LsTool)
                .tool(WebFetchTool::new())
                .tool(WebSearchTool::new(brave_api_key))
                .tool(save_memory)
                .build();
            spawn_stream_task(agent, history, prompt, tx, max_turns, approval_hook)
        }
        AiProvider::OpenRouter { client, model } => {
            let agent = client
                .agent(model)
                .preamble(&system_prompt)
                .max_tokens(4096)
                .tool(ShellTool::new(working_dir.clone()))
                .tool(ReadTool)
                .tool(WriteTool)
                .tool(EditTool)
                .tool(GrepTool::new(working_dir.clone()))
                .tool(GlobTool::new(working_dir))
                .tool(LsTool)
                .tool(WebFetchTool::new())
                .tool(WebSearchTool::new(brave_api_key))
                .tool(save_memory)
                .build();
            spawn_stream_task(agent, history, prompt, tx, max_turns, approval_hook)
        }
    }
}

/// Generic stream task spawner for any agent type.
#[allow(clippy::too_many_lines)]
fn spawn_stream_task<M>(
    agent: rig::agent::Agent<M>,
    history: Vec<Message>,
    prompt: String,
    tx: UnboundedSender<Action>,
    max_turns: usize,
    hook: ApprovalHook,
) -> tokio::task::JoinHandle<()>
where
    M: rig::completion::CompletionModel + 'static,
    M::StreamingResponse: Send + GetTokenUsage,
{
    tokio::spawn(async move {
        use rig::agent::MultiTurnStreamItem;
        use rig::streaming::{StreamedAssistantContent, StreamedUserContent};

        let mut stream = agent
            .stream_chat(&prompt, history)
            .multi_turn(max_turns)
            .with_hook(hook)
            .await;

        let mut input_tokens = 0u64;
        let mut output_tokens = 0u64;
        let mut last_turn_input = 0u64;

        // Track tool call names by internal_call_id for correlating with results.
        let mut tool_call_names: std::collections::HashMap<String, (String, std::time::Instant)> =
            std::collections::HashMap::new();

        while let Some(item) = stream.next().await {
            match item {
                Ok(MultiTurnStreamItem::StreamAssistantItem(
                    StreamedAssistantContent::Text(text),
                )) => {
                    let _ = tx.send(Action::StreamChunk(text.text));
                }
                Ok(MultiTurnStreamItem::StreamAssistantItem(
                    StreamedAssistantContent::ToolCall {
                        tool_call,
                        internal_call_id,
                    },
                )) => {
                    let name = tool_call.function.name.clone();
                    let args_json = tool_call.function.arguments.to_string();
                    tool_call_names
                        .insert(internal_call_id, (name.clone(), std::time::Instant::now()));
                    let _ = tx.send(Action::ToolCallStart {
                        name,
                        args_json,
                    });
                }
                Ok(MultiTurnStreamItem::StreamUserItem(
                    StreamedUserContent::ToolResult {
                        tool_result,
                        internal_call_id,
                    },
                )) => {
                    let (name, start_time) = tool_call_names
                        .remove(&internal_call_id)
                        .unwrap_or_else(|| ("unknown".to_string(), std::time::Instant::now()));
                    let duration_ms =
                        u64::try_from(start_time.elapsed().as_millis()).unwrap_or(u64::MAX);

                    // Extract text from tool result content.
                    // Rig JSON-serializes tool output, so a String result
                    // becomes `"\"line1\\nline2\""`. Try to deserialize each
                    // chunk as a JSON string to recover the original value.
                    let result_text: String = tool_result
                        .content
                        .iter()
                        .filter_map(|c| {
                            if let rig::message::ToolResultContent::Text(t) = c {
                                Some(
                                    serde_json::from_str::<String>(&t.text)
                                        .unwrap_or_else(|_| t.text.clone()),
                                )
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    let _ = tx.send(Action::ToolResult {
                        name,
                        result: result_text,
                        duration_ms,
                    });
                }
                Ok(MultiTurnStreamItem::StreamAssistantItem(
                    StreamedAssistantContent::Final(ref res),
                )) => {
                    // Accumulate per-turn token usage from each Final event.
                    if let Some(usage) = res.token_usage() {
                        input_tokens += usage.input_tokens;
                        output_tokens += usage.output_tokens;
                        // Track last turn's input tokens (= actual context size).
                        last_turn_input = usage.input_tokens;
                        // Send live update so status bar reflects progress mid-loop.
                        let _ = tx.send(Action::TokenUpdate {
                            output_tokens,
                            context_tokens: last_turn_input,
                        });
                    }
                }
                Ok(MultiTurnStreamItem::FinalResponse(res)) => {
                    // FinalResponse carries aggregated usage across all turns.
                    // Prefer it over our per-turn accumulation.
                    let usage = res.usage();
                    input_tokens = usage.input_tokens;
                    output_tokens = usage.output_tokens;
                    // last_turn_input stays as the last Final event's value
                }
                Err(e) => {
                    let err_str = e.to_string();
                    if err_str.contains("MaxTurnError") || err_str.contains("max turn limit") {
                        // Max turns reached: complete normally with accumulated tokens
                        // so the token counter updates, then show the limit message.
                        let _ = tx.send(Action::StreamComplete {
                            input_tokens,
                            output_tokens,
                            context_tokens: last_turn_input,
                        });
                        let _ = tx.send(Action::ShowSystemMessage(format!(
                            "Agentic loop reached the {max_turns}-turn limit. \
                             You can continue by sending another message."
                        )));
                    } else {
                        let error_msg = format_error(&err_str);
                        let _ = tx.send(Action::StreamError(error_msg));
                    }
                    return;
                }
                _ => {
                    // ToolCallDelta, Reasoning, ReasoningDelta -- not forwarded yet
                }
            }
        }

        let _ = tx.send(Action::StreamComplete {
            input_tokens,
            output_tokens,
            context_tokens: last_turn_input,
        });
    })
}

/// Format error messages, detecting auth-related issues.
fn format_error(error: &str) -> String {
    let lower = error.to_lowercase();
    if lower.contains("401")
        || lower.contains("403")
        || lower.contains("unauthorized")
        || lower.contains("authentication")
        || lower.contains("invalid.*key")
    {
        format!("Authentication failed: check your API key in ~/.seval/config.toml. Original error: {error}")
    } else {
        error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_error_detects_auth_failures() {
        let msg = format_error("HTTP 401 Unauthorized");
        assert!(msg.contains("Authentication failed"));
    }

    #[test]
    fn format_error_detects_403() {
        let msg = format_error("HTTP 403 Forbidden");
        assert!(msg.contains("Authentication failed"));
    }

    #[test]
    fn format_error_passes_through_normal_errors() {
        let msg = format_error("Connection refused");
        assert_eq!(msg, "Connection refused");
    }
}
