use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use rig::completion::Message;
use rig::prelude::*;
use rig::streaming::StreamingChat;
use serde::{Deserialize, Serialize};

use crate::action::Action;
use crate::ai::provider::AiProvider;
use crate::approval::ApprovalHook;
use crate::tools::{
    EditTool, GlobTool, GrepTool, LsTool, ReadTool, ShellTool, WebFetchTool, WebSearchTool,
    WriteTool,
};

/// Status of a completed agent execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Completed,
    TimedOut,
    Cancelled,
}

/// Result of a spawned agent execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentResult {
    pub agent_name: String,
    pub status: AgentStatus,
    pub turns_completed: u32,
    pub max_turns: u32,
    pub elapsed_secs: u64,
    pub full_output: String,
    pub display_output: String,
}

impl AgentResult {
    /// Return a human-readable label for the agent status.
    pub fn status_label(&self) -> &'static str {
        match self.status {
            AgentStatus::Completed => "completed",
            AgentStatus::TimedOut => "timed out",
            AgentStatus::Cancelled => "cancelled",
        }
    }

    /// Create a new `AgentResult`, automatically computing `display_output`
    /// from `full_output`.
    ///
    /// If `full_output` exceeds 50 lines, `display_output` contains the first
    /// 45 lines followed by a "[N more lines...]" trailer.
    pub fn new(
        agent_name: String,
        status: AgentStatus,
        turns_completed: u32,
        max_turns: u32,
        elapsed_secs: u64,
        full_output: String,
    ) -> Self {
        let display_output = compute_display_output(&full_output);
        Self {
            agent_name,
            status,
            turns_completed,
            max_turns,
            elapsed_secs,
            full_output,
            display_output,
        }
    }
}

/// Compute the display output from full output.
///
/// If the output has more than 50 lines, truncates to the first 45 lines
/// and appends a "[N more lines...]" trailer.
fn compute_display_output(full_output: &str) -> String {
    let lines: Vec<&str> = full_output.lines().collect();
    if lines.len() > 50 {
        let remaining = lines.len() - 45;
        let first_part = lines[..45].join("\n");
        format!("{first_part}\n[{remaining} more lines...]")
    } else {
        full_output.to_string()
    }
}

/// Parameters for spawning an agent execution.
///
/// Consumed by the executor in Plan 02.
pub struct AgentExecParams {
    pub agent_name: String,
    pub task: String,
    pub context: Option<String>,
    pub system_prompt: String,
    pub model: String,
    pub temperature: f64,
    pub max_turns: u32,
    pub max_time_minutes: u32,
    pub effective_tools: Vec<String>,
    pub approval_mode: crate::config::ApprovalMode,
    pub deny_rules: Vec<String>,
    pub tx: tokio::sync::mpsc::UnboundedSender<crate::action::Action>,
    pub working_dir: std::path::PathBuf,
    pub brave_api_key: Option<String>,
    pub db: Option<std::sync::Arc<crate::session::db::Database>>,
    pub parent_session_id: Option<String>,
    pub project_path: String,
}

/// Spawn an agent task asynchronously.
///
/// Returns a `(JoinHandle, Arc<Mutex<String>>)` tuple:
/// - The `JoinHandle` can be used to abort the task (Phase 11 cancellation).
/// - The `Arc<Mutex<String>>` is a partial output buffer that accumulates
///   assistant text and can be read after `abort()` to get partial results.
///
/// On completion, the task sends `Action::AgentCompleted` via `params.tx`.
pub fn spawn_agent_task(
    provider: &AiProvider,
    params: AgentExecParams,
) -> (tokio::task::JoinHandle<()>, Arc<Mutex<String>>) {
    // Create the partial output buffer before spawning so the caller can hold
    // a reference for cancellation (AGENTEXEC-05 / D-16).
    let last_output: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let last_output_for_task = Arc::clone(&last_output);

    let provider = provider.clone();

    let handle = tokio::spawn(async move {
        run_agent_task(provider, params, last_output_for_task).await;
    });

    (handle, last_output)
}

