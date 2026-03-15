//! Approval flow methods.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::action::Action;
use crate::approval::{ApprovalDecision, ApprovalRequest};
use crate::chat::message::{ChatMessage, Role};

use super::component::{Chat, ChatState};
use super::rendering::{display_tool_name, tool_color};

impl Chat {
    /// Receive an approval request from the hook and transition to awaiting approval state.
    pub fn receive_approval_request(&mut self, request: ApprovalRequest) {
        let display_name = display_tool_name(&request.tool_name);
        let color = tool_color(&request.tool_name);

        let content = format!("\u{25cf} {display_name} {}", request.formatted_display);
        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  \u{25cf} ", Style::default().fg(color)),
                Span::styled(
                    display_name,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ]),
        ];
        // Show formatted display lines.
        for line in request.formatted_display.lines() {
            lines.push(Line::from(Span::styled(
                format!("  {line}"),
                Style::default().fg(Color::White),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  Allow?  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "[Y]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("es  ", Style::default().fg(Color::Green)),
            Span::styled(
                "[N]",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled("o  ", Style::default().fg(Color::Red)),
            Span::styled(
                "[A]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("ll  ", Style::default().fg(Color::Cyan)),
            Span::styled(
                "[Esc]",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        let msg = ChatMessage::new(Role::System, content);
        self.messages.push(msg);
        self.rendered_messages.push(lines);

        self.streaming.chat_state = ChatState::AwaitingApproval {
            tool_name: request.tool_name,
            response_tx: Some(request.response_tx),
        };
        self.scroll_offset = 0;
    }

    /// Handle key events during the approval prompt.
    ///
    /// Returns `Some(Action)` only for quit; other decisions are side effects.
    pub(super) fn handle_approval_key(&mut self, key: KeyEvent) -> Option<Action> {
        if let ChatState::AwaitingApproval {
            ref mut response_tx,
            ref tool_name,
        } = self.streaming.chat_state
        {
            match key.code {
                KeyCode::Char('y' | 'Y') => {
                    if let Some(tx) = response_tx.take() {
                        let _ = tx.send(ApprovalDecision::Approve);
                    }
                    self.add_system_message(format!("Approved: {tool_name}"));
                    self.streaming.chat_state = ChatState::Streaming;
                }
                KeyCode::Char('n' | 'N') => {
                    let name = tool_name.clone();
                    if let Some(tx) = response_tx.take() {
                        let _ = tx.send(ApprovalDecision::Deny);
                    }
                    self.handle_tool_denied(&name, "Denied by user");
                    self.streaming.chat_state = ChatState::Streaming;
                }
                KeyCode::Char('a' | 'A') => {
                    if let Some(tx) = response_tx.take() {
                        let _ = tx.send(ApprovalDecision::ApproveAll);
                    }
                    self.add_system_message(format!("Approved all: {tool_name}"));
                    self.streaming.chat_state = ChatState::Streaming;
                }
                KeyCode::Esc => {
                    let _ = response_tx.take();
                    self.cancel_agentic_loop();
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Some(Action::Quit);
                }
                _ => {} // All other input blocked during approval
            }
        }
        None
    }

    /// Handle a tool denied action by adding it as an inline denial message.
    pub(super) fn handle_tool_denied(&mut self, name: &str, _reason: &str) {
        let display_name = display_tool_name(name);
        let content = format!("\u{2718} {display_name} denied");
        let rendered = vec![Line::from(vec![
            Span::styled("  \u{2718} ", Style::default().fg(Color::Red)),
            Span::styled(
                format!("{display_name} denied"),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
        ])];
        let msg = ChatMessage::new(Role::System, content);
        self.messages.push(msg);
        self.rendered_messages.push(rendered);
    }
}

#[cfg(test)]
mod tests {
    use crate::action::Action;
    use crate::approval::ApprovalDecision;
    use crate::approval::ApprovalRequest;
    use crate::tui::Component;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::super::component::Chat;
    use super::super::component::tests::make_chat;

    #[tokio::test]
    async fn receive_approval_request_transitions_to_awaiting() {
        let mut chat: Chat = make_chat().await;
        let (tx, _rx) = tokio::sync::oneshot::channel();
        let request = ApprovalRequest {
            tool_name: "shell".to_string(),
            args_json: r#"{"command": "ls"}"#.to_string(),
            formatted_display: "$ ls".to_string(),
            response_tx: tx,
        };
        chat.receive_approval_request(request);
        assert!(chat.is_awaiting_approval());
        // Should have added an approval block message
        assert!(
            chat.messages
                .last()
                .unwrap()
                .content
                .contains("\u{25cf} Shell")
        );
    }

    #[tokio::test]
    async fn approval_y_key_approves_and_returns_to_streaming() {
        let mut chat: Chat = make_chat().await;
        chat.streaming.is_streaming = true;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let request = ApprovalRequest {
            tool_name: "shell".to_string(),
            args_json: "{}".to_string(),
            formatted_display: "$ ls".to_string(),
            response_tx: tx,
        };
        chat.receive_approval_request(request);
        assert!(chat.is_awaiting_approval());

        // Press Y
        let key = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
        let _ = chat.handle_key_event(key);

        assert!(!chat.is_awaiting_approval());
        assert_eq!(rx.await.unwrap(), ApprovalDecision::Approve);
    }

    #[tokio::test]
    async fn approval_n_key_denies() {
        let mut chat: Chat = make_chat().await;
        chat.streaming.is_streaming = true;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let request = ApprovalRequest {
            tool_name: "shell".to_string(),
            args_json: "{}".to_string(),
            formatted_display: "$ ls".to_string(),
            response_tx: tx,
        };
        chat.receive_approval_request(request);

        let key = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE);
        let _ = chat.handle_key_event(key);
        assert_eq!(rx.await.unwrap(), ApprovalDecision::Deny);
        // Should have a denied message
        assert!(chat.messages.iter().any(|m| m.content.contains("denied")));
    }

    #[tokio::test]
    async fn approval_a_key_approves_all() {
        let mut chat: Chat = make_chat().await;
        chat.streaming.is_streaming = true;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let request = ApprovalRequest {
            tool_name: "write".to_string(),
            args_json: "{}".to_string(),
            formatted_display: "Write test.rs".to_string(),
            response_tx: tx,
        };
        chat.receive_approval_request(request);

        let key = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE);
        let _ = chat.handle_key_event(key);
        assert_eq!(rx.await.unwrap(), ApprovalDecision::ApproveAll);
    }

    #[tokio::test]
    async fn approval_esc_cancels_loop() {
        let mut chat: Chat = make_chat().await;
        chat.streaming.is_streaming = true;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let request = ApprovalRequest {
            tool_name: "shell".to_string(),
            args_json: "{}".to_string(),
            formatted_display: "$ rm foo".to_string(),
            response_tx: tx,
        };
        chat.receive_approval_request(request);

        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let _ = chat.handle_key_event(key);

        assert!(!chat.streaming.is_streaming);
        assert!(!chat.is_awaiting_approval());
        // Oneshot sender was dropped, receiver gets error
        assert!(rx.await.is_err());
        // Should have cancellation message
        assert!(
            chat.messages
                .iter()
                .any(|m| m.content.contains("cancelled"))
        );
    }

    #[tokio::test]
    async fn approval_blocks_normal_input() {
        let mut chat: Chat = make_chat().await;
        chat.streaming.is_streaming = true;
        let (tx, _rx) = tokio::sync::oneshot::channel();
        let request = ApprovalRequest {
            tool_name: "shell".to_string(),
            args_json: "{}".to_string(),
            formatted_display: "$ ls".to_string(),
            response_tx: tx,
        };
        chat.receive_approval_request(request);

        // Try typing a regular character -- should be ignored
        let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        let _ = chat.handle_key_event(key);
        assert!(chat.input.is_empty());
    }

    #[tokio::test]
    async fn tool_denied_action_adds_message() {
        let mut chat: Chat = make_chat().await;
        let _ = chat.update(Action::ToolDenied {
            name: "shell".to_string(),
            reason: "Command blocked by deny rule: rm -rf /".to_string(),
        });
        let last = chat.messages.last().unwrap();
        assert!(last.content.contains("\u{2718} Shell denied"));
    }
}
