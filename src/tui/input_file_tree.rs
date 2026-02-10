//! FileTree input handling
//!
//! Handles keyboard input when the FileTree panel is focused.
//! Supports navigation, file open, and file actions (add/rename/copy/move/delete).

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Focus};
use crate::app::types::FileTreeAction;
use super::keybindings::{Action, lookup_action};

/// Handle keyboard input for the FileTree panel
pub fn handle_file_tree_input(key: KeyEvent, app: &mut App) -> Result<()> {
    // Copy/Move clipboard mode: allow normal navigation, but intercept Enter to paste
    if matches!(app.file_tree_action, Some(FileTreeAction::Copy(_) | FileTreeAction::Move(_))) {
        return handle_clipboard_input(key, app);
    }
    // Text-input actions (Add, Rename) and Delete confirmation
    if app.file_tree_action.is_some() {
        return handle_action_input(key, app);
    }

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

        // File actions — enter inline input mode
        Some(Action::AddFile) => {
            app.file_tree_action = Some(FileTreeAction::Add(String::new()));
        }
        Some(Action::RenameFile) => {
            // Pre-fill with the current name so user can edit it
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    app.file_tree_action = Some(FileTreeAction::Rename(entry.name.clone()));
                }
            }
        }
        Some(Action::CopyFile) => {
            // Enter clipboard copy mode — store source path, user navigates to target dir
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    app.file_tree_action = Some(FileTreeAction::Copy(entry.path.clone()));
                    app.set_status(format!("Copy: select target dir, Enter to paste"));
                    app.invalidate_file_tree();
                }
            }
        }
        Some(Action::MoveFile) => {
            // Enter clipboard move mode — store source path, user navigates to target dir
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    app.file_tree_action = Some(FileTreeAction::Move(entry.path.clone()));
                    app.set_status(format!("Move: select target dir, Enter to paste"));
                    app.invalidate_file_tree();
                }
            }
        }
        Some(Action::DeleteFile) => {
            if app.file_tree_selected.is_some() {
                app.file_tree_action = Some(FileTreeAction::Delete);
            }
        }

        // Escape or f: close file tree overlay and return to worktrees list
        Some(Action::Escape) => {
            app.show_file_tree = false;
            app.focus = Focus::Worktrees;
            app.invalidate_sidebar();
        }

        // f toggles file tree off (same as Esc but not in keybinding table —
        // lookup_action won't match it since 'f' is now not in FILE_TREE array)
        None if key.modifiers == KeyModifiers::NONE && key.code == KeyCode::Char('f') => {
            app.show_file_tree = false;
            app.focus = Focus::Worktrees;
            app.invalidate_sidebar();
        }

        _ => {}
    }

    Ok(())
}

/// Handle input while a text-input action (Add, Rename) or Delete confirmation is active
fn handle_action_input(key: KeyEvent, app: &mut App) -> Result<()> {
    let action = app.file_tree_action.take().unwrap();

    // Delete confirmation — 'y' confirms, anything else cancels
    if matches!(action, FileTreeAction::Delete) {
        if key.code == KeyCode::Char('y') || key.code == KeyCode::Char('Y') {
            app.file_tree_exec_delete();
        } else {
            app.set_status("Delete cancelled");
        }
        app.invalidate_file_tree();
        return Ok(());
    }

    // Extract variant tag and mutable buffer from text-input actions (Add=0, Rename=1)
    let (tag, mut buf) = match action {
        FileTreeAction::Add(b) => (0u8, b),
        FileTreeAction::Rename(b) => (1, b),
        _ => unreachable!(),
    };
    let rebuild = |t: u8, b: String| -> FileTreeAction {
        if t == 0 { FileTreeAction::Add(b) } else { FileTreeAction::Rename(b) }
    };

    match key.code {
        KeyCode::Esc => { app.set_status("Cancelled"); }
        KeyCode::Enter => {
            let input = buf.trim().to_string();
            if input.is_empty() {
                app.set_status("Cancelled (empty input)");
            } else if tag == 0 {
                app.file_tree_exec_add(&input);
            } else {
                app.file_tree_exec_rename(&input);
            }
        }
        KeyCode::Backspace => { buf.pop(); app.file_tree_action = Some(rebuild(tag, buf)); }
        KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
            buf.clear(); app.file_tree_action = Some(rebuild(tag, buf));
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            buf.push(c); app.file_tree_action = Some(rebuild(tag, buf));
        }
        _ => { app.file_tree_action = Some(rebuild(tag, buf)); }
    }

    app.invalidate_file_tree();
    Ok(())
}

/// Handle input in clipboard Copy/Move mode — normal navigation plus Enter to paste
fn handle_clipboard_input(key: KeyEvent, app: &mut App) -> Result<()> {
    // Esc cancels the clipboard operation
    if key.code == KeyCode::Esc {
        app.file_tree_action = None;
        app.set_status("Cancelled");
        app.invalidate_file_tree();
        return Ok(());
    }

    // Enter pastes into the selected dir (or selected file's parent dir)
    if key.code == KeyCode::Enter {
        let action = app.file_tree_action.take().unwrap();
        let Some(idx) = app.file_tree_selected else {
            app.set_status("No target selected");
            return Ok(());
        };
        let Some(entry) = app.file_tree_entries.get(idx) else {
            app.set_status("No target selected");
            return Ok(());
        };
        // Target directory: if selected entry is a dir use it, otherwise use its parent
        let target_dir = if entry.is_dir {
            entry.path.clone()
        } else {
            entry.path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| entry.path.clone())
        };
        match action {
            FileTreeAction::Copy(src) => app.file_tree_exec_copy_to(&src, &target_dir),
            FileTreeAction::Move(src) => app.file_tree_exec_move_to(&src, &target_dir),
            _ => unreachable!(),
        }
        app.invalidate_file_tree();
        return Ok(());
    }

    // All other keys: normal file tree navigation (keep the clipboard action active)
    let action = lookup_action(Focus::FileTree, key.modifiers, key.code, false, false, false);
    match action {
        Some(Action::NavDown) => app.file_tree_next(),
        Some(Action::NavUp) => app.file_tree_prev(),
        Some(Action::GoToTop) => app.file_tree_first_sibling(),
        Some(Action::GoToBottom) => app.file_tree_last_sibling(),
        Some(Action::NavRight) | Some(Action::OpenFile) | Some(Action::ToggleDir) => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx).cloned() {
                    if entry.is_dir && !app.file_tree_expanded.contains(&entry.path) {
                        app.toggle_file_tree_dir();
                    }
                }
            }
        }
        Some(Action::NavLeft) => {
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx).cloned() {
                    if entry.is_dir && app.file_tree_expanded.contains(&entry.path) {
                        app.toggle_file_tree_dir();
                    } else if let Some(parent) = entry.path.parent() {
                        let parent_path = parent.to_path_buf();
                        if let Some(pi) = app.file_tree_entries.iter().position(|e| e.path == parent_path && e.is_dir) {
                            if app.file_tree_expanded.contains(&parent_path) {
                                app.file_tree_selected = Some(pi);
                                app.toggle_file_tree_dir();
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
    app.invalidate_file_tree();
    Ok(())
}
