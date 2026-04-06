//! Chat component — the main chat TUI.
//!
//! Integrates the AI provider, streaming responses, markdown rendering, and
//! slash commands into a complete chat experience. Owns conversation state,
//! handles user input, and renders the chat view with 30fps buffered display.

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::ListState;
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;
use crate::agents::AgentRegistry;
use crate::agents::executor::AgentResult;
use crate::ai::provider::AiProvider;
use crate::ai::streaming::{StreamChatParams, spawn_streaming_chat};
use crate::ai::system_prompt::load_system_prompt;
use crate::approval::{ApprovalDecision, ApprovalHook, ApprovalRequest};
use crate::chat::commands::SlashCommand;
use crate::chat::context::ContextState;
use crate::chat::input::ChatInput;
use crate::chat::markdown::render_markdown;
use crate::chat::message::{ChatMessage, NEXT_ID, Role, TokenUsage};
use crate::chat::verbs::random_thinking_verb;
use crate::config::AppConfig;
use crate::config::types::ApprovalMode;
use crate::session::db::Database;
use crate::tui::Component;

use super::rendering::{format_compact_tokens, format_elapsed, render_user_message};

/// Streaming/agentic loop state.
pub(super) struct StreamingState {
    /// Whether a response is in progress.
    pub(super) is_streaming: bool,
    /// Current chat state for approval flow.
    pub(super) chat_state: ChatState,
    /// Handle to the streaming task for cancellation.
    pub(super) task: Option<tokio::task::JoinHandle<()>>,
    /// Turn counter Arc from the current streaming session's approval hook.
    pub(super) turn_counter: Option<Arc<AtomicUsize>>,
    /// When the current streaming response started.
    pub(super) started_at: Option<Instant>,
    /// Random thinking verb pair (present, past) for the current streaming session.
    pub(super) thinking_verb: (&'static str, &'static str),
    /// Spinner frame index for "Thinking..." animation.
    pub(super) spinner_frame: usize,
    /// Accumulated text during streaming.
    pub(super) buffer: String,
}

impl StreamingState {
    /// Whether the chat is busy (streaming or awaiting approval).
    pub(super) fn is_busy(&self) -> bool {
        self.is_streaming || matches!(self.chat_state, ChatState::AwaitingApproval { .. })
    }
}

/// Session-related state (database, session ID, action channel).
pub(super) struct SessionState {
    /// Shared database handle for session persistence.
    pub(super) db: Option<Arc<Database>>,
    /// Current session ID in the database.
    pub(super) session_id: Option<String>,
    /// Row ID of the last saved assistant message (for tool call association).
    pub(super) last_assistant_msg_id: Option<i64>,
    /// Action channel sender.
    pub(super) action_tx: Option<UnboundedSender<Action>>,
}

impl SessionState {
    /// Get cloned DB handle and action sender, or `None` if either is missing.
    pub(super) fn db_and_tx(&self) -> Option<(Arc<Database>, UnboundedSender<Action>)> {
        let db = self.db.as_ref().map(Arc::clone)?;
        let tx = self.action_tx.clone()?;
        Some((db, tx))
    }
}

/// State for the interactive model picker overlay.
pub(super) struct ModelPickerState {
    /// Whether the interactive model picker overlay is active.
    pub(super) active: bool,
    /// List selection state for the model picker.
    pub(super) list_state: ListState,
}

/// The main chat component.
pub struct Chat {
    /// Conversation messages.
    pub(super) messages: Vec<ChatMessage>,
    /// Rig-format history for API calls.
    pub(super) rig_history: Vec<rig::message::Message>,
    /// Text input area.
    pub(super) input: ChatInput,
    /// Pre-rendered message display cache (one Vec<Line> per message).
    pub(super) rendered_messages: Vec<Vec<Line<'static>>>,
    /// Scroll offset for chat history (lines from bottom).
    pub(super) scroll_offset: u16,
    /// Cached max scroll from last render (used to clamp `scroll_offset`).
    /// Uses `Cell` for interior mutability since `draw` takes `&self`.
    pub(super) max_scroll: Cell<u16>,
    /// The AI provider (shared with streaming task via Arc).
    pub(super) provider: Option<Arc<AiProvider>>,
    /// Loaded system prompt.
    pub(super) system_prompt: String,
    /// Cumulative token usage.
    pub(super) total_tokens: TokenUsage,
    /// Whether help overlay is visible.
    pub(super) show_help: bool,
    /// Error message from provider initialization (shown inline).
    pub(super) provider_error: Option<String>,
    /// Brave Search API key for web search tool.
    pub(super) brave_api_key: Option<String>,
    /// Approval mode from config.
    pub(super) approval_mode: ApprovalMode,
    /// Deny rules from config.
    pub(super) deny_rules: Vec<String>,
    /// Max turns for agentic loop.
    pub(super) max_turns: usize,
    /// Approval channel sender (shared with `ApprovalHook`).
    pub(super) approval_tx: Option<tokio::sync::mpsc::UnboundedSender<ApprovalRequest>>,
    /// Agent registry for spawning agents via the `spawn_agent` tool.
    /// Wired in Plan 03; defaults to an empty registry until then.
    pub(super) agent_registry: Arc<AgentRegistry>,
    /// Map of spawned agent handles for cancellation support (Phase 11).
    /// Wired in Plan 03; defaults to an empty map.
    pub(super) agent_handles: crate::tools::spawn_agent::AgentHandleMap,
    /// Pending agent results waiting to be injected into `rig_history` on next turn (D-01).
    pub(super) pending_agent_results: Vec<AgentResult>,
    /// Live status of running agents (for /agents status command).
    pub(super) agent_status: std::collections::HashMap<String, super::agents::AgentStatusEntry>,
    /// Log of completed agents this session (for /agents status command).
    pub(super) completed_agent_log: Vec<super::agents::CompletedAgentInfo>,
    /// Context window state tracking.
    pub(super) context_state: ContextState,
    /// Session state (DB, session ID, action channel).
    pub(super) session: SessionState,
    /// Streaming/agentic loop state.
    pub(super) streaming: StreamingState,
    /// Model picker overlay state.
    pub(super) model_picker: ModelPickerState,
}

