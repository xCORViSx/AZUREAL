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

        // h/l or left/right: collapse/expand directory
        KeyCode::Char('h') | KeyCode::Left => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    if entry.is_dir && app.file_tree_expanded.contains(&entry.path) {
                        app.toggle_file_tree_dir();
                    }
                }
            }
        }
        KeyCode::Char('l') | KeyCode::Right => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    if entry.is_dir && !app.file_tree_expanded.contains(&entry.path) {
                        app.toggle_file_tree_dir();
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
            app.focus = Focus::Sessions;
        }

        _ => {}
    }

    Ok(())
}
