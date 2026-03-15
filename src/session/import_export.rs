//! SEVAL-CLI session format compatibility.
//!
//! Provides import/export between SEVAL-CLI's `ConversationRecord` JSON format
//! and seval's `SQLite` database. Enables migration between tools.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::db::Database;

/// SEVAL-CLI conversation record (top-level JSON structure).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SevalConversationRecord {
    /// Unique session ID.
    pub session_id: String,
    /// Hashed project identifier.
    pub project_hash: String,
    /// ISO 8601 start timestamp.
    pub start_time: String,
    /// ISO 8601 last-updated timestamp.
    pub last_updated: String,
    /// Optional session name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Ordered list of messages.
    pub messages: Vec<SevalMessageRecord>,
}

/// A message in the SEVAL-CLI conversation record.
///
/// Internally tagged by `"type"` field with lowercase variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SevalMessageRecord {
    /// User message.
    #[serde(rename = "user")]
    #[serde(rename_all = "camelCase")]
    User {
        id: String,
        timestamp: String,
        content: String,
    },
    /// Assistant message with optional tool calls and token info.
    #[serde(rename = "assistant")]
    #[serde(rename_all = "camelCase")]
    Assistant {
        id: String,
        timestamp: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<SevalToolCallRecord>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tokens: Option<SevalTokensSummary>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    /// Compression marker (context window management).
    #[serde(rename = "compression")]
    #[serde(rename_all = "camelCase")]
    Compression {
        id: String,
        timestamp: String,
        content: String,
        summary: String,
        tokens_before: i64,
        tokens_after: i64,
    },
}

/// A tool call record in SEVAL-CLI format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SevalToolCallRecord {
    /// Tool name.
    pub name: String,
    /// Tool arguments (arbitrary JSON).
    pub args: serde_json::Value,
    /// Tool result text, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

/// Token usage summary in SEVAL-CLI format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SevalTokensSummary {
    /// Input tokens consumed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<i64>,
    /// Output tokens generated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<i64>,
}

/// Import a SEVAL-CLI `ConversationRecord` JSON file into the database.
///
/// Creates a new session and saves all messages (and their tool calls).
/// Returns the session ID of the newly imported session.
pub fn import_seval_session(db: &Database, json_path: &Path) -> Result<String> {
    let json_str =
        std::fs::read_to_string(json_path).context("Failed to read SEVAL-CLI JSON file")?;
    import_seval_session_from_str(db, &json_str)
}

/// Import from a JSON string (used by both file import and tests).
pub fn import_seval_session_from_str(db: &Database, json_str: &str) -> Result<String> {
    let record: SevalConversationRecord =
        serde_json::from_str(json_str).context("Failed to parse SEVAL-CLI JSON")?;

    let project_path = format!("imported/{}", record.project_hash);
    let session = db.create_session(&project_path, None)?;

    // Set session name if present.
    if let Some(ref name) = record.name {
        db.update_session_name(&session.id, name)?;
    }

    for msg in &record.messages {
        match msg {
            SevalMessageRecord::User {
                content,
                ..
            } => {
                db.save_message(&session.id, "user", content, None, None)?;
            }
            SevalMessageRecord::Assistant {
                content,
                tool_calls,
                tokens,
                ..
            } => {
                let (tok_in, tok_out) = tokens
                    .as_ref()
                    .map_or((None, None), |t| (t.input_tokens, t.output_tokens));
                let msg_id =
                    db.save_message(&session.id, "assistant", content, tok_in, tok_out)?;

                if let Some(calls) = tool_calls {
                    for tc in calls {
                        let args_json = serde_json::to_string(&tc.args).unwrap_or_default();
                        db.save_tool_call(
                            msg_id,
                            &tc.name,
                            &args_json,
                            tc.result.as_deref(),
                            "success",
                            None,
                        )?;
                    }
                }
            }
            SevalMessageRecord::Compression {
                content, summary, ..
            } => {
                // Store compression events as system messages with summary.
                let text = format!("[Compression] {summary}\n{content}");
                db.save_message(&session.id, "system", &text, None, None)?;
            }
        }
    }

    Ok(session.id)
}

