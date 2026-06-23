//! Application state module
//!
//! Contains the App struct and all methods for managing application state,
//! session discovery, output processing, and UI coordination.
//!
//! Split into submodules for organization:
//! - `app`: App struct definition and initialization
//! - `load`: Session loading, discovery, and file monitoring (4 submodules)
//! - `sessions`: Session navigation and CRUD
//! - `output`: Output processing
//! - `scroll`: Scroll operations
//! - `claude`: Claude session handling (3 submodules: event_handling, process_lifecycle, store_ops)
//! - `file_browser`: File tree and viewer
//! - `health`: Worktree Health (god files + documentation coverage)
//! - `ui`: Focus, dialogs, menus, wizard
//! - `helpers`: Utility functions

/// Core app state type and focused state submodules.
mod app;
/// Agent lifecycle, event handling, and session persistence behavior.
mod claude;
/// File tree browsing and viewer state behavior.
mod file_browser;
/// Worktree health checks and remediation prompts.
pub(crate) mod health;
/// Shared state helpers used by health and UI modules.
pub(crate) mod helpers;
/// Issue-session state and issue workflow helpers.
pub(crate) mod issues;
/// Session, worktree, and persisted output loading behavior.
pub(crate) mod load;
/// Session output buffer and rendered text state behavior.
mod output;
/// Per-project UI snapshot state used during project switching.
pub(crate) mod project_snapshot;
/// Hidden context payload assembly for continued prompts.
mod prompt_context;
/// Session, viewer, file tree, and terminal scrolling behavior.
mod scroll;
/// Persisted session display-name helpers.
mod session_names;
/// Session navigation, creation, deletion, and store recovery behavior.
mod sessions;
/// UI focus, dialogs, menus, and project switching behavior.
mod ui;
/// Viewer text editing behavior.
mod viewer_edit;

// Re-export types used by submodules
use crate::claude::AgentEvent;
use crate::events::DisplayEvent;

// Re-export FileTreeEntry for helpers module
pub use crate::app::types::FileTreeEntry;

// Re-export App, todo types, DeferredAction, and model helpers as public
pub use app::model::{backend_for_model, default_model, model_color};
pub use app::{
    App, AutoPromptKey, AutoPromptTarget, CompactionJob, DeferredAction, TodoItem, TodoStatus,
};
