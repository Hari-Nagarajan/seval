//! Write tool -- writes content to files.
//!
//! Implements the Rig `Tool` trait for writing file contents with
//! parent directory safety checks.

use std::path::Path;

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;

/// Arguments for the write tool, deserialized from AI-provided JSON.
#[derive(Debug, Deserialize)]
pub struct WriteArgs {
    /// Path to the file to write.
    pub path: String,
    /// Content to write to the file.
    pub content: String,
}

/// Errors that can occur during write tool execution.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// Parent directory does not exist.
    #[error("Parent directory does not exist: {0}. Create it first with the shell tool.")]
    NoParentDir(String),
    /// I/O error during file writing.
    #[error("I/O error writing {path}: {source}")]
    IoError {
        path: String,
        source: std::io::Error,
    },
}

/// Write tool for creating and overwriting files.
///
/// Writes content to the specified path. Fails if the parent directory
/// does not exist (no auto-mkdir) to prevent accidental deep path creation.
pub struct WriteTool;

impl Tool for WriteTool {
    const NAME: &'static str = "write";

    type Error = WriteError;
    type Args = WriteArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "write".to_string(),
            description: "Write content to a file. Creates the file if it doesn't exist, \
                          or overwrites it if it does. Fails if the parent directory does not exist."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path to the file to write"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file"
                    }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = Path::new(&args.path);

        // Check parent directory exists
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && tokio::fs::metadata(parent).await.is_err()
        {
            return Err(WriteError::NoParentDir(parent.display().to_string()));
        }

        let byte_count = args.content.len();
        tokio::fs::write(&args.path, &args.content)
            .await
            .map_err(|e| WriteError::IoError {
                path: args.path.clone(),
                source: e,
            })?;

        Ok(format!("Wrote {byte_count} bytes to {}", args.path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_write_new_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        let result = WriteTool
            .call(WriteArgs {
                path: path.to_str().unwrap().to_string(),
                content: "hello world".to_string(),
            })
            .await
            .unwrap();
        assert!(result.contains("11 bytes"), "should report byte count: {result}");
        assert!(result.contains("test.txt"), "should report path: {result}");
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_write_fails_no_parent_dir() {
        let result = WriteTool
            .call(WriteArgs {
                path: "/tmp/nonexistent_parent_dir_12345/file.txt".to_string(),
                content: "test".to_string(),
            })
            .await;
        assert!(result.is_err(), "should fail when parent doesn't exist");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Parent directory does not exist"),
            "should mention parent dir: {err}"
        );
    }

    #[tokio::test]
    async fn test_write_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("overwrite.txt");
        std::fs::write(&path, "old content").unwrap();

        let result = WriteTool
            .call(WriteArgs {
                path: path.to_str().unwrap().to_string(),
                content: "new content".to_string(),
            })
            .await
            .unwrap();
        assert!(result.contains("11 bytes"), "should report byte count: {result}");
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "new content");
    }

    #[tokio::test]
    async fn test_write_definition() {
        let def = WriteTool.definition(String::new()).await;
        assert_eq!(def.name, "write");
        let required = def.parameters["required"]
            .as_array()
            .expect("required should be array");
        assert!(
            required.iter().any(|v| v.as_str() == Some("path")),
            "path should be required"
        );
        assert!(
            required.iter().any(|v| v.as_str() == Some("content")),
            "content should be required"
        );
    }
}