/// Chat component state machine.
pub(super) enum ChatState {
    /// Normal input mode.
    Normal,
    /// Streaming response in progress.
    Streaming,
    /// Waiting for user to approve/deny a tool call.
    AwaitingApproval {
        tool_name: String,
        response_tx: Option<tokio::sync::oneshot::Sender<ApprovalDecision>>,
    },
}

impl Chat {
    /// Create a new chat component from the application config.
    ///
    /// If the AI provider cannot be initialized (e.g. missing API key),
    /// the component still works but shows an inline error. Slash commands
    /// remain functional.
    pub async fn new(
        config: &AppConfig,
        approval_tx: tokio::sync::mpsc::UnboundedSender<ApprovalRequest>,
        db: Option<Arc<Database>>,
    ) -> Self {
        let (provider, provider_error) = match AiProvider::from_config(config).await {
            Ok(p) => (Some(Arc::new(p)), None),
            Err(e) => (None, Some(format!("AI provider unavailable: {e}"))),
        };

        let system_prompt = load_system_prompt();
        let brave_api_key = config.brave_api_key.clone();

        Self {
            messages: Vec::new(),
            rig_history: Vec::new(),
            input: ChatInput::new(),
            rendered_messages: Vec::new(),
            scroll_offset: 0,
            max_scroll: Cell::new(0),
            provider,
            system_prompt,
            total_tokens: TokenUsage::default(),
            show_help: false,
            provider_error,
            brave_api_key,
            approval_mode: config.tools.approval_mode,
            deny_rules: config.tools.deny_rules.clone(),
            max_turns: config.tools.max_turns,
            approval_tx: Some(approval_tx),
            agent_registry: Arc::new(AgentRegistry::default()),
            agent_handles: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            pending_agent_results: Vec::new(),
            agent_status: std::collections::HashMap::new(),
            completed_agent_log: Vec::new(),
            context_state: ContextState::new(128_000),
            streaming: StreamingState {
                buffer: String::new(),
                is_streaming: false,
                chat_state: ChatState::Normal,
                task: None,
                turn_counter: None,
                started_at: None,
                thinking_verb: ("Thinking", "Thought"),
                spinner_frame: 0,
            },
            session: SessionState {
                db,
                session_id: None,
                last_assistant_msg_id: None,
                action_tx: None,
            },
            model_picker: ModelPickerState {
                active: false,
                list_state: ListState::default(),
            },
        }
    }

    /// Initialize a new session in the database.
    ///
    /// Called after `register_action_handler` so that `action_tx` is available.
    pub fn init_session(&mut self) {
        if let (Some(db), Some(tx)) = (&self.session.db, &self.session.action_tx) {
            let db = Arc::clone(db);
            let tx = tx.clone();
            let project = project_path();
            let model = self.provider.as_ref().map(|p| p.model_name().to_string());
            tokio::task::spawn_blocking(move || {
                match db.create_session(&project, model.as_deref()) {
                    Ok(session) => {
                        let _ = tx.send(Action::SessionCreated(session.id));
                    }
                    Err(e) => {
                        tracing::warn!("Failed to create session: {e}");
                    }
                }
            });
        }
    }

