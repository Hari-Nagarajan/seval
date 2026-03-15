//! Tool call display handlers.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use regex::Regex;
use std::sync::LazyLock;

use crate::chat::message::{ChatMessage, Role};
use crate::tui::sidebar::format_tool_detail;

use super::component::Chat;
use super::rendering::{display_tool_name, tool_color};

/// Regex to strip raw tool call XML that some models emit as text.
static TOOL_CALL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<tool_call>.*?</tool_call>").expect("valid regex")
});

/// Strip `<tool_call>...</tool_call>` blocks (complete and incomplete) and
/// clean up leftover whitespace.
pub(super) fn strip_tool_call_xml(text: &str) -> String {
    let cleaned = TOOL_CALL_RE.replace_all(text, "");
    // Also strip incomplete/trailing `<tool_call>...` without closing tag
    // (happens during streaming before the full tag arrives).
    let cleaned = if let Some(idx) = cleaned.rfind("<tool_call>") {
        if !cleaned[idx..].contains("</tool_call>") {
            &cleaned[..idx]
        } else {
            &cleaned
        }
    } else {
        &cleaned
    };
    // Collapse runs of blank lines left behind.
    let mut result = String::with_capacity(cleaned.len());
    let mut prev_blank = false;
    for line in cleaned.lines() {
        let blank = line.trim().is_empty();
        if blank && prev_blank {
            continue;
        }
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(line);
        prev_blank = blank;
    }
    result.trim().to_string()
}

impl Chat {
    /// Handle a tool call start action by adding it as an inline message.
    ///
    /// If there is accumulated text in the streaming buffer, flush it to a
    /// committed message first so that the agent's text appears *before* the
    /// tool call in the conversation rather than after it.
    pub(super) fn handle_tool_call_start(&mut self, name: &str, args_json: &str) {
        // Flush any buffered streaming text so it appears before the tool call.
        // Strip raw tool call XML that some models emit as text.
        if !self.streaming.buffer.is_empty() {
            let text = strip_tool_call_xml(&std::mem::take(&mut self.streaming.buffer));
            if !text.is_empty() {
                let rendered = crate::chat::markdown::render_markdown(&text);
                let msg = ChatMessage::new(Role::Assistant, &text);
                self.rig_history.push(msg.to_rig_message());
                self.messages.push(msg);
                self.rendered_messages.push(rendered);
            }
        }

        let detail = format_tool_detail(name, args_json);
        let display_name = display_tool_name(name);
        let color = tool_color(name);

        let content = format!("\u{25cf} {display_name} {detail}");
        let mut spans = vec![
            Span::styled(
                "  \u{25cf} ",
                Style::default().fg(color),
            ),
            Span::styled(
                display_name,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ];
        if !detail.is_empty() {
            spans.push(Span::styled(
                format!(" {detail}"),
                Style::default().fg(Color::DarkGray),
            ));
        }
        let rendered = vec![Line::from(spans)];
        let msg = ChatMessage::new(Role::System, content);
        self.messages.push(msg);
        self.rendered_messages.push(rendered);
    }

    /// Handle a tool result action by adding it as an inline message.
    pub(super) fn handle_tool_result(&mut self, name: &str, _result: &str, duration_ms: u64) {
        let display_name = display_tool_name(name);
        let content = format!("\u{2714} {display_name} ({duration_ms}ms)");
        let rendered = vec![Line::from(vec![
            Span::styled(
                "  \u{2714} ",
                Style::default().fg(Color::Green),
            ),
            Span::styled(
                display_name,
                Style::default().fg(Color::Green),
            ),
            Span::styled(
                format!(" ({duration_ms}ms)"),
                Style::default().fg(Color::DarkGray),
            ),
        ])];
        let msg = ChatMessage::new(Role::System, content);
        self.messages.push(msg);
        self.rendered_messages.push(rendered);
    }

    /// Handle a tool error action by adding it as an inline error message.
    pub(super) fn handle_tool_error(&mut self, name: &str, error: &str) {
        let display_name = display_tool_name(name);
        // Truncate error to first line, max ~60 chars.
        let brief = error.lines().next().unwrap_or(error);
        let brief = if brief.len() > 60 {
            let mut end = 57;
            while !brief.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            format!("{}...", &brief[..end])
        } else {
            brief.to_string()
        };

        let content = format!("\u{2718} {display_name}: {brief}");
        let rendered = vec![Line::from(vec![
            Span::styled(
                "  \u{2718} ",
                Style::default().fg(Color::Red),
            ),
            Span::styled(
                format!("{display_name}: "),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                brief,
                Style::default().fg(Color::Red),
            ),
        ])];
        let msg = ChatMessage::new(Role::System, content);
        self.messages.push(msg);
        self.rendered_messages.push(rendered);
    }
}
