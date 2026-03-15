//! Integration tests for signal handling and terminal safety.
//!
//! Tests that the binary handles signals correctly and that terminal restore
//! functions are safe to call in any state.

use std::process::Command;
use std::time::Duration;

/// Verify that `restore_terminal` can be called safely even when the terminal
/// is not in raw mode.
#[test]
fn restore_terminal_safe_when_not_raw() {
    seval::errors::restore_terminal();
    // If we reach here without panic, the test passes.
}

/// Verify that the panic hook can be installed without error.
#[test]
fn panic_hook_installs() {
    seval::errors::install_panic_hook();
    // If we reach here without panic, the test passes.
}

/// Verify that the binary can be built and that it exits cleanly when sent
/// SIGTERM. This spawns the actual binary as a child process.
#[tokio::test]
async fn binary_exits_on_sigterm() {
    // Build in release mode is slow; use debug binary.
    let binary = env!("CARGO_BIN_EXE_seval");

    // Spawn the process. It will try to init a terminal, which may fail in CI
    // environments without a TTY. We test that SIGTERM is handled gracefully.
    let mut child = Command::new(binary)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn seval binary");

    // Give the process a moment to start up.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Send SIGTERM.
    #[cfg(unix)]
    unsafe {
        libc::kill(child.id().cast_signed(), libc::SIGTERM);
    }

    // Wait for the process to exit (with timeout).
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(status) = child.try_wait().ok().flatten() {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;

    if let Err(_timeout) = result {
        // Timeout -- kill the process and fail.
        child.kill().ok();
        panic!("seval binary did not exit within 5 seconds after SIGTERM");
    }
    // Ok case: process exited. In a non-TTY environment it may exit with an
    // error (can't init terminal), but it should not hang.
}
