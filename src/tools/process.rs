//! Process management tool -- start, monitor, signal, and list background processes.
//!
//! Enables long-running commands (e.g. `airodump-ng`, `tcpdump`) to run in
//! the background while the AI continues other work. Output is captured to
//! temporary files and can be tailed on demand.

use std::collections::HashMap;
use std::io::{Read as _, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;

use crate::tools::truncate_output;

/// Global registry of background processes, shared across tool invocations.
pub type ProcessRegistry = Arc<Mutex<HashMap<u32, ProcessEntry>>>;

/// Create an empty process registry.
pub fn new_registry() -> ProcessRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Metadata for a tracked background process.
pub struct ProcessEntry {
    pub command: String,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
    pub started_at: std::time::Instant,
}

/// Arguments for the process tool, deserialized from AI-provided JSON.
#[derive(Debug, Deserialize)]
pub struct ProcessArgs {
    pub action: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub signal: Option<String>,
    #[serde(default)]
    pub tail_bytes: Option<usize>,
}

/// Errors that can occur during process tool execution.
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("Missing required field: {0}")]
    MissingField(String),
    #[error("Unknown action: {0}. Expected: start, read_output, signal, list")]
    UnknownAction(String),
    #[error("Process {0} not found in registry")]
    NotFound(u32),
    #[error("Invalid signal: {0}. Expected: SIGINT, SIGTERM, SIGKILL")]
    InvalidSignal(String),
    #[error("Failed to spawn command: {0}")]
    SpawnError(String),
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Failed to signal process: {0}")]
    SignalError(String),
}

/// Process management tool for background command execution.
pub struct ProcessTool {
    working_dir: PathBuf,
    registry: ProcessRegistry,
    max_output_bytes: usize,
}

impl ProcessTool {
    pub fn new(working_dir: PathBuf, registry: ProcessRegistry) -> Self {
        Self {
            working_dir,
            registry,
            max_output_bytes: 102_400,
        }
    }

    fn start(&self, command: &str) -> Result<String, ProcessError> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

        let stdout_file = tempfile::NamedTempFile::new()?;
        let stderr_file = tempfile::NamedTempFile::new()?;
        let (stdout_std, stdout_path) = stdout_file
            .keep()
            .map_err(|e| ProcessError::IoError(e.error))?;
        let (stderr_std, stderr_path) = stderr_file
            .keep()
            .map_err(|e| ProcessError::IoError(e.error))?;

        let child = std::process::Command::new(&shell)
            .arg("-c")
            .arg(command)
            .current_dir(&self.working_dir)
            .stdout(stdout_std)
            .stderr(stderr_std)
            .stdin(std::process::Stdio::null())
            .spawn()
            .map_err(|e| ProcessError::SpawnError(e.to_string()))?;

        let pid = child.id();

        // Intentionally leak the Child handle so the process runs detached.
        // We track it by PID and signal it via libc::kill.
        std::mem::forget(child);

        let entry = ProcessEntry {
            command: command.to_string(),
            stdout_path: stdout_path.clone(),
            stderr_path: stderr_path.clone(),
            started_at: std::time::Instant::now(),
        };

        let mut reg = self
            .registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reg.insert(pid, entry);

