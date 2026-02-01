//! FileTree input handling
//!
//! Handles keyboard input when the FileTree panel is focused.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Focus};

/// Handle keyboard input for the FileTree panel
pub fn handle_file_tree_input(key: KeyEvent, app: &mut App) -> Result<()> {
    match key.code {
        // Navigation: j/k or arrow keys
        KeyCode::Char('j') | KeyCode::Down => {
            app.file_tree_next();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.file_tree_prev();
        }

        // Enter: expand directory or load file into viewer
        KeyCode::Enter => {
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

        // h/l or left/right: collapse/expand directory (works from any item in that dir)
        KeyCode::Char('h') | KeyCode::Left => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx).cloned() {
                    if entry.is_dir && app.file_tree_expanded.contains(&entry.path) {
                        // Collapse this directory
                        app.toggle_file_tree_dir();
                    } else if let Some(parent) = entry.path.parent() {
                        // Find parent dir in entries and collapse it
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
        KeyCode::Char('l') | KeyCode::Right => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx).cloned() {
                    if entry.is_dir && !app.file_tree_expanded.contains(&entry.path) {
                        // Expand this directory
                        app.toggle_file_tree_dir();
                    } else if !entry.is_dir {
                        // On a file: find parent dir and expand it (usually already expanded)
                        // This is a no-op if parent is expanded, but allows intuitive behavior
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

        // Space: toggle directory expand/collapse
        KeyCode::Char(' ') => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    if entry.is_dir {
                        app.toggle_file_tree_dir();
                    }
                }
            }
        }

        // Escape: unfocus
        KeyCode::Esc => {
            app.focus = Focus::Worktrees;
        }

        _ => {}
    }

    Ok(())
}
