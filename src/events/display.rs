//! Display events for TUI rendering
//!
//! Processed events ready for the UI, transformed from raw Claude Code events.

/// Parsed and displayable event for the UI
#[derive(Debug, Clone)]
pub enum DisplayEvent {
    /// System initialization
    Init {
        session_id: String,
        cwd: String,
        model: String,
    },
    /// Hook output
    Hook {
        name: String,
        output: String,
    },
    /// User's message
    UserMessage {
        uuid: String,
        content: String,
    },
    /// Slash command (e.g., /compact, /crt)
    Command {
        name: String,
    },
    /// Context compaction starting indicator
    Compacting,
    /// Context compaction completed indicator
    Compacted,
    /// Assistant text response
    AssistantText {
        uuid: String,
        message_id: String,
        text: String,
    },
    /// Tool being called
    ToolCall {
        uuid: String,
        tool_use_id: String,
        tool_name: String,
        /// Extracted file path if applicable
        file_path: Option<String>,
        /// Full input for display
        input: serde_json::Value,
    },
    /// Tool result (output from a tool call)
    ToolResult {
        tool_use_id: String,
        tool_name: String,
        /// For file-based tools: the path that was operated on
        file_path: Option<String>,
        /// The raw output content from the tool
        content: String,
    },
    /// Session complete
    Complete {
        session_id: String,
        success: bool,
        duration_ms: u64,
        cost_usd: f64,
    },
    /// Error
    Error {
        message: String,
    },
    /// Filtered out (used for rewound/edited messages that were superseded)
    Filtered,
}
