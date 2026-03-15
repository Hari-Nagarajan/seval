//! Glob tool -- discovers files by glob pattern matching.
//!
//! Uses the `globset` crate for pattern matching and the `ignore` crate
//! for gitignore-aware directory walking. Runs in `spawn_blocking` to
//! avoid blocking the tokio runtime.

use std::path::PathBuf;

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;

use crate::tools::truncate_output;

/// Maximum output size in bytes before truncation.
const MAX_OUTPUT_BYTES: usize = 102_400;

/// Arguments for the glob tool.
#[derive(Debug, Deserialize)]
pub struct GlobArgs {
    /// Glob pattern to match files against (e.g., "*.rs", "**/*.toml").
    pub pattern: String,
    /// Directory to search in.
    pub path: String,
}

/// Errors that can occur during glob execution.
#[derive(Debug, thiserror::Error)]
pub enum GlobError {
    /// Invalid glob pattern.
    #[error("Invalid glob pattern: {0}")]
    InvalidGlob(String),
    /// The search path does not exist.
    #[error("Path does not exist: {0}")]
    PathNotFound(String),
    /// Internal error during search.
    #[error("Search error: {0}")]
    SearchError(String),
}

/// Glob tool for discovering files by pattern.
///
/// Walks the directory tree respecting `.gitignore` rules and matches
/// file paths against the provided glob pattern.
pub struct GlobTool {
    /// Working directory used to resolve relative paths.
    working_dir: PathBuf,
}

impl GlobTool {
    /// Create a new `GlobTool` with the given working directory.
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

impl Tool for GlobTool {
    const NAME: &'static str = "glob";

    type Error = GlobError;
    type Args = GlobArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "glob".to_string(),
            description: "Find files matching a glob pattern. \
                          Returns sorted list of matching file paths. \
                          Respects .gitignore. Supports ** for recursive matching."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern to match files (e.g., \"*.rs\", \"**/*.toml\", \"src/**/*.rs\")"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in"
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
            return Err(GlobError::PathNotFound(args.path));
        }

        let pattern = args.pattern;

        let result = tokio::task::spawn_blocking(move || glob_search(&pattern, &search_path))
            .await
            .map_err(|e| GlobError::SearchError(e.to_string()))??;

        Ok(result)
    }
}

/// Perform a synchronous glob search (called inside `spawn_blocking`).
fn glob_search(pattern: &str, path: &std::path::Path) -> Result<String, GlobError> {
    let glob = globset::Glob::new(pattern)
        .map_err(|e| GlobError::InvalidGlob(e.to_string()))?
        .compile_matcher();

    let walker = ignore::WalkBuilder::new(path);
    let mut matches: Vec<String> = Vec::new();

    for entry in walker.build().flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let rel_path = entry.path().strip_prefix(path).unwrap_or(entry.path());

        // Match against the relative path and just the filename
        if glob.is_match(rel_path) || glob.is_match(rel_path.file_name().unwrap_or_default()) {
            matches.push(rel_path.display().to_string());
        }
    }

    matches.sort();

    let output = matches.join("\n");
    Ok(truncate_output(&output, MAX_OUTPUT_BYTES))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("lib.rs"), "pub fn lib() {}").unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        let sub = dir.path().join("src");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("mod.rs"), "mod test;").unwrap();
        fs::write(sub.join("config.toml"), "key = \"val\"").unwrap();
        dir
    }

    #[tokio::test]
    async fn test_glob_rs_files() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(dir.path().to_path_buf());
        let result = tool
            .call(GlobArgs {
                pattern: "*.rs".to_string(),
                path: dir.path().to_string_lossy().to_string(),
            })
            .await
            .unwrap();
        assert!(result.contains("main.rs"), "should find main.rs");
        assert!(result.contains("lib.rs"), "should find lib.rs");
        assert!(
            !result.contains("Cargo.toml"),
            "should not match .toml files"
        );
    }

    #[tokio::test]
    async fn test_glob_nested_pattern() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(dir.path().to_path_buf());
        let result = tool
            .call(GlobArgs {
                pattern: "**/*.toml".to_string(),
                path: dir.path().to_string_lossy().to_string(),
            })
            .await
            .unwrap();
        assert!(result.contains("Cargo.toml"), "should find Cargo.toml");
        assert!(
            result.contains("config.toml"),
            "should find nested config.toml"
        );
    }

    #[tokio::test]
    async fn test_glob_invalid_pattern() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(dir.path().to_path_buf());
        let result = tool
            .call(GlobArgs {
                pattern: "[invalid".to_string(),
                path: dir.path().to_string_lossy().to_string(),
            })
            .await;
        assert!(result.is_err(), "invalid glob should error");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Invalid glob"),
            "should mention invalid glob: {err}"
        );
    }

    #[tokio::test]
    async fn test_glob_sorted_output() {
        let dir = setup_test_dir();
        let tool = GlobTool::new(dir.path().to_path_buf());
        let result = tool
            .call(GlobArgs {
                pattern: "*.rs".to_string(),
                path: dir.path().to_string_lossy().to_string(),
            })
            .await
            .unwrap();
        let lines: Vec<&str> = result.lines().collect();
        let mut sorted = lines.clone();
        sorted.sort_unstable();
        assert_eq!(lines, sorted, "output should be sorted");
    }

    #[tokio::test]
    async fn test_glob_path_not_found() {
        let tool = GlobTool::new(PathBuf::from("/tmp"));
        let result = tool
            .call(GlobArgs {
                pattern: "*.rs".to_string(),
                path: "/nonexistent/path/that/does/not/exist".to_string(),
            })
            .await;
        assert!(result.is_err(), "nonexistent path should error");
    }

    #[tokio::test]
    async fn test_glob_definition() {
        let tool = GlobTool::new(PathBuf::from("/tmp"));
        let def = tool.definition(String::new()).await;
        assert_eq!(def.name, "glob");
        let required = def.parameters["required"]
            .as_array()
            .expect("required array");
        let required_names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(required_names.contains(&"pattern"), "pattern required");
        assert!(required_names.contains(&"path"), "path required");
    }
}
