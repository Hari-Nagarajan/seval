//! Application core with the main event loop.
//!
//! The [`App`] struct owns the terminal, components, and action channel. Its
//! [`run`](App::run) method uses `tokio::select!` to multiplex crossterm
//! events, action dispatch, and signal handlers. During streaming, a 30fps
//! render interval batches terminal redraws for smooth output.
//!
//! In normal mode the app renders a split-pane dashboard: chat pane (left),
//! sidebar (right), and an app-level status bar (bottom). In wizard mode
//! the full screen is given to the Wizard component.

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::action::Action;
use crate::approval::ApprovalRequest;
use crate::chat::Chat;
use crate::colors;
use crate::config::{AppConfig, ApprovalMode};
use crate::session::db::Database;
use crate::tui::Component;
use crate::tui::Tui;
use crate::tui::sidebar::Sidebar;
use crate::tui::wizard::Wizard;

/// Minimum terminal width for the normal UI.
const MIN_WIDTH: u16 = 80;
/// Minimum terminal height for the normal UI.
const MIN_HEIGHT: u16 = 24;

/// Render interval for 30fps buffered rendering during streaming.
const RENDER_INTERVAL: Duration = Duration::from_millis(33);

/// Spinner frames for the status bar streaming indicator.
const SPINNER_FRAMES: &[char] = &['|', '/', '-', '\\'];

/// The main application struct.
///
/// Owns the terminal wrapper, UI components, and the action channel used for
/// Elm-style unidirectional data flow. In normal mode, `chat` and `sidebar`
/// are active. In wizard mode, `components` holds the Wizard.
pub struct App {
    tui: Tui,
    /// Chat component (None in wizard mode).
    chat: Option<Chat>,
    /// Sidebar component (always present, only drawn in normal mode).
    sidebar: Sidebar,
    /// Legacy component list — only used for wizard mode.
    components: Vec<Box<dyn Component>>,
    should_quit: bool,
    show_size_warning: bool,
    terminal_width: u16,
    terminal_height: u16,
    action_tx: UnboundedSender<Action>,
    action_rx: UnboundedReceiver<Action>,
    /// The configured approval mode, displayed in the status bar.
    approval_mode: ApprovalMode,
    /// Whether the app was started in wizard mode (used for mode transitions).
    wizard_mode: bool,
    /// Receiver for approval requests from the `ApprovalHook`.
    approval_rx: mpsc::UnboundedReceiver<ApprovalRequest>,
}

impl App {
    /// Create a new application instance with the given configuration.
    ///
    /// Initializes the terminal, creates the action channel, and sets up the
    /// Chat and Sidebar components. Detects and logs terminal color support.
    pub async fn new(config: &AppConfig) -> Result<Self> {
        let (action_tx, action_rx) = mpsc::unbounded_channel();
        let (approval_tx, approval_rx) = mpsc::unbounded_channel();
        let tui = Tui::init()?;

        // Detect and log color support.
        let color_level = colors::detect_color_level();
        tracing::info!("Color support: {:?}", color_level);

        // Initialize session database (graceful degradation on failure).
        let db = match Database::open() {
            Ok(db) => {
                tracing::info!("Session database opened");
                Some(std::sync::Arc::new(db))
            }
            Err(e) => {
                tracing::warn!("Failed to open session database: {e}");
                None
            }
        };

        let mut chat = Chat::new(config, approval_tx, db).await;
        chat.register_action_handler(action_tx.clone())?;

        // Create a new session in the database.
        chat.init_session();

        // Load project memories and inject into system prompt.
        if let Some(db) = chat.db().cloned() {
            let project = crate::chat::component::project_path();
            match tokio::task::spawn_blocking(move || db.get_memories(&project))
                .await
            {
                Ok(Ok(memories)) if !memories.is_empty() => {
                    let contents: Vec<String> =
                        memories.iter().map(|m| m.content.clone()).collect();
                    tracing::info!("Injecting {} project memories", contents.len());
                    chat.inject_memory_context(&contents);
                }
                Ok(Err(e)) => {
                    tracing::warn!("Failed to load memories: {e}");
                }
                _ => {}
            }
        }

        let mut sidebar = Sidebar::new();

        // Initialize sidebar with context and session info.
        let context_window = chat.query_context_window().await;
        sidebar.update_context(0, context_window);
        sidebar.update_session_info(chat.provider_display(), 0);

        let approval_mode = config.tools.approval_mode;

        Ok(Self {
            tui,
            chat: Some(chat),
            sidebar,
            components: Vec::new(),
            should_quit: false,
            show_size_warning: false,
            terminal_width: 0,
            terminal_height: 0,
            action_tx,
            action_rx,
            approval_mode,
            wizard_mode: false,
            approval_rx,
        })
    }

