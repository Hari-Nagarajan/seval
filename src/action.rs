use serde::{Deserialize, Serialize};

/// Central action enum for the event-driven architecture.
///
/// All state changes in the application flow through this enum via channels,
/// following the Elm-style unidirectional data flow pattern.
#[derive(Debug, Clone, PartialEq, Eq, strum::Display, Serialize, Deserialize)]
pub enum Action {
    /// Periodic tick for time-based updates.
    Tick,
    /// Trigger a re-render of the UI.
    Render,
    /// Quit the application.
    Quit,
    /// Suspend the application (Ctrl+Z).
    Suspend,
    /// Resume after suspension.
    Resume,
    /// Terminal was resized to the given dimensions.
    Resize(u16, u16),
    /// An error occurred.
    Error(String),
    /// Advance to the next wizard step.
    WizardNext,
    /// Go back to the previous wizard step.
    WizardBack,
    /// Wizard finished -- save config and exit wizard mode.
    WizardComplete,
    /// User submitted a chat message.
    SendMessage(String),
    /// A text chunk arrived from the AI stream.
    StreamChunk(String),
    /// Mid-stream token update after each agentic turn.
    ///
    /// Sent from `Final` events so the status bar updates live during
    /// multi-turn agentic loops (context %, output tokens).
    TokenUpdate {
        /// Cumulative output tokens so far.
        output_tokens: u64,
        /// This turn's input tokens (= current context window usage).
        context_tokens: u64,
    },
    /// AI stream completed with token usage.
    StreamComplete {
        /// Input tokens consumed (cumulative across all turns).
        input_tokens: u64,
        /// Output tokens generated (cumulative across all turns).
        output_tokens: u64,
        /// Last turn's input tokens (= actual context window usage).
        context_tokens: u64,
    },
    /// AI streaming failed with an error message.
    StreamError(String),
    /// User entered a slash command (raw command string).
    ExecuteCommand(String),
    /// Pasted text from clipboard (via bracketed paste).
    Paste(String),
    /// AI requested a tool call (tool name + JSON arguments).
    ToolCallStart {
        /// Name of the tool being invoked.
        name: String,
        /// JSON-encoded arguments for the tool.
        args_json: String,
    },
    /// Tool execution completed with result.
    ToolResult {
        /// Name of the tool that completed.
        name: String,
        /// Tool output/result text.
        result: String,
        /// Execution duration in milliseconds.
        duration_ms: u64,
    },
    /// Tool execution failed.
    ToolError {
        /// Name of the tool that failed.
        name: String,
        /// Error description.
        error: String,
    },
    /// Tool call was denied (by deny rule, permission mode, or user).
    ToolDenied {
        /// Name of the tool that was denied.
        name: String,
        /// Reason for denial.
        reason: String,
    },
    /// Context window size discovered for current model.
    ContextWindowUpdate(u64),
    /// Compression finished with before/after token stats and summary.
    CompressionComplete {
        /// Token count before compression.
        original_tokens: u64,
        /// Token count after compression.
        compressed_tokens: u64,
        /// AI-generated summary of compressed messages.
        summary: String,
        /// Number of messages that were removed.
        messages_removed: usize,
    },
    /// A new session was created in the database.
    SessionCreated(String),
    /// AI generated a title for the current session.
    SessionTitleGenerated(String),
    /// Session resume requested -- carries `session_id`.
    SessionResume(String),
    /// Session data loaded for resume.
    SessionResumed {
        /// Restored chat messages.
        messages: Vec<crate::chat::message::ChatMessage>,
    },
    /// Session was deleted (carries `session_id`).
    SessionDeleted(String),
    /// Cancel the current streaming/agentic loop (Ctrl+C during streaming).
    CancelStream,
    /// Display an informational system message in the chat.
    ShowSystemMessage(String),
}
