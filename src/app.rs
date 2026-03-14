//! Application state module
//!
//! Split into focused submodules:
//! - `state`: App struct and core state management methods
//! - `types`: Enums and dialog types (BranchDialog, ContextMenu, WorktreeAction, etc.)
//! - `input`: Input handling methods
//! - `terminal`: PTY terminal management
//! - `util`: Utility functions (ANSI stripping, JSON parsing)
//! - `session_parser`: Claude session file parsing

pub(crate) mod codex_session_parser;
pub(crate) mod context_injection;
mod input;
pub(crate) mod session_parser;
pub(crate) mod session_store;
pub(crate) mod state;
mod terminal;
pub(crate) mod types;
mod util;

pub use state::{App, DeferredAction, TodoItem, TodoStatus};
pub(crate) use state::health::save_health_scope;
pub use types::{BranchDialog, Focus, RunCommand, ViewMode, ViewerMode};
