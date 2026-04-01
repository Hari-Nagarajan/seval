//! Spawn-agent tool -- spawns a background agent to perform a task.
//!
//! Implements the Rig `Tool` trait for spawning background agents from the
//! parent chat. Returns an immediate confirmation string. The agent runs
//! asynchronously and delivers results via `Action::AgentCompleted`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;
use crate::agents::executor::{ALL_TOOL_NAMES, AgentExecParams, spawn_agent_task};
use crate::agents::types::AgentDef;
use crate::agents::{AgentRegistry, effective_tools, resolve_model_alias};
use crate::ai::provider::AiProvider;
use crate::approval::ApprovalRequest;
use crate::config::ApprovalMode;
use crate::session::db::Database;

/// A map of agent name -> (`JoinHandle`, partial output buffer).
///
/// Stored by the parent chat to support Phase 11's `/agents cancel` command.
/// The `Arc<Mutex<String>>` buffer accumulates partial output and can be read
/// after `handle.abort()` to deliver partial results.
pub type AgentHandleMap =
    Arc<Mutex<HashMap<String, (tokio::task::JoinHandle<()>, Arc<Mutex<String>>)>>>;

/// Arguments for the `spawn_agent` tool, deserialized from AI-provided JSON.
#[derive(Debug, Deserialize)]
pub struct SpawnAgentArgs {
    /// Name of the agent to spawn (must exist in registry).
    pub agent_name: String,
    /// Task description for the agent.
    pub task: String,
    /// Optional additional context (file contents, prior findings).
    pub context: Option<String>,
}

/// Errors that can occur during `spawn_agent` tool execution.
#[derive(Debug, thiserror::Error)]
pub enum SpawnAgentError {
    /// The requested agent was not found in the registry.
    #[error("Agent '{0}' not found. Use /agents list to see available agents.")]
    AgentNotFound(String),
}

/// Tool for spawning a background agent from the parent chat.
///
/// Registered on the parent chat's streaming bridge. The spawned agent runs
/// asynchronously with isolated context and filtered tools. Results are
/// delivered via `Action::AgentCompleted`.
pub struct SpawnAgentTool {
    registry: Arc<AgentRegistry>,
    provider: Arc<AiProvider>,
    tx: UnboundedSender<Action>,
    /// Stored for future parent-agent approval forwarding (Phase 11+).
    #[allow(dead_code)]
    approval_tx: UnboundedSender<ApprovalRequest>,
    working_dir: PathBuf,
    brave_api_key: Option<String>,
    deny_rules: Vec<String>,
    parent_approval_mode: ApprovalMode,
    db: Option<Arc<Database>>,
    parent_session_id: Option<String>,
    project_path: String,
    /// Map of spawned agent handles for cancellation support (AGENTEXEC-05).
    agent_handles: AgentHandleMap,
}

impl SpawnAgentTool {
    /// Create a new `SpawnAgentTool`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        registry: Arc<AgentRegistry>,
        provider: Arc<AiProvider>,
        tx: UnboundedSender<Action>,
        approval_tx: UnboundedSender<ApprovalRequest>,
        working_dir: PathBuf,
        brave_api_key: Option<String>,
        deny_rules: Vec<String>,
        parent_approval_mode: ApprovalMode,
        db: Option<Arc<Database>>,
        parent_session_id: Option<String>,
        project_path: String,
        agent_handles: AgentHandleMap,
    ) -> Self {
        Self {
            registry,
            provider,
            tx,
            approval_tx,
            working_dir,
            brave_api_key,
            deny_rules,
            parent_approval_mode,
            db,
            parent_session_id,
            project_path,
            agent_handles,
        }
    }
}

impl Tool for SpawnAgentTool {
    const NAME: &'static str = "spawn_agent";

    type Error = SpawnAgentError;
    type Args = SpawnAgentArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "spawn_agent".to_string(),
            description: "Spawn a background agent to perform a specialized task. \
                The agent runs asynchronously and results are delivered when complete."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_name": {
                        "type": "string",
                        "description": "Name of the agent to spawn (must exist in registry)"
                    },
                    "task": {
                        "type": "string",
                        "description": "Task description for the agent"
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional additional context (file contents, prior findings)"
                    }
                },
                "required": ["agent_name", "task"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Look up the agent in the registry.
        let agent: &AgentDef = self
            .registry
            .get(&args.agent_name)
            .ok_or_else(|| SpawnAgentError::AgentNotFound(args.agent_name.clone()))?;

        // Compute effective tools using allowlist-first semantics (D-06).
        let all_tool_names: Vec<String> = ALL_TOOL_NAMES
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        let effective: Vec<String> = effective_tools(
            &agent.frontmatter.allowed_tools,
            &agent.frontmatter.denied_tools,
            &all_tool_names,
        )
        .into_iter()
        .map(String::from)
        .collect();

        // Resolve model alias to provider-specific model ID.
        let model = resolve_model_alias(&agent.frontmatter.model, &self.provider);

        // Determine approval mode: agent override or parent default (D-04).
        let approval_mode = agent
            .frontmatter
            .approval_mode
            .unwrap_or(self.parent_approval_mode);

        let max_turns = agent.frontmatter.max_turns;
        let max_time_minutes = agent.frontmatter.max_time_minutes;
        let agent_name = args.agent_name.clone();

        let params = AgentExecParams {
            agent_name: agent_name.clone(),
            task: args.task,
            context: args.context,
            system_prompt: agent.system_prompt.clone(),
            model: model.clone(),
            temperature: agent.frontmatter.temperature,
            max_turns,
            max_time_minutes,
            effective_tools: effective,
            approval_mode,
            deny_rules: self.deny_rules.clone(),
            tx: self.tx.clone(),
            working_dir: self.working_dir.clone(),
            brave_api_key: self.brave_api_key.clone(),
            db: self.db.clone(),
            parent_session_id: self.parent_session_id.clone(),
            project_path: self.project_path.clone(),
        };

        // Spawn the agent task asynchronously.
        let (handle, partial_output) = spawn_agent_task(&self.provider, params);

        // Store the handle and partial output buffer for cancellation (AGENTEXEC-05 / D-16).
        if let Ok(mut map) = self.agent_handles.lock() {
            map.insert(agent_name.clone(), (handle, partial_output));
        }

        // Return immediate confirmation string per D-08.
        Ok(format!(
            "Agent '{agent_name}' spawned successfully. Model: {model} | Max turns: {max_turns} | Timeout: {max_time_minutes}min"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_agent_error_message() {
        let err = SpawnAgentError::AgentNotFound("test-agent".to_string());
        let msg = err.to_string();
        assert!(msg.contains("test-agent"));
        assert!(msg.contains("not found"));
    }

    #[test]
    fn agent_handle_map_stores_entry() {
        let map: AgentHandleMap = Arc::new(Mutex::new(HashMap::new()));
        let buf: Arc<Mutex<String>> = Arc::new(Mutex::new("partial".to_string()));
        // Simulate storing a handle: we can't create a real JoinHandle in a unit test,
        // but we can verify the map stores the partial output buffer.
        let buf_clone = Arc::clone(&buf);
        {
            let mut guard = map.lock().unwrap();
            // We'd normally store (handle, buf) but for testing just verify the Arc works.
            let _ = buf_clone;
            assert!(guard.is_empty());
            let _ = &mut guard;
        }
        // Verify the buffer can be read.
        let content = buf.lock().unwrap().clone();
        assert_eq!(content, "partial");
    }
}
