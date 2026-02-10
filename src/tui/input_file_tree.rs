//! FileTree input handling
//!
//! Handles keyboard input when the FileTree panel is focused.

use anyhow::Result;
use crossterm::event::KeyEvent;

use crate::app::{App, Focus};
use super::keybindings::{Action, lookup_action};

/// Handle keyboard input for the FileTree panel
pub fn handle_file_tree_input(key: KeyEvent, app: &mut App) -> Result<()> {
    // Use centralized keybindings lookup (⌥↑/⌥↓ → GoToTop/GoToBottom defined in FILE_TREE array)
    let action = lookup_action(Focus::FileTree, key.modifiers, key.code, false, false, false);

    match action {
        // Navigation: j/k or arrow keys
        Some(Action::NavDown) => app.file_tree_next(),
        Some(Action::NavUp) => app.file_tree_prev(),
        // ⌥↑/⌥↓: jump to first/last sibling in current folder
        Some(Action::GoToTop) => app.file_tree_first_sibling(),
        Some(Action::GoToBottom) => app.file_tree_last_sibling(),

        // Enter: expand directory or load file into viewer
        Some(Action::OpenFile) => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    if entry.is_dir {
                        app.toggle_file_tree_dir();
                    } else {
                        app.load_file_into_viewer();
                        app.focus = Focus::Viewer;
                    }
                }
            }
        }

        // Space: toggle directory expand/collapse
        Some(Action::ToggleDir) => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    if entry.is_dir {
                        app.toggle_file_tree_dir();
                    }
                }
            }
        }

        // h/l or left/right: collapse/expand directory
        Some(Action::NavLeft) => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx).cloned() {
                    if entry.is_dir && app.file_tree_expanded.contains(&entry.path) {
                        app.toggle_file_tree_dir();
                    } else if let Some(parent) = entry.path.parent() {
                        let parent_path = parent.to_path_buf();
                        if let Some(parent_idx) = app.file_tree_entries.iter().position(|e| e.path == parent_path && e.is_dir) {
                            if app.file_tree_expanded.contains(&parent_path) {
                                app.file_tree_selected = Some(parent_idx);
                                app.toggle_file_tree_dir();
                            }
                        }
                    }
                }
            }
        }
        Some(Action::NavRight) => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx).cloned() {
                    if entry.is_dir && !app.file_tree_expanded.contains(&entry.path) {
                        app.toggle_file_tree_dir();
                    } else if !entry.is_dir {
                        if let Some(parent) = entry.path.parent() {
                            let parent_path = parent.to_path_buf();
                            if let Some(parent_idx) = app.file_tree_entries.iter().position(|e| e.path == parent_path && e.is_dir) {
                                if !app.file_tree_expanded.contains(&parent_path) {
                                    app.file_tree_selected = Some(parent_idx);
                                    app.toggle_file_tree_dir();
                                }
                            }
                        }
                    }
                }
            }
        }

        // Escape: close file tree overlay and return to worktrees list
        Some(Action::Escape) => {
            app.show_file_tree = false;
            app.focus = Focus::Worktrees;
            app.invalidate_sidebar();
        }

        _ => {}
    }

    Ok(())
}
