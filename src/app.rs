//! Application state module
//!
//! Split into focused submodules:
//! - `state`: App struct and core state management methods
//! - `types`: Enums and dialog types (BranchDialog, ContextMenu, SessionAction, etc.)
//! - `input`: Input handling methods
//! - `terminal`: PTY terminal management
//! - `util`: Utility functions (ANSI stripping, JSON parsing)
//! - `session_parser`: Claude session file parsing

mod input;
mod session_parser;
mod state;
mod terminal;
pub(crate) mod types;
mod util;

pub use state::App;
pub use types::{BranchDialog, Focus, RunCommand, SessionAction, ViewMode, ViewerMode};