    /// Create a new application instance in wizard mode.
    ///
    /// Uses the `Wizard` component instead of `Chat` for the first-run
    /// setup experience.
    pub fn new_wizard_mode() -> Result<Self> {
        let (action_tx, action_rx) = mpsc::unbounded_channel();
        let (_approval_tx, approval_rx) = mpsc::unbounded_channel();
        let tui = Tui::init()?;

        let color_level = colors::detect_color_level();
        tracing::info!("Color support: {:?}", color_level);

        let mut wizard = Wizard::new();
        wizard.register_action_handler(action_tx.clone())?;

        let components: Vec<Box<dyn Component>> = vec![Box::new(wizard)];

        Ok(Self {
            tui,
            chat: None,
            sidebar: Sidebar::new(),
            components,
            should_quit: false,
            show_size_warning: false,
            terminal_width: 0,
            terminal_height: 0,
            action_tx,
            action_rx,
            approval_mode: ApprovalMode::Default,
            wizard_mode: true,
            approval_rx,
        })
    }

    /// Run the main event loop.
    ///
    /// Multiplexes crossterm events, action channel, and Unix signal handlers
    /// using `tokio::select!`. The loop exits when `Action::Quit` is processed.
    /// A 30fps render interval provides smooth, batched rendering during
    /// streaming responses.
    pub async fn run(&mut self) -> Result<()> {
        // Initialize components with the terminal area.
        let area = self.tui.size()?;
        if let Some(ref mut chat) = self.chat {
            chat.init(area)?;
        }
        for component in &mut self.components {
            component.init(area)?;
        }

        // Check initial terminal size.
        self.update_size(area.width, area.height);

        // Initial render.
        self.action_tx.send(Action::Render)?;

        let mut event_stream = EventStream::new();

        // Set up Unix signal handlers.
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let mut sigint =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
        let mut sigcont = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::from_raw(signal_hook::consts::SIGCONT),
        )?;

        // 30fps render interval for smooth streaming display.
        let mut render_interval = tokio::time::interval(RENDER_INTERVAL);
        render_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            if self.should_quit {
                break;
            }

