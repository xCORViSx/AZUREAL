//! Claude Code stream-json event types and parser
//!
//! Split into focused submodules:
//! - `types`: Raw Claude Code event types (serde structs)
//! - `display`: DisplayEvent enum for TUI rendering
//! - `parser`: EventParser for stream-json parsing

mod display;
mod parser;
mod types;

pub use display::DisplayEvent;
pub use parser::EventParser;
pub use types::*;
