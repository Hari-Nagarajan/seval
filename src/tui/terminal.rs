//! Terminal wrapper with lifecycle management.
//!
//! Provides [`Tui`] which wraps the ratatui terminal and handles init, restore,
//! suspend, and resume operations. Uses stderr as the backend so stdout remains
//! available for piping.

use std::io::{self, Stderr};

use anyhow::Result;
use crossterm::ExecutableCommand;
use crossterm::cursor;
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

type Backend = CrosstermBackend<Stderr>;

/// Terminal wrapper managing the ratatui terminal lifecycle.
///
/// Handles entering/leaving alternate screen, raw mode, and cursor visibility.
/// Implements [`Drop`] for best-effort cleanup on unexpected exits.
pub struct Tui {
    terminal: Terminal<Backend>,
}

impl Tui {
    /// Initialize the terminal: enable raw mode, enter alternate screen, hide cursor.
    pub fn init() -> Result<Self> {
        enable_raw_mode()?;
        io::stderr().execute(EnterAlternateScreen)?;
        io::stderr().execute(cursor::Hide)?;
        io::stderr().execute(EnableBracketedPaste)?;

        let backend = CrosstermBackend::new(io::stderr());
        let terminal = Terminal::new(backend)?;

        Ok(Self { terminal })
    }

    /// Restore the terminal to its original state.
    pub fn restore(&mut self) -> Result<()> {
        disable_raw_mode()?;
        io::stderr().execute(DisableBracketedPaste)?;
        io::stderr().execute(LeaveAlternateScreen)?;
        io::stderr().execute(cursor::Show)?;
        Ok(())
    }

    /// Suspend the terminal for shell access (Ctrl+Z).
    ///
    /// Restores the terminal state so the user can interact with the shell.
    pub fn suspend(&mut self) -> Result<()> {
        self.restore()
    }

    /// Resume the terminal after suspension.
    ///
    /// Re-enters raw mode and alternate screen.
    pub fn resume(&mut self) -> Result<()> {
        enable_raw_mode()?;
        io::stderr().execute(EnterAlternateScreen)?;
        io::stderr().execute(cursor::Hide)?;
        io::stderr().execute(EnableBracketedPaste)?;
        self.terminal.clear()?;
        Ok(())
    }

    /// Draw a frame using the provided rendering closure.
    pub fn draw<F>(&mut self, f: F) -> Result<()>
    where
        F: FnOnce(&mut ratatui::Frame),
    {
        self.terminal.draw(f)?;
        Ok(())
    }

    /// Get the current terminal size as a `Rect`.
    pub fn size(&self) -> Result<ratatui::layout::Rect> {
        let size = self.terminal.size()?;
        Ok(ratatui::layout::Rect::new(0, 0, size.width, size.height))
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        // Best-effort restore on drop.
        let _ = self.restore();
    }
}
