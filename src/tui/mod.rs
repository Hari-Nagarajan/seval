//! Terminal UI module.
//!
//! Manages the Ratatui terminal lifecycle, rendering, and the main event loop.
//! This module owns the terminal instance and coordinates frame rendering.
//!
//! The module defines the [`Component`] trait that all UI components implement,
//! following the Elm-style unidirectional data flow pattern.

pub mod home;
pub mod sidebar;
pub mod terminal;
pub mod wizard;

pub use terminal::Tui;

use anyhow::Result;
use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;

/// Trait for UI components that participate in the event loop.
///
/// Components receive key events, process actions, and render themselves.
/// All trait methods except [`draw`] have default no-op implementations,
/// allowing components to opt in to only the behavior they need.
pub trait Component {
    /// Register the action sender so the component can dispatch actions.
    ///
    /// Called once during app initialization.
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        let _ = tx;
        Ok(())
    }

    /// Initialize the component with its available area.
    ///
    /// Called once after the terminal is set up.
    fn init(&mut self, area: Rect) -> Result<()> {
        let _ = area;
        Ok(())
    }

    /// Handle a keyboard event, optionally returning an action.
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        let _ = key;
        Ok(None)
    }

    /// Process an action dispatched through the event loop.
    ///
    /// May return a follow-up action to be dispatched.
    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        let _ = action;
        Ok(None)
    }

    /// Render the component into the given frame area.
    ///
    /// This is the only required method.
    fn draw(&self, frame: &mut Frame, area: Rect) -> Result<()>;
}
