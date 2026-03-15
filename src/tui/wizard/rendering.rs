//! Rendering for the setup wizard.
//!
//! Contains all `draw_*` methods and rendering helpers that paint
//! the wizard UI to the terminal frame.

use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::config::{project_config_path, ProviderKind};

use super::{Wizard, WizardStep, MODE_DESCRIPTIONS, PROVIDER_OPTIONS};

impl Wizard {
    /// Center a rect within the given area.
    pub(super) fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
        let vertical = Layout::vertical([Constraint::Length(height)])
            .flex(Flex::Center)
            .split(area);
        let horizontal = Layout::horizontal([Constraint::Length(width)])
            .flex(Flex::Center)
            .split(vertical[0]);
        horizontal[0]
    }

    /// Draw the step header with step indicator.
    pub(super) fn draw_header(&self, frame: &mut Frame, area: Rect) {
        let step_num = self.step.number();
        let label = self.step.label();
        let total = WizardStep::TOTAL;
        let header = Line::from(vec![
            Span::styled(
                " SEVAL Setup ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("Step {step_num}/{total} - {label}"),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        let paragraph = Paragraph::new(header).alignment(Alignment::Center);
        frame.render_widget(paragraph, area);
    }

    /// Draw the provider selection step.
    pub(super) fn draw_provider(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = PROVIDER_OPTIONS
            .iter()
            .map(|(name, desc)| {
                ListItem::new(vec![
                    Line::from(Span::styled(
                        format!("  {name}"),
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from(Span::styled(
                        format!("    {desc}"),
                        Style::default().fg(Color::DarkGray),
                    )),
                ])
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Select AI Provider ")
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut self.provider_state.clone());
    }

    /// Mask a secret value for display.
    fn mask_value(value: &str, masked: bool) -> String {
        if value.is_empty() {
            return String::new();
        }
        if !masked {
            return value.to_string();
        }
        let len = value.len();
        if len <= 8 {
            "*".repeat(len)
        } else {
            format!("{}...{}", &value[..4], "*".repeat(len.min(20) - 4))
        }
    }

    /// Draw the API key / credentials input step.
    pub(super) fn draw_api_key(&self, frame: &mut Frame, area: Rect) {
        match self.selected_provider {
            ProviderKind::Bedrock => self.draw_bedrock_creds(frame, area),
            ProviderKind::OpenRouter => self.draw_openrouter_key(frame, area),
        }
    }

    /// Draw Bedrock credential fields (access key, secret key, region).
    fn draw_bedrock_creds(&self, frame: &mut Frame, area: Rect) {
        let fields: [(&str, &str, bool); 3] = [
            ("Access Key ID", &self.bedrock_access_key, true),
            ("Secret Access Key", &self.bedrock_secret_key, true),
            ("Region", &self.bedrock_region, false),
        ];

        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Enter your AWS Bedrock credentials:",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];

        for (i, (label, value, is_secret)) in fields.iter().enumerate() {
            let active = i == self.bedrock_field_index;
            let prefix = if active { "> " } else { "  " };
            let label_style = if active {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let display = if *is_secret {
                Self::mask_value(value, self.api_key_masked)
            } else {
                (*value).to_string()
            };

            let cursor = if active { "_" } else { "" };

            lines.push(Line::from(vec![
                Span::styled(format!("{prefix}{label}: "), label_style),
                Span::raw(display),
                Span::styled(cursor, Style::default().fg(Color::DarkGray)),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Tab: next field  |  Ctrl+H: toggle mask  |  Enter: confirm  |  Esc: back",
            Style::default().fg(Color::DarkGray),
        )));

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .title(" Bedrock Credentials ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(paragraph, area);
    }

    /// Draw `OpenRouter` single API key input.
    fn draw_openrouter_key(&self, frame: &mut Frame, area: Rect) {
        let display_key = Self::mask_value(&self.api_key_input, self.api_key_masked);

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Enter your OpenRouter API key:",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("  > ", Style::default().fg(Color::Cyan)),
                Span::raw(&display_key),
                Span::styled("_", Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "  Ctrl+H: toggle visibility  |  Enter: confirm  |  Esc: back",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .title(" OpenRouter API Key ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(paragraph, area);
    }

    /// Draw the model selection step.
    pub(super) fn draw_model_select(&self, frame: &mut Frame, area: Rect) {
        let models = self.models_for_provider();
        let items: Vec<ListItem> = models
            .iter()
            .map(|(id, desc)| {
                ListItem::new(vec![
                    Line::from(Span::styled(
                        format!("  {id}"),
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from(Span::styled(
                        format!("    {desc}"),
                        Style::default().fg(Color::DarkGray),
                    )),
                ])
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Select Model ")
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut self.model_state.clone());
    }

    /// Draw the approval mode selection step.
    pub(super) fn draw_approval_mode(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = MODE_DESCRIPTIONS
            .iter()
            .map(|(name, desc)| {
                ListItem::new(vec![
                    Line::from(Span::styled(
                        format!("  {name}"),
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from(Span::styled(
                        format!("    {desc}"),
                        Style::default().fg(Color::DarkGray),
                    )),
                ])
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Select Approval Mode ")
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut self.approval_mode_state.clone());
    }

    /// Draw the deny rules editing step.
    pub(super) fn draw_deny_rules(&self, frame: &mut Frame, area: Rect) {
        // Split area: list on top, instructions/input at bottom.
        let chunks =
            Layout::vertical([Constraint::Min(4), Constraint::Length(3)]).split(area);

        let items: Vec<ListItem> = self
            .deny_rules
            .iter()
            .map(|rule| ListItem::new(Line::from(format!("  {rule}"))))
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Deny Rules ")
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .highlight_style(
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, chunks[0], &mut self.deny_rule_state.clone());

        // Input / instructions area.
        let bottom_content = if self.is_adding_rule {
            let rule_text = self.editing_deny_rule.as_deref().unwrap_or("");
            Line::from(vec![
                Span::styled("New rule: ", Style::default().fg(Color::Yellow)),
                Span::raw(rule_text),
                Span::styled("_", Style::default().fg(Color::DarkGray)),
            ])
        } else {
            Line::from(vec![
                Span::styled("a", Style::default().fg(Color::Yellow)),
                Span::raw(": add  "),
                Span::styled("d", Style::default().fg(Color::Yellow)),
                Span::raw(": delete  "),
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::raw(": continue  "),
                Span::styled("Esc", Style::default().fg(Color::Yellow)),
                Span::raw(": back"),
            ])
        };

        let instructions = Paragraph::new(bottom_content)
            .block(Block::default().borders(Borders::TOP))
            .alignment(Alignment::Center);
        frame.render_widget(instructions, chunks[1]);
    }

    /// Draw the project init step.
    pub(super) fn draw_project_init(&self, frame: &mut Frame, area: Rect) {
        let yes_style = if self.create_project_config {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let no_style = if self.create_project_config {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD)
        };

        let project_path = project_config_path();
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Create project-local config in .seval/?",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("  Path: {}", project_path.display()),
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("[Y]es", yes_style),
                Span::raw("    "),
                Span::styled("[N]o", no_style),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::raw(": confirm  "),
                Span::styled("Esc", Style::default().fg(Color::Yellow)),
                Span::raw(": back"),
            ]),
        ];

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .title(" Project Config ")
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .alignment(Alignment::Center);

        frame.render_widget(paragraph, area);
    }
}
