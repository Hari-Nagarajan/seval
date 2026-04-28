//! Draw methods and helper functions.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::chat::message::Role;

use super::component::Chat;

/// ASCII art logo displayed as a watermark on the empty chat screen.
pub(super) const ASCII_LOGO: &[&str] = &[
    "",
    "   \u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557}\u{2588}\u{2588}\u{2557}   \u{2588}\u{2588}\u{2557} \u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557} \u{2588}\u{2588}\u{2557}     ",
    "   \u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{2550}\u{2550}\u{255D}\u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{2550}\u{2550}\u{255D}\u{2588}\u{2588}\u{2551}   \u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{2588}\u{2588}\u{2557}\u{2588}\u{2588}\u{2551}     ",
    "   \u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557}  \u{2588}\u{2588}\u{2551}   \u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2551}     ",
    "   \u{255A}\u{2550}\u{2550}\u{2550}\u{2550}\u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{255D}  \u{255A}\u{2588}\u{2588}\u{2557} \u{2588}\u{2588}\u{2554}\u{255D}\u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2551}     ",
    "   \u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557} \u{255A}\u{2588}\u{2588}\u{2588}\u{2588}\u{2554}\u{255D} \u{2588}\u{2588}\u{2551}  \u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557}",
    "   \u{255A}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255D}\u{255A}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255D}  \u{255A}\u{2550}\u{2550}\u{2550}\u{255D}  \u{255A}\u{2550}\u{255D}  \u{255A}\u{2550}\u{255D}\u{255A}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255D}",
    "",
];

/// Per-tool bullet color for chat display.
pub(super) fn tool_color(name: &str) -> Color {
    match name {
        "read" => Color::Cyan,
        "grep" | "glob" | "ls" => Color::Blue,
        "shell" => Color::Yellow,
        "process" => Color::LightYellow,
        "write" | "edit" => Color::Magenta,
        "web_fetch" | "web_search" => Color::Green,
        "save_memory" => Color::DarkGray,
        _ => Color::White,
    }
}

/// Capitalize the first letter of a tool name for display.
pub(super) fn display_tool_name(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let upper: String = c.to_uppercase().collect();
            format!("{upper}{}", chars.as_str())
        }
    }
}

