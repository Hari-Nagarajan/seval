//! Session and memory slash command handlers.

use std::fmt::Write;

use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;
use crate::chat::message::{ChatMessage, Role, TokenUsage};
use crate::session::db::Database;

use super::component::{Chat, project_path};

/// Resolve a session by ID prefix: returns `Some(session_id)` on unique match,
/// sends an error message and returns `None` on 0 or multiple matches.
fn resolve_session_prefix(
    db: &Database,
    prefix: &str,
    tx: &UnboundedSender<Action>,
) -> Option<String> {
    match db.list_sessions(None) {
        Ok(sessions) => {
            let matching: Vec<_> = sessions
                .iter()
                .filter(|s| s.id.starts_with(prefix))
                .collect();
            if matching.is_empty() {
                let _ = tx.send(Action::ShowSystemMessage(format!(
                    "No session found matching '{prefix}'"
                )));
                None
            } else if matching.len() > 1 {
                let _ = tx.send(Action::ShowSystemMessage(format!(
                    "Multiple sessions match '{prefix}'. Be more specific."
                )));
                None
            } else {
                Some(matching[0].id.clone())
            }
        }
        Err(e) => {
            let _ = tx.send(Action::ShowSystemMessage(format!(
                "Error finding session: {e}"
            )));
            None
        }
    }
}

impl Chat {
    /// Handle /sessions subcommands.
    #[allow(clippy::too_many_lines)]
    pub(super) fn handle_sessions_command(&mut self, sub: Option<&str>) {
        let Some((db, tx)) = self.db_and_tx() else {
            self.add_system_message("Database not available.".to_string());
            return;
        };

        match sub {
            None | Some("list") => {
                // List sessions for current project.
                let project = project_path();
                tokio::task::spawn_blocking(move || match db.list_sessions(Some(&project)) {
                    Ok(sessions) => {
                        if sessions.is_empty() {
                            let _ = tx.send(Action::ShowSystemMessage(
                                "No saved sessions for this project.".to_string(),
                            ));
                        } else {
                            let mut text = String::from("Saved sessions:\n");
                            for s in &sessions {
                                let name = s.name.as_deref().unwrap_or("(untitled)");
                                let id_short = &s.id[..8.min(s.id.len())];
                                let _ = writeln!(
                                    text,
                                    "  {id_short}  {name}  ({} msgs, {})",
                                    s.message_count, s.updated_at
                                );
                            }
                            text.push_str("\nUse /sessions resume <id> or /sessions delete <id>");
                            let _ = tx.send(Action::ShowSystemMessage(text));
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Action::ShowSystemMessage(format!(
                            "Error listing sessions: {e}"
                        )));
                    }
                });
            }
            Some(cmd) if cmd.starts_with("resume ") => {
                let id_prefix = cmd.trim_start_matches("resume ").trim();
                if id_prefix.is_empty() {
                    self.add_system_message("Usage: /sessions resume <id>".to_string());
                    return;
                }
                let id_prefix = id_prefix.to_string();
                tokio::task::spawn_blocking(move || {
                    let Some(session_id) = resolve_session_prefix(&db, &id_prefix, &tx) else {
                        return;
                    };
                    match db.get_session_messages(&session_id) {
                        Ok(stored) => {
                            let mut messages = Vec::new();
                            for m in &stored {
                                let role = match m.role.as_str() {
                                    "user" => Role::User,
                                    "assistant" => Role::Assistant,
                                    _ => Role::System,
                                };
                                let mut cm = ChatMessage::new(role, &m.content);
                                if let (Some(inp), Some(out)) = (m.token_input, m.token_output) {
                                    #[allow(clippy::cast_sign_loss)]
                                    {
                                        cm.token_usage = Some(TokenUsage {
                                            input_tokens: inp as u64,
                                            output_tokens: out as u64,
                                            total_tokens: (inp + out) as u64,
                                        });
                                    }
                                }
                                messages.push(cm);
                            }
                            let _ = tx.send(Action::SessionResumed { messages });
                        }
                        Err(e) => {
                            let _ = tx.send(Action::ShowSystemMessage(format!(
                                "Error loading session: {e}"
                            )));
                        }
                    }
                });
            }
            Some(cmd) if cmd.starts_with("delete ") => {
                let id_prefix = cmd.trim_start_matches("delete ").trim();
                if id_prefix.is_empty() {
                    self.add_system_message("Usage: /sessions delete <id>".to_string());
                    return;
                }
                let id_prefix = id_prefix.to_string();
                tokio::task::spawn_blocking(move || {
                    let Some(session_id) = resolve_session_prefix(&db, &id_prefix, &tx) else {
                        return;
                    };
                    match db.delete_session(&session_id) {
                        Ok(()) => {
                            let _ = tx.send(Action::SessionDeleted(session_id));
                        }
                        Err(e) => {
                            let _ = tx.send(Action::ShowSystemMessage(format!(
                                "Error deleting session: {e}"
                            )));
                        }
                    }
                });
            }
            Some(other) => {
                self.add_system_message(format!(
                    "Unknown sessions subcommand: {other}\nUsage: /sessions [list|resume <id>|delete <id>]"
                ));
            }
        }
    }