    /// Inject project memories into the system prompt.
    ///
    /// Called on startup after loading memories from the database. Appends
    /// a memory context section to the system prompt so the AI starts with
    /// knowledge from previous sessions.
    pub fn inject_memory_context(&mut self, memories: &[String]) {
        if memories.is_empty() {
            return;
        }
        let mut section = String::from(
            "\n\n## Project Memory\nThe following key findings were saved from previous sessions:\n\n",
        );
        for mem in memories {
            section.push_str("- ");
            section.push_str(mem);
            section.push('\n');
        }
        section.push_str("\nUse this context to inform your responses.");
        self.system_prompt.push_str(&section);
    }

    /// Set the agent registry (called by App after loading agents at startup).
    pub fn set_agent_registry(&mut self, registry: AgentRegistry) {
        self.agent_registry = Arc::new(registry);
    }

    /// Get the shared database handle (for use by App layer).
    pub fn db(&self) -> Option<&Arc<Database>> {
        self.session.db.as_ref()
    }

    /// Handle a slash command.
    fn handle_slash_command(&mut self, cmd: SlashCommand) -> Option<Action> {
        match cmd {
            SlashCommand::Model(maybe_name) => {
                if let Some(name) = maybe_name {
                    if self.provider.is_some() {
                        self.switch_model(&name);
                    } else {
                        self.add_system_message(
                            "No AI provider configured. Set an API key in ~/.seval/config.toml"
                                .to_string(),
                        );
                    }
                } else if self.provider.is_some() {
                    self.open_model_picker();
                } else {
                    self.add_system_message(
                        "No AI provider configured. Set an API key in ~/.seval/config.toml"
                            .to_string(),
                    );
                }
                None
            }
            SlashCommand::Help => {
                self.show_help = !self.show_help;
                if self.show_help {
                    self.add_system_message(SlashCommand::help_text().to_string());
                }
                None
            }
            SlashCommand::Clear => {
                self.messages.clear();
                self.rig_history.clear();
                self.rendered_messages.clear();
                self.total_tokens = TokenUsage::default();
                self.scroll_offset = 0;
                self.streaming.buffer.clear();
                self.show_help = false;
                self.agent_status.clear();
                self.completed_agent_log.clear();
                None
            }
            SlashCommand::Sessions(sub) => {
                self.handle_sessions_command(sub.as_deref());
                None
            }
            SlashCommand::Memory(sub) => {
                self.handle_memory_command(sub.as_deref());
                None
            }
            SlashCommand::Import(path) => {
                self.handle_import_command(&path);
                None
            }
            SlashCommand::Export(session_id_opt) => {
                self.handle_export_command(session_id_opt.as_deref());
                None
            }
            SlashCommand::Agents(sub) => {
                self.handle_agents_command(sub.as_deref());
                None
            }
            SlashCommand::Quit => Some(Action::Quit),
            SlashCommand::Unknown(cmd_name) => {
                self.add_system_message(format!(
                    "Unknown command: /{cmd_name}. Type /help for available commands."
                ));
                None
            }
        }
    }