/// Inner async implementation of agent execution.
async fn run_agent_task(
    provider: AiProvider,
    params: AgentExecParams,
    last_output: Arc<Mutex<String>>,
) {
    let start = Instant::now();
    let agent_name = params.agent_name.clone();
    let max_turns = params.max_turns;
    let tx = params.tx.clone();

    // Create a child session in SQLite (fire-and-forget).
    let child_session_id: Option<String> =
        if let (Some(db), Some(parent_id)) = (&params.db, &params.parent_session_id) {
            let db_clone = Arc::clone(db);
            let project_path = params.project_path.clone();
            let model = params.model.clone();
            let parent_id = parent_id.clone();
            let result = tokio::task::spawn_blocking(move || {
                db_clone.create_child_session(&project_path, Some(&model), &parent_id)
            })
            .await;
            match result {
                Ok(Ok(record)) => Some(record.id),
                Ok(Err(e)) => {
                    tracing::warn!("Failed to create child session: {e}");
                    None
                }
                Err(e) => {
                    tracing::warn!("spawn_blocking for child session failed: {e}");
                    None
                }
            }
        } else {
            None
        };

    // Build the initial message: task + optional context.
    let prompt_text = if let Some(ctx) = &params.context {
        if ctx.is_empty() {
            params.task.clone()
        } else {
            format!("{}\n\n{}", params.task, ctx)
        }
    } else {
        params.task.clone()
    };

    let history: Vec<Message> = vec![];

    // Create an ApprovalHook for the agent with the effective tool filter.
    // Agents share the parent approval channel so user sees approvals in the TUI.
    // We create a fresh disconnected approval channel since agents may run in the
    // background and we don't want to block on approval for spawned agents in
    // Yolo-default mode. The tool filter on the hook enforces effective_tools.
    let (agent_approval_tx, _agent_approval_rx) = tokio::sync::mpsc::unbounded_channel();
    let effective_filter = Some(params.effective_tools.clone());
    let hook = ApprovalHook::new(
        params.approval_mode,
        params.deny_rules.clone(),
        agent_approval_tx,
        tx.clone(),
        max_turns as usize,
        effective_filter,
    );

    let timeout_duration = Duration::from_secs(u64::from(params.max_time_minutes) * 60);

    // Clone the turn counter Arc before the hook is consumed by run_*_agent.
    // This allows reading the actual turn count after stream completion and in
    // timeout branches (AGENTEXEC-04).
    let turn_counter = hook.turn_counter();

    // Build agent and run with timeout based on provider type.
    let result = match &provider {
        AiProvider::Bedrock { client, .. } => {
            run_bedrock_agent(
                client,
                &params.model,
                &params.system_prompt,
                &params.effective_tools,
                params.working_dir.clone(),
                params.brave_api_key.clone(),
                hook,
                max_turns as usize,
                history,
                prompt_text,
                timeout_duration,
                Arc::clone(&last_output),
                Arc::clone(&turn_counter),
                tx.clone(),
                agent_name.clone(),
            )
            .await
        }
        AiProvider::OpenRouter { client, .. } => {
            run_openrouter_agent(
                client,
                &params.model,
                &params.system_prompt,
                &params.effective_tools,
                params.working_dir.clone(),
                params.brave_api_key.clone(),
                hook,
                max_turns as usize,
                history,
                prompt_text,
                timeout_duration,
                Arc::clone(&last_output),
                Arc::clone(&turn_counter),
                tx.clone(),
                agent_name.clone(),
            )
            .await
        }
    };

    let elapsed_secs = start.elapsed().as_secs();

    // Save messages to child session (fire-and-forget).
    if let (Some(db), Some(session_id)) = (&params.db, &child_session_id) {
        let db_clone = Arc::clone(db);
        let session_id = session_id.clone();
        let task_text = params.task.clone();
        let output_text = last_output
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        tokio::task::spawn_blocking(move || {
            let _ = db_clone.save_message(&session_id, "user", &task_text, None, None);
            if !output_text.is_empty() {
                let _ = db_clone.save_message(&session_id, "assistant", &output_text, None, None);
            }
        });
    }

    // Build the AgentResult and send via action channel.
    let (status, full_output, turns_completed) = result;
    let agent_result = AgentResult::new(
        agent_name,
        status,
        turns_completed,
        max_turns,
        elapsed_secs,
        full_output,
    );

    let _ = tx.send(Action::AgentCompleted(agent_result));
}

