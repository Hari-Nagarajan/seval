//! Search-memory tool for AI-driven memory retrieval.
//!
//! Implements the Rig `Tool` trait so the AI can search previously saved
//! findings using full-text search instead of relying on all memories
//! being injected into the system prompt.

use std::fmt::Write;
use std::sync::Arc;

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;

use super::db::Database;

/// Arguments for the `search_memory` tool.
#[derive(Debug, Deserialize)]
pub struct SearchMemoryArgs {
    /// FTS5 search query (supports phrases, OR, prefix*).
    pub query: String,
    /// Maximum number of results to return (default: 10).
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Errors from `search_memory` tool execution.
#[derive(Debug, thiserror::Error)]
pub enum SearchMemoryError {
    #[error("Search failed: {0}")]
    SearchError(String),
}

/// Tool that searches project memory using full-text search.
pub struct SearchMemoryTool {
    db: Arc<Database>,
    project_path: String,
}

impl SearchMemoryTool {
    pub fn new(db: Arc<Database>, project_path: String) -> Self {
        Self { db, project_path }
    }
}

impl Tool for SearchMemoryTool {
    const NAME: &'static str = "search_memory";

    type Error = SearchMemoryError;
    type Args = SearchMemoryArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "search_memory".to_string(),
            description: "Search previously saved project memories using full-text search. \
                          Use this to find specific findings, credentials, network details, \
                          or any previously discovered information. Supports quoted phrases \
                          (\"exact match\"), OR for alternatives, and prefix* for wildcards."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query (e.g. \"WPA2 password\", \"192.168.*\", \"drone OR robot\")"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results to return (default: 10)"
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let db = Arc::clone(&self.db);
        let project_path = self.project_path.clone();
        let query = args.query.clone();
        let limit = args.limit.unwrap_or(10);

        let results = tokio::task::spawn_blocking(move || {
            db.search_memories(&project_path, &query, limit)
                .map_err(|e| SearchMemoryError::SearchError(e.to_string()))
        })
        .await
        .map_err(|e| SearchMemoryError::SearchError(e.to_string()))??;

        if results.is_empty() {
            return Ok(format!("No memories found matching: {}", args.query));
        }

        let mut output = format!("Found {} matching memories:\n\n", results.len());
        for (i, mem) in results.iter().enumerate() {
            let _ = writeln!(output, "{}. [{}] {}", i + 1, mem.created_at, mem.content);
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn search_memory_finds_matching_content() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.save_memory("/tmp/project", "Found SSH on port 2222", "auto")
            .unwrap();
        db.save_memory("/tmp/project", "WPA2 password is berenang", "auto")
            .unwrap();
        db.save_memory("/tmp/project", "Robot dog AP on channel 36", "auto")
            .unwrap();

        let tool = SearchMemoryTool::new(Arc::clone(&db), "/tmp/project".to_string());
        let result = tool
            .call(SearchMemoryArgs {
                query: "WPA2".to_string(),
                limit: None,
            })
            .await
            .unwrap();

        assert!(
            result.contains("berenang"),
            "should find WPA2 memory: {result}"
        );
        assert!(
            !result.contains("SSH"),
            "should not include SSH memory: {result}"
        );
    }

    #[tokio::test]
    async fn search_memory_returns_no_results_message() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.save_memory("/tmp/project", "Found SSH on port 2222", "auto")
            .unwrap();

        let tool = SearchMemoryTool::new(Arc::clone(&db), "/tmp/project".to_string());
        let result = tool
            .call(SearchMemoryArgs {
                query: "bluetooth".to_string(),
                limit: None,
            })
            .await
            .unwrap();

        assert!(result.contains("No memories found"), "got: {result}");
    }

    #[tokio::test]
    async fn search_memory_respects_project_path() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        db.save_memory("/project/a", "Secret key found", "auto")
            .unwrap();
        db.save_memory("/project/b", "Secret door found", "auto")
            .unwrap();

        let tool = SearchMemoryTool::new(Arc::clone(&db), "/project/a".to_string());
        let result = tool
            .call(SearchMemoryArgs {
                query: "secret".to_string(),
                limit: None,
            })
            .await
            .unwrap();

        assert!(
            result.contains("key"),
            "should find project A memory: {result}"
        );
        assert!(
            !result.contains("door"),
            "should not find project B memory: {result}"
        );
    }

    #[tokio::test]
    async fn search_memory_limits_results() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        for i in 0..20 {
            db.save_memory(
                "/tmp/project",
                &format!("Finding number {i} about ports"),
                "auto",
            )
            .unwrap();
        }

        let tool = SearchMemoryTool::new(Arc::clone(&db), "/tmp/project".to_string());
        let result = tool
            .call(SearchMemoryArgs {
                query: "ports".to_string(),
                limit: Some(5),
            })
            .await
            .unwrap();

        assert!(result.contains("Found 5"), "should limit to 5: {result}");
    }
}