    /// Add a system message to the chat (for inline info/errors).
    pub(super) fn add_system_message(&mut self, content: String) {
        let msg = ChatMessage::new(Role::System, content);
        let style = Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC);
        let rendered: Vec<Line<'static>> = msg
            .content
            .lines()
            .map(|line| Line::from(Span::styled(line.to_string(), style)))
            .collect();
        self.messages.push(msg);
        self.rendered_messages.push(rendered);
    }

    /// Send a chat message to the AI provider.
    fn send_message(&mut self, text: String) {
        // D-01: Drain pending agent results and inject into rig_history before new turn
        for result in self.pending_agent_results.drain(..) {
            let injection = format!(
                "=== Agent '{}' {} ===\nTurns: {}/{} | Time: {}s\n\n{}",
                result.agent_name,
                result.status_label(),
                result.turns_completed,
                result.max_turns,
                result.elapsed_secs,
                result.full_output,
            );
            // Rig has no System variant -- inject as user message (same pattern as System->rig_message)
            self.rig_history
                .push(rig::message::Message::user(injection));
        }

        // Add user message.
        let user_msg = ChatMessage::new(Role::User, &text);
        let rendered_user = render_user_message(&text);
        // Update rig history with user message.
        self.rig_history.push(user_msg.to_rig_message());
        self.messages.push(user_msg);
        self.rendered_messages.push(rendered_user);

        // Auto-save user message to DB.
        self.save_message_to_db("user", &text, None, None);

        // Start streaming.
        self.streaming.buffer.clear();
        self.streaming.is_streaming = true;
        self.streaming.chat_state = ChatState::Streaming;
        self.streaming.spinner_frame = 0;
        self.scroll_offset = 0;
        self.streaming.started_at = Some(Instant::now());
        self.streaming.thinking_verb = random_thinking_verb();

        if let Some(ref provider) = self.provider {
            if let Some(ref tx) = self.session.action_tx {
                let working_dir =
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                // Create approval hook for this streaming session.
                let hook = if let Some(ref approval_tx) = self.approval_tx {
                    ApprovalHook::new(
                        self.approval_mode,
                        self.deny_rules.clone(),
                        approval_tx.clone(),
                        tx.clone(),
                        self.max_turns,
                        None,
                    )
                } else {
                    // Fallback: create a disconnected hook (approvals will fail gracefully)
                    let (atx, _) = tokio::sync::mpsc::unbounded_channel();
                    ApprovalHook::new(
                        self.approval_mode,
                        self.deny_rules.clone(),
                        atx,
                        tx.clone(),
                        self.max_turns,
                        None,
                    )
                };
                // Store the turn counter for status bar display.
                self.streaming.turn_counter = Some(hook.turn_counter());
                let project_path = working_dir.to_string_lossy().to_string();
                let approval_tx_for_spawn = self.approval_tx.clone().unwrap_or_else(|| {
                    let (atx, _) = tokio::sync::mpsc::unbounded_channel();
                    atx
                });
                let handle = spawn_streaming_chat(
                    provider,
                    StreamChatParams {
                        history: self.rig_history.clone(),
                        prompt: text,
                        system_prompt: self.system_prompt.clone(),
                        tx: tx.clone(),
                        working_dir,
                        brave_api_key: self.brave_api_key.clone(),
                        max_turns: self.max_turns,
                        approval_hook: hook,
                        db: self.session.db.clone(),
                        project_path,
                        agent_registry: Arc::clone(&self.agent_registry),
                        agent_handles: Arc::clone(&self.agent_handles),
                        parent_session_id: self.session.session_id.clone(),
                        approval_tx: approval_tx_for_spawn,
                        parent_approval_mode: self.approval_mode,
                    },
                );
                self.streaming.task = Some(handle);
            }
        } else {
            // No provider - show error inline.
            self.streaming.is_streaming = false;
            let err_msg = self
                .provider_error
                .clone()
                .unwrap_or_else(|| "No AI provider configured".to_string());
            self.add_system_message(format!(
                "Error: {err_msg}\nSet your API key in ~/.seval/config.toml"
            ));
        }
    }

    // --- Public accessors for app-level status bar ---

    /// Number of messages in the conversation.
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Cumulative output tokens generated across all exchanges.
    ///
    /// Uses output tokens only because input tokens include re-sent context
    /// on every agentic turn, inflating the count. Context usage is shown
    /// separately in the status bar.
    pub fn output_tokens(&self) -> u64 {
        self.total_tokens.output_tokens
    }

    /// Whether a streaming response is in progress.
    pub fn is_streaming(&self) -> bool {
        self.streaming.is_streaming
    }

    /// Whether a streaming response is in progress (streaming or awaiting approval).
    pub(super) fn is_busy(&self) -> bool {
        self.streaming.is_busy()
    }

    /// Formatted provider and model string (e.g. "bedrock: claude-sonnet-4-6").
    pub fn provider_display(&self) -> String {
        self.provider.as_ref().map_or_else(
            || "no provider".to_string(),
            |p| format!("{}: {}", p.provider_name(), p.model_name()),
        )
    }

    /// Current spinner animation frame index.
    pub fn spinner_frame(&self) -> usize {
        self.streaming.spinner_frame
    }

    /// Current turn info for the status bar: (`current_turn`, `max_turns`).
    ///
    /// Returns `Some` when streaming with an active turn counter, `None` otherwise.
    pub fn turn_info(&self) -> Option<(usize, usize)> {
        if !self.streaming.is_streaming {
            return None;
        }
        let counter = self.streaming.turn_counter.as_ref()?;
        let current = counter.load(Ordering::Relaxed);
        if current == 0 {
            return None;
        }
        Some((current, self.max_turns))
    }

    /// Query the context window size from the provider.
    ///
    /// Returns 128,000 if no provider is configured.
    pub async fn query_context_window(&self) -> u64 {
        if let Some(ref provider) = self.provider {
            provider.context_window_size().await
        } else {
            128_000
        }
    }

    /// Get the current context token usage as (used, max).
    #[must_use]
    pub fn context_tokens(&self) -> (u64, u64) {
        (
            self.context_state.tokens_used,
            self.context_state.context_window,
        )
    }

    /// Get a mutable reference to the context state.
    pub fn context_state_mut(&mut self) -> &mut ContextState {
        &mut self.context_state
    }

    /// Whether the chat is currently awaiting an approval decision.
    pub fn is_awaiting_approval(&self) -> bool {
        matches!(
            self.streaming.chat_state,
            ChatState::AwaitingApproval { .. }
        )
    }

    /// Get cloned DB handle and action sender, or `None` if either is missing.
    pub(super) fn db_and_tx(
        &self,
    ) -> Option<(Arc<Database>, tokio::sync::mpsc::UnboundedSender<Action>)> {
        self.session.db_and_tx()
    }
}

