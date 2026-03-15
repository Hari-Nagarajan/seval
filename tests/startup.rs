//! Integration tests for startup performance and log creation.

use std::process::Command;
use std::time::{Duration, Instant};

/// Get the path to the seval binary built by cargo.
fn seval_bin() -> String {
    env!("CARGO_BIN_EXE_seval").to_string()
}

/// The binary should start and respond to SIGTERM in under 200ms.
///
/// We spawn the binary, wait briefly for it to initialize, then send SIGTERM.
/// The total elapsed time (start -> exit) must be under 200ms.
#[test]
fn startup_time_under_200ms() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let log_dir = tmp.path().join("logs");

    let start = Instant::now();

    let mut child = Command::new(seval_bin())
        .env("SEVAL_LOG_DIR", &log_dir)
        .spawn()
        .expect("failed to spawn seval binary");

    // Give it a moment to start, then signal termination.
    std::thread::sleep(Duration::from_millis(50));

    let pid = child.id();
    unsafe {
        libc::kill(pid.cast_signed(), libc::SIGTERM);
    }

    let _status = child.wait().expect("failed to wait for seval");
    let elapsed = start.elapsed();

    // We only care about timing. The binary may exit with code 0 (clean quit),
    // signal-terminated (SIGTERM), or code 1 (no terminal available in test env).
    // All are acceptable — we're measuring startup + shutdown latency.
    assert!(
        elapsed < Duration::from_millis(200),
        "startup + shutdown took {elapsed:?}, expected < 200ms"
    );
}

/// The binary should create a log file on startup.
///
/// We use `SEVAL_LOG_DIR` to redirect logs to a temp directory and verify
/// a log file is written. The binary is run with piped I/O (no terminal),
/// so it exits after logging init but before TUI setup - this is expected.
#[test]
fn creates_log_on_startup() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let log_dir = tmp.path().join("logs");

    // Run with piped I/O. The binary will fail on Tui::init (no terminal)
    // but init_logging() runs first, so the log directory and file are created.
    let output = Command::new(seval_bin())
        .env("SEVAL_LOG_DIR", &log_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .expect("failed to run seval binary");

    // Binary exits with error (no terminal) — that's expected.
    // What matters is that logging was initialized.
    assert!(
        !output.status.success(),
        "expected non-zero exit (no terminal), got: {:?}",
        output.status
    );

    // Check that the log directory was created and contains at least one file.
    assert!(
        log_dir.exists(),
        "log directory was not created at {log_dir:?}"
    );

    let entries: Vec<_> = std::fs::read_dir(&log_dir)
        .expect("failed to read log dir")
        .collect();

    assert!(
        !entries.is_empty(),
        "expected at least one log file in {log_dir:?}"
    );
}