        Ok(format!(
            "Process started (PID {pid})\nStdout: {}\nStderr: {}",
            stdout_path.display(),
            stderr_path.display()
        ))
    }

    fn read_output(&self, pid: u32, tail_bytes: usize) -> Result<String, ProcessError> {
        let reg = self
            .registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = reg.get(&pid).ok_or(ProcessError::NotFound(pid))?;

        let stdout = read_tail(&entry.stdout_path, tail_bytes)?;
        let stderr = read_tail(&entry.stderr_path, tail_bytes / 4)?;

        let alive = is_alive(pid);
        let elapsed = entry.started_at.elapsed().as_secs();

        let truncated_stdout = truncate_output(&stdout, self.max_output_bytes);
        let truncated_stderr = truncate_output(&stderr, self.max_output_bytes / 4);

        let mut result = format!(
            "PID {pid} | Status: {} | Uptime: {elapsed}s",
            if alive { "running" } else { "exited" }
        );

        if !truncated_stdout.is_empty() {
            result.push_str("\n\nSTDOUT (tail):\n");
            result.push_str(&truncated_stdout);
        }

        if !truncated_stderr.is_empty() {
            result.push_str("\n\nSTDERR (tail):\n");
            result.push_str(&truncated_stderr);
        }

        if truncated_stdout.is_empty() && truncated_stderr.is_empty() {
            result.push_str("\n\n(no output yet)");
        }

        Ok(result)
    }

    fn signal_process(&self, pid: u32, signal: &str) -> Result<String, ProcessError> {
        let sig = match signal.to_uppercase().as_str() {
            "SIGINT" | "INT" | "2" => libc::SIGINT,
            "SIGTERM" | "TERM" | "15" => libc::SIGTERM,
            "SIGKILL" | "KILL" | "9" => libc::SIGKILL,
            other => return Err(ProcessError::InvalidSignal(other.to_string())),
        };

        // SAFETY: kill() is a standard POSIX signal-delivery syscall; the PID
        // comes from a process we spawned and the signal is validated above.
        let ret = unsafe { libc::kill(pid.cast_signed(), sig) };
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            return Err(ProcessError::SignalError(format!(
                "kill({pid}, {signal}): {err}"
            )));
        }

        if sig == libc::SIGKILL || sig == libc::SIGTERM {
            let mut reg = self
                .registry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            reg.remove(&pid);
        }

        Ok(format!("Sent {signal} to PID {pid}"))
    }

    fn list_processes(&self) -> String {
        let reg = self
            .registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if reg.is_empty() {
            return "No background processes running.".to_string();
        }

        let mut lines = Vec::with_capacity(reg.len() + 1);
        lines.push(format!("{:<8} {:<10} {}", "PID", "UPTIME", "COMMAND"));

        for (pid, entry) in &*reg {
            let alive = is_alive(*pid);
            let elapsed = entry.started_at.elapsed().as_secs();
            let status = if alive {
                format!("{elapsed}s")
            } else {
                "exited".to_string()
            };
            lines.push(format!("{pid:<8} {status:<10} {}", entry.command));
        }

        lines.join("\n")
    }
}

/// Read the last `tail_bytes` from a file.
fn read_tail(path: &Path, tail_bytes: usize) -> Result<String, ProcessError> {
    let mut file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    let size = metadata.len();

    if size == 0 {
        return Ok(String::new());
    }

    let read_from = size.saturating_sub(tail_bytes as u64);

    file.seek(SeekFrom::Start(read_from))?;
    let capacity = usize::try_from(size.min(tail_bytes as u64)).unwrap_or(tail_bytes);
    let mut buf = Vec::with_capacity(capacity);
    file.read_to_end(&mut buf)?;

    Ok(String::from_utf8_lossy(&buf).to_string())
}

/// Check if a process is still alive via `kill(pid, 0)`.
fn is_alive(pid: u32) -> bool {
    // SAFETY: kill(pid, 0) checks process existence without sending a signal.
    unsafe { libc::kill(pid.cast_signed(), 0) == 0 }
}

impl Tool for ProcessTool {
    const NAME: &'static str = "process";

