//! Save-memory tool for AI-driven persistent memory.
//!
//! Implements the Rig `Tool` trait so the AI can save key findings,
//! discovered credentials, configurations, and architectural decisions
//! to the project's persistent memory store.

use std::sync::Arc;

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;

use super::db::Database;

/// Arguments for the `save_memory` tool.
#[derive(Debug, Deserialize)]
pub struct SaveMemoryArgs {
    /// The content to save to memory. Should be concise but contextual.
    pub content: String,
}

/// Errors from `save_memory` tool execution.
#[derive(Debug, thiserror::Error)]
pub enum SaveMemoryError {
    /// Database operation failed.
    #[error("Failed to save memory: {0}")]
    DbError(String),
}

/// Tool that allows the AI to save important findings to persistent memory.
///
/// Memories are stored per-project in the `SQLite` database and loaded on
/// subsequent session starts to provide continuity across conversations.
pub struct SaveMemoryTool {
    /// Shared database handle.
    db: Arc<Database>,
    /// Project path for scoping memories.
    project_path: String,
}

impl SaveMemoryTool {
    /// Create a new `SaveMemoryTool`.
    pub fn new(db: Arc<Database>, project_path: String) -> Self {
        Self { db, project_path }
    }
}

impl Tool for SaveMemoryTool {
    const NAME: &'static str = "save_memory";

    type Error = SaveMemoryError;
    type Args = SaveMemoryArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "save_memory".to_string(),
            description: "Save an important finding to persistent project memory. \
                          Use this to remember key discoveries across sessions: \
                          credentials found, vulnerability details, architectural decisions, \
                          important configurations, service endpoints, or any critical \
                          technical details that would be valuable in future conversations."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The finding or detail to save. Be concise but include enough context to be useful later."
                    }
                },
                "required": ["content"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = Arc::clone(&self.db);
        let project_path = self.project_path.clone();
        let content = args.content.clone();

        tokio::task::spawn_blocking(move || {
            db.save_memory(&project_path, &content, "auto")
                .map_err(|e| SaveMemoryError::DbError(e.to_string()))
        })
        .await
        .map_err(|e| SaveMemoryError::DbError(e.to_string()))??;

        Ok(format!("Memory saved: {}", args.content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn save_memory_tool_call_writes_to_db_and_returns_confirmation() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let tool = SaveMemoryTool::new(Arc::clone(&db), "/tmp/project".to_string());

        let args = SaveMemoryArgs {
            content: "SSH on port 2222 with key auth".to_string(),
        };
        let result = tool.call(args).await.unwrap();

        assert_eq!(result, "Memory saved: SSH on port 2222 with key auth");

        // Verify it's actually in the DB.
        let memories = db.get_memories("/tmp/project").unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].content, "SSH on port 2222 with key auth");
        assert_eq!(memories[0].source, "auto");
    }
}