/// Outcome from the agent streaming loop.
type AgentOutcome = (AgentStatus, String, u32);

/// Run a Bedrock agent with timeout.
#[allow(clippy::too_many_arguments)]
async fn run_bedrock_agent(
    client: &rig_bedrock::client::Client,
    model: &str,
    system_prompt: &str,
    effective_tools: &[String],
    working_dir: PathBuf,
    brave_api_key: Option<String>,
    hook: ApprovalHook,
    max_turns: usize,
    history: Vec<Message>,
    prompt: String,
    timeout_duration: Duration,
    last_output: Arc<Mutex<String>>,
    turn_counter: Arc<AtomicUsize>,
    tx: tokio::sync::mpsc::UnboundedSender<Action>,
    agent_name: String,
) -> AgentOutcome {
    let agent = build_bedrock_agent(
        client,
        model,
        system_prompt,
        effective_tools,
        working_dir,
        brave_api_key,
    );

    let result = tokio::time::timeout(
        timeout_duration,
        run_agent_stream(
            agent,
            history,
            prompt,
            hook,
            max_turns,
            Arc::clone(&last_output),
            tx,
            agent_name,
        ),
    )
    .await;

    match result {
        Ok(outcome) => outcome,
        Err(_elapsed) => {
            let partial = last_output.lock().map(|g| g.clone()).unwrap_or_default();
            let timeout_turns =
                u32::try_from(turn_counter.load(Ordering::Relaxed)).unwrap_or(u32::MAX);
            (AgentStatus::TimedOut, partial, timeout_turns)
        }
    }
}

/// Run an `OpenRouter` agent with timeout.
#[allow(clippy::too_many_arguments)]
async fn run_openrouter_agent(
    client: &rig::providers::openrouter::Client,
    model: &str,
    system_prompt: &str,
    effective_tools: &[String],
    working_dir: PathBuf,
    brave_api_key: Option<String>,
    hook: ApprovalHook,
    max_turns: usize,
    history: Vec<Message>,
    prompt: String,
    timeout_duration: Duration,
    last_output: Arc<Mutex<String>>,
    turn_counter: Arc<AtomicUsize>,
    tx: tokio::sync::mpsc::UnboundedSender<Action>,
    agent_name: String,
) -> AgentOutcome {
    let agent = build_openrouter_agent(
        client,
        model,
        system_prompt,
        effective_tools,
        working_dir,
        brave_api_key,
    );

    let result = tokio::time::timeout(
        timeout_duration,
        run_agent_stream(
            agent,
            history,
            prompt,
            hook,
            max_turns,
            Arc::clone(&last_output),
            tx,
            agent_name,
        ),
    )
    .await;

    match result {
        Ok(outcome) => outcome,
        Err(_elapsed) => {
            let partial = last_output.lock().map(|g| g.clone()).unwrap_or_default();
            let timeout_turns =
                u32::try_from(turn_counter.load(Ordering::Relaxed)).unwrap_or(u32::MAX);
            (AgentStatus::TimedOut, partial, timeout_turns)
        }
    }
}

/// Build a Bedrock agent with ALL tools registered.
///
/// Tool filtering is enforced via the `ApprovalHook`'s `effective_tool_filter`,
/// not by selectively registering tools. This avoids Rig's type-level builder
/// constraint where each `.tool()` call changes the builder type.
fn build_bedrock_agent(
    client: &rig_bedrock::client::Client,
    model: &str,
    system_prompt: &str,
    _effective_tools: &[String],
    working_dir: PathBuf,
    brave_api_key: Option<String>,
) -> rig::agent::Agent<rig_bedrock::completion::CompletionModel> {
    client
        .agent(model)
        .preamble(system_prompt)
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
        .build()
}