    type Error = ProcessError;
    type Args = ProcessArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "process".to_string(),
            description: "Manage background processes. Start long-running commands, \
                          read their output, send signals, and list running processes. \
                          Use this for commands that need to run continuously (e.g. \
                          airodump-ng, tcpdump, nmap) while you do other work."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["start", "read_output", "signal", "list"],
                        "description": "The action to perform"
                    },
                    "command": {
                        "type": "string",
                        "description": "Shell command to run in background (required for 'start')"
                    },
                    "pid": {
                        "type": "integer",
                        "description": "Process ID (required for 'read_output' and 'signal')"
                    },
                    "signal": {
                        "type": "string",
                        "enum": ["SIGINT", "SIGTERM", "SIGKILL"],
                        "description": "Signal to send (required for 'signal')"
                    },
                    "tail_bytes": {
                        "type": "integer",
                        "description": "Bytes to read from tail of output (default: 4096, for 'read_output')"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        match args.action.as_str() {
            "start" => {
                let cmd = args
                    .command
                    .as_deref()
                    .ok_or_else(|| ProcessError::MissingField("command".to_string()))?;
                self.start(cmd)
            }
            "read_output" => {
                let pid = args
                    .pid
                    .ok_or_else(|| ProcessError::MissingField("pid".to_string()))?;
                let tail = args.tail_bytes.unwrap_or(4096);
                self.read_output(pid, tail)
            }
            "signal" => {
                let pid = args
                    .pid
                    .ok_or_else(|| ProcessError::MissingField("pid".to_string()))?;
                let signal = args
                    .signal
                    .as_deref()
                    .ok_or_else(|| ProcessError::MissingField("signal".to_string()))?;
                self.signal_process(pid, signal)
            }
            "list" => Ok(self.list_processes()),
            other => Err(ProcessError::UnknownAction(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn test_tool() -> ProcessTool {
        ProcessTool::new(env::temp_dir(), new_registry())
    }

    #[tokio::test]
    async fn test_start_and_list() {
        let tool = test_tool();
        let result = tool
            .call(ProcessArgs {
                action: "start".to_string(),
                command: Some("sleep 10".to_string()),
                pid: None,
                signal: None,
                tail_bytes: None,
            })
            .await
            .unwrap();
        assert!(result.contains("Process started"), "got: {result}");
        assert!(result.contains("PID"), "got: {result}");

        let list = tool
            .call(ProcessArgs {
                action: "list".to_string(),
                command: None,
                pid: None,
                signal: None,
                tail_bytes: None,
            })
            .await
            .unwrap();
        assert!(
            list.contains("sleep 10"),
            "list should show command: {list}"
        );

        // Clean up
        let pid: u32 = result
            .split("PID ")
            .nth(1)
            .and_then(|s| s.split(')').next())
            .and_then(|s| s.parse().ok())
            .unwrap();
        let _ = tool
            .call(ProcessArgs {
                action: "signal".to_string(),
                command: None,
                pid: Some(pid),
                signal: Some("SIGKILL".to_string()),
                tail_bytes: None,
            })
            .await;
    }

    #[tokio::test]
    async fn test_start_and_read_output() {
        let tool = test_tool();
        let result = tool
            .call(ProcessArgs {
                action: "start".to_string(),
                command: Some("echo hello_from_bg".to_string()),
                pid: None,
                signal: None,
                tail_bytes: None,
            })
            .await
            .unwrap();

        let pid: u32 = result
            .split("PID ")
            .nth(1)
            .and_then(|s| s.split(')').next())
            .and_then(|s| s.parse().ok())
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let output = tool
            .call(ProcessArgs {
                action: "read_output".to_string(),
                command: None,
                pid: Some(pid),
                signal: None,
                tail_bytes: None,
            })
            .await
            .unwrap();
        assert!(
            output.contains("hello_from_bg"),
            "output should contain command output: {output}"
        );
    }

    #[tokio::test]
    async fn test_signal_sigint() {
        let tool = test_tool();
        let result = tool
            .call(ProcessArgs {
                action: "start".to_string(),
                command: Some("sleep 60".to_string()),
                pid: None,
                signal: None,
                tail_bytes: None,
            })
            .await
            .unwrap();

        let pid: u32 = result
            .split("PID ")
            .nth(1)
            .and_then(|s| s.split(')').next())
            .and_then(|s| s.parse().ok())
            .unwrap();

        let sig_result = tool
            .call(ProcessArgs {
                action: "signal".to_string(),
                command: None,
                pid: Some(pid),
                signal: Some("SIGINT".to_string()),
                tail_bytes: None,
            })
            .await
            .unwrap();
        assert!(sig_result.contains("SIGINT"), "got: {sig_result}");
    }

    #[tokio::test]
    async fn test_list_empty() {
        let tool = test_tool();
        let result = tool
            .call(ProcessArgs {
                action: "list".to_string(),
                command: None,
                pid: None,
                signal: None,
                tail_bytes: None,
            })
            .await
            .unwrap();
        assert_eq!(result, "No background processes running.");
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = test_tool();
        let result = tool
            .call(ProcessArgs {
                action: "bad".to_string(),
                command: None,
                pid: None,
                signal: None,
                tail_bytes: None,
            })
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unknown action"), "got: {err}");
    }

    #[tokio::test]
    async fn test_start_missing_command() {
        let tool = test_tool();
        let result = tool
            .call(ProcessArgs {
                action: "start".to_string(),
                command: None,
                pid: None,
                signal: None,
                tail_bytes: None,
            })
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("command"), "got: {err}");
    }

    #[tokio::test]
    async fn test_read_output_not_found() {
        let tool = test_tool();
        let result = tool
            .call(ProcessArgs {
                action: "read_output".to_string(),
                command: None,
                pid: Some(999_999),
                signal: None,
                tail_bytes: None,
            })
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"), "got: {err}");
    }

    #[tokio::test]
    async fn test_definition_has_action() {
        let tool = test_tool();
        let def = tool.definition(String::new()).await;
        assert_eq!(def.name, "process");
        let required = def.parameters["required"]
            .as_array()
            .expect("required should be array");
        assert!(required.iter().any(|v| v.as_str() == Some("action")));
    }
}
