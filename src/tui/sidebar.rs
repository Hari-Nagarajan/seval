//! Sidebar component for the dashboard layout.
//!
//! Displays the application name, version, context usage bar, session info,
//! and live tool status during agentic execution. The context bar shows
//! color-coded token usage (green/yellow/red), and session info shows the
//! active model and message count.

use std::collections::VecDeque;
use std::time::Instant;

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::tui::Component;

/// Maximum display length for detail text in the sidebar (28 col - 4 for padding/borders).
const MAX_DETAIL_LEN: usize = 22;

/// Spinner frames for the running tool indicator.
const TOOL_SPINNER: &[char] = &['|', '/', '-', '\\'];

/// Status of a tool entry in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    /// Tool is currently executing.
    Running,
    /// Tool completed successfully.
    Completed,
    /// Tool failed with an error.
    Error,
    /// Tool was denied by user or permission mode.
    Denied,
    /// Tool was blocked by a deny rule.
    Blocked,
}

/// Status of an agent entry in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSidebarStatus {
    /// Agent is currently running.
    Running,
    /// Agent completed successfully.
    Completed,
    /// Agent was cancelled.
    Cancelled,
    /// Agent timed out.
    TimedOut,
}

/// An agent entry displayed in the sidebar.
#[derive(Debug, Clone)]
pub struct AgentSidebarEntry {
    /// Name of the agent.
    pub name: String,
    /// Current turn number.
    pub turn: u32,
    /// Maximum turns configured for the agent.
    pub max_turns: u32,
    /// When the agent started (for elapsed time calculation).
    pub started_at: Instant,
    /// Current status.
    pub status: AgentSidebarStatus,
    /// Elapsed seconds at completion (set on completion).
    pub elapsed_secs: Option<u64>,
}

/// A tool entry displayed in the sidebar.
#[derive(Debug, Clone)]
pub struct ToolEntry {
    /// Name of the tool.
    pub name: String,
    /// Short detail extracted from args (e.g. file path, pattern, command).
    pub detail: String,
    /// Current status.
    pub status: ToolStatus,
    /// When the tool started (for duration calculation).
    pub started_at: Instant,
    /// Execution duration in milliseconds (set on completion).
    pub duration_ms: Option<u64>,
}

/// Sidebar component with tool status tracking.
///
/// Tracks context window usage, session metadata, and maintains a history
/// of the last 8 completed/denied/errored tool executions with color-coded
/// indicators.
#[derive(Default)]
pub struct Sidebar {
    /// Currently executing tool (at most one).
    active_tool: Option<ToolEntry>,
    /// History of completed/denied/errored tools (newest at back).
    tool_history: VecDeque<ToolEntry>,
    /// Animation frame counter for the running tool spinner.
    spinner_frame: usize,
    /// Current token count (context used).
    context_used: u64,
    /// Context window size (max tokens).
    context_max: u64,
    /// Display name of the active model.
    model_name: Option<String>,
    /// Number of messages in the session.
    message_count: usize,
    /// Whether any agent has been spawned this session (controls header visibility).
    agent_section_visible: bool,
    /// Currently running agents (all shown).
    running_agents: Vec<AgentSidebarEntry>,
    /// Recently completed agents (newest at back, capped at 3).
    completed_agents: VecDeque<AgentSidebarEntry>,
}

impl Sidebar {
    /// Create a new sidebar instance with empty tool state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that a tool has started executing.
    ///
    /// If there is already an active tool, it is moved to history as completed
    /// (edge case: tool start without a result event).
    pub fn tool_call_start(&mut self, name: String, args_json: &str) {
        // If there's an existing active tool, move it to history as completed.
        if let Some(prev) = self.active_tool.take() {
            let mut entry = prev;
            entry.status = ToolStatus::Completed;
            self.push_history(entry);
        }
        let detail = format_tool_detail(&name, args_json);
        self.active_tool = Some(ToolEntry {
            name,
            detail,
            status: ToolStatus::Running,
            started_at: Instant::now(),
            duration_ms: None,
        });
    }

