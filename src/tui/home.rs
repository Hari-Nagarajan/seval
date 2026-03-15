//! Home component — the default landing view.
//!
//! Displays a centered welcome screen with version info, key shortcuts, and
//! configuration guidance. Handles `q` and Ctrl+C to quit, Ctrl+Z to suspend.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;
use crate::tui::Component;

/// The home/welcome screen component.
pub struct Home {
    action_tx: Option<UnboundedSender<Action>>,
}

impl Home {
    pub fn new() -> Self {
        Self { action_tx: None }
    }
}

impl Default for Home {
    fn default() -> Self {
        Self::new()
    }
}

/// Center a rect of `width` x `height` within the given `area`.
fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .split(area);
    let horizontal = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .split(vertical[0]);
    horizontal[0]
}

impl Component for Home {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(tx);
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        let action = match key.code {
            KeyCode::Char('q') => Some(Action::Quit),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Action::Quit)
            }
            KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Action::Suspend)
            }
            _ => None,
        };
        Ok(action)
    }

    fn draw(&self, frame: &mut Frame, area: Rect) -> Result<()> {
        let version = env!("CARGO_PKG_VERSION");

        let lines = vec![
            Line::from(Span::styled(
                format!("v{version}"),
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Welcome to Seval - AI-powered security CLI",
                Style::default().fg(Color::White),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("  q        ", Style::default().fg(Color::Yellow)),
                Span::raw("Quit"),
            ]),
            Line::from(vec![
                Span::styled("  Ctrl+C   ", Style::default().fg(Color::Yellow)),
                Span::raw("Force quit"),
            ]),
            Line::from(vec![
                Span::styled("  Ctrl+Z   ", Style::default().fg(Color::Yellow)),
                Span::raw("Suspend"),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Configuration: Run with --config <path> or set up ~/.seval/config.toml",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        // Content height = lines + 2 for top/bottom border.
        let content_height = u16::try_from(lines.len()).unwrap_or(12) + 2;
        let content_width = 74_u16; // Wide enough for the longest line + padding.

        let block = Block::default()
            .title(Span::styled(
                " Seval ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Center);

        let rect = centered_rect(area, content_width, content_height);
        frame.render_widget(paragraph, rect);

        Ok(())
    }
}
