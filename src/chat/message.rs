//! Chat message types.
//!
//! Defines the core message model for conversations, with conversion
//! to and from Rig's message types.

use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub(crate) static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Role of a message participant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// User message.
    User,
    /// Assistant (AI) message.
    Assistant,
    /// System message.
    System,
}

/// Token usage statistics for a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    /// Tokens consumed by input.
    pub input_tokens: u64,
    /// Tokens generated as output.
    pub output_tokens: u64,
    /// Total tokens (input + output).
    pub total_tokens: u64,
}

/// A chat message with metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Unique message ID.
    pub id: u64,
    /// Role of the message author.
    pub role: Role,
    /// Message content.
    pub content: String,
    /// When the message was created.
    pub timestamp: DateTime<Utc>,
    /// Token usage (populated after AI response completes).
    pub token_usage: Option<TokenUsage>,
}

impl ChatMessage {
    /// Create a new chat message with the given role and content.
    #[must_use]
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            role,
            content: content.into(),
            timestamp: Utc::now(),
            token_usage: None,
        }
    }

    /// Convert this message to a Rig `Message` for API calls.
    #[must_use]
    pub fn to_rig_message(&self) -> rig::message::Message {
        match self.role {
            Role::User | Role::System => rig::message::Message::user(&self.content),
            Role::Assistant => rig::message::Message::assistant(&self.content),
        }
    }

    /// Create a `ChatMessage` from a Rig `Message`.
    #[must_use]
    pub fn from_rig_message(msg: &rig::message::Message) -> Self {
        let (role, content) = match msg {
            rig::message::Message::User { content } => {
                let text = content
                    .iter()
                    .filter_map(|c| {
                        if let rig::message::UserContent::Text(t) = c {
                            Some(t.text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("");
                (Role::User, text)
            }
            rig::message::Message::Assistant { content, .. } => {
                let text = content
                    .iter()
                    .filter_map(|c| {
                        if let rig::message::AssistantContent::Text(t) = c {
                            Some(t.text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("");
                (Role::Assistant, text)
            }
            rig::message::Message::System { content } => (Role::System, content.clone()),
        };
        Self::new(role, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_message_with_correct_fields() {
        let msg = ChatMessage::new(Role::User, "hello world");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "hello world");
        assert!(msg.id > 0);
        assert!(msg.token_usage.is_none());
        // Timestamp should be recent (within last second)
        let elapsed = Utc::now() - msg.timestamp;
        assert!(elapsed.num_seconds() < 2);
    }

    #[test]
    fn new_assistant_message() {
        let msg = ChatMessage::new(Role::Assistant, "I can help with that");
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content, "I can help with that");
    }

    #[test]
    fn ids_are_unique() {
        let m1 = ChatMessage::new(Role::User, "a");
        let m2 = ChatMessage::new(Role::User, "b");
        assert_ne!(m1.id, m2.id);
    }

    #[test]
    fn to_rig_message_user() {
        let msg = ChatMessage::new(Role::User, "test query");
        let rig_msg = msg.to_rig_message();
        // Should be a Human variant
        assert!(matches!(rig_msg, rig::message::Message::User { .. }));
    }

    #[test]
    fn to_rig_message_assistant() {
        let msg = ChatMessage::new(Role::Assistant, "test response");
        let rig_msg = msg.to_rig_message();
        assert!(matches!(rig_msg, rig::message::Message::Assistant { .. }));
    }

    #[test]
    fn from_rig_message_user_roundtrip() {
        let original = ChatMessage::new(Role::User, "hello from user");
        let rig_msg = original.to_rig_message();
        let restored = ChatMessage::from_rig_message(&rig_msg);
        assert_eq!(restored.role, Role::User);
        assert_eq!(restored.content, "hello from user");
    }

    #[test]
    fn from_rig_message_assistant_roundtrip() {
        let original = ChatMessage::new(Role::Assistant, "hello from assistant");
        let rig_msg = original.to_rig_message();
        let restored = ChatMessage::from_rig_message(&rig_msg);
        assert_eq!(restored.role, Role::Assistant);
        assert_eq!(restored.content, "hello from assistant");
    }

    #[test]
    fn token_usage_tracks_counts() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
        };
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn token_usage_default_is_zero() {
        let usage = TokenUsage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
    }
}
