//! Read tool -- reads files with line numbers.
//!
//! Implements the Rig `Tool` trait for reading file contents with
//! cat -n style line numbering, optional offset/limit, and output truncation.

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;

use std::fmt::Write;

use crate::tools::truncate_output;

/// Maximum output size in bytes before truncation (100KB).
const MAX_OUTPUT_BYTES: usize = 102_400;

/// Arguments for the read tool, deserialized from AI-provided JSON.
#[derive(Debug, Deserialize)]
pub struct ReadArgs {
    /// Path to the file to read.
    pub path: String,
    /// Optional line offset (0-based: skip this many lines from the start).
    #[serde(default)]
    pub offset: Option<usize>,
    /// Optional limit on number of lines to return.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Errors that can occur during read tool execution.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// File not found or inaccessible.
    #[error("Cannot read file: {0}")]
    FileError(String),
    /// I/O error during file reading.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Read tool for reading file contents with line numbers.
///
/// Returns file content with cat -n style line numbering, supporting
/// optional offset and limit for reading specific line ranges.
pub struct ReadTool;

impl Tool for ReadTool {
    const NAME: &'static str = "read";

    type Error = ReadError;
    type Args = ReadArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "read".to_string(),
            description: "Read a file and return its contents with line numbers (cat -n style). \
                          Supports optional offset and limit to read specific line ranges."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path to the file to read"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Number of lines to skip from the start (0-based)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of lines to return"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let content = tokio::fs::read_to_string(&args.path)
            .await
            .map_err(|e| ReadError::FileError(format!("{}: {e}", args.path)))?;

        let all_lines: Vec<&str> = content.lines().collect();
        let total_lines = all_lines.len();

        let offset = args.offset.unwrap_or(0);
        let lines: Vec<&str> = all_lines
            .into_iter()
            .skip(offset)
            .take(args.limit.unwrap_or(usize::MAX))
            .collect();

        if lines.is_empty() {
            return Ok(format!(
                "File has {total_lines} lines, nothing in requested range."
            ));
        }

        // Calculate line number width based on the highest line number we'll display
        let max_line_num = offset + lines.len();
        let width = max_line_num.to_string().len().max(4);

        let mut output = String::new();
        for (i, line) in lines.iter().enumerate() {
            let line_num = offset + i + 1;
            let _ = writeln!(output, "{line_num:>width$}\t{line}");
        }

        Ok(truncate_output(&output, MAX_OUTPUT_BYTES))
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
    async fn test_read_with_line_numbers() {
        let f = temp_file_with("first line\nsecond line\nthird line\n");
        let result = ReadTool
            .call(ReadArgs {
                path: f.path().to_str().unwrap().to_string(),
                offset: None,
                limit: None,
            })
            .await
            .unwrap();
        assert!(
            result.contains("1\tfirst line"),
            "should have line 1: {result}"
        );
        assert!(
            result.contains("2\tsecond line"),
            "should have line 2: {result}"
        );
        assert!(
            result.contains("3\tthird line"),
            "should have line 3: {result}"
        );
    }

    #[tokio::test]
    async fn test_read_with_offset_and_limit() {
        let f = temp_file_with("line1\nline2\nline3\nline4\nline5\n");
        let result = ReadTool
            .call(ReadArgs {
                path: f.path().to_str().unwrap().to_string(),
                offset: Some(2),
                limit: Some(1),
            })
            .await
            .unwrap();
        // Should return only line 3 (offset 2 skips lines 1,2; limit 1 returns one line)
        assert!(result.contains("3\tline3"), "should have line 3: {result}");
        assert!(
            !result.contains("line2"),
            "should not have line 2: {result}"
        );
        assert!(
            !result.contains("line4"),
            "should not have line 4: {result}"
        );
    }

    #[tokio::test]
    async fn test_read_nonexistent_file() {
        let result = ReadTool
            .call(ReadArgs {
                path: "/tmp/definitely_does_not_exist_12345.txt".to_string(),
                offset: None,
                limit: None,
            })
            .await;
        assert!(result.is_err(), "should fail for nonexistent file");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Cannot read file"),
            "error should be clear: {err}"
        );
    }

    #[tokio::test]
    async fn test_line_number_width_adjusts() {
        // Create a file with 100+ lines to force wider line numbers
        let content: String = (1..=150).fold(String::new(), |mut acc, i| {
            use std::fmt::Write;
            let _ = writeln!(acc, "line {i}");
            acc
        });
        let f = temp_file_with(&content);
        let result = ReadTool
            .call(ReadArgs {
                path: f.path().to_str().unwrap().to_string(),
                offset: Some(148),
                limit: Some(2),
            })
            .await
            .unwrap();
        // Lines 149 and 150 should have width for 3-digit numbers
        assert!(result.contains("149\t"), "should have line 149: {result}");
        assert!(result.contains("150\t"), "should have line 150: {result}");
    }

    #[tokio::test]
    async fn test_read_definition() {
        let def = ReadTool.definition(String::new()).await;
        assert_eq!(def.name, "read");
        let required = def.parameters["required"]
            .as_array()
            .expect("required should be array");
        assert!(
            required.iter().any(|v| v.as_str() == Some("path")),
            "path should be required"
        );
    }
}