    /// Record that a tool completed successfully.
    ///
    /// If the active tool matches the name, moves it to history with the
    /// given duration. Otherwise adds a new completed entry directly.
    pub fn tool_completed(&mut self, name: String, duration_ms: u64) {
        if let Some(active) = self.active_tool.take() {
            let mut entry = active;
            entry.status = ToolStatus::Completed;
            entry.duration_ms = Some(duration_ms);
            if entry.name != name {
                entry.name = name;
            }
            self.push_history(entry);
        } else {
            self.push_history(ToolEntry {
                name,
                detail: String::new(),
                status: ToolStatus::Completed,
                started_at: Instant::now(),
                duration_ms: Some(duration_ms),
            });
        }
    }

    /// Record that a tool failed with an error.
    pub fn tool_error(&mut self, name: String) {
        if let Some(active) = self.active_tool.take() {
            let mut entry = active;
            entry.status = ToolStatus::Error;
            if entry.name != name {
                entry.name = name;
            }
            self.push_history(entry);
        } else {
            self.push_history(ToolEntry {
                name,
                detail: String::new(),
                status: ToolStatus::Error,
                started_at: Instant::now(),
                duration_ms: None,
            });
        }
    }

    /// Record that a tool call was denied (by user or permission mode).
    ///
    /// Denied tools were never "active" — they go directly to history.
    pub fn tool_denied(&mut self, name: String) {
        self.push_history(ToolEntry {
            name,
            detail: String::new(),
            status: ToolStatus::Denied,
            started_at: Instant::now(),
            duration_ms: None,
        });
    }

    /// Record that a tool call was blocked by a deny rule.
    pub fn tool_blocked(&mut self, name: String) {
        self.push_history(ToolEntry {
            name,
            detail: String::new(),
            status: ToolStatus::Blocked,
            started_at: Instant::now(),
            duration_ms: None,
        });
    }

    /// Advance the spinner animation frame.
    pub fn tick(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }

    /// Update context usage display.
    pub fn update_context(&mut self, used: u64, max: u64) {
        self.context_used = used;
        self.context_max = max;
    }

    /// Update session info display.
    pub fn update_session_info(&mut self, model_name: String, message_count: usize) {
        self.model_name = Some(model_name);
        self.message_count = message_count;
    }

    /// Get context usage as (used, max) tokens.
    pub fn context_usage(&self) -> (u64, u64) {
        (self.context_used, self.context_max)
    }

    /// Record that a new agent has started (makes agent section visible).
    pub fn agent_started(&mut self, name: String, max_turns: u32) {
        self.agent_section_visible = true;
        self.running_agents.push(AgentSidebarEntry {
            name,
            turn: 0,
            max_turns,
            started_at: Instant::now(),
            status: AgentSidebarStatus::Running,
            elapsed_secs: None,
        });
    }

    /// Update turn counter for a running agent.
    pub fn agent_turn_update(&mut self, name: &str, turn: u32) {
        if let Some(entry) = self.running_agents.iter_mut().find(|e| e.name == name) {
            entry.turn = turn;
        }
    }

    /// Move a running agent to completed history (capped at 3 entries).
    pub fn agent_completed(&mut self, name: &str, elapsed_secs: u64, status: AgentSidebarStatus) {
        if let Some(pos) = self.running_agents.iter().position(|e| e.name == name) {
            let mut entry = self.running_agents.remove(pos);
            entry.status = status;
            entry.elapsed_secs = Some(elapsed_secs);
            self.completed_agents.push_back(entry);
            while self.completed_agents.len() > 3 {
                self.completed_agents.pop_front();
            }
        }
    }

    /// Reset agent state (called on /clear).
    pub fn clear_agents(&mut self) {
        self.agent_section_visible = false;
        self.running_agents.clear();
        self.completed_agents.clear();
    }

    /// Get a snapshot of running agents.
    pub fn running_agents(&self) -> &[AgentSidebarEntry] {
        &self.running_agents
    }

    /// Get a snapshot of completed agents.
    pub fn completed_agents(&self) -> &VecDeque<AgentSidebarEntry> {
        &self.completed_agents
    }

    /// Build the agent status lines for the sidebar display.
    fn build_agent_lines(&self) -> Vec<Line<'static>> {
        if !self.agent_section_visible {
            return Vec::new();
        }

