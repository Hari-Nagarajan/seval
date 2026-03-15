//! Data models for session persistence.
//!
//! Defines the structs used to represent sessions, messages, and tool calls
//! as stored in the `SQLite` database.

use serde::{Deserialize, Serialize};

/// A stored session record from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    /// Unique session ID (UUID v4).
    pub id: String,
    /// Filesystem path of the project this session belongs to.
    pub project_path: String,
    /// Human-readable session name (AI-generated or user-set).
    pub name: Option<String>,
    /// Model identifier used for this session.
    pub model: Option<String>,
    /// Number of messages in this session.
    pub message_count: i64,
    /// ISO 8601 timestamp of session creation.
    pub created_at: String,
    /// ISO 8601 timestamp of last activity.
    pub updated_at: String,
}

/// A stored message from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    /// Auto-increment row ID.
    pub id: i64,
    /// Session this message belongs to.
    pub session_id: String,
    /// Message role: "user", "assistant", or "system".
    pub role: String,
    /// Message content text.
    pub content: String,
    /// Input tokens consumed (for assistant messages).
    pub token_input: Option<i64>,
    /// Output tokens generated (for assistant messages).
    pub token_output: Option<i64>,
    /// ISO 8601 timestamp.
    pub created_at: String,
}

/// A stored memory entry from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    /// Auto-increment row ID.
    pub id: i64,
    /// Filesystem path of the project this memory belongs to.
    pub project_path: String,
    /// Memory content text.
    pub content: String,
    /// Source of the memory: "auto" (AI-saved) or "user" (manually added).
    pub source: String,
    /// ISO 8601 timestamp.
    pub created_at: String,
}

/// A stored tool call from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredToolCall {
    /// Auto-increment row ID.
    pub id: i64,
    /// The message this tool call is associated with.
    pub message_id: i64,
    /// Tool name.
    pub name: String,
    /// JSON-encoded arguments.
    pub args_json: String,
    /// Tool output text (None if pending).
    pub result_text: Option<String>,
    /// Execution status: "pending", "success", "error", "denied".
    pub status: String,
    /// Execution duration in milliseconds.
    pub duration_ms: Option<i64>,
    /// ISO 8601 timestamp.
    pub created_at: String,
}
