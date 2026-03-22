use serde::{Deserialize, Serialize};

/// Status of a completed agent execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Completed,
    TimedOut,
    Cancelled,
}

/// Result of a spawned agent execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentResult {
    pub agent_name: String,
    pub status: AgentStatus,
    pub turns_completed: u32,
    pub max_turns: u32,
    pub elapsed_secs: u64,
    pub full_output: String,
    pub display_output: String,
}

impl AgentResult {
    /// Return a human-readable label for the agent status.
    pub fn status_label(&self) -> &'static str {
        match self.status {
            AgentStatus::Completed => "completed",
            AgentStatus::TimedOut => "timed out",
            AgentStatus::Cancelled => "cancelled",
        }
    }

    /// Create a new `AgentResult`, automatically computing `display_output`
    /// from `full_output`.
    ///
    /// If `full_output` exceeds 50 lines, `display_output` contains the first
    /// 45 lines followed by a "[N more lines...]" trailer.
    pub fn new(
        agent_name: String,
        status: AgentStatus,
        turns_completed: u32,
        max_turns: u32,
        elapsed_secs: u64,
        full_output: String,
    ) -> Self {
        let display_output = compute_display_output(&full_output);
        Self {
            agent_name,
            status,
            turns_completed,
            max_turns,
            elapsed_secs,
            full_output,
            display_output,
        }
    }
}

/// Compute the display output from full output.
///
/// If the output has more than 50 lines, truncates to the first 45 lines
/// and appends a "[N more lines...]" trailer.
fn compute_display_output(full_output: &str) -> String {
    let lines: Vec<&str> = full_output.lines().collect();
    if lines.len() > 50 {
        let remaining = lines.len() - 45;
        let first_part = lines[..45].join("\n");
        format!("{first_part}\n[{remaining} more lines...]")
    } else {
        full_output.to_string()
    }
}

/// Parameters for spawning an agent execution.
///
/// Consumed by the executor in Plan 02.
pub struct AgentExecParams {
    pub agent_name: String,
    pub task: String,
    pub context: Option<String>,
    pub system_prompt: String,
    pub model: String,
    pub temperature: f64,
    pub max_turns: u32,
    pub max_time_minutes: u32,
    pub effective_tools: Vec<String>,
    pub approval_mode: crate::config::ApprovalMode,
    pub deny_rules: Vec<String>,
    pub tx: tokio::sync::mpsc::UnboundedSender<crate::action::Action>,
    pub working_dir: std::path::PathBuf,
    pub brave_api_key: Option<String>,
    pub db: Option<std::sync::Arc<crate::session::db::Database>>,
    pub parent_session_id: Option<String>,
    pub project_path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // AgentStatus serialization tests
    // -------------------------------------------------------------------------

    #[test]
    fn agent_status_serde_round_trip() {
        let statuses = [
            AgentStatus::Completed,
            AgentStatus::TimedOut,
            AgentStatus::Cancelled,
        ];
        for status in statuses {
            let json = serde_json::to_string(&status).expect("serialize");
            let back: AgentStatus = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(status, back, "round-trip failed for {status:?}");
        }
    }

    // -------------------------------------------------------------------------
    // AgentResult serialization tests
    // -------------------------------------------------------------------------

    #[test]
    fn agent_result_serde_round_trip() {
        let result = AgentResult::new(
            "test-agent".to_string(),
            AgentStatus::Completed,
            5,
            10,
            42,
            "line1\nline2\nline3".to_string(),
        );
        let json = serde_json::to_string(&result).expect("serialize");
        let back: AgentResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(result, back);
    }

    // -------------------------------------------------------------------------
    // AgentResult::status_label tests
    // -------------------------------------------------------------------------

    #[test]
    fn status_label_completed() {
        let r = AgentResult::new(
            "a".to_string(),
            AgentStatus::Completed,
            1,
            10,
            5,
            String::new(),
        );
        assert_eq!(r.status_label(), "completed");
    }

    #[test]
    fn status_label_timed_out() {
        let r = AgentResult::new(
            "a".to_string(),
            AgentStatus::TimedOut,
            1,
            10,
            5,
            String::new(),
        );
        assert_eq!(r.status_label(), "timed out");
    }

    #[test]
    fn status_label_cancelled() {
        let r = AgentResult::new(
            "a".to_string(),
            AgentStatus::Cancelled,
            1,
            10,
            5,
            String::new(),
        );
        assert_eq!(r.status_label(), "cancelled");
    }

    // -------------------------------------------------------------------------
    // display_output truncation tests
    // -------------------------------------------------------------------------

    #[test]
    fn display_output_short_not_truncated() {
        let lines: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
        let full_output = lines.join("\n");
        let r = AgentResult::new(
            "a".to_string(),
            AgentStatus::Completed,
            1,
            10,
            5,
            full_output.clone(),
        );
        assert_eq!(r.display_output, full_output);
    }

    #[test]
    fn display_output_exactly_50_not_truncated() {
        let lines: Vec<String> = (1..=50).map(|i| format!("line {i}")).collect();
        let full_output = lines.join("\n");
        let r = AgentResult::new(
            "a".to_string(),
            AgentStatus::Completed,
            1,
            10,
            5,
            full_output.clone(),
        );
        assert_eq!(r.display_output, full_output, "exactly 50 lines should not be truncated");
    }

    #[test]
    fn display_output_51_lines_truncated() {
        let lines: Vec<String> = (1..=51).map(|i| format!("line {i}")).collect();
        let full_output = lines.join("\n");
        let r = AgentResult::new(
            "a".to_string(),
            AgentStatus::Completed,
            1,
            10,
            5,
            full_output,
        );
        // display_output should have 45 lines + trailer
        let display_lines: Vec<&str> = r.display_output.lines().collect();
        // Last line should be the trailer
        let last = display_lines.last().unwrap();
        assert!(
            last.contains("more lines"),
            "expected trailer, got: {last}"
        );
        // Trailer shows 51 - 45 = 6 more lines
        assert!(last.contains("6"), "expected 6 more lines, got: {last}");
        // First 45 lines are preserved
        assert_eq!(display_lines[0], "line 1");
        assert_eq!(display_lines[44], "line 45");
    }

    #[test]
    fn display_output_100_lines_truncated() {
        let lines: Vec<String> = (1..=100).map(|i| format!("line {i}")).collect();
        let full_output = lines.join("\n");
        let r = AgentResult::new(
            "a".to_string(),
            AgentStatus::Completed,
            1,
            10,
            5,
            full_output,
        );
        // 100 - 45 = 55 more lines
        assert!(r.display_output.contains("55 more lines"), "got: {}", r.display_output);
    }
}
