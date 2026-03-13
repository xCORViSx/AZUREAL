//! Agent event types and parsers
//!
//! Split into focused submodules:
//! - `types`: Raw Claude Code event types (serde structs)
//! - `display`: DisplayEvent enum for TUI rendering
//! - `parser`: EventParser for Claude stream-json parsing
//! - `codex_parser`: CodexEventParser for Codex --json JSONL parsing

mod codex_parser;
mod display;
mod parser;
mod types;

pub use codex_parser::CodexEventParser;
pub use display::DisplayEvent;
pub use parser::EventParser;
