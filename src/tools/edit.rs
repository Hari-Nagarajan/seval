//! Edit tool -- search-and-replace editing of files.
//!
//! Implements the Rig `Tool` trait for making surgical edits to files
//! by replacing exact text matches, with uniqueness validation.

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;

/// Arguments for the edit tool, deserialized from AI-provided JSON.
#[derive(Debug, Deserialize)]
pub struct EditArgs {
    /// Path to the file to edit.
    pub path: String,
    /// The exact text to find and replace.
    pub old_text: String,
    /// The replacement text.
    pub new_text: String,
}

/// Errors that can occur during edit tool execution.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    /// File not found or inaccessible.
    #[error("Cannot read file: {0}")]
    FileError(String),
    /// The `old_text` was not found in the file.
    #[error("old_text not found in {0}. Ensure the text matches exactly including whitespace and indentation.")]
    NotFound(String),
    /// The `old_text` was found multiple times.
    #[error("old_text found multiple times in {0}. Provide more surrounding context to make the match unique.")]
    MultipleMatches(String),
    /// I/O error during file operations.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Edit tool for making surgical search-and-replace edits.
///
/// Finds an exact match of `old_text` in the file and replaces it with
/// `new_text`. Validates that the match is unique -- if found zero or
/// multiple times, returns a clear error message.
pub struct EditTool;

impl Tool for EditTool {
    const NAME: &'static str = "edit";

    type Error = EditError;
    type Args = EditArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "edit".to_string(),
            description: "Edit a file by replacing exact text. Finds old_text in the file and \
                          replaces it with new_text. The old_text must match exactly once -- \
                          if not found or found multiple times, returns an error."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path to the file to edit"
                    },
                    "old_text": {
                        "type": "string",
                        "description": "The exact text to find (must match exactly, including whitespace)"
                    },
                    "new_text": {
                        "type": "string",
                        "description": "The replacement text"
                    }
                },
                "required": ["path", "old_text", "new_text"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let content = tokio::fs::read_to_string(&args.path)
            .await
            .map_err(|e| EditError::FileError(format!("{}: {e}", args.path)))?;

        // Find first occurrence
        let first_pos = content
            .find(&args.old_text)
            .ok_or_else(|| EditError::NotFound(args.path.clone()))?;

        // Check for second occurrence (uniqueness)
        let after_first = first_pos + args.old_text.len();
        if content[after_first..].contains(&args.old_text) {
            return Err(EditError::MultipleMatches(args.path.clone()));
        }

        // Replace (exactly one occurrence)
        let new_content = content.replacen(&args.old_text, &args.new_text, 1);
        let old_len = args.old_text.len();
        let new_len = args.new_text.len();

        tokio::fs::write(&args.path, &new_content).await?;

        Ok(format!(
            "Edited {}: replaced {old_len} bytes with {new_len} bytes",
            args.path
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    fn temp_file_with(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[tokio::test]
    async fn test_edit_replaces_text() {
        let f = temp_file_with("hello world");
        let path = f.path().to_str().unwrap().to_string();
        let result = EditTool
            .call(EditArgs {
                path: path.clone(),
                old_text: "world".to_string(),
                new_text: "rust".to_string(),
            })
            .await
            .unwrap();
        assert!(result.contains("replaced 5 bytes with 4 bytes"), "result: {result}");
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "hello rust");
    }

    #[tokio::test]
    async fn test_edit_not_found() {
        let f = temp_file_with("hello world");
        let result = EditTool
            .call(EditArgs {
                path: f.path().to_str().unwrap().to_string(),
                old_text: "xyz_not_here".to_string(),
                new_text: "replacement".to_string(),
            })
            .await;
        assert!(result.is_err(), "should fail when text not found");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"), "error should mention not found: {err}");
        assert!(
            err.contains("exactly"),
            "error should mention exact match: {err}"
        );
    }

    #[tokio::test]
    async fn test_edit_multiple_matches() {
        let f = temp_file_with("foo bar foo baz foo");
        let result = EditTool
            .call(EditArgs {
                path: f.path().to_str().unwrap().to_string(),
                old_text: "foo".to_string(),
                new_text: "qux".to_string(),
            })
            .await;
        assert!(result.is_err(), "should fail for multiple matches");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("multiple times"),
            "error should mention multiple: {err}"
        );
    }

    #[tokio::test]
    async fn test_edit_nonexistent_file() {
        let result = EditTool
            .call(EditArgs {
                path: "/tmp/definitely_does_not_exist_edit_12345.txt".to_string(),
                old_text: "old".to_string(),
                new_text: "new".to_string(),
            })
            .await;
        assert!(result.is_err(), "should fail for nonexistent file");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Cannot read file"),
            "should mention file error: {err}"
        );
    }

    #[tokio::test]
    async fn test_edit_preserves_rest() {
        let f = temp_file_with("aaa REPLACE_ME bbb");
        let path = f.path().to_str().unwrap().to_string();
        EditTool
            .call(EditArgs {
                path: path.clone(),
                old_text: "REPLACE_ME".to_string(),
                new_text: "DONE".to_string(),
            })
            .await
            .unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "aaa DONE bbb");
    }

    #[tokio::test]
    async fn test_edit_definition() {
        let def = EditTool.definition(String::new()).await;
        assert_eq!(def.name, "edit");
        let required = def.parameters["required"]
            .as_array()
            .expect("required should be array");
        let required_names: Vec<&str> = required
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(required_names.contains(&"path"), "path should be required");
        assert!(required_names.contains(&"old_text"), "old_text should be required");
        assert!(required_names.contains(&"new_text"), "new_text should be required");
    }
}
