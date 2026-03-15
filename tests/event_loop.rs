//! Integration tests for the event loop and action dispatch.
//!
//! These tests verify the action channel mechanics and Action enum behavior
//! without requiring an actual terminal.

use seval::action::Action;
use tokio::sync::mpsc;

/// Verify that actions can be sent and received through the mpsc channel.
#[tokio::test]
async fn action_dispatch_through_channel() {
    let (tx, mut rx) = mpsc::unbounded_channel::<Action>();

    tx.send(Action::Quit).unwrap();
    tx.send(Action::Render).unwrap();
    tx.send(Action::Resize(80, 24)).unwrap();

    assert_eq!(rx.recv().await.unwrap(), Action::Quit);
    assert_eq!(rx.recv().await.unwrap(), Action::Render);
    assert_eq!(rx.recv().await.unwrap(), Action::Resize(80, 24));
}

/// Verify that `Action::Quit` is received when sent through the channel,
/// simulating the quit flow without needing a terminal.
#[tokio::test]
async fn quit_action_received() {
    let (tx, mut rx) = mpsc::unbounded_channel::<Action>();
    tx.send(Action::Quit).unwrap();

    let action = rx.recv().await.unwrap();
    assert_eq!(action, Action::Quit);
}

/// Verify that Action enum variants display correctly via strum.
#[test]
fn action_display() {
    assert_eq!(Action::Tick.to_string(), "Tick");
    assert_eq!(Action::Render.to_string(), "Render");
    assert_eq!(Action::Quit.to_string(), "Quit");
    assert_eq!(Action::Suspend.to_string(), "Suspend");
    assert_eq!(Action::Resume.to_string(), "Resume");
    assert_eq!(Action::Error("test".to_string()).to_string(), "Error");
}

/// Verify that Action enum supports equality comparison.
#[test]
fn action_equality() {
    assert_eq!(Action::Quit, Action::Quit);
    assert_ne!(Action::Quit, Action::Render);
    assert_eq!(Action::Resize(80, 24), Action::Resize(80, 24));
    assert_ne!(Action::Resize(80, 24), Action::Resize(100, 40));
}

/// Verify that Action can be cloned (required for component dispatch).
#[test]
fn action_clone() {
    let original = Action::Resize(120, 40);
    let cloned = original.clone();
    assert_eq!(original, cloned);
}

/// Verify that dropping the sender closes the channel.
#[tokio::test]
async fn channel_closes_on_sender_drop() {
    let (tx, mut rx) = mpsc::unbounded_channel::<Action>();
    tx.send(Action::Render).unwrap();
    drop(tx);

    // Should receive the queued item.
    assert_eq!(rx.recv().await.unwrap(), Action::Render);
    // Then channel closes.
    assert!(rx.recv().await.is_none());
}
