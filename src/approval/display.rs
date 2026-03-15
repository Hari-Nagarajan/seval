/// Format tool arguments for display in the approval UI.
///
/// Each tool type gets a human-readable format:
/// - shell: `$ {command}`
/// - write: `Write {path} ({N} lines)`
/// - edit: `Edit {path}` with old/new preview
/// - grep/glob: pattern display
/// - fallback: truncated JSON
pub fn format_tool_display(tool_name: &str, args_json: &str) -> String {
    let args: serde_json::Value =
        serde_json::from_str(args_json).unwrap_or(serde_json::Value::Null);

    match tool_name {
        "shell" => {
            let cmd = args
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("$ {cmd}")
        }
        "write" => {
            let path = args
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let lines = content.lines().count();
            format!("Write {path} ({lines} lines)")
        }
        "edit" => {
            let path = args
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let old = args
                .get("old_text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let new = args
                .get("new_text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let old_preview = truncate_str(old, 80);
            let new_preview = truncate_str(new, 80);
            format!("Edit {path}\n  - {old_preview}\n  + {new_preview}")
        }
        "grep" => {
            let pattern = args
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("Grep: {pattern}")
        }
        "glob" => {
            let pattern = args
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("Glob: {pattern}")
        }
        _ => {
            let truncated = truncate_str(args_json, 100);
            format!("{tool_name}: {truncated}")
        }
    }
}

/// Truncate a string to `max_len` characters, appending "..." if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        // Find a valid char boundary at or before max_len
        let mut end = max_len;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_shell() {
        let args = r#"{"command": "ls -la"}"#;
        assert_eq!(format_tool_display("shell", args), "$ ls -la");
    }

    #[test]
    fn test_format_write() {
        let args = r#"{"file_path": "/tmp/test.rs", "content": "line1\nline2\nline3"}"#;
        assert_eq!(
            format_tool_display("write", args),
            "Write /tmp/test.rs (3 lines)"
        );
    }

    #[test]
    fn test_format_edit() {
        let args = r#"{"file_path": "/tmp/test.rs", "old_text": "foo", "new_text": "bar"}"#;
        let result = format_tool_display("edit", args);
        assert!(result.starts_with("Edit /tmp/test.rs"));
        assert!(result.contains("- foo"));
        assert!(result.contains("+ bar"));
    }

    #[test]
    fn test_format_edit_truncation() {
        let long_text = "x".repeat(200);
        let args = format!(
            r#"{{"file_path": "/tmp/test.rs", "old_text": "{long_text}", "new_text": "short"}}"#
        );
        let result = format_tool_display("edit", &args);
        // The old text preview should be truncated
        assert!(result.contains("..."));
    }

    #[test]
    fn test_format_grep() {
        let args = r#"{"pattern": "TODO"}"#;
        assert_eq!(format_tool_display("grep", args), "Grep: TODO");
    }

    #[test]
    fn test_format_glob() {
        let args = r#"{"pattern": "**/*.rs"}"#;
        assert_eq!(format_tool_display("glob", args), "Glob: **/*.rs");
    }

    #[test]
    fn test_format_unknown_tool() {
        let args = r#"{"foo": "bar"}"#;
        let result = format_tool_display("custom_tool", args);
        assert!(result.starts_with("custom_tool:"));
    }

    #[test]
    fn test_format_invalid_json() {
        let result = format_tool_display("shell", "not json");
        assert_eq!(result, "$ ?");
    }
}
