//! Non-interactive pipe mode.
//!
//! Sends a single prompt through the streaming agent, prints text to stdout
//! and tool status to stderr, then exits. No TUI, no ratatui, no crossterm.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::action::Action;
use crate::agents;
use crate::ai::provider::AiProvider;
use crate::ai::streaming::{StreamChatParams, spawn_streaming_chat};
use crate::ai::system_prompt::load_system_prompt;
use crate::approval::ApprovalHook;
use crate::config::{AppConfig, ApprovalMode};
use crate::session::db::Database;
use crate::tools::process;
use crate::tools::spawn_agent::AgentHandleMap;

/// Run seval in non-interactive pipe mode.
pub async fn run_pipe_mode(config: &AppConfig, prompt: String) -> anyhow::Result<()> {
    let provider = AiProvider::from_config(config).await?;
    let system_prompt = load_system_prompt();
    let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let project_path = working_dir.to_string_lossy().to_string();

    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<Action>();
    let (approval_tx, _approval_rx) = mpsc::unbounded_channel();

    let approval_hook = ApprovalHook::new(
        ApprovalMode::Yolo,
        config.tools.deny_rules.clone(),
        approval_tx.clone(),
        action_tx.clone(),
        config.tools.max_turns,
        None,
    );

    let db = match Database::open() {
        Ok(db) => Some(Arc::new(db)),
        Err(e) => {
            tracing::warn!("Failed to open session database: {e}");
            None
        }
    };

    if let Err(e) = agents::install_builtins() {
        tracing::warn!("failed to install built-in agents: {e}");
    }
    let agent_registry = agents::load_agents();

    let process_registry = process::new_registry();
    let agent_handles: AgentHandleMap = Arc::new(std::sync::Mutex::new(HashMap::new()));

    let _handle = spawn_streaming_chat(
        &provider,
        StreamChatParams {
            history: vec![],
            prompt,
            system_prompt,
            tx: action_tx,
            working_dir,
            brave_api_key: config.brave_api_key.clone(),
            max_turns: config.tools.max_turns,
            approval_hook,
            db,
            project_path,
            agent_registry: Arc::new(agent_registry),
            agent_handles,
            parent_session_id: None,
            approval_tx,
            parent_approval_mode: ApprovalMode::Yolo,
            process_registry,
        },
    );

    while let Some(action) = action_rx.recv().await {
        match action {
            Action::StreamChunk(text) => {
                use std::io::Write;
                print!("{text}");
                let _ = std::io::stdout().flush();
            }
            Action::ToolCallStart { name, args_json } => {
                let summary = if args_json.len() > 80 {
                    format!("{}...", &args_json[..args_json.floor_char_boundary(80)])
                } else {
                    args_json
                };
                eprintln!("\x1b[36m● {name}\x1b[0m {summary}");
            }
            Action::ToolResult {
                name, duration_ms, ..
            } => {
                eprintln!("\x1b[32m✔ {name}\x1b[0m ({duration_ms}ms)");
            }
            Action::ToolDenied { name, reason } => {
                eprintln!("\x1b[33m✘ {name}\x1b[0m denied: {reason}");
            }
            Action::ToolError { name, error } => {
                eprintln!("\x1b[31m✘ {name}\x1b[0m error: {error}");
            }
            Action::ShowSystemMessage(msg) => {
                eprintln!("\x1b[33m{msg}\x1b[0m");
            }
            Action::StreamComplete { .. } => {
                println!();
                break;
            }
            Action::StreamError(err) => {
                eprintln!("\x1b[31merror:\x1b[0m {err}");
                std::process::exit(1);
            }
            _ => {}
        }
    }

    Ok(())
}
