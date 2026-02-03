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
//! - `ui`: Focus, dialogs, menus, wizard
//! - `helpers`: Utility functions

mod app;
mod claude;
mod file_browser;
mod helpers;
mod load;
mod output;
mod scroll;
mod session_names;
mod sessions;
mod ui;
mod viewer_edit;

// Re-export types used by submodules
use crate::claude::ClaudeEvent;
use crate::events::DisplayEvent;

// Re-export FileTreeEntry for helpers module
pub use crate::app::types::FileTreeEntry;

// Re-export App as the main public type
pub use app::App;

// Re-export helper functions for external use (used by sessions.rs)
pub(crate) use helpers::{generate_session_name, sanitize_for_branch};
