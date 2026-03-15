//! Shell tool -- executes commands via the system shell.
//!
//! Implements the Rig `Tool` trait for executing arbitrary shell commands
//! with configurable timeout and output truncation.

use std::path::PathBuf;

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;

use crate::tools::truncate_output;

/// Arguments for the shell tool, deserialized from AI-provided JSON.
#[derive(Debug, Deserialize)]
pub struct ShellArgs {
    /// The shell command to execute.
    pub command: String,
    /// Optional timeout in seconds (defaults to the tool's `default_timeout`).
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// Errors that can occur during shell tool execution.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    /// Command timed out.
    #[error("Command timed out after {0}s")]
    Timeout(u64),
    /// Failed to spawn the shell process.
    #[error("Failed to spawn command: {0}")]
    SpawnError(String),
    /// I/O error during command execution.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Shell tool for executing system commands.
///
/// Executes commands via `$SHELL -c` (or `/bin/sh -c` as fallback),
/// capturing stdout and stderr concurrently with configurable timeout
/// and output truncation.
pub struct ShellTool {
    /// Working directory where commands are executed.
    working_dir: PathBuf,
    /// Default timeout in seconds (used when not specified in args).
    default_timeout: u64,
    /// Maximum output size in bytes before truncation.
    max_output_bytes: usize,
}

impl ShellTool {
    /// Create a new `ShellTool` with the given working directory.
    ///
    /// Uses default timeout of 30 seconds and max output of 100KB.
    pub fn new(working_dir: PathBuf) -> Self {
        Self {
            working_dir,
            default_timeout: 30,
            max_output_bytes: 102_400,
        }
    }
}

impl Tool for ShellTool {
    const NAME: &'static str = "shell";

    type Error = ShellError;
    type Args = ShellArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "shell".to_string(),
            description: "Execute a shell command and return its output. \
                          The command runs via the system shell ($SHELL or /bin/sh). \
                          Use for running CLI tools, scripts, file operations, and system commands."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Optional timeout in seconds (default: 30)"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let timeout_secs = args.timeout_secs.unwrap_or(self.default_timeout);
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

        let output_future = tokio::process::Command::new(&shell)
            .arg("-c")
            .arg(&args.command)
            .current_dir(&self.working_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();

        let output =
            tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), output_future)
                .await
                .map_err(|_| ShellError::Timeout(timeout_secs))?
                .map_err(|e| ShellError::SpawnError(e.to_string()))?;

        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let truncated_stdout = truncate_output(&stdout, self.max_output_bytes);
        let truncated_stderr = truncate_output(&stderr, self.max_output_bytes / 4);

        let mut result = format!("Exit code: {exit_code}");

        if !truncated_stdout.is_empty() {
            result.push_str("\n\nSTDOUT:\n");
            result.push_str(&truncated_stdout);
        }

        if !truncated_stderr.is_empty() {
            result.push_str("\n\nSTDERR:\n");
            result.push_str(&truncated_stderr);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn test_tool() -> ShellTool {
        ShellTool::new(env::temp_dir())
    }

    #[tokio::test]
    async fn test_echo_hello() {
        let tool = test_tool();
        let result = tool
            .call(ShellArgs {
                command: "echo hello".to_string(),
                timeout_secs: None,
            })
            .await
            .unwrap();
        assert!(result.contains("hello"), "stdout should contain hello");
        assert!(result.contains("Exit code: 0"), "should have exit code 0");
    }

    #[tokio::test]
    async fn test_stderr_capture() {
        let tool = test_tool();
        let result = tool
            .call(ShellArgs {
                command: "echo error_output >&2".to_string(),
                timeout_secs: None,
            })
            .await
            .unwrap();
        assert!(result.contains("error_output"), "stderr should be captured");
        assert!(result.contains("STDERR:"), "should have STDERR section");
    }

    #[tokio::test]
    async fn test_nonzero_exit() {
        let tool = test_tool();
        let result = tool
            .call(ShellArgs {
                command: "exit 1".to_string(),
                timeout_secs: None,
            })
            .await
            .unwrap();
        assert!(
            result.contains("Exit code: 1"),
            "should report non-zero exit code"
        );
    }

    #[tokio::test]
    async fn test_timeout() {
        let tool = ShellTool {
            working_dir: env::temp_dir(),
            default_timeout: 1,
            max_output_bytes: 102_400,
        };
        let result = tool
            .call(ShellArgs {
                command: "sleep 60".to_string(),
                timeout_secs: Some(1),
            })
            .await;
        assert!(result.is_err(), "should timeout");
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("timed out"),
            "error should mention timeout: {err_msg}"
        );
    }

    #[tokio::test]
    async fn test_definition_has_command() {
        let tool = test_tool();
        let def = tool.definition(String::new()).await;
        assert_eq!(def.name, "shell");
        let params = &def.parameters;
        let required = params["required"]
            .as_array()
            .expect("required should be array");
        assert!(
            required.iter().any(|v| v.as_str() == Some("command")),
            "command should be required"
        );
        assert!(
            params["properties"]["command"].is_object(),
            "command property should exist"
        );
    }

    #[tokio::test]
    async fn test_concurrent_output() {
        let tool = test_tool();
        let result = tool
            .call(ShellArgs {
                command: "echo stdout_data && echo stderr_data >&2".to_string(),
                timeout_secs: None,
            })
            .await
            .unwrap();
        assert!(result.contains("stdout_data"), "should capture stdout");
        assert!(result.contains("stderr_data"), "should capture stderr");
        assert!(result.contains("Exit code: 0"), "should succeed");
    }

    #[tokio::test]
    async fn test_output_truncation() {
        let tool = ShellTool {
            working_dir: env::temp_dir(),
            default_timeout: 30,
            max_output_bytes: 200,
        };
        // Generate output larger than 200 bytes
        let result = tool
            .call(ShellArgs {
                command: "python3 -c \"print('x' * 1000)\"".to_string(),
                timeout_secs: None,
            })
            .await
            .unwrap();
        assert!(
            result.contains("bytes truncated"),
            "large output should be truncated"
        );
    }
}
