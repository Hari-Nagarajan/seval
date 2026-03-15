//! Grep tool -- searches file contents via regex with gitignore awareness.
//!
//! Uses the `ignore` crate for gitignore-aware file walking and the `regex`
//! crate for pattern matching. Runs in `spawn_blocking` to avoid blocking
//! the tokio runtime.

use std::path::PathBuf;

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;

use crate::tools::truncate_output;

/// Maximum output size in bytes before truncation.
const MAX_OUTPUT_BYTES: usize = 102_400;

/// Arguments for the grep tool.
#[derive(Debug, Deserialize)]
pub struct GrepArgs {
    /// Regex pattern to search for.
    pub pattern: String,
    /// Directory or file path to search in.
    pub path: String,
    /// Optional glob pattern to filter files (e.g., "*.rs").
    pub file_glob: Option<String>,
}

/// Errors that can occur during grep execution.
#[derive(Debug, thiserror::Error)]
pub enum GrepError {
    /// Invalid regex pattern.
    #[error("Invalid regex pattern: {0}")]
    InvalidRegex(String),
    /// Invalid file glob pattern.
    #[error("Invalid file glob: {0}")]
    InvalidGlob(String),
    /// The search path does not exist.
    #[error("Path does not exist: {0}")]
    PathNotFound(String),
    /// Internal error during search.
    #[error("Search error: {0}")]
    SearchError(String),
}

/// Grep tool for searching file contents with regex patterns.
///
/// Walks the directory tree respecting `.gitignore` rules and matches
/// lines against the provided regex pattern. Optionally filters files
/// by a glob pattern (e.g., `*.rs`).
pub struct GrepTool {
    /// Working directory used to resolve relative paths.
    working_dir: PathBuf,
}

impl GrepTool {
    /// Create a new `GrepTool` with the given working directory.
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

impl Tool for GrepTool {
    const NAME: &'static str = "grep";

    type Error = GrepError;
    type Args = GrepArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "grep".to_string(),
            description: "Search file contents using a regex pattern. \
                          Returns matching lines in file:line:content format. \
                          Respects .gitignore. Optionally filter by file glob pattern."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Regular expression pattern to search for"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory or file path to search in"
                    },
                    "file_glob": {
                        "type": "string",
                        "description": "Optional glob pattern to filter files (e.g., \"*.rs\", \"*.toml\")"
                    }
                },
                "required": ["pattern", "path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let search_path = if std::path::Path::new(&args.path).is_absolute() {
            PathBuf::from(&args.path)
        } else {
            self.working_dir.join(&args.path)
        };

        if !search_path.exists() {
            return Err(GrepError::PathNotFound(args.path));
        }

        let pattern = args.pattern;
        let file_glob = args.file_glob;

        let result = tokio::task::spawn_blocking(move || {
            grep_search(&pattern, &search_path, file_glob.as_deref())
        })
        .await
        .map_err(|e| GrepError::SearchError(e.to_string()))??;

        Ok(result)
    }
}

