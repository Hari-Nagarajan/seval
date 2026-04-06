//! Chat module.
//!
//! Manages chat messages, slash commands, conversation state, and the
//! main chat component that integrates streaming AI responses.

pub mod commands;
pub mod component;
pub mod context;
pub(crate) mod input;
pub mod markdown;
pub mod message;
pub mod syntax;
pub(crate) mod verbs;

mod agents;
mod approval;
mod model_picker;
mod persistence;
mod rendering;
mod sessions;
mod tools;

pub use component::Chat;
