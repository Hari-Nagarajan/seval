//! Custom error types and panic hook.
//!
//! Provides a panic hook that restores the terminal to a clean state before
//! printing a friendly error message, and a helper to restore terminal state
//! that can be called from other recovery paths.

use std::io::stderr;

use crossterm::ExecutableCommand;
use crossterm::cursor;
use crossterm::terminal::{LeaveAlternateScreen, disable_raw_mode};

/// Best-effort terminal state restoration.
///
/// Disables raw mode, shows the cursor, and leaves the alternate screen. All
/// errors are silently ignored — this function is designed to be safe to call
/// even when the terminal is not in raw mode or alternate screen.
pub fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = stderr().execute(cursor::Show);
    let _ = stderr().execute(LeaveAlternateScreen);
}

/// Installs a custom panic hook that restores the terminal before printing a
/// friendly error message.
///
/// In debug builds the hook also prints the panic location and invokes the
/// default panic hook for a full backtrace. In release builds only the
/// user-friendly message is shown.
pub fn install_panic_hook() {
    let original_hook = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |panic_info| {
        // Always restore terminal first.
        restore_terminal();

        eprintln!();
        eprintln!("Something went wrong. The application has crashed.");
        eprintln!("Log file: ~/.seval/logs/");

        if cfg!(debug_assertions) {
            if let Some(location) = panic_info.location() {
                eprintln!(
                    "Panic at {}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                );
            }
            original_hook(panic_info);
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `restore_terminal()` must not panic even when the terminal is not in raw
    /// mode or alternate screen.
    #[test]
    fn restore_terminal_no_panic() {
        // Simply calling this should succeed without panic.
        restore_terminal();
    }

    /// `install_panic_hook()` should install the hook without panicking.
    /// We cannot easily test the hook itself (it replaces the global handler),
    /// but we verify the install path works.
    #[test]
    fn install_panic_hook_no_panic() {
        install_panic_hook();
    }
}
