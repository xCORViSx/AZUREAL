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
pub use types::{Action, HelpSection, KeyCombo, Keybinding, LeaderState};

// Binding arrays
pub use bindings::{
    BRANCH_DIALOG, EDIT_MODE, FILE_TREE, GIT_ACTIONS, GLOBAL, HEALTH_DOCS, HEALTH_GOD_FILES,
    HEALTH_SHARED, INPUT, PICKER, PROJECTS_BROWSE, SESSION, TERMINAL, VIEWER, WORKTREES,
};

// Lookup functions + KeyContext
pub use lookup::{
    lookup_action, lookup_branch_dialog_action, lookup_git_actions_action, lookup_health_action,
    lookup_leader_action, lookup_picker_action, lookup_projects_action, KeyContext,
};

// Hint generators
pub use hints::{
    dialog_footer_hint_pairs, find_key_adaptive, find_key_for_action, find_key_pair,
    git_actions_footer, git_actions_labels, git_files_pane_footer, health_docs_hints,
    health_god_files_hints, help_sections, picker_title,
    projects_browse_hint_pairs, prompt_command_title, prompt_type_title, terminal_command_title,
    terminal_scroll_title, terminal_type_title,
};

// Platform utilities
pub use platform::{is_cmd, is_cmd_key, is_cmd_shift, macos_opt_key};