/// Get the current working directory as a `String`, falling back to `"."`.
pub(crate) fn project_path() -> String {
    std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .to_string_lossy()
        .to_string()
}

impl Component for Chat {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.session.action_tx = Some(tx);
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        // Model picker overlay: capture all keys.
        if self.model_picker.active {
            return Ok(self.handle_model_picker_key(key));
        }

        // AwaitingApproval state: only Y/N/A/Esc/Ctrl+C accepted.
        if matches!(
            self.streaming.chat_state,
            ChatState::AwaitingApproval { .. }
        ) {
            return Ok(self.handle_approval_key(key));
        }

        // During streaming: limited key handling.
        if self.streaming.is_streaming {
            return match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.cancel_agentic_loop();
                    Ok(None)
                }
                KeyCode::Esc => {
                    self.cancel_agentic_loop();
                    Ok(None)
                }
                _ => Ok(None),
            };
        }

        // Normal mode.
        match key.code {
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    || key.modifiers.contains(KeyModifiers::ALT)
                {
                    // Multi-line: insert newline.
                    self.input.insert_newline();
                    return Ok(None);
                }

                if self.input.is_empty() {
                    return Ok(None);
                }

                let text = self.input.submit();

                // Check for slash command.
                if let Some(cmd) = SlashCommand::parse(&text) {
                    let action = self.handle_slash_command(cmd);
                    return Ok(action);
                }

                // Regular message.
                self.send_message(text);
                Ok(None)
            }

            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Ok(Some(Action::Quit))
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Clear screen (same as /clear).
                let _ = self.handle_slash_command(SlashCommand::Clear);
                Ok(None)
            }
            KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Ok(Some(Action::Suspend))
            }

            KeyCode::Backspace => {
                self.input.backspace();
                Ok(None)
            }
            KeyCode::Delete => {
                self.input.delete();
                Ok(None)
            }
            KeyCode::Left => {
                self.input.move_left();
                Ok(None)
            }
            KeyCode::Right => {
                self.input.move_right();
                Ok(None)
            }
            KeyCode::Up => {
                if self.input.cursor_position().0 > 0 {
                    // Multi-line input: move up within input.
                    self.input.move_up();
                } else {
                    // Scroll chat history up, clamped to max.
                    self.scroll_offset = self
                        .scroll_offset
                        .saturating_add(3)
                        .min(self.max_scroll.get());
                }
                Ok(None)
            }
            KeyCode::Down => {
                if self.input.cursor_position().0 < self.input.lines().len().saturating_sub(1) {
                    // Multi-line input: move down within input.
                    self.input.move_down();
                } else {
                    // Scroll chat history down.
                    self.scroll_offset = self.scroll_offset.saturating_sub(3);
                }
                Ok(None)
            }
            KeyCode::Home => {
                self.input.move_home();
                Ok(None)
            }
            KeyCode::End => {
                self.input.move_end();
                Ok(None)
            }
            KeyCode::PageUp => {
                self.scroll_offset = self
                    .scroll_offset
                    .saturating_add(10)
                    .min(self.max_scroll.get());
                Ok(None)
            }
            KeyCode::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
                Ok(None)
            }

            KeyCode::Char(ch) => {
                self.input.insert(ch);
                Ok(None)
            }

            _ => Ok(None),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::TokenUpdate {
                output_tokens,
                context_tokens,
            } => {
                // Live update during multi-turn agentic loops.
                self.total_tokens.output_tokens = output_tokens;
                self.context_state.update_tokens(context_tokens);
                Ok(None)
            }
            Action::StreamChunk(text) => {
                self.streaming.buffer.push_str(&text);
                // Rendering happens on the 30fps tick, not per-chunk.
                Ok(None)
            }
            Action::StreamComplete {
                input_tokens,
                output_tokens,
                context_tokens,
            } => {
                self.streaming.is_streaming = false;
                self.streaming.chat_state = ChatState::Normal;
                self.streaming.task = None;
                self.streaming.turn_counter = None;

                // Capture elapsed time and verb before clearing.
                let elapsed = self
                    .streaming
                    .started_at
                    .take()
                    .map(|t| format_elapsed(t.elapsed()))
                    .unwrap_or_default();
                let (_, past_verb) = self.streaming.thinking_verb;

                // Update cumulative tokens.
                let usage = TokenUsage {
                    input_tokens,
                    output_tokens,
                    total_tokens: input_tokens + output_tokens,
                };
                self.total_tokens.input_tokens += input_tokens;
                self.total_tokens.output_tokens += output_tokens;
                self.total_tokens.total_tokens += input_tokens + output_tokens;

                // Update context state with last turn's input tokens (actual context window usage).
                self.context_state.update_tokens(context_tokens);

                // Check compression thresholds after token update.
                if self.context_state.needs_enforced_compression() {
                    self.start_compression(true); // aggressive
                } else if self.context_state.needs_proactive_compression() {
                    self.start_compression(false); // standard
                }

                // Create assistant message with any remaining buffered text.
                // The buffer may have been partially flushed by tool calls
                // mid-stream, so it might be empty or contain only the tail.
                // Strip raw tool call XML that some models emit as text.
                let clean_buffer = super::tools::strip_tool_call_xml(&self.streaming.buffer);
                if !clean_buffer.is_empty() {
                    let mut assistant_msg = ChatMessage::new(Role::Assistant, &clean_buffer);
                    assistant_msg.token_usage = Some(usage);

                    // Auto-save assistant message to DB.
                    self.save_assistant_message_to_db(&clean_buffer, input_tokens, output_tokens);

                    let rendered = render_markdown(&clean_buffer);
                    self.rig_history.push(assistant_msg.to_rig_message());
                    self.messages.push(assistant_msg);
                    self.rendered_messages.push(rendered);
                }

                // Add completion summary line after the response.
                // Show output tokens only — input tokens include re-sent context
                // on every agentic turn, which inflates the number.
                let summary = format!(
                    "\u{273b} {past_verb}{elapsed} \u{00b7} {} tokens",
                    format_compact_tokens(output_tokens),
                );
                let summary_msg = ChatMessage::new(Role::System, &summary);
                let summary_rendered = vec![Line::from(Span::styled(
                    summary,
                    Style::default().fg(Color::DarkGray),
                ))];
                self.messages.push(summary_msg);
                self.rendered_messages.push(summary_rendered);

                // Clear streaming buffer.
                self.streaming.buffer.clear();
                self.scroll_offset = 0;

                // Generate session title after first exchange (user + assistant = 2 non-system msgs).
                let non_system = self
                    .messages
                    .iter()
                    .filter(|m| m.role != Role::System)
                    .count();
                if non_system == 2 {
                    self.generate_session_title();
                }

                Ok(None)
            }
            Action::StreamError(err) => {
                self.streaming.is_streaming = false;
                self.streaming.chat_state = ChatState::Normal;
                self.streaming.task = None;
                self.streaming.turn_counter = None;
                self.streaming.started_at = None;

                let error_msg = ChatMessage::new(Role::System, format!("Error: {err}"));
                let rendered = vec![Line::from(Span::styled(
                    format!("Error: {err}"),
                    Style::default().fg(Color::Red),
                ))];
                self.messages.push(error_msg);
                self.rendered_messages.push(rendered);

                self.streaming.buffer.clear();
                Ok(None)
            }
            Action::Tick => {
                if self.streaming.is_streaming {
                    self.streaming.spinner_frame = self.streaming.spinner_frame.wrapping_add(1);
                }
                Ok(None)
            }
            Action::ToolCallStart { name, args_json } => {
                self.handle_tool_call_start(&name, &args_json);
                Ok(None)
            }
            Action::ToolResult {
                name,
                result,
                duration_ms,
            } => {
                self.handle_tool_result(&name, &result, duration_ms);
                // Persist tool call to DB.
                if let (Some(db), Some(msg_id)) =
                    (&self.session.db, self.session.last_assistant_msg_id)
                {
                    let db = Arc::clone(db);
                    let name = name.clone();
                    let result = result.clone();
                    #[allow(clippy::cast_possible_wrap)]
                    let dur = Some(duration_ms as i64);
                    tokio::task::spawn_blocking(move || {
                        if let Err(e) =
                            db.save_tool_call(msg_id, &name, "{}", Some(&result), "success", dur)
                        {
                            tracing::warn!("Failed to save tool call: {e}");
                        }
                    });
                }
                Ok(None)
            }
            Action::ToolError { name, error } => {
                self.handle_tool_error(&name, &error);
                Ok(None)
            }
            Action::ToolDenied { name, reason } => {
                self.handle_tool_denied(&name, &reason);
                Ok(None)
            }
            Action::CompressionComplete {
                original_tokens,
                compressed_tokens,
                ref summary,
                messages_removed,
            } => {
                self.handle_compression_complete(
                    original_tokens,
                    compressed_tokens,
                    summary,
                    messages_removed,
                );
                Ok(None)
            }
            Action::ContextWindowUpdate(size) => {
                self.context_state.context_window = size;
                Ok(None)
            }
            Action::Paste(text) => {
                if !self.streaming.is_streaming {
                    self.input.insert_str(&text);
                }
                Ok(None)
            }
            Action::SessionCreated(session_id) => {
                self.session.session_id = Some(session_id);
                Ok(None)
            }
            Action::SessionTitleGenerated(_title) => {
                // Title already persisted in the background task.
                // Could update sidebar or status bar here if needed.
                Ok(None)
            }
            Action::SessionResumed { messages } => {
                // Replace current state with resumed session data.
                self.messages.clear();
                self.rendered_messages.clear();
                self.rig_history.clear();
                self.total_tokens = TokenUsage::default();

                // Re-render all loaded messages and rebuild rig_history.
                for msg in &messages {
                    let rendered = match msg.role {
                        Role::User => render_user_message(&msg.content),
                        Role::Assistant => render_markdown(&msg.content),
                        Role::System => {
                            vec![Line::from(Span::styled(
                                msg.content.clone(),
                                Style::default()
                                    .fg(Color::DarkGray)
                                    .add_modifier(Modifier::ITALIC),
                            ))]
                        }
                    };
                    self.rendered_messages.push(rendered);

                    // Rebuild rig_history from user and assistant messages.
                    if msg.role == Role::User || msg.role == Role::Assistant {
                        self.rig_history.push(msg.to_rig_message());
                    }

                    // Accumulate token counts.
                    if let Some(usage) = &msg.token_usage {
                        self.total_tokens.input_tokens += usage.input_tokens;
                        self.total_tokens.output_tokens += usage.output_tokens;
                        self.total_tokens.total_tokens += usage.total_tokens;
                    }
                }
                self.messages = messages;

                // Update NEXT_ID to avoid collisions.
                let max_id = self.messages.iter().map(|m| m.id).max().unwrap_or(0);
                NEXT_ID.store(max_id + 1, Ordering::Relaxed);

                self.scroll_offset = 0;
                self.add_system_message("Session resumed.".to_string());
                Ok(None)
            }
            Action::SessionDeleted(session_id) => {
                let short = &session_id[..8.min(session_id.len())];
                self.add_system_message(format!("Session {short} deleted."));
                Ok(None)
            }
            Action::ShowSystemMessage(text) => {
                self.add_system_message(text);
                Ok(None)
            }
            Action::AgentStarted { name, max_turns } => {
                self.agent_status.insert(
                    name.clone(),
                    super::agents::AgentStatusEntry {
                        max_turns,
                        current_turn: 0,
                        started_at: std::time::Instant::now(),
                    },
                );
                Ok(None)
            }
            Action::AgentTurnUpdate { name, turn, .. } => {
                if let Some(entry) = self.agent_status.get_mut(name.as_str()) {
                    entry.current_turn = turn;
                }
                Ok(None)
            }
            Action::AgentCompleted(result) => {
                // D-02: Immediate display in chat
                let status_line = format!(
                    "[Agent '{}' {} in {}/{} turns ({}s)]",
                    result.agent_name,
                    result.status_label(),
                    result.turns_completed,
                    result.max_turns,
                    result.elapsed_secs,
                );
                let display_text = format!("{}\n\n{}", status_line, result.display_output,);
                // Show as system message in chat (visible immediately)
                self.add_system_message(display_text);

                // Remove the agent's JoinHandle from the tracking map
                if let Ok(mut handles) = self.agent_handles.lock() {
                    handles.remove(&result.agent_name);
                }

                // Update status tracking for /agents status
                self.agent_status.remove(&result.agent_name);
                self.completed_agent_log.push(super::agents::CompletedAgentInfo {
                    name: result.agent_name.clone(),
                    turns_completed: result.turns_completed,
                    max_turns: result.max_turns,
                    elapsed_secs: result.elapsed_secs,
                    status: result.status_label().to_string(),
                });
                // Cap completed log at 10
                if self.completed_agent_log.len() > 10 {
                    self.completed_agent_log.remove(0);
                }

                // D-01: Queue for next-turn injection into rig_history
                self.pending_agent_results.push(result);

                Ok(None)
            }
            Action::CancelStream => {
                if self.streaming.is_streaming {
                    self.cancel_agentic_loop();
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn draw(&self, frame: &mut Frame, area: Rect) -> Result<()> {
        self.draw_chat(frame, area);
        Ok(())
    }
}

#[cfg(test)]
pub(in crate::chat) mod tests {
    use super::*;
    use crate::config::{
        AppConfig, AwsConfig, BedrockConfig, OpenRouterConfig, ProviderConfig, ProviderKind,
        ToolsConfig,
    };

    fn make_config_no_key() -> AppConfig {
        AppConfig {
            aws: AwsConfig::default(),
            tools: ToolsConfig::default(),
            provider: ProviderConfig {
                active: ProviderKind::Bedrock,
                model: None,
            },
            bedrock: BedrockConfig::default(),
            openrouter: OpenRouterConfig::default(),
            brave_api_key: None,
        }
    }

    pub(in crate::chat) async fn make_chat() -> Chat {
        let config = make_config_no_key();
        let (atx, _arx) = tokio::sync::mpsc::unbounded_channel();
        Chat::new(&config, atx, None).await
    }

    #[tokio::test]
    async fn new_without_api_key_stores_error() {
        let chat = make_chat().await;
        assert!(chat.provider.is_none());
        assert!(chat.provider_error.is_some());
    }

    #[tokio::test]
    async fn slash_command_clear_resets_state() {
        let mut chat = make_chat().await;
        chat.add_system_message("test".to_string());
        assert!(!chat.messages.is_empty());

        chat.handle_slash_command(SlashCommand::Clear);
        assert!(chat.messages.is_empty());
        assert!(chat.rendered_messages.is_empty());
        assert_eq!(chat.total_tokens.total_tokens, 0);
    }

    #[tokio::test]
    async fn slash_command_quit_returns_quit_action() {
        let mut chat = make_chat().await;
        let action = chat.handle_slash_command(SlashCommand::Quit);
        assert_eq!(action, Some(Action::Quit));
    }

    #[tokio::test]
    async fn slash_command_help_adds_system_message() {
        let mut chat = make_chat().await;
        chat.handle_slash_command(SlashCommand::Help);
        assert!(chat.show_help);
        assert!(chat.messages.iter().any(|m| m.role == Role::System));
    }

    #[tokio::test]
    async fn slash_command_unknown_shows_error() {
        let mut chat = make_chat().await;
        chat.handle_slash_command(SlashCommand::Unknown("foo".to_string()));
        let last = chat.messages.last().unwrap();
        assert!(last.content.contains("Unknown command: /foo"));
    }

    #[tokio::test]
    async fn stream_complete_updates_tokens() {
        let mut chat = make_chat().await;
        chat.streaming.is_streaming = true;
        chat.streaming.buffer = "Hello world".to_string();

        let result = chat
            .update(Action::StreamComplete {
                input_tokens: 10,
                output_tokens: 5,
                context_tokens: 10,
            })
            .unwrap();
        assert!(result.is_none());
        assert!(!chat.streaming.is_streaming);
        assert_eq!(chat.total_tokens.input_tokens, 10);
        assert_eq!(chat.total_tokens.output_tokens, 5);
        assert_eq!(chat.total_tokens.total_tokens, 15);
        assert!(chat.streaming.buffer.is_empty());
    }

    #[tokio::test]
    async fn stream_chunk_appends_to_buffer() {
        let mut chat = make_chat().await;
        chat.streaming.is_streaming = true;

        chat.update(Action::StreamChunk("Hello ".to_string()))
            .unwrap();
        chat.update(Action::StreamChunk("world".to_string()))
            .unwrap();
        assert_eq!(chat.streaming.buffer, "Hello world");
    }

    #[tokio::test]
    async fn stream_error_shows_error_message() {
        let mut chat = make_chat().await;
        chat.streaming.is_streaming = true;

        chat.update(Action::StreamError("test error".to_string()))
            .unwrap();
        assert!(!chat.streaming.is_streaming);
        let last = chat.messages.last().unwrap();
        assert!(last.content.contains("test error"));
    }

    #[tokio::test]
    async fn tick_advances_spinner_during_streaming() {
        let mut chat = make_chat().await;
        chat.streaming.is_streaming = true;
        let initial = chat.streaming.spinner_frame;
        chat.update(Action::Tick).unwrap();
        assert_eq!(chat.streaming.spinner_frame, initial + 1);
    }

    #[tokio::test]
    async fn tick_does_not_advance_spinner_when_not_streaming() {
        let mut chat = make_chat().await;
        let initial = chat.streaming.spinner_frame;
        chat.update(Action::Tick).unwrap();
        assert_eq!(chat.streaming.spinner_frame, initial);
    }

    #[tokio::test]
    async fn paste_action_inserts_text() {
        let mut chat = make_chat().await;
        chat.update(Action::Paste("pasted text".to_string()))
            .unwrap();
        assert_eq!(chat.input.content(), "pasted text");
    }

    #[tokio::test]
    async fn paste_action_ignored_during_streaming() {
        let mut chat = make_chat().await;
        chat.streaming.is_streaming = true;
        chat.update(Action::Paste("text".to_string())).unwrap();
        assert!(chat.input.is_empty());
    }

    #[tokio::test]
    async fn send_message_without_provider_shows_error() {
        let mut chat = make_chat().await;
        chat.send_message("hello".to_string());
        // Should have user message + error system message.
        assert!(chat.messages.len() >= 2);
        assert!(!chat.streaming.is_streaming); // streaming should not start
    }

    #[tokio::test]
    async fn stream_complete_resets_state_to_normal() {
        let mut chat = make_chat().await;
        chat.streaming.is_streaming = true;
        chat.streaming.chat_state = ChatState::Streaming;
        chat.streaming.buffer = "response".to_string();

        let _ = chat.update(Action::StreamComplete {
            input_tokens: 10,
            output_tokens: 5,
            context_tokens: 10,
        });
        assert!(!chat.streaming.is_streaming);
        assert!(matches!(chat.streaming.chat_state, ChatState::Normal));
    }
}
