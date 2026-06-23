//! Application state module
//!
//! Split into focused submodules:
//! - `state`: App struct and core state management methods
//! - `types`: Enums and dialog types (BranchDialog, ContextMenu, WorktreeAction, etc.)
//! - `input`: Input handling methods
//! - `terminal`: PTY terminal management
//! - `util`: Utility functions (ANSI stripping, JSON parsing)
//! - `session_parser`: Claude session file parsing

/// System clipboard bridge with an internal fallback for app-local paste.
mod clipboard;
/// Codex JSONL parsing for persisted Codex CLI sessions.
pub(crate) mod codex_session_parser;
/// Context injection and transcript sanitization before prompts are sent.
pub(crate) mod context_injection;
/// Prompt input editing and history navigation methods.
mod input;
/// Session-independent prompt input history.
pub(crate) mod prompt_history;
/// Claude JSONL parsing for persisted Claude Code sessions.
pub(crate) mod session_parser;
/// SQLite-backed transcript and compaction store.
pub(crate) mod session_store;
/// Core application state and state-management modules.
pub(crate) mod state;
/// Embedded PTY terminal state and operations.
mod terminal;
/// Shared UI and domain types for application state.
pub(crate) mod types;
/// Small parsing and formatting utilities used across app modules.
mod util;

pub(crate) use state::health::save_health_scope;
pub use state::{App, DeferredAction, TodoItem, TodoStatus};
pub use types::{BranchDialog, Focus, RunCommand, ViewMode, ViewerMode};