/// Export a database session to SEVAL-CLI `ConversationRecord` JSON format.
///
/// Writes the JSON to the specified output path.
pub fn export_seval_session(db: &Database, session_id: &str, output_path: &Path) -> Result<()> {
    let json_str = export_seval_session_to_string(db, session_id)?;
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output_path, json_str)?;
    Ok(())
}

/// Export a database session to a JSON string (used by both file export and tests).
pub fn export_seval_session_to_string(db: &Database, session_id: &str) -> Result<String> {
    let sessions = db.list_sessions(None)?;
    let session = sessions
        .iter()
        .find(|s| s.id == session_id)
        .context("Session not found")?;

    let stored_messages = db.get_session_messages(session_id)?;

    let mut messages = Vec::new();
    for m in &stored_messages {
        match m.role.as_str() {
            "user" => {
                messages.push(SevalMessageRecord::User {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: m.created_at.clone(),
                    content: m.content.clone(),
                });
            }
            "assistant" => {
                let tool_calls_stored = db.get_message_tool_calls(m.id)?;
                let tool_calls = if tool_calls_stored.is_empty() {
                    None
                } else {
                    Some(
                        tool_calls_stored
                            .iter()
                            .map(|tc| SevalToolCallRecord {
                                name: tc.name.clone(),
                                args: serde_json::from_str(&tc.args_json)
                                    .unwrap_or(serde_json::Value::Object(serde_json::Map::default())),
                                result: tc.result_text.clone(),
                            })
                            .collect(),
                    )
                };
                let tokens = match (m.token_input, m.token_output) {
                    (None, None) => None,
                    (i, o) => Some(SevalTokensSummary {
                        input_tokens: i,
                        output_tokens: o,
                    }),
                };
                messages.push(SevalMessageRecord::Assistant {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: m.created_at.clone(),
                    content: m.content.clone(),
                    tool_calls,
                    tokens,
                    model: session.model.clone(),
                });
            }
            _ => {
                // System messages (including compression) become user messages
                // in SEVAL-CLI format since it has no system type.
                messages.push(SevalMessageRecord::User {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: m.created_at.clone(),
                    content: m.content.clone(),
                });
            }
        }
    }

    // Derive project hash from project_path.
    let project_hash = if session.project_path.starts_with("imported/") {
        session.project_path.trim_start_matches("imported/").to_string()
    } else {
        // Hash the project path for export.
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        session.project_path.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    };

    let record = SevalConversationRecord {
        session_id: session.id.clone(),
        project_hash,
        start_time: session.created_at.clone(),
        last_updated: session.updated_at.clone(),
        name: session.name.clone(),
        messages,
    };

    serde_json::to_string_pretty(&record).context("Failed to serialize to JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sample SEVAL-CLI JSON with user, assistant (with tool calls), and compression messages.
    fn sample_seval_json() -> &'static str {
        r#"{
            "sessionId": "abc-123-def",
            "projectHash": "hash42",
            "startTime": "2025-01-01T00:00:00Z",
            "lastUpdated": "2025-01-01T01:00:00Z",
            "name": "Security Audit",
            "messages": [
                {
                    "type": "user",
                    "id": "msg-1",
                    "timestamp": "2025-01-01T00:01:00Z",
                    "content": "Check this server for vulnerabilities"
                },
                {
                    "type": "assistant",
                    "id": "msg-2",
                    "timestamp": "2025-01-01T00:02:00Z",
                    "content": "I'll scan the server now.",
                    "toolCalls": [
                        {
                            "name": "shell",
                            "args": {"command": "nmap -sV localhost"},
                            "result": "PORT  STATE  SERVICE\n22/tcp open ssh"
                        }
                    ],
                    "tokens": {
                        "inputTokens": 150,
                        "outputTokens": 50
                    },
                    "model": "claude-sonnet"
                },
                {
                    "type": "compression",
                    "id": "msg-3",
                    "timestamp": "2025-01-01T00:30:00Z",
                    "content": "Context compressed",
                    "summary": "Scanned server, found SSH on port 22",
                    "tokensBefore": 5000,
                    "tokensAfter": 500
                }
            ]
        }"#
    }

    #[test]
    fn deserialize_seval_conversation_record() {
        let record: SevalConversationRecord = serde_json::from_str(sample_seval_json()).unwrap();
        assert_eq!(record.session_id, "abc-123-def");
        assert_eq!(record.project_hash, "hash42");
        assert_eq!(record.name, Some("Security Audit".to_string()));
        assert_eq!(record.messages.len(), 3);

        // Check user message.
        match &record.messages[0] {
            SevalMessageRecord::User { content, .. } => {
                assert_eq!(content, "Check this server for vulnerabilities");
            }
            other => panic!("Expected User, got {:?}", other),
        }

        // Check assistant message with tool calls.
        match &record.messages[1] {
            SevalMessageRecord::Assistant {
                content,
                tool_calls,
                tokens,
                model,
                ..
            } => {
                assert_eq!(content, "I'll scan the server now.");
                let calls = tool_calls.as_ref().unwrap();
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].name, "shell");
                assert!(calls[0].result.is_some());
                let tok = tokens.as_ref().unwrap();
                assert_eq!(tok.input_tokens, Some(150));
                assert_eq!(tok.output_tokens, Some(50));
                assert_eq!(model.as_deref(), Some("claude-sonnet"));
            }
            other => panic!("Expected Assistant, got {:?}", other),
        }

        // Check compression message.
        match &record.messages[2] {
            SevalMessageRecord::Compression {
                summary,
                tokens_before,
                tokens_after,
                ..
            } => {
                assert_eq!(summary, "Scanned server, found SSH on port 22");
                assert_eq!(*tokens_before, 5000);
                assert_eq!(*tokens_after, 500);
            }
            other => panic!("Expected Compression, got {:?}", other),
        }
    }

    #[test]
    fn serialize_seval_conversation_record_camel_case() {
        let record = SevalConversationRecord {
            session_id: "test-id".to_string(),
            project_hash: "hash".to_string(),
            start_time: "2025-01-01T00:00:00Z".to_string(),
            last_updated: "2025-01-01T01:00:00Z".to_string(),
            name: None,
            messages: vec![SevalMessageRecord::User {
                id: "u1".to_string(),
                timestamp: "2025-01-01T00:00:00Z".to_string(),
                content: "hello".to_string(),
            }],
        };
        let json = serde_json::to_string(&record).unwrap();
        // Verify camelCase field names.
        assert!(json.contains("\"sessionId\""));
        assert!(json.contains("\"projectHash\""));
        assert!(json.contains("\"startTime\""));
        assert!(json.contains("\"lastUpdated\""));
        // Should NOT contain snake_case.
        assert!(!json.contains("\"session_id\""));
        assert!(!json.contains("\"project_hash\""));
    }

    #[test]
    fn import_seval_session_creates_session_and_messages() {
        let db = Database::open_in_memory().unwrap();
        let session_id = import_seval_session_from_str(&db, sample_seval_json()).unwrap();

        // Session should exist.
        let sessions = db.list_sessions(None).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, session_id);
        assert_eq!(sessions[0].name.as_deref(), Some("Security Audit"));
        assert_eq!(sessions[0].project_path, "imported/hash42");

        // Should have 3 messages (user + assistant + compression-as-system).
        let messages = db.get_session_messages(&session_id).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "Check this server for vulnerabilities");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].token_input, Some(150));

        // Assistant message should have a tool call.
        let tool_calls = db.get_message_tool_calls(messages[1].id).unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].name, "shell");
        assert!(tool_calls[0].result_text.is_some());
    }

    #[test]
    fn export_seval_session_produces_valid_json() {
        let db = Database::open_in_memory().unwrap();
        let session = db.create_session("/my/project", Some("claude-sonnet")).unwrap();
        db.save_message(&session.id, "user", "hello", None, None)
            .unwrap();
        let msg_id = db
            .save_message(&session.id, "assistant", "hi there", Some(100), Some(20))
            .unwrap();
        db.save_tool_call(msg_id, "shell", r#"{"command":"ls"}"#, Some("file.txt"), "success", None)
            .unwrap();

        let json_str = export_seval_session_to_string(&db, &session.id).unwrap();
        let record: SevalConversationRecord = serde_json::from_str(&json_str).unwrap();

        assert_eq!(record.session_id, session.id);
        assert_eq!(record.messages.len(), 2);

        // Check user message.
        match &record.messages[0] {
            SevalMessageRecord::User { content, .. } => assert_eq!(content, "hello"),
            other => panic!("Expected User, got {:?}", other),
        }

        // Check assistant message with tool call.
        match &record.messages[1] {
            SevalMessageRecord::Assistant {
                content,
                tool_calls,
                tokens,
                model,
                ..
            } => {
                assert_eq!(content, "hi there");
                let calls = tool_calls.as_ref().unwrap();
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].name, "shell");
                let tok = tokens.as_ref().unwrap();
                assert_eq!(tok.input_tokens, Some(100));
                assert_eq!(tok.output_tokens, Some(20));
                assert_eq!(model.as_deref(), Some("claude-sonnet"));
            }
            other => panic!("Expected Assistant, got {:?}", other),
        }
    }

    #[test]
    fn round_trip_preserves_message_content_and_order() {
        let db = Database::open_in_memory().unwrap();

        // Import.
        let session_id = import_seval_session_from_str(&db, sample_seval_json()).unwrap();

        // Export.
        let json_str = export_seval_session_to_string(&db, &session_id).unwrap();
        let exported: SevalConversationRecord = serde_json::from_str(&json_str).unwrap();

        // Verify content and order.
        assert_eq!(exported.messages.len(), 3);

        // Message 0: user.
        match &exported.messages[0] {
            SevalMessageRecord::User { content, .. } => {
                assert_eq!(content, "Check this server for vulnerabilities");
            }
            other => panic!("Expected User, got {:?}", other),
        }

        // Message 1: assistant with tool call content preserved.
        match &exported.messages[1] {
            SevalMessageRecord::Assistant {
                content,
                tool_calls,
                ..
            } => {
                assert_eq!(content, "I'll scan the server now.");
                let calls = tool_calls.as_ref().unwrap();
                assert_eq!(calls[0].name, "shell");
            }
            other => panic!("Expected Assistant, got {:?}", other),
        }

        // Message 2: compression stored as system, exported as user.
        match &exported.messages[2] {
            SevalMessageRecord::User { content, .. } => {
                assert!(content.contains("Compression"));
            }
            other => panic!("Expected User (from system), got {:?}", other),
        }

        // Name preserved.
        assert_eq!(exported.name, Some("Security Audit".to_string()));
    }

    #[test]
    fn import_handles_compression_message() {
        let json = r#"{
            "sessionId": "s1",
            "projectHash": "h1",
            "startTime": "2025-01-01T00:00:00Z",
            "lastUpdated": "2025-01-01T01:00:00Z",
            "messages": [
                {
                    "type": "compression",
                    "id": "c1",
                    "timestamp": "2025-01-01T00:30:00Z",
                    "content": "Context compressed",
                    "summary": "Earlier conversation about ports",
                    "tokensBefore": 3000,
                    "tokensAfter": 300
                }
            ]
        }"#;

        let db = Database::open_in_memory().unwrap();
        let session_id = import_seval_session_from_str(&db, json).unwrap();

        let messages = db.get_session_messages(&session_id).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "system");
        assert!(messages[0].content.contains("Earlier conversation about ports"));
    }

    #[test]
    fn imported_sessions_appear_in_list() {
        let db = Database::open_in_memory().unwrap();

        // Create a regular session.
        db.create_session("/my/project", None).unwrap();

        // Import a SEVAL-CLI session.
        import_seval_session_from_str(&db, sample_seval_json()).unwrap();

        // Both should appear in the unfiltered list.
        let sessions = db.list_sessions(None).unwrap();
        assert_eq!(sessions.len(), 2);
    }
}
