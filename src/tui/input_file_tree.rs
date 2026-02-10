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
    // If a file action is in progress, route all input there
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
            // Pre-fill with current name + "_copy" suffix
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    let name = &entry.name;
                    let default = if let Some(dot) = name.rfind('.') {
                        format!("{}_copy{}", &name[..dot], &name[dot..])
                    } else {
                        format!("{}_copy", name)
                    };
                    app.file_tree_action = Some(FileTreeAction::Copy(default));
                }
            }
        }
        Some(Action::MoveFile) => {
            // Pre-fill with current name
            if let Some(idx) = app.file_tree_selected {
                if let Some(entry) = app.file_tree_entries.get(idx) {
                    app.file_tree_action = Some(FileTreeAction::Move(entry.name.clone()));
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

/// Handle input while a file action is in progress (inline text input or confirmation)
fn handle_action_input(key: KeyEvent, app: &mut App) -> Result<()> {
    let action = app.file_tree_action.take().unwrap();

    // Delete confirmation is a special case — 'y' confirms, anything else cancels
    if matches!(action, FileTreeAction::Delete) {
        if key.code == KeyCode::Char('y') || key.code == KeyCode::Char('Y') {
            app.file_tree_exec_delete();
        } else {
            app.set_status("Delete cancelled");
        }
        app.invalidate_file_tree();
        return Ok(());
    }

    // Extract variant tag and mutable buffer from text-input actions
    let (tag, mut buf) = match action {
        FileTreeAction::Add(b) => (0u8, b),
        FileTreeAction::Rename(b) => (1, b),
        FileTreeAction::Copy(b) => (2, b),
        FileTreeAction::Move(b) => (3, b),
        FileTreeAction::Delete => unreachable!(),
    };

    // Reconstruct the enum variant from tag + buffer
    let rebuild = |t: u8, b: String| -> FileTreeAction {
        match t { 0 => FileTreeAction::Add(b), 1 => FileTreeAction::Rename(b),
                   2 => FileTreeAction::Copy(b), _ => FileTreeAction::Move(b) }
    };

    match key.code {
        KeyCode::Esc => { app.set_status("Cancelled"); }
        KeyCode::Enter => {
            let input = buf.trim().to_string();
            if input.is_empty() {
                app.set_status("Cancelled (empty input)");
            } else {
                match tag {
                    0 => app.file_tree_exec_add(&input),
                    1 => app.file_tree_exec_rename(&input),
                    2 => app.file_tree_exec_copy(&input),
                    _ => app.file_tree_exec_move(&input),
                }
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
