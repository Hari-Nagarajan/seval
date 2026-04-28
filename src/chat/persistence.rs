//! DB persistence and compression methods.

use std::sync::Arc;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::action::Action;
use crate::ai::compression;
use crate::ai::provider::AiProvider;
use crate::chat::markdown::render_markdown;
use crate::chat::message::{ChatMessage, Role};

use super::component::{Chat, ChatState};

impl Chat {
    /// Spawn a background task to generate a session title from the first exchange.
    pub(super) fn generate_session_title(&self) {
        let (Some(provider), Some(tx), Some(session_id), Some(db)) = (
            self.provider.as_ref().map(Arc::clone),
            self.session.action_tx.clone(),
            self.session.session_id.clone(),
            self.session.db.as_ref().map(Arc::clone),
        ) else {
            return;
        };

        // Get first user and first assistant message content.
        let first_user = self
            .messages
            .iter()
            .find(|m| m.role == Role::User)
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let first_assistant = self
            .messages
            .iter()
            .find(|m| m.role == Role::Assistant)
            .map(|m| m.content.clone())
            .unwrap_or_default();

        tokio::spawn(async move {
            use rig::client::CompletionClient;
            use rig::completion::Prompt;

            let user_preview = &first_user[..first_user.len().min(200)];
            let asst_preview = &first_assistant[..first_assistant.len().min(200)];
            let prompt = format!(
                "Generate a short (3-6 word) descriptive title for this conversation. \
                 Respond with ONLY the title, no quotes or punctuation.\n\n\
                 User: {user_preview}\nAssistant: {asst_preview}"
            );

            let result: Result<String, _> = match provider.as_ref() {
                AiProvider::Bedrock { client, model } => {
                    let agent = client.agent(model).max_tokens(50).build();
                    agent.prompt(&prompt).await.map_err(|e| e.to_string())
                }
                AiProvider::OpenRouter { client, model } => {
                    let agent = client.agent(model).max_tokens(50).build();
                    agent.prompt(&prompt).await.map_err(|e| e.to_string())
                }
                AiProvider::ChatGpt { client, model } => {
                    let codex_model =
                        crate::ai::codex_model::CodexCompletionModel::new(client.clone(), model);
                    let agent = rig::agent::AgentBuilder::new(codex_model)
                        .max_tokens(50)
                        .build();
                    agent.prompt(&prompt).await.map_err(|e| e.to_string())
                }
            };

            if let Ok(title) = result {
                let title = title.trim().to_string();
                if !title.is_empty() {
                    // Persist the title.
                    let db2 = db;
                    let sid = session_id;
                    let t = title.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Err(e) = db2.update_session_name(&sid, &t) {
                            tracing::warn!("Failed to save session title: {e}");
                        }
                    })
                    .await;
                    let _ = tx.send(Action::SessionTitleGenerated(title));
                }
            }
        });
    }

    /// Persist a message to the database (fire-and-forget).
    pub(super) fn save_message_to_db(
        &self,
        role: &str,
        content: &str,
        token_input: Option<i64>,
        token_output: Option<i64>,
    ) {
        if let (Some(db), Some(session_id)) = (&self.session.db, &self.session.session_id) {
            let db = Arc::clone(db);
            let sid = session_id.clone();
            let role = role.to_string();
            let content = content.to_string();
            let tx = self.session.action_tx.clone();
            tokio::task::spawn_blocking(move || {
                match db.save_message(&sid, &role, &content, token_input, token_output) {
                    Ok(_msg_id) => {
                        // We don't track msg_id for user messages, only assistant.
                        // If needed, could send back via action.
                    }
                    Err(e) => {
                        tracing::warn!("Failed to save message: {e}");
                        if let Some(tx) = tx {
                            let _ = tx.send(Action::Error(format!("DB save failed: {e}")));
                        }
                    }
                }
            });
        }
    }

    /// Save an assistant message and track its DB row ID for tool call association.
    pub(super) fn save_assistant_message_to_db(
        &mut self,
        content: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) {
        if let (Some(db), Some(session_id)) = (&self.session.db, &self.session.session_id) {
            let db = Arc::clone(db);
            let sid = session_id.clone();
            let content = content.to_string();
            #[allow(clippy::cast_possible_wrap)]
            let ti = Some(input_tokens as i64);
            #[allow(clippy::cast_possible_wrap)]
            let to = Some(output_tokens as i64);

            // We need the msg_id back for tool call association.
            // Use a channel to get it synchronously enough.
            let (id_tx, id_rx) = std::sync::mpsc::channel();
            tokio::task::spawn_blocking(move || {
                match db.save_message(&sid, "assistant", &content, ti, to) {
                    Ok(msg_id) => {
                        let _ = id_tx.send(Some(msg_id));
                    }
                    Err(e) => {
                        tracing::warn!("Failed to save assistant message: {e}");
                        let _ = id_tx.send(None);
                    }
                }
            });
            // Try to get the ID quickly (non-blocking attempt then fallback).
            self.session.last_assistant_msg_id = id_rx.try_recv().ok().flatten();
        }
    }

    /// Cancel the agentic loop: abort the streaming task, drop approval sender.
    pub(super) fn cancel_agentic_loop(&mut self) {
        // Abort the streaming task if running.
        if let Some(handle) = self.streaming.task.take() {
            handle.abort();
        }
        // Drop the oneshot sender if in approval state (causes hook to receive error).
        self.streaming.chat_state = ChatState::Normal;
        self.streaming.is_streaming = false;
        self.streaming.turn_counter = None;
        self.streaming.started_at = None;

        // Keep partial results in the streaming buffer.
        if !self.streaming.buffer.is_empty() {
            let partial = std::mem::take(&mut self.streaming.buffer);
            let rendered = render_markdown(&partial);
            let msg = ChatMessage::new(Role::Assistant, &partial);
            self.rig_history.push(msg.to_rig_message());
            self.messages.push(msg);
            self.rendered_messages.push(rendered);
        }

        self.add_system_message("Agentic loop cancelled.".to_string());
    }

    /// Start a background compression task.
    ///
    /// Sets the compressing flag, clones the messages for the background task,
    /// and adds a system message notification.
    pub(super) fn start_compression(&mut self, aggressive: bool) {
        self.context_state.compressing = true;

        let messages = self.messages.clone();
        if let (Some(provider), Some(tx)) = (&self.provider, &self.session.action_tx) {
            compression::spawn_compression_task(
                Arc::clone(provider),
                messages,
                aggressive,
                tx.clone(),
            );
            let mode = if aggressive { "enforced" } else { "proactive" };
            self.add_system_message(format!("Compressing context ({mode})..."));
        }
    }

    /// Handle compression completion: replace old messages with summary.
    #[allow(clippy::needless_pass_by_value)] // summary comes from Action enum destructuring
    pub(super) fn handle_compression_complete(
        &mut self,
        original_tokens: u64,
        compressed_tokens: u64,
        summary: &str,
        messages_removed: usize,
    ) {
        // Remove compressed messages from the front.
        if messages_removed <= self.messages.len() {
            self.messages.drain(..messages_removed);
            // Also drain rendered messages to stay in sync.
            if messages_removed <= self.rendered_messages.len() {
                self.rendered_messages.drain(..messages_removed);
            }
        }

        // Insert the summary as a system message at position 0.
        let summary_msg = ChatMessage::new(Role::System, format!("[Context Summary]\n{summary}"));
        let rendered_summary = vec![Line::from(Span::styled(
            format!("[Context Summary] ({messages_removed} messages compressed)"),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        ))];
        self.messages.insert(0, summary_msg);
        self.rendered_messages.insert(0, rendered_summary);

        // Rebuild rig_history from scratch to stay in sync.
        self.rig_history = self
            .messages
            .iter()
            .map(ChatMessage::to_rig_message)
            .collect();

        // Reset context state after compression.
        self.context_state
            .reset_after_compression(compressed_tokens);

        // Add notification system message with stats.
        let orig_display = crate::chat::context::format_token_count(original_tokens);
        let comp_display = crate::chat::context::format_token_count(compressed_tokens);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let reduction = if original_tokens > 0 {
            ((1.0 - (compressed_tokens as f64 / original_tokens as f64)) * 100.0) as u64
        } else {
            0
        };
        self.add_system_message(format!(
            "Context compressed: {orig_display} -> {comp_display} ({reduction}% reduction)"
        ));
    }
}