/// Perform a synchronous grep search (called inside `spawn_blocking`).
fn grep_search(
    pattern: &str,
    path: &std::path::Path,
    file_glob: Option<&str>,
) -> Result<String, GrepError> {
    let re = regex::Regex::new(pattern).map_err(|e| GrepError::InvalidRegex(e.to_string()))?;

    let mut builder = ignore::WalkBuilder::new(path);
    builder.hidden(false).git_ignore(true);

    if let Some(glob_pattern) = file_glob {
        let mut types_builder = ignore::types::TypesBuilder::new();
        types_builder
            .add("custom", glob_pattern)
            .map_err(|e| GrepError::InvalidGlob(e.to_string()))?;
        types_builder.select("custom");
        let types = types_builder
            .build()
            .map_err(|e| GrepError::InvalidGlob(e.to_string()))?;
        builder.types(types);
    }

    let mut results = Vec::new();
    let mut total_bytes = 0usize;

    for entry in builder.build().flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(entry.path()) else {
            continue; // Skip binary/unreadable files
        };
        // Build relative path for display
        let display_path = entry.path().strip_prefix(path).unwrap_or(entry.path());

        for (i, line) in content.lines().enumerate() {
            if re.is_match(line) {
                let result_line = format!("{}:{}:{}", display_path.display(), i + 1, line);
                total_bytes += result_line.len() + 1; // +1 for newline
                results.push(result_line);

                // Early exit if we've exceeded truncation limit
                if total_bytes > MAX_OUTPUT_BYTES * 2 {
                    break;
                }
            }
        }
        if total_bytes > MAX_OUTPUT_BYTES * 2 {
            break;
        }
    }

    let output = results.join("\n");
    Ok(truncate_output(&output, MAX_OUTPUT_BYTES))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("hello.rs"),
            "fn main() {\n    println!(\"hello world\");\n}\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            "pub fn greet() {\n    println!(\"hi\");\n}\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("notes.txt"),
            "some notes\nhello from notes\n",
        )
        .unwrap();
        dir
    }

    #[tokio::test]
    async fn test_grep_simple_match() {
        let dir = setup_test_dir();
        let tool = GrepTool::new(dir.path().to_path_buf());
        let result = tool
            .call(GrepArgs {
                pattern: "hello".to_string(),
                path: dir.path().to_string_lossy().to_string(),
                file_glob: None,
            })
            .await
            .unwrap();
        assert!(result.contains("hello"), "should find 'hello' matches");
        // Should have file:line:content format
        assert!(result.contains(':'), "should have colon-separated format");
    }

    #[tokio::test]
    async fn test_grep_regex_pattern() {
        let dir = setup_test_dir();
        let tool = GrepTool::new(dir.path().to_path_buf());
        let result = tool
            .call(GrepArgs {
                pattern: r"fn\s+\w+".to_string(),
                path: dir.path().to_string_lossy().to_string(),
                file_glob: None,
            })
            .await
            .unwrap();
        assert!(result.contains("fn main"), "should find fn main");
        assert!(result.contains("fn greet"), "should find fn greet");
    }

    #[tokio::test]
    async fn test_grep_invalid_regex() {
        let dir = setup_test_dir();
        let tool = GrepTool::new(dir.path().to_path_buf());
        let result = tool
            .call(GrepArgs {
                pattern: "[invalid".to_string(),
                path: dir.path().to_string_lossy().to_string(),
                file_glob: None,
            })
            .await;
        assert!(result.is_err(), "invalid regex should error");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Invalid regex"),
            "should mention invalid regex: {err}"
        );
    }

    #[tokio::test]
    async fn test_grep_file_glob_filter() {
        let dir = setup_test_dir();
        let tool = GrepTool::new(dir.path().to_path_buf());
        let result = tool
            .call(GrepArgs {
                pattern: "hello".to_string(),
                path: dir.path().to_string_lossy().to_string(),
                file_glob: Some("*.rs".to_string()),
            })
            .await
            .unwrap();
        // Should find hello in .rs files but NOT in .txt files
        assert!(result.contains("hello"), "should find hello in .rs files");
        assert!(
            !result.contains("notes.txt"),
            "should not include .txt files when filtering *.rs"
        );
    }

    #[tokio::test]
    async fn test_grep_path_not_found() {
        let tool = GrepTool::new(PathBuf::from("/tmp"));
        let result = tool
            .call(GrepArgs {
                pattern: "test".to_string(),
                path: "/nonexistent/path/that/does/not/exist".to_string(),
                file_glob: None,
            })
            .await;
        assert!(result.is_err(), "nonexistent path should error");
    }

    #[tokio::test]
    async fn test_grep_result_format() {
        let dir = setup_test_dir();
        let tool = GrepTool::new(dir.path().to_path_buf());
        let result = tool
            .call(GrepArgs {
                pattern: "fn main".to_string(),
                path: dir.path().to_string_lossy().to_string(),
                file_glob: None,
            })
            .await
            .unwrap();
        // Format should be "file:line:content"
        let line = result.lines().next().unwrap();
        let parts: Vec<&str> = line.splitn(3, ':').collect();
        assert_eq!(parts.len(), 3, "should have file:line:content format");
        assert!(
            parts[1].parse::<usize>().is_ok(),
            "second part should be line number"
        );
    }

    #[tokio::test]
    async fn test_grep_definition() {
        let tool = GrepTool::new(PathBuf::from("/tmp"));
        let def = tool.definition(String::new()).await;
        assert_eq!(def.name, "grep");
        let required = def.parameters["required"]
            .as_array()
            .expect("required array");
        let required_names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(
            required_names.contains(&"pattern"),
            "pattern should be required"
        );
        assert!(required_names.contains(&"path"), "path should be required");
    }
}