/// Build an `OpenRouter` agent with ALL tools registered.
fn build_openrouter_agent(
    client: &rig::providers::openrouter::Client,
    model: &str,
    system_prompt: &str,
    _effective_tools: &[String],
    working_dir: PathBuf,
    brave_api_key: Option<String>,
) -> rig::agent::Agent<rig::providers::openrouter::completion::CompletionModel> {
    client
        .agent(model)
        .preamble(system_prompt)
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
        .build()
}

/// Generic streaming loop for any agent type.
///
/// Accumulates assistant text into `last_output`. Returns
/// `(status, full_output, turns_completed)`.
async fn run_agent_stream<M>(
    agent: rig::agent::Agent<M>,
    history: Vec<Message>,
    prompt: String,
    hook: ApprovalHook,
    max_turns: usize,
    last_output: Arc<Mutex<String>>,
    tx: tokio::sync::mpsc::UnboundedSender<Action>,
    agent_name: String,
) -> AgentOutcome
where
    M: rig::completion::CompletionModel + 'static,
    M::StreamingResponse: Send + rig::completion::GetTokenUsage,
{
    use rig::agent::MultiTurnStreamItem;
    use rig::streaming::StreamedAssistantContent;

    // Clone the turn counter Arc before .with_hook() consumes the hook so we
    // can read the actual turn count after the stream ends (AGENTEXEC-04).
    let turn_counter = hook.turn_counter();

    let mut stream = agent
        .stream_chat(&prompt, history)
        .multi_turn(max_turns)
        .with_hook(hook)
        .await;

    let mut full_output = String::new();
    let mut last_turn_sent: usize = 0;

    while let Some(item) = stream.next().await {
        match item {
            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(text))) => {
                full_output.push_str(&text.text);
                // Update the partial output buffer for external cancellation.
                if let Ok(mut guard) = last_output.lock() {
                    guard.clone_from(&full_output);
                }
                // Send turn update if turn count has advanced (per D-06, Pitfall 1).
                let current_turn = turn_counter.load(Ordering::Relaxed);
                if current_turn != last_turn_sent {
                    last_turn_sent = current_turn;
                    let _ = tx.send(Action::AgentTurnUpdate {
                        name: agent_name.clone(),
                        turn: u32::try_from(current_turn).unwrap_or(u32::MAX),
                        max_turns: u32::try_from(max_turns).unwrap_or(u32::MAX),
                    });
                }
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("MaxTurnError") || err_str.contains("max turn limit") {
                    // Max turns reached — still a Completed status.
                    let turns = u32::try_from(max_turns).unwrap_or(u32::MAX);
                    return (AgentStatus::Completed, full_output, turns);
                }
                tracing::warn!("Agent stream error: {err_str}");
                break;
            }
            _ => {
                // ToolCall, ToolResult, Final, FinalResponse, Reasoning etc.
                // Not accumulated into the text output.
            }
        }
    }

    let turns = u32::try_from(turn_counter.load(Ordering::Relaxed)).unwrap_or(u32::MAX);

    (AgentStatus::Completed, full_output, turns)
}