/// Render a user message with `❯` prompt prefix.
pub(super) fn render_user_message(text: &str) -> Vec<Line<'static>> {
    let mut lines_iter = text.lines();
    let mut result = Vec::new();
    if let Some(first) = lines_iter.next() {
        result.push(Line::from(vec![
            Span::styled(
                "\u{276f} ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(first.to_string(), Style::default().fg(Color::White)),
        ]));
    }
    for line in lines_iter {
        result.push(Line::from(Span::styled(
            format!("  {line}"),
            Style::default().fg(Color::White),
        )));
    }
    if result.is_empty() {
        result.push(Line::from(Span::styled(
            "\u{276f} ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
    }
    result
}

/// Check if a rendered line starts with a tool indicator (for compact stacking).
fn is_tool_indicator_line(line: &Line<'_>) -> bool {
    let text = line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>();
    let trimmed = text.trim_start();
    trimmed.starts_with('\u{25cf}')  // ●
        || trimmed.starts_with('\u{2714}')  // ✔
        || trimmed.starts_with('\u{2718}')  // ✘
        || trimmed.starts_with('!')
}

impl Chat {
    /// Build the lines for the message display area.
    pub(super) fn build_message_lines(&self) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        // Track whether the previous message's rendered lines end with a tool indicator,
        // so we can skip the blank separator between consecutive tool messages.
        let mut prev_is_tool = false;

        for (i, msg) in self.messages.iter().enumerate() {
            let Some(rendered) = self.rendered_messages.get(i) else {
                continue;
            };

            let curr_is_tool =
                msg.role == Role::System && rendered.first().is_some_and(is_tool_indicator_line);

            // Add spacing between messages, but skip between consecutive tool lines.
            if i > 0 && !(prev_is_tool && curr_is_tool) {
                lines.push(Line::from(""));
            }

            lines.extend(rendered.iter().cloned());
            prev_is_tool = curr_is_tool;
        }

        // If streaming or awaiting approval, show current streaming buffer content inline.
        // Strip raw tool call XML so it doesn't flash on screen during streaming.
        if self.is_busy() && !self.streaming.buffer.is_empty() {
            let display_text = super::tools::strip_tool_call_xml(&self.streaming.buffer);
            if !display_text.is_empty() {
                if !lines.is_empty() {
                    lines.push(Line::from(""));
                }
                for line in display_text.lines() {
                    lines.push(Line::from(line.to_string()));
                }
            }
        }

        // Show ASCII art watermark when chat is empty.
        if self.messages.is_empty() && !self.streaming.is_streaming {
            if let Some(ref err) = self.provider_error {
                lines.push(Line::from(Span::styled(
                    err.clone(),
                    Style::default().fg(Color::Red),
                )));
                lines.push(Line::from(Span::styled(
                    "Set your API key in ~/.seval/config.toml to enable chat.",
                    Style::default().fg(Color::DarkGray),
                )));
                lines.push(Line::from(""));
            }
            let dim = Style::default().fg(Color::Rgb(50, 50, 50));
            for line in ASCII_LOGO {
                lines.push(Line::from(Span::styled(*line, dim)));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Type a message or /help for commands.",
                Style::default().fg(Color::DarkGray),
            )));
        }

        lines
    }

    /// Draw the full chat view (delegated from `Component::draw`).
    #[allow(clippy::too_many_lines)]
    pub(super) fn draw_chat(&self, frame: &mut Frame, area: Rect) {
        // Layout: chat area | [thinking bar] | input area.
        let input_line_count = u16::try_from(self.input.lines().len().clamp(1, 5)).unwrap_or(5);
        let input_height = input_line_count + 2; // +2 for border
        let thinking_height = u16::from(self.is_busy());

        let chunks = Layout::vertical([
            Constraint::Min(1),                  // Chat messages area
            Constraint::Length(thinking_height), // Thinking indicator
            Constraint::Length(input_height),    // Input area
        ])
        .split(area);

        // --- Chat messages area ---
        let message_lines = self.build_message_lines();
        let visible_height = chunks[0].height.saturating_sub(2); // account for borders
        let inner_width = chunks[0].width.saturating_sub(2); // account for borders

        let chat_block = Block::default()
            .title(Span::styled(
                " Seval Chat ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));

        // Calculate wrapped line count for accurate scrolling.
        // Each line wraps based on its width vs the inner area width.
        let total_lines = if inner_width > 0 {
            let count: usize = message_lines
                .iter()
                .map(|line| {
                    let width: usize = line.spans.iter().map(|s| s.content.len()).sum();
                    if width == 0 {
                        1
                    } else {
                        width.div_ceil(usize::from(inner_width))
                    }
                })
                .sum();
            u16::try_from(count).unwrap_or(u16::MAX)
        } else {
            u16::try_from(message_lines.len()).unwrap_or(u16::MAX)
        };

        // Calculate scroll: auto-scroll to bottom unless user scrolled up.
        let max_scroll = total_lines.saturating_sub(visible_height);
        // Cache for key handler clamping (prevents scroll_offset overshooting).
        self.max_scroll.set(max_scroll);
        let scroll = if self.scroll_offset > 0 {
            max_scroll.saturating_sub(self.scroll_offset)
        } else {
            max_scroll
        };

        let chat_paragraph = Paragraph::new(message_lines)
            .block(chat_block)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0));

        frame.render_widget(chat_paragraph, chunks[0]);

        // --- Thinking indicator bar ---
        if thinking_height > 0 {
            let elapsed = self
                .streaming
                .started_at
                .map(|t| format_elapsed(t.elapsed()))
                .unwrap_or_default();
            let (verb, _) = self.streaming.thinking_verb;
            let indicator = format!(" \u{27e1} {verb}...{elapsed}");
            let thinking_line = Line::from(Span::styled(
                indicator,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::ITALIC),
            ));
            frame.render_widget(Paragraph::new(thinking_line), chunks[1]);
        }

        // --- Input area ---
        let input_lines: Vec<Line<'static>> = self
            .input
            .lines()
            .iter()
            .map(|l| Line::from(l.to_string()))
            .collect();

        let placeholder = if self.input.is_empty() {
            " Send a message... "
        } else {
            " Message "
        };

        let input_block = Block::default()
            .title(Span::styled(
                placeholder,
                Style::default().fg(Color::DarkGray),
            ))
            .borders(Borders::ALL)
            .border_style(if self.is_busy() {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Cyan)
            });

        let input_paragraph = Paragraph::new(input_lines).block(input_block);
        frame.render_widget(input_paragraph, chunks[2]);

        // Set cursor position in input area (hidden during streaming/approval/picker).
        if !self.is_busy() && !self.model_picker.active {
            let (cursor_line, cursor_col) = self.input.cursor_position();
            let cursor_x = chunks[2].x + 1 + u16::try_from(cursor_col).unwrap_or(0);
            let cursor_y = chunks[2].y + 1 + u16::try_from(cursor_line).unwrap_or(0);
            if cursor_x < chunks[2].x + chunks[2].width - 1
                && cursor_y < chunks[2].y + chunks[2].height - 1
            {
                frame.set_cursor_position((cursor_x, cursor_y));
            }
        }

        // --- Model picker overlay ---
        if self.model_picker.active {
            self.draw_model_picker(frame, area);
        }
    }
}

/// Format a duration as a human-readable string (e.g. " (5s)", " (1m 12s)").
pub(super) fn format_elapsed(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 1 {
        String::new()
    } else if secs < 60 {
        format!(" ({secs}s)")
    } else {
        format!(" ({}m {}s)", secs / 60, secs % 60)
    }
}

/// Format tokens compactly (e.g. "1.2k", "500").
pub(super) fn format_compact_tokens(tokens: u64) -> String {
    if tokens >= 1000 {
        #[allow(clippy::cast_precision_loss)]
        let k = tokens as f64 / 1000.0;
        format!("{k:.1}k")
    } else {
        tokens.to_string()
    }
}

/// Center a popup rect of `width` x `height` within `area`.
pub(super) fn centered_popup(area: Rect, width: u16, height: u16) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .split(area);
    let horizontal = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .split(vertical[0]);
    horizontal[0]
}

#[cfg(test)]
mod tests {
    use super::super::component::Chat;
    use super::super::component::tests::make_chat;

    #[tokio::test]
    async fn build_message_lines_empty_shows_logo_and_help() {
        let chat: Chat = make_chat().await;
        let lines = chat.build_message_lines();
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.contains("\u{2588}\u{2588}\u{2588}"),
            "should show ASCII logo"
        );
        assert!(text.contains("/help"), "should mention /help");
    }
}
