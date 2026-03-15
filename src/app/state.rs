//! Application state module
//!
//! Contains the App struct and all methods for managing application state,
//! session discovery, output processing, and UI coordination.
//!
//! Split into submodules for organization:
//! - `app`: App struct definition and initialization
//! - `load`: Session loading and discovery
//! - `sessions`: Session navigation and CRUD
//! - `output`: Output processing
//! - `scroll`: Scroll operations
//! - `claude`: Claude session handling
//! - `file_browser`: File tree and viewer
//! - `health`: Worktree Health (god files + documentation coverage)
//! - `ui`: Focus, dialogs, menus, wizard
//! - `helpers`: Utility functions

mod app;
mod claude;
mod file_browser;
pub(crate) mod health;
pub(crate) mod helpers;
pub(crate) mod load;
mod output;
pub(crate) mod project_snapshot;
mod scroll;
mod session_names;
mod sessions;
mod ui;
mod viewer_edit;

// Re-export types used by submodules
use crate::claude::AgentEvent;
use crate::events::DisplayEvent;

// Re-export FileTreeEntry for helpers module
pub use crate::app::types::FileTreeEntry;

// Re-export App, todo types, DeferredAction, and model helpers as public
pub use app::{App, DeferredAction, TodoItem, TodoStatus};
pub use app::model::{backend_for_model, default_model};

