//! FileTree input handling
//!
//! Handles keyboard input when the FileTree panel is focused.
//! Supports navigation, file open, and file actions (add/rename/copy/move/delete).

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;
use crate::app::types::FileTreeAction;
use super::keybindings::{Action, KeyContext, lookup_action};

/// Names matching the options overlay (same order as draw_file_tree.rs)
const FT_OPTIONS: &[&str] = &[".git", ".claude", ".azureal", "worktrees", ".DS_Store"];

/// Handle keyboard input for the FileTree panel.
/// ALL command keybindings are resolved by lookup_action() in event_loop.rs BEFORE
/// this is called. This handler only receives unresolved keys — meaning only
/// clipboard mode (Copy/Move paste target selection), text-input actions
/// (Add filename, Rename, Delete confirmation), and options mode reach here.
pub fn handle_file_tree_input(key: KeyEvent, app: &mut App) -> Result<()> {
    // Options overlay mode: j/k navigate, Space/Enter toggle, Esc/O closes
    if app.file_tree_options_mode {
        return handle_options_input(key, app);
    }

    // Copy/Move clipboard mode: allow navigation + Enter to paste
    if matches!(app.file_tree_action, Some(FileTreeAction::Copy(_) | FileTreeAction::Move(_))) {
        return handle_clipboard_input(key, app);
    }
    // Text-input actions (Add, Rename) and Delete confirmation
    if app.file_tree_action.is_some() {
        return handle_action_input(key, app);
    }

    // All file tree command bindings resolved upstream — nothing to handle here
    Ok(())
}

/// Handle input in file tree options overlay (hidden directory toggles)
fn handle_options_input(key: KeyEvent, app: &mut App) -> Result<()> {
    match key.code {
        // Navigate options list with wrapping
        KeyCode::Char('j') | KeyCode::Down => {
            app.file_tree_options_selected = (app.file_tree_options_selected + 1) % FT_OPTIONS.len();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.file_tree_options_selected = app.file_tree_options_selected.checked_sub(1)
                .unwrap_or(FT_OPTIONS.len() - 1);
        }
        // Toggle the selected directory's hidden state
        KeyCode::Char(' ') | KeyCode::Enter => {
            let name = FT_OPTIONS[app.file_tree_options_selected].to_string();
            if app.file_tree_hidden_dirs.contains(&name) {
                app.file_tree_hidden_dirs.remove(&name);
            } else {
                app.file_tree_hidden_dirs.insert(name);
            }
            app.refresh_file_tree();
        }
        // Close options overlay
        KeyCode::Esc => {
            app.file_tree_options_mode = false;
        }
        // Shift+O also closes (same key that opened it)
        KeyCode::Char('O') if key.modifiers == KeyModifiers::SHIFT || key.modifiers == KeyModifiers::NONE => {
            app.file_tree_options_mode = false;
        }
        _ => {}
    }
    app.invalidate_file_tree();
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
    let ctx = KeyContext::from_app(app);
    let action = lookup_action(&ctx, key.modifiers, key.code);
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
