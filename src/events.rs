//! Agent event types and parsers
//!
//! Split into focused submodules:
//! - `types`: Raw Claude Code event types (serde structs)
//! - `display`: DisplayEvent enum for TUI rendering
//! - `parser`: EventParser for Claude stream-json parsing
//! - `codex_parser`: CodexEventParser for Codex --json JSONL parsing

/// Parses Codex JSONL into display events.
mod codex_parser;
/// Normalizes provider-specific Codex tool names and payloads.
pub(crate) mod codex_tool_payload;
/// Defines provider-neutral events rendered by the terminal UI.
mod display;
/// Parses Claude stream JSON into display events.
mod parser;
/// Defines raw event structures received from agent backends.
mod types;

pub use codex_parser::CodexEventParser;
pub use display::DisplayEvent;
pub use parser::EventParser;