    /// Handle /memory subcommands.
    pub(super) fn handle_memory_command(&mut self, sub: Option<&str>) {
        let Some((db, tx)) = self.db_and_tx() else {
            self.add_system_message("Database not available.".to_string());
            return;
        };

        match sub {
            None | Some("list") => {
                let project = project_path();
                tokio::task::spawn_blocking(move || match db.get_memories(&project) {
                    Ok(memories) => {
                        if memories.is_empty() {
                            let _ = tx.send(Action::ShowSystemMessage(
                                "No saved memories for this project.".to_string(),
                            ));
                        } else {
                            let mut text = String::from("Project memories:\n");
                            for m in &memories {
                                let _ =
                                    writeln!(text, "  [{}] {} ({})", m.id, m.content, m.created_at);
                            }
                            text.push_str("\nUse /memory delete <id> to remove an entry.");
                            let _ = tx.send(Action::ShowSystemMessage(text));
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Action::ShowSystemMessage(format!(
                            "Error listing memories: {e}"
                        )));
                    }
                });
            }
            Some(cmd) if cmd.starts_with("delete ") => {
                let id_str = cmd.trim_start_matches("delete ").trim();
                let Ok(id) = id_str.parse::<i64>() else {
                    let _ = tx.send(Action::ShowSystemMessage(format!(
                        "Invalid memory ID: {id_str}. Use /memory to see IDs."
                    )));
                    return;
                };
                tokio::task::spawn_blocking(move || match db.delete_memory(id) {
                    Ok(()) => {
                        let _ = tx.send(Action::ShowSystemMessage(format!("Memory {id} deleted.")));
                    }
                    Err(e) => {
                        let _ = tx.send(Action::ShowSystemMessage(format!(
                            "Error deleting memory: {e}"
                        )));
                    }
                });
            }
            Some(other) => {
                self.add_system_message(format!(
                    "Unknown memory subcommand: {other}\nUsage: /memory [list|delete <id>]"
                ));
            }
        }
    }

    /// Handle the /import command.
    pub(super) fn handle_import_command(&mut self, path: &str) {
        let Some((db, tx)) = self.db_and_tx() else {
            self.add_system_message("Database not available.".to_string());
            return;
        };

        let path = std::path::PathBuf::from(path);
        tokio::task::spawn_blocking(move || {
            match crate::session::import_export::import_seval_session(&db, &path) {
                Ok(session_id) => {
                    // Get session details for the message.
                    let detail = if let Ok(sessions) = db.list_sessions(None) {
                        if let Some(s) = sessions.iter().find(|s| s.id == session_id) {
                            let name = s.name.as_deref().unwrap_or("(untitled)");
                            let short_id = &s.id[..8.min(s.id.len())];
                            format!(
                                "Imported session: {name} ({short_id}) with {} messages",
                                s.message_count
                            )
                        } else {
                            let short_id = &session_id[..8.min(session_id.len())];
                            format!("Imported session: {short_id}")
                        }
                    } else {
                        let short_id = &session_id[..8.min(session_id.len())];
                        format!("Imported session: {short_id}")
                    };
                    let _ = tx.send(Action::ShowSystemMessage(detail));
                }
                Err(e) => {
                    let _ = tx.send(Action::ShowSystemMessage(format!("Import failed: {e}")));
                }
            }
        });
    }

    /// Handle the /export command.
    pub(super) fn handle_export_command(&mut self, session_id_opt: Option<&str>) {
        let Some((db, tx)) = self.db_and_tx() else {
            self.add_system_message("Database not available.".to_string());
            return;
        };

        let session_id = session_id_opt
            .map(String::from)
            .or_else(|| self.session.session_id.clone());

        let Some(sid) = session_id else {
            self.add_system_message("No active session to export.".to_string());
            return;
        };

        tokio::task::spawn_blocking(move || {
            let export_dir = directories::BaseDirs::new().map_or_else(
                || std::path::PathBuf::from("exports"),
                |b| b.home_dir().join(".seval").join("exports"),
            );
            let short_id = &sid[..8.min(sid.len())];
            let output_path = export_dir.join(format!("{sid}.json"));

            match crate::session::import_export::export_seval_session(&db, &sid, &output_path) {
                Ok(()) => {
                    let _ = tx.send(Action::ShowSystemMessage(format!(
                        "Exported session {short_id} to: {}",
                        export_dir.display()
                    )));
                }
                Err(e) => {
                    let _ = tx.send(Action::ShowSystemMessage(format!("Export failed: {e}")));
                }
            }
        });
    }
}