        let mut lines = Vec::new();

        // Section header.
        lines.push(Line::from(Span::styled(
            "Agents",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));

        // Running agents: 2 lines each (name with spinner + turn counter).
        for entry in &self.running_agents {
            let spinner_ch = TOOL_SPINNER[self.spinner_frame % TOOL_SPINNER.len()];
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {spinner_ch} "),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    entry.name.clone(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(Span::styled(
                format!("    turn {}/{}", entry.turn, entry.max_turns),
                Style::default().fg(Color::DarkGray),
            )));
        }

        // Completed agents: 1 line each (newest first).
        for entry in self.completed_agents.iter().rev() {
            let elapsed = entry.elapsed_secs.unwrap_or(0);
            let status_char = match entry.status {
                AgentSidebarStatus::Completed => "\u{2713}",
                AgentSidebarStatus::Cancelled => "!",
                AgentSidebarStatus::TimedOut => "!",
                AgentSidebarStatus::Running => "?", // shouldn't happen
            };
            let color = match entry.status {
                AgentSidebarStatus::Completed => Color::Green,
                _ => Color::Yellow,
            };
            lines.push(Line::from(Span::styled(
                format!("  {} {} ({}s)", status_char, entry.name, elapsed),
                Style::default().fg(color),
            )));
        }

        // Empty line separator before tools section.
        lines.push(Line::from(""));

        lines
    }

    /// Push a tool entry to history.
    fn push_history(&mut self, entry: ToolEntry) {
        self.tool_history.push_back(entry);
    }

    /// Truncate a string to fit the sidebar width.
    fn truncate(s: &str, max_len: usize) -> String {
        if s.len() > max_len {
            let mut end = max_len.saturating_sub(3);
            while !s.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            format!("{}...", &s[..end])
        } else {
            s.to_string()
        }
    }

    /// Build the tool status lines for the sidebar display.
    ///
    /// `max_lines` limits how many lines to render (based on available height).
    fn build_tool_lines(&self, max_lines: usize) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        // Section header.
        lines.push(Line::from(Span::styled(
            "Tools",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));

        if max_lines <= 1 {
            return lines;
        }

        // Active tool with spinner (takes 2 lines: name + detail).
        let active_lines = if self.active_tool.is_some() { 2 } else { 0 };
        // Remaining lines for history (each entry = 2 lines: status + detail).
        let history_budget = max_lines.saturating_sub(1 + active_lines); // -1 for header
        let history_slots = history_budget / 2;

        if let Some(ref active) = self.active_tool {
            let spinner_ch = TOOL_SPINNER[self.spinner_frame % TOOL_SPINNER.len()];
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {spinner_ch} "),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    active.name.clone(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            if active.detail.is_empty() {
                lines.push(Line::from(""));
            } else {
                lines.push(Line::from(Span::styled(
                    format!("    {}", Self::truncate(&active.detail, MAX_DETAIL_LEN)),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        // History entries (newest first), limited to available slots.
        let skip = self.tool_history.len().saturating_sub(history_slots);
        for entry in self.tool_history.iter().skip(skip).rev() {
            let (indicator, color) = match entry.status {
                ToolStatus::Completed => {
                    let dur = entry
                        .duration_ms
                        .map(|ms| format!(" {ms}ms"))
                        .unwrap_or_default();
                    (format!("  \u{2713} {}{dur}", entry.name), Color::Green)
                }
                ToolStatus::Error => (format!("  \u{2717} {}", entry.name), Color::Red),
                ToolStatus::Denied => (format!("  ! {} denied", entry.name), Color::Yellow),
                ToolStatus::Blocked => (format!("  ! {} blocked", entry.name), Color::Yellow),
                ToolStatus::Running => (format!("  ? {}", entry.name), Color::DarkGray),
            };
            lines.push(Line::from(Span::styled(
                Self::truncate(&indicator, MAX_DETAIL_LEN + 4),
                Style::default().fg(color),
            )));
            // Detail line.
            if entry.detail.is_empty() {
                lines.push(Line::from(""));
            } else {
                lines.push(Line::from(Span::styled(
                    format!("    {}", Self::truncate(&entry.detail, MAX_DETAIL_LEN)),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        // If no tools at all, show placeholder.
        if self.active_tool.is_none() && self.tool_history.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No activity",
                Style::default().fg(Color::DarkGray),
            )));
        }

        lines
    }
}

impl Component for Sidebar {
    fn draw(&self, frame: &mut Frame, area: Rect) -> Result<()> {
        let version = env!("CARGO_PKG_VERSION");

        // Inner height = area minus 2 for borders, minus 2 for version + blank.
        let inner_height = area.height.saturating_sub(4) as usize;

        let mut lines = vec![
            Line::from(Span::styled(
                format!("v{version}"),
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
        ];

        // Agent status section (hidden until first agent spawn).
        let agent_lines = self.build_agent_lines();
        let agent_line_count = agent_lines.len();
        lines.extend(agent_lines);

        // Tool status section — fills remaining height (subtract agent lines to prevent overflow).
        let tool_budget = inner_height.saturating_sub(2 + agent_line_count); // 2 for version+blank
        lines.extend(self.build_tool_lines(tool_budget));

        let block = Block::default()
            .title(Span::styled(
                " Seval ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));

        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, area);

        Ok(())
    }
}

/// Extract a short detail string from tool args for sidebar display.
///
/// Returns a compact one-line summary like a file path, pattern, or command.
pub fn format_tool_detail(tool_name: &str, args_json: &str) -> String {
    let args: serde_json::Value =
        serde_json::from_str(args_json).unwrap_or(serde_json::Value::Null);

    match tool_name {
        "shell" => args
            .get("command")
            .and_then(|v| v.as_str())
            .map(|cmd| format!("$ {cmd}"))
            .unwrap_or_default(),
        "read" | "write" | "edit" => args
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(short_path)
            .unwrap_or_default(),
        "grep" => args
            .get("pattern")
            .and_then(|v| v.as_str())
            .map(|p| format!("/{p}/"))
            .unwrap_or_default(),
        "glob" => args
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "ls" => args
            .get("path")
            .and_then(|v| v.as_str())
            .map(short_path)
            .unwrap_or_default(),
        "web_fetch" | "web_search" => args
            .get("url")
            .or_else(|| args.get("query"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "save_memory" => "saving...".to_string(),
        _ => String::new(),
    }
}

/// Shorten a file path to just the filename (or last 2 components if short).
pub fn short_path(path: &str) -> String {
    let p = std::path::Path::new(path);
    let components: Vec<_> = p.components().collect();
    if components.len() <= 2 {
        return path.to_string();
    }
    // Show last 2 components: parent/file
    let parent = components[components.len() - 2]
        .as_os_str()
        .to_string_lossy();
    let file = components[components.len() - 1]
        .as_os_str()
        .to_string_lossy();
    format!("{parent}/{file}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_call_start_sets_active() {
        let mut sidebar = Sidebar::new();
        sidebar.tool_call_start("shell".to_string(), r#"{"command":"ls"}"#);
        assert!(sidebar.active_tool.is_some());
        let active = sidebar.active_tool.as_ref().unwrap();
        assert_eq!(active.name, "shell");
        assert_eq!(active.status, ToolStatus::Running);
        assert_eq!(active.detail, "$ ls");
    }

    #[test]
    fn test_tool_completed_moves_to_history() {
        let mut sidebar = Sidebar::new();
        sidebar.tool_call_start("shell".to_string(), "{}");
        sidebar.tool_completed("shell".to_string(), 150);
        assert!(sidebar.active_tool.is_none());
        assert_eq!(sidebar.tool_history.len(), 1);
        let entry = &sidebar.tool_history[0];
        assert_eq!(entry.name, "shell");
        assert_eq!(entry.status, ToolStatus::Completed);
        assert_eq!(entry.duration_ms, Some(150));
    }

    #[test]
    fn test_tool_error_moves_to_history() {
        let mut sidebar = Sidebar::new();
        sidebar.tool_call_start("shell".to_string(), "{}");
        sidebar.tool_error("shell".to_string());
        assert!(sidebar.active_tool.is_none());
        assert_eq!(sidebar.tool_history.len(), 1);
        assert_eq!(sidebar.tool_history[0].status, ToolStatus::Error);
    }

    #[test]
    fn test_tool_denied_adds_to_history_not_active() {
        let mut sidebar = Sidebar::new();
        sidebar.tool_denied("write".to_string());
        assert!(sidebar.active_tool.is_none());
        assert_eq!(sidebar.tool_history.len(), 1);
        assert_eq!(sidebar.tool_history[0].status, ToolStatus::Denied);
        assert_eq!(sidebar.tool_history[0].name, "write");
    }

    #[test]
    fn test_tool_blocked_adds_to_history() {
        let mut sidebar = Sidebar::new();
        sidebar.tool_blocked("shell".to_string());
        assert_eq!(sidebar.tool_history.len(), 1);
        assert_eq!(sidebar.tool_history[0].status, ToolStatus::Blocked);
    }

    #[test]
    fn test_history_grows_unbounded() {
        let mut sidebar = Sidebar::new();
        for i in 0..20 {
            sidebar.tool_completed(format!("tool_{i}"), 100);
        }
        // No cap — all 20 entries kept.
        assert_eq!(sidebar.tool_history.len(), 20);
        assert_eq!(sidebar.tool_history[0].name, "tool_0");
        assert_eq!(sidebar.tool_history[19].name, "tool_19");
    }

    #[test]
    fn test_build_tool_lines_limits_to_height() {
        let mut sidebar = Sidebar::new();
        for i in 0..20 {
            sidebar.tool_completed(format!("tool_{i}"), 100);
        }
        // With max_lines=7: 1 header + 3 history slots (2 lines each) = 7
        let lines = sidebar.build_tool_lines(7);
        assert!(lines.len() <= 7);
    }

    #[test]
    fn test_tool_call_start_moves_previous_active_to_history() {
        let mut sidebar = Sidebar::new();
        sidebar.tool_call_start("read".to_string(), "{}");
        sidebar.tool_call_start("write".to_string(), "{}");
        assert_eq!(sidebar.tool_history.len(), 1);
        assert_eq!(sidebar.tool_history[0].name, "read");
        assert_eq!(sidebar.tool_history[0].status, ToolStatus::Completed);
        assert_eq!(sidebar.active_tool.as_ref().unwrap().name, "write");
    }

    #[test]
    fn test_tick_advances_spinner() {
        let mut sidebar = Sidebar::new();
        assert_eq!(sidebar.spinner_frame, 0);
        sidebar.tick();
        assert_eq!(sidebar.spinner_frame, 1);
        sidebar.tick();
        assert_eq!(sidebar.spinner_frame, 2);
    }

    #[test]
    fn test_truncate_short() {
        assert_eq!(Sidebar::truncate("shell", 24), "shell");
    }

    #[test]
    fn test_truncate_long() {
        let long = "a".repeat(30);
        let truncated = Sidebar::truncate(&long, 24);
        assert!(truncated.len() <= 24);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_build_tool_lines_empty() {
        let sidebar = Sidebar::new();
        let lines = sidebar.build_tool_lines(20);
        // "Tools" header + "No activity" placeholder.
        assert!(lines.len() >= 2);
    }

    #[test]
    fn test_build_tool_lines_with_active() {
        let mut sidebar = Sidebar::new();
        sidebar.tool_call_start("shell".to_string(), r#"{"command":"ls -la"}"#);
        let lines = sidebar.build_tool_lines(20);
        // "Tools" header + active name + active detail.
        assert!(lines.len() >= 3);
    }

    #[test]
    fn test_build_tool_lines_with_history() {
        let mut sidebar = Sidebar::new();
        sidebar.tool_completed("read".to_string(), 50);
        sidebar.tool_error("write".to_string());
        sidebar.tool_denied("shell".to_string());
        let lines = sidebar.build_tool_lines(20);
        // "Tools" header + 3 entries * 2 lines each = 7.
        assert_eq!(lines.len(), 7);
    }

    #[test]
    fn test_completed_without_active() {
        let mut sidebar = Sidebar::new();
        sidebar.tool_completed("shell".to_string(), 200);
        assert_eq!(sidebar.tool_history.len(), 1);
        assert_eq!(sidebar.tool_history[0].status, ToolStatus::Completed);
        assert_eq!(sidebar.tool_history[0].duration_ms, Some(200));
    }

    #[test]
    fn test_context_usage_getter() {
        let mut sidebar = Sidebar::new();
        sidebar.update_context(156_000, 200_000);
        assert_eq!(sidebar.context_usage(), (156_000, 200_000));
    }

    #[test]
    fn test_format_tool_detail_shell() {
        assert_eq!(
            format_tool_detail("shell", r#"{"command":"cargo test"}"#),
            "$ cargo test"
        );
    }

    #[test]
    fn test_format_tool_detail_read() {
        assert_eq!(
            format_tool_detail("read", r#"{"file_path":"/home/user/project/src/main.rs"}"#),
            "src/main.rs"
        );
    }

    #[test]
    fn test_format_tool_detail_grep() {
        assert_eq!(
            format_tool_detail("grep", r#"{"pattern":"TODO"}"#),
            "/TODO/"
        );
    }

    #[test]
    fn test_format_tool_detail_glob() {
        assert_eq!(
            format_tool_detail("glob", r#"{"pattern":"**/*.rs"}"#),
            "**/*.rs"
        );
    }

    #[test]
    fn test_short_path_short() {
        assert_eq!(short_path("main.rs"), "main.rs");
    }

    #[test]
    fn test_short_path_long() {
        assert_eq!(short_path("/home/user/project/src/main.rs"), "src/main.rs");
    }

    // -------------------------------------------------------------------------
    // Agent sidebar tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_agent_started_creates_running_entry() {
        let mut sidebar = Sidebar::new();
        sidebar.agent_started("test-agent".to_string(), 10);
        assert_eq!(sidebar.running_agents.len(), 1);
        assert!(sidebar.agent_section_visible);
        assert_eq!(sidebar.running_agents[0].name, "test-agent");
        assert_eq!(sidebar.running_agents[0].max_turns, 10);
        assert_eq!(sidebar.running_agents[0].turn, 0);
        assert_eq!(
            sidebar.running_agents[0].status,
            AgentSidebarStatus::Running
        );
    }

    #[test]
    fn test_agent_turn_update_increments_counter() {
        let mut sidebar = Sidebar::new();
        sidebar.agent_started("test-agent".to_string(), 10);
        sidebar.agent_turn_update("test-agent", 3);
        assert_eq!(sidebar.running_agents[0].turn, 3);
    }

    #[test]
    fn test_agent_completed_moves_to_history() {
        let mut sidebar = Sidebar::new();
        sidebar.agent_started("test-agent".to_string(), 10);
        sidebar.agent_completed("test-agent", 45, AgentSidebarStatus::Completed);
        assert!(sidebar.running_agents.is_empty());
        assert_eq!(sidebar.completed_agents.len(), 1);
        assert_eq!(sidebar.completed_agents[0].elapsed_secs, Some(45));
        assert_eq!(
            sidebar.completed_agents[0].status,
            AgentSidebarStatus::Completed
        );
    }

    #[test]
    fn test_completed_agents_capped_at_3() {
        let mut sidebar = Sidebar::new();
        for i in 0..4 {
            let name = format!("agent-{i}");
            sidebar.agent_started(name.clone(), 5);
            sidebar.agent_completed(&name, 10, AgentSidebarStatus::Completed);
        }
        assert_eq!(sidebar.completed_agents.len(), 3);
    }

    #[test]
    fn test_agent_section_hidden_by_default() {
        let sidebar = Sidebar::new();
        assert!(sidebar.build_agent_lines().is_empty());
    }

    #[test]
    fn test_clear_agents_resets_visibility() {
        let mut sidebar = Sidebar::new();
        sidebar.agent_started("test-agent".to_string(), 5);
        sidebar.clear_agents();
        assert!(!sidebar.agent_section_visible);
        assert!(sidebar.running_agents.is_empty());
        assert!(sidebar.completed_agents.is_empty());
    }
}