            tokio::select! {
                // Signal handlers.
                _ = sigterm.recv() => {
                    self.action_tx.send(Action::Quit)?;
                }
                _ = sigint.recv() => {
                    // If streaming, cancel the agentic loop instead of quitting.
                    if self.chat.as_ref().is_some_and(Chat::is_streaming) {
                        self.action_tx.send(Action::CancelStream)?;
                    } else {
                        self.action_tx.send(Action::Quit)?;
                    }
                }
                _ = sigcont.recv() => {
                    self.tui.resume()?;
                    self.action_tx.send(Action::Render)?;
                }

                // 30fps render tick — sends Tick (for spinner) and Render.
                _ = render_interval.tick() => {
                    self.action_tx.send(Action::Tick)?;
                    self.action_tx.send(Action::Render)?;
                }

                // Approval requests from the hook.
                Some(request) = self.approval_rx.recv() => {
                    if let Some(ref mut chat) = self.chat {
                        chat.receive_approval_request(request);
                    }
                    self.action_tx.send(Action::Render)?;
                }

                // Crossterm events.
                Some(Ok(event)) = event_stream.next() => {
                    self.handle_crossterm_event(event)?;
                }

                // Action channel.
                Some(action) = self.action_rx.recv() => {
                    self.process_action(action)?;
                }
            }
        }

        self.tui.restore()?;
        Ok(())
    }

    /// Update terminal size tracking and the size warning flag.
    fn update_size(&mut self, w: u16, h: u16) {
        self.terminal_width = w;
        self.terminal_height = h;
        self.show_size_warning = w < MIN_WIDTH || h < MIN_HEIGHT;
    }

    /// Convert a crossterm event into actions and dispatch them.
    #[allow(clippy::needless_pass_by_value)]
    fn handle_crossterm_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Key(key) => {
                if self.wizard_mode {
                    // Wizard mode: forward to components list.
                    for component in &mut self.components {
                        if let Some(action) = component.handle_key_event(key)? {
                            self.action_tx.send(action)?;
                        }
                    }
                } else if let Some(ref mut chat) = self.chat {
                    // Normal mode: forward to chat only (sidebar is non-interactive).
                    if let Some(action) = chat.handle_key_event(key)? {
                        self.action_tx.send(action)?;
                    }
                }
                // Trigger a render after key input (event-driven rendering).
                self.action_tx.send(Action::Render)?;
            }
            Event::Paste(text) => {
                self.action_tx.send(Action::Paste(text))?;
                self.action_tx.send(Action::Render)?;
            }
            Event::Resize(w, h) => {
                self.action_tx.send(Action::Resize(w, h))?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Process a single action from the channel.
    ///
    /// Delegates to focused helpers for sidebar updates and post-forward sync,
    /// keeping only app-level concerns (quit, render, resize, suspend) inline.
    #[allow(clippy::needless_pass_by_value)] // Action is cloned for component dispatch
    fn process_action(&mut self, action: Action) -> Result<()> {
        // Direct app state changes — these are app-level concerns.
        match &action {
            Action::Quit | Action::WizardComplete => {
                self.should_quit = true;
            }
            Action::Render => {
                self.render()?;
            }
            Action::Resize(w, h) => {
                self.update_size(*w, *h);
                self.action_tx.send(Action::Render)?;
            }
            Action::Suspend => {
                self.tui.suspend()?;
                #[cfg(unix)]
                signal_hook::low_level::raise(signal_hook::consts::SIGTSTP)?;
            }
            _ => {}
        }

        // Sidebar-specific updates.
        self.update_sidebar_for_action(&action);

        // Forward to components.
        if self.wizard_mode {
            for component in &mut self.components {
                if let Some(follow_up) = component.update(action.clone())? {
                    self.action_tx.send(follow_up)?;
                }
            }
        } else if let Some(ref mut chat) = self.chat
            && let Some(follow_up) = chat.update(action.clone())?
        {
            self.action_tx.send(follow_up)?;
        }

        // Post-forward sidebar sync.
        self.sync_sidebar_after_forward(&action);

        Ok(())
    }

    /// Update sidebar state in response to an action.
    ///
    /// Handles tool lifecycle events, tick animations, and context window
    /// updates. Called before forwarding the action to components.
    fn update_sidebar_for_action(&mut self, action: &Action) {
        match action {
            Action::ToolCallStart { name, args_json } => {
                self.sidebar.tool_call_start(name.clone(), args_json);
            }
            Action::ToolResult {
                name, duration_ms, ..
            } => {
                self.sidebar.tool_completed(name.clone(), *duration_ms);
            }
            Action::ToolError { name, .. } => {
                self.sidebar.tool_error(name.clone());
            }
            Action::ToolDenied { name, .. } => {
                self.sidebar.tool_denied(name.clone());
            }
            Action::Tick => {
                self.sidebar.tick();
            }
            Action::ContextWindowUpdate(size) => {
                if let Some(ref mut chat) = self.chat {
                    chat.context_state_mut().context_window = *size;
                    let (used, max) = chat.context_tokens();
                    self.sidebar.update_context(used, max);
                }
            }
            _ => {}
        }
    }

    /// Sync sidebar context and session info after forwarding an action.
    ///
    /// Only runs for actions that change conversation state (stream completion,
    /// message send, compression, session resume). Called after components have
    /// processed the action so token counts reflect the latest state.
    fn sync_sidebar_after_forward(&mut self, action: &Action) {
        if !matches!(
            action,
            Action::TokenUpdate { .. }
                | Action::StreamComplete { .. }
                | Action::SendMessage(_)
                | Action::CompressionComplete { .. }
                | Action::SessionResumed { .. }
        ) {
            return;
        }
        if let Some(ref chat) = self.chat {
            let (used, max) = chat.context_tokens();
            self.sidebar.update_context(used, max);
            self.sidebar
                .update_session_info(chat.provider_display(), chat.message_count());
        }
    }

    /// Render all components, or the size warning if terminal is too small.
    fn render(&mut self) -> Result<()> {
        if self.show_size_warning {
            let w = self.terminal_width;
            let h = self.terminal_height;
            self.tui.draw(|frame| {
                let area = frame.area();
                let warning = Paragraph::new(vec![
                    Line::from(Span::styled(
                        "Terminal too small",
                        Style::default()
                            .fg(Color::Red)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(format!("Current: {w}x{h}")),
                    Line::from(format!("Minimum: {MIN_WIDTH}x{MIN_HEIGHT}")),
                    Line::from(""),
                    Line::from("Please resize your terminal."),
                ])
                .alignment(Alignment::Center);
                frame.render_widget(warning, area);
            })?;
        } else if self.wizard_mode {
            // Wizard mode: full-screen wizard component.
            let components = &self.components;
            self.tui.draw(|frame| {
                let area = frame.area();
                for component in components {
                    if let Err(e) = component.draw(frame, area) {
                        tracing::error!("component draw error: {e}");
                    }
                }
            })?;
        } else {
            // Normal mode: split-pane dashboard layout.
            let chat = self.chat.as_ref();
            let sidebar = &self.sidebar;
            let approval_mode = self.approval_mode;
            let turn_info = chat.and_then(Chat::turn_info);
            let context_usage = sidebar.context_usage();

            self.tui.draw(|frame| {
                let area = frame.area();

                // Vertical split: content area + status bar.
                let outer = Layout::vertical([
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
                .split(area);

                // Horizontal split: chat pane + sidebar.
                let inner = Layout::horizontal([
                    Constraint::Min(40),
                    Constraint::Length(28),
                ])
                .split(outer[0]);

                // Draw chat pane.
                if let Some(chat) = chat
                    && let Err(e) = chat.draw(frame, inner[0])
                {
                    tracing::error!("chat draw error: {e}");
                }

                // Draw sidebar.
                if let Err(e) = sidebar.draw(frame, inner[1]) {
                    tracing::error!("sidebar draw error: {e}");
                }

                // Draw status bar.
                render_status_bar(frame, outer[1], chat, approval_mode, turn_info, context_usage);
            })?;
        }
        Ok(())
    }
}

/// Format a token count for display (e.g. 1234 -> "1.2k", 500 -> "500").
fn format_tokens(tokens: u64) -> String {
    if tokens >= 1000 {
        #[allow(clippy::cast_precision_loss)]
        let k = tokens as f64 / 1000.0;
        format!("{k:.1}k")
    } else {
        tokens.to_string()
    }
}

/// Render the app-level status bar.
///
/// Left side: provider, model, approval mode, and streaming spinner.
/// Right side: context usage, message count, token usage, and keyboard shortcuts.
fn render_status_bar(
    frame: &mut ratatui::Frame,
    area: Rect,
    chat: Option<&Chat>,
    approval_mode: ApprovalMode,
    turn_info: Option<(usize, usize)>,
    context_usage: (u64, u64),
) {
    let (provider_display, msg_count, tokens, is_streaming, spinner_frame) =
        if let Some(chat) = chat {
            (
                chat.provider_display(),
                chat.message_count(),
                chat.total_tokens(),
                chat.is_streaming(),
                chat.spinner_frame(),
            )
        } else {
            ("no provider".to_string(), 0, 0, false, 0)
        };

    let mode_str = match approval_mode {
        ApprovalMode::Plan => "PLAN",
        ApprovalMode::Default => "DEFAULT",
        ApprovalMode::AutoEdit => "AUTO-EDIT",
        ApprovalMode::Yolo => "YOLO",
    };

    let turn_str = match turn_info {
        Some((current, max)) => format!(" Turn {current}/{max}"),
        None => String::new(),
    };

    let spinner_str = if is_streaming {
        let ch = SPINNER_FRAMES[spinner_frame % SPINNER_FRAMES.len()];
        format!(" {ch}")
    } else {
        String::new()
    };

    let (ctx_used, ctx_max) = context_usage;
    let context_str = if ctx_max > 0 {
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let pct = (ctx_used as f64 / ctx_max as f64 * 100.0).round() as u64;
        format!(
            "{}/{} ({}%)",
            format_tokens(ctx_used),
            format_tokens(ctx_max),
            pct,
        )
    } else {
        "...".to_string()
    };

    let left = format!(" {provider_display} [{mode_str}]{turn_str}{spinner_str}");
    let right = format!(
        "ctx: {context_str} | {} msgs | {} out | /help ",
        msg_count,
        format_tokens(tokens),
    );

    // Pad between left and right to fill the status bar width.
    let total_width = usize::from(area.width);
    let used = left.len() + right.len();
    let padding = if total_width > used {
        total_width - used
    } else {
        1
    };

    let status_line = Line::from(vec![
        Span::styled(
            left,
            Style::default().fg(Color::White).bg(Color::DarkGray),
        ),
        Span::styled(
            " ".repeat(padding),
            Style::default().bg(Color::DarkGray),
        ),
        Span::styled(
            right,
            Style::default().fg(Color::White).bg(Color::DarkGray),
        ),
    ]);

    frame.render_widget(Paragraph::new(status_line), area);
}
