//! Shared update types for TUI communication
//!
//! This module defines update types that are used by both
//! the agent loop and the TUI, avoiding circular
//! dependencies between the two modules.

use std::sync::Arc;

/// Updates sent from agent executor to TUI
///
/// This type is used by agent::TUIExecutor to send
/// updates to the TUI through a channel.
#[derive(Debug, Clone)]
pub enum TuiUpdate {
    /// Action message (e.g., "bash: ls -la")
    ActionMessage(Arc<str>),

    /// Progress message (e.g., "⚙ Reading file.txt...")
    ToolProgress(Arc<str>),

    /// Output from tool/bash execution
    OutputMessage(Arc<String>),

    /// Error message from tool/LLM execution
    ErrorMessage(Arc<str>),

    /// Tool execution needs user approval
    NeedsApproval {
        action_type: Arc<str>,
        content: Arc<String>,
        is_tool: bool,
    },

    /// User submitted a message (from TUI to agent)
    UserMessage {
        content: Arc<String>,
    },

    /// User approved a pending action
    ActionApproved {
        action_type: String,
        content: Arc<String>,
    },

    /// User rejected a pending action
    ActionRejected,

    /// Agent interruption signal (Ctrl+C)
    InterruptAgent,

    /// Streaming content from LLM (tokens as they arrive)
    StreamingContent {
        role: Arc<str>,  // "user" or "assistant"
        content: Arc<str>,  // Streaming token/chunk
    },

    /// Thinking content (wrapped in <thinking> tags)
    ThinkingContent(Arc<str>),

    /// Streaming complete (message finished)
    StreamingComplete {
        role: Arc<str>,
    },

    /// Token usage update
    UsageUpdate {
        input_tokens: u32,
        output_tokens: u32,
        cost: f64,
    },
}
