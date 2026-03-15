//! Ls tool -- lists directory contents with metadata.
//!
//! Provides file type, size, and modification time for each entry.
//! Sorts directories first (alphabetically), then files (alphabetically).

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;

use crate::tools::truncate_output;

/// Maximum output size in bytes before truncation.
const MAX_OUTPUT_BYTES: usize = 102_400;

/// Arguments for the ls tool.
#[derive(Debug, Deserialize)]
pub struct LsArgs {
    /// Directory path to list.
    pub path: String,
}

/// Errors that can occur during directory listing.
#[derive(Debug, thiserror::Error)]
pub enum LsError {
    /// The path does not exist.
    #[error("Path does not exist: {0}")]
    PathNotFound(String),
    /// The path is not a directory.
    #[error("Not a directory: {0}")]
    NotADirectory(String),
    /// I/O error reading the directory.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Ls tool for listing directory contents with metadata.
///
/// Returns entries with type indicator (d/f/l), human-readable size,
/// modification time, and name. Directories are listed first, then files,
/// both sorted alphabetically.
pub struct LsTool;

impl Tool for LsTool {
    const NAME: &'static str = "ls";

    type Error = LsError;
    type Args = LsArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "ls".to_string(),
            description: "List directory contents with file type, size, and modification time. \
                          Directories are listed first, sorted alphabetically."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path to list"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = std::path::PathBuf::from(&args.path);

        if !path.exists() {
            return Err(LsError::PathNotFound(args.path));
        }
        if !path.is_dir() {
            return Err(LsError::NotADirectory(args.path));
        }

        let mut dirs: Vec<String> = Vec::new();
        let mut files: Vec<String> = Vec::new();

        let mut read_dir = tokio::fs::read_dir(&path).await?;

        while let Some(entry) = read_dir.next_entry().await? {
            let metadata = entry.metadata().await?;
            let file_type = metadata.file_type();
            let name = entry.file_name().to_string_lossy().to_string();

            let (type_indicator, display_name) = if file_type.is_dir() {
                ("d", format!("{name}/"))
            } else if file_type.is_symlink() {
                ("l", name.clone())
            } else {
                ("f", name.clone())
            };

            let size = if file_type.is_file() {
                format_size(metadata.len())
            } else {
                "-".to_string()
            };

            let modified = metadata.modified().ok().map_or_else(
                || "-".to_string(),
                |t| {
                    let datetime: chrono::DateTime<chrono::Local> = t.into();
                    datetime.format("%Y-%m-%d %H:%M").to_string()
                },
            );

            let line = format!("{type_indicator} {size:>8} {modified} {display_name}");

            if file_type.is_dir() {
                dirs.push(line);
            } else {
                files.push(line);
            }
        }

        dirs.sort();
        files.sort();

        let mut all = dirs;
        all.extend(files);

        let output = all.join("\n");
        Ok(truncate_output(&output, MAX_OUTPUT_BYTES))
    }
}

/// Format a byte count as a human-readable size string.
#[allow(clippy::cast_precision_loss)]
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("file_a.txt"), "hello world").unwrap();
        fs::write(dir.path().join("file_b.rs"), "fn main() {}").unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();
        fs::create_dir(dir.path().join("another_dir")).unwrap();
        dir
    }

    #[tokio::test]
    async fn test_ls_directory() {
        let dir = setup_test_dir();
        let tool = LsTool;
        let result = tool
            .call(LsArgs {
                path: dir.path().to_string_lossy().to_string(),
            })
            .await
            .unwrap();
        assert!(result.contains("file_a.txt"), "should list file_a.txt");
        assert!(result.contains("file_b.rs"), "should list file_b.rs");
        assert!(
            result.contains("subdir/"),
            "should list subdir with trailing /"
        );
        assert!(
            result.contains("another_dir/"),
            "should list another_dir with trailing /"
        );
    }

    #[tokio::test]
    async fn test_ls_nonexistent() {
        let tool = LsTool;
        let result = tool
            .call(LsArgs {
                path: "/nonexistent/path/that/does/not/exist".to_string(),
            })
            .await;
        assert!(result.is_err(), "nonexistent path should error");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("does not exist"),
            "should mention path not found: {err}"
        );
    }

    #[tokio::test]
    async fn test_ls_file_not_dir() {
        let dir = setup_test_dir();
        let file_path = dir.path().join("file_a.txt");
        let tool = LsTool;
        let result = tool
            .call(LsArgs {
                path: file_path.to_string_lossy().to_string(),
            })
            .await;
        assert!(result.is_err(), "file path should error");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Not a directory"),
            "should mention not a directory: {err}"
        );
    }

    #[tokio::test]
    async fn test_ls_metadata() {
        let dir = setup_test_dir();
        let tool = LsTool;
        let result = tool
            .call(LsArgs {
                path: dir.path().to_string_lossy().to_string(),
            })
            .await
            .unwrap();
        // Files should have "f" type indicator
        assert!(result.contains("f "), "should have file type indicator");
        // Dirs should have "d" type indicator
        assert!(result.contains("d "), "should have dir type indicator");
        // Files should show size in bytes
        assert!(result.contains(" B "), "should show size for files");
    }

    #[tokio::test]
    async fn test_ls_dirs_first() {
        let dir = setup_test_dir();
        let tool = LsTool;
        let result = tool
            .call(LsArgs {
                path: dir.path().to_string_lossy().to_string(),
            })
            .await
            .unwrap();
        // Find first "d " and first "f " positions
        let first_dir = result.find("d ");
        let first_file = result.find("f ");
        assert!(
            first_dir.is_some() && first_file.is_some(),
            "should have both dirs and files"
        );
        assert!(
            first_dir.unwrap() < first_file.unwrap(),
            "directories should come before files"
        );
    }

    #[tokio::test]
    async fn test_ls_definition() {
        let tool = LsTool;
        let def = tool.definition(String::new()).await;
        assert_eq!(def.name, "ls");
        let required = def.parameters["required"]
            .as_array()
            .expect("required array");
        assert!(
            required.iter().any(|v| v.as_str() == Some("path")),
            "path should be required"
        );
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1_048_576), "1.0 MB");
        assert_eq!(format_size(1_073_741_824), "1.0 GB");
    }
}
