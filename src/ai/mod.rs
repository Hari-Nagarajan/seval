//! AI module.
//!
//! Manages the AI client, provider abstraction, streaming responses, and
//! system prompt loading.

pub mod codex_auth;
pub mod codex_model;
pub mod compression;
pub mod provider;
pub mod streaming;
pub mod system_prompt;

pub use provider::AiProvider;
pub use streaming::spawn_streaming_chat;
pub use system_prompt::load_system_prompt;