/// All tool names available for agent filtering.
///
/// Used by `SpawnAgentTool` to compute `effective_tools`.
pub const ALL_TOOL_NAMES: &[&str] = &[
    "shell",
    "read",
    "write",
    "edit",
    "grep",
    "glob",
    "ls",
    "web_fetch",
    "web_search",
];

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // AgentStatus serialization tests
    // -------------------------------------------------------------------------

    #[test]
    fn agent_status_serde_round_trip() {
        let statuses = [
            AgentStatus::Completed,
            AgentStatus::TimedOut,
            AgentStatus::Cancelled,
        ];
        for status in statuses {
            let json = serde_json::to_string(&status).expect("serialize");
            let back: AgentStatus = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(status, back, "round-trip failed for {status:?}");
        }
    }

    // -------------------------------------------------------------------------
    // AgentResult serialization tests
    // -------------------------------------------------------------------------

    #[test]
    fn agent_result_serde_round_trip() {
        let result = AgentResult::new(
            "test-agent".to_string(),
            AgentStatus::Completed,
            5,
            10,
            42,
            "line1\nline2\nline3".to_string(),
        );
        let json = serde_json::to_string(&result).expect("serialize");
        let back: AgentResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(result, back);
    }

    // -------------------------------------------------------------------------
    // AgentResult::status_label tests
    // -------------------------------------------------------------------------

    #[test]
    fn status_label_completed() {
        let r = AgentResult::new(
            "a".to_string(),
            AgentStatus::Completed,
            1,
            10,
            5,
            String::new(),
        );
        assert_eq!(r.status_label(), "completed");
    }

    #[test]
    fn status_label_timed_out() {
        let r = AgentResult::new(
            "a".to_string(),
            AgentStatus::TimedOut,
            1,
            10,
            5,
            String::new(),
        );
        assert_eq!(r.status_label(), "timed out");
    }

    #[test]
    fn status_label_cancelled() {
        let r = AgentResult::new(
            "a".to_string(),
            AgentStatus::Cancelled,
            1,
            10,
            5,
            String::new(),
        );
        assert_eq!(r.status_label(), "cancelled");
    }

    // -------------------------------------------------------------------------
    // display_output truncation tests
    // -------------------------------------------------------------------------

    #[test]
    fn display_output_short_not_truncated() {
        let lines: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
        let full_output = lines.join("\n");
        let r = AgentResult::new(
            "a".to_string(),
            AgentStatus::Completed,
            1,
            10,
            5,
            full_output.clone(),
        );
        assert_eq!(r.display_output, full_output);
    }

    #[test]
    fn display_output_exactly_50_not_truncated() {
        let lines: Vec<String> = (1..=50).map(|i| format!("line {i}")).collect();
        let full_output = lines.join("\n");
        let r = AgentResult::new(
            "a".to_string(),
            AgentStatus::Completed,
            1,
            10,
            5,
            full_output.clone(),
        );
        assert_eq!(
            r.display_output, full_output,
            "exactly 50 lines should not be truncated"
        );
    }

    #[test]
    fn display_output_51_lines_truncated() {
        let lines: Vec<String> = (1..=51).map(|i| format!("line {i}")).collect();
        let full_output = lines.join("\n");
        let r = AgentResult::new(
            "a".to_string(),
            AgentStatus::Completed,
            1,
            10,
            5,
            full_output,
        );
        // display_output should have 45 lines + trailer
        let display_lines: Vec<&str> = r.display_output.lines().collect();
        // Last line should be the trailer
        let last = display_lines.last().unwrap();
        assert!(last.contains("more lines"), "expected trailer, got: {last}");
        // Trailer shows 51 - 45 = 6 more lines
        assert!(last.contains('6'), "expected 6 more lines, got: {last}");
        // First 45 lines are preserved
        assert_eq!(display_lines[0], "line 1");
        assert_eq!(display_lines[44], "line 45");
    }

    #[test]
    fn display_output_100_lines_truncated() {
        let lines: Vec<String> = (1..=100).map(|i| format!("line {i}")).collect();
        let full_output = lines.join("\n");
        let r = AgentResult::new(
            "a".to_string(),
            AgentStatus::Completed,
            1,
            10,
            5,
            full_output,
        );
        // 100 - 45 = 55 more lines
        assert!(
            r.display_output.contains("55 more lines"),
            "got: {}",
            r.display_output
        );
    }

    // -------------------------------------------------------------------------
    // ALL_TOOL_NAMES constant test
    // -------------------------------------------------------------------------

    #[test]
    fn all_tool_names_contains_expected() {
        assert!(ALL_TOOL_NAMES.contains(&"shell"));
        assert!(ALL_TOOL_NAMES.contains(&"read"));
        assert!(ALL_TOOL_NAMES.contains(&"write"));
        assert!(ALL_TOOL_NAMES.contains(&"grep"));
        assert!(!ALL_TOOL_NAMES.contains(&"spawn_agent"));
        assert!(!ALL_TOOL_NAMES.contains(&"save_memory"));
    }
}
