//! Centralized keybinding definitions
//!
//! All keybindings are defined once here and referenced by:
//! - Input handlers (for executing actions)
//! - Help dialog (for display)
//!
//! Split into focused submodules:
//! - `types`: Core types — `KeyCombo`, `Action`, `Keybinding`, `HelpSection`
//! - `bindings`: Static binding arrays for every context and modal panel
//! - `lookup`: Key-to-action resolution (`lookup_action` + modal lookups)
//! - `hints`: UI hint generators for title bars, footers, and help overlay
//! - `platform`: macOS ⌥+letter unicode remapping

// Some re-exports are only accessed via the module path (e.g., `keybindings::HEALTH_SHARED`),
// which Rust's unused-import lint doesn't track — suppress the false positives.
#![allow(unused_imports)]

mod bindings;
mod hints;
mod lookup;
mod platform;
mod types;

// Re-export all public items so existing `use super::keybindings::*` paths work unchanged.

// Types
pub use types::{KeyCombo, Action, Keybinding, HelpSection};

// Binding arrays
pub use bindings::{
    GLOBAL, WORKTREES, FILE_TREE, VIEWER, EDIT_MODE, SESSION, INPUT, TERMINAL,
    HEALTH_SHARED, HEALTH_GOD_FILES, HEALTH_DOCS,
    GIT_ACTIONS, PROJECTS_BROWSE, PICKER, BRANCH_DIALOG,
};

// Lookup functions + KeyContext
pub use lookup::{
    KeyContext, lookup_action,
    lookup_health_action, lookup_git_actions_action,
    lookup_projects_action, lookup_picker_action, lookup_branch_dialog_action,
};

// Hint generators
pub use hints::{
    help_sections,
    prompt_type_title, prompt_command_title,
    terminal_type_title, terminal_command_title, terminal_scroll_title,
    health_god_files_hints, health_docs_hints,
    git_actions_labels, git_actions_footer,
    projects_browse_hint_pairs, picker_title, dialog_footer_hint_pairs,
    find_key_for_action, find_key_pair,
};

// Platform utilities
pub use platform::{macos_opt_key, is_cmd, is_cmd_shift};
