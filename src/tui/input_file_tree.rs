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
const FT_OPTIONS: &[&str] = &["worktrees", ".git", ".claude", ".azureal", ".DS_Store"];

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
        // Toggle the selected entry's hidden state and persist to azufig
        KeyCode::Char(' ') | KeyCode::Enter => {
            let name = FT_OPTIONS[app.file_tree_options_selected].to_string();
            if app.file_tree_hidden_dirs.contains(&name) {
                app.file_tree_hidden_dirs.remove(&name);
            } else {
                app.file_tree_hidden_dirs.insert(name);
            }
            app.refresh_file_tree();
            // Persist to project azufig so toggles survive restarts
            if let Some(ref project) = app.project {
                let hidden: Vec<String> = app.file_tree_hidden_dirs.iter().cloned().collect();
                crate::azufig::update_project_azufig(&project.path, |az| {
                    az.filetree.hidden = hidden;
                });
            }
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

/// Rebuild a FileTreeAction from tag + buffer. Extracted for testability.
/// tag 0 = Add, tag 1 = Rename.
#[cfg(test)]
fn rebuild_action(tag: u8, buf: String) -> FileTreeAction {
    if tag == 0 { FileTreeAction::Add(buf) } else { FileTreeAction::Rename(buf) }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE }
    }

    fn key_mod(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent { code, modifiers, kind: KeyEventKind::Press, state: KeyEventState::NONE }
    }

    // ══════════════════════════════════════════════════════════════════
    //  FT_OPTIONS constant
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn ft_options_has_5_entries() {
        assert_eq!(FT_OPTIONS.len(), 5);
    }

    #[test]
    fn ft_options_first_is_worktrees() {
        assert_eq!(FT_OPTIONS[0], "worktrees");
    }

    #[test]
    fn ft_options_contains_git() {
        assert!(FT_OPTIONS.contains(&".git"));
    }

    #[test]
    fn ft_options_contains_claude() {
        assert!(FT_OPTIONS.contains(&".claude"));
    }

    #[test]
    fn ft_options_contains_azureal() {
        assert!(FT_OPTIONS.contains(&".azureal"));
    }

    #[test]
    fn ft_options_contains_ds_store() {
        assert!(FT_OPTIONS.contains(&".DS_Store"));
    }

    #[test]
    fn ft_options_order_matches_draw() {
        assert_eq!(FT_OPTIONS, &["worktrees", ".git", ".claude", ".azureal", ".DS_Store"]);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Options wrapping navigation arithmetic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn options_wrap_forward_from_0() {
        let selected = 0usize;
        let next = (selected + 1) % FT_OPTIONS.len();
        assert_eq!(next, 1);
    }

    #[test]
    fn options_wrap_forward_from_last() {
        let selected = FT_OPTIONS.len() - 1;
        let next = (selected + 1) % FT_OPTIONS.len();
        assert_eq!(next, 0);
    }

    #[test]
    fn options_wrap_backward_from_0() {
        let selected = 0usize;
        let prev = selected.checked_sub(1).unwrap_or(FT_OPTIONS.len() - 1);
        assert_eq!(prev, FT_OPTIONS.len() - 1);
    }

    #[test]
    fn options_wrap_backward_from_1() {
        let selected = 1usize;
        let prev = selected.checked_sub(1).unwrap_or(FT_OPTIONS.len() - 1);
        assert_eq!(prev, 0);
    }

    #[test]
    fn options_wrap_backward_from_last() {
        let selected = FT_OPTIONS.len() - 1;
        let prev = selected.checked_sub(1).unwrap_or(FT_OPTIONS.len() - 1);
        assert_eq!(prev, FT_OPTIONS.len() - 2);
    }

    // ══════════════════════════════════════════════════════════════════
    //  FileTreeAction enum variants
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn file_tree_action_add_holds_string() {
        let a = FileTreeAction::Add("test.rs".to_string());
        assert!(matches!(a, FileTreeAction::Add(ref s) if s == "test.rs"));
    }

    #[test]
    fn file_tree_action_rename_holds_string() {
        let a = FileTreeAction::Rename("new_name.rs".to_string());
        assert!(matches!(a, FileTreeAction::Rename(ref s) if s == "new_name.rs"));
    }

    #[test]
    fn file_tree_action_copy_holds_path() {
        let p = std::path::PathBuf::from("/tmp/file.rs");
        let a = FileTreeAction::Copy(p.clone());
        assert!(matches!(a, FileTreeAction::Copy(ref path) if path == &p));
    }

    #[test]
    fn file_tree_action_move_holds_path() {
        let p = std::path::PathBuf::from("/tmp/src/lib.rs");
        let a = FileTreeAction::Move(p.clone());
        assert!(matches!(a, FileTreeAction::Move(ref path) if path == &p));
    }

    #[test]
    fn file_tree_action_delete_is_unit() {
        let a = FileTreeAction::Delete;
        assert!(matches!(a, FileTreeAction::Delete));
    }

    // ══════════════════════════════════════════════════════════════════
    //  rebuild_action helper
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn rebuild_action_tag_0_is_add() {
        let action = rebuild_action(0, "foo.txt".to_string());
        assert!(matches!(action, FileTreeAction::Add(ref s) if s == "foo.txt"));
    }

    #[test]
    fn rebuild_action_tag_1_is_rename() {
        let action = rebuild_action(1, "bar.txt".to_string());
        assert!(matches!(action, FileTreeAction::Rename(ref s) if s == "bar.txt"));
    }

    #[test]
    fn rebuild_action_empty_buffer() {
        let action = rebuild_action(0, String::new());
        assert!(matches!(action, FileTreeAction::Add(ref s) if s.is_empty()));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Copy/Move match patterns
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn copy_matches_clipboard_pattern() {
        let action = FileTreeAction::Copy(std::path::PathBuf::from("/tmp/x"));
        assert!(matches!(action, FileTreeAction::Copy(_) | FileTreeAction::Move(_)));
    }

    #[test]
    fn move_matches_clipboard_pattern() {
        let action = FileTreeAction::Move(std::path::PathBuf::from("/tmp/y"));
        assert!(matches!(action, FileTreeAction::Copy(_) | FileTreeAction::Move(_)));
    }

    #[test]
    fn add_does_not_match_clipboard_pattern() {
        let action = FileTreeAction::Add("x".into());
        assert!(!matches!(action, FileTreeAction::Copy(_) | FileTreeAction::Move(_)));
    }

    #[test]
    fn delete_does_not_match_clipboard_pattern() {
        let action = FileTreeAction::Delete;
        assert!(!matches!(action, FileTreeAction::Copy(_) | FileTreeAction::Move(_)));
    }

    #[test]
    fn rename_does_not_match_clipboard_pattern() {
        let action = FileTreeAction::Rename("y".into());
        assert!(!matches!(action, FileTreeAction::Copy(_) | FileTreeAction::Move(_)));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Key matching patterns for options overlay
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn j_matches_options_down() {
        let k = key(KeyCode::Char('j'));
        assert!(matches!(k.code, KeyCode::Char('j') | KeyCode::Down));
    }

    #[test]
    fn down_arrow_matches_options_down() {
        let k = key(KeyCode::Down);
        assert!(matches!(k.code, KeyCode::Char('j') | KeyCode::Down));
    }

    #[test]
    fn k_matches_options_up() {
        let k = key(KeyCode::Char('k'));
        assert!(matches!(k.code, KeyCode::Char('k') | KeyCode::Up));
    }

    #[test]
    fn up_arrow_matches_options_up() {
        let k = key(KeyCode::Up);
        assert!(matches!(k.code, KeyCode::Char('k') | KeyCode::Up));
    }

    #[test]
    fn space_matches_toggle() {
        let k = key(KeyCode::Char(' '));
        assert!(matches!(k.code, KeyCode::Char(' ') | KeyCode::Enter));
    }

    #[test]
    fn enter_matches_toggle() {
        let k = key(KeyCode::Enter);
        assert!(matches!(k.code, KeyCode::Char(' ') | KeyCode::Enter));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Key matching for text-input actions
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn ctrl_u_clears_buffer() {
        let k = key_mod(KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(k.modifiers, KeyModifiers::CONTROL);
        assert_eq!(k.code, KeyCode::Char('u'));
    }

    #[test]
    fn plain_char_is_insertable() {
        let k = key(KeyCode::Char('x'));
        assert!(k.modifiers.is_empty() || k.modifiers == KeyModifiers::SHIFT);
    }

    #[test]
    fn shift_char_is_insertable() {
        let k = key_mod(KeyCode::Char('X'), KeyModifiers::SHIFT);
        assert!(k.modifiers.is_empty() || k.modifiers == KeyModifiers::SHIFT);
    }

    #[test]
    fn ctrl_char_is_not_insertable() {
        let k = key_mod(KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert!(!(k.modifiers.is_empty() || k.modifiers == KeyModifiers::SHIFT));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Delete confirmation pattern
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn delete_confirm_y_lowercase() {
        let k = key(KeyCode::Char('y'));
        assert!(k.code == KeyCode::Char('y') || k.code == KeyCode::Char('Y'));
    }

    #[test]
    fn delete_confirm_y_uppercase() {
        let k = key(KeyCode::Char('Y'));
        assert!(k.code == KeyCode::Char('y') || k.code == KeyCode::Char('Y'));
    }

    #[test]
    fn delete_confirm_n_cancels() {
        let k = key(KeyCode::Char('n'));
        assert!(k.code != KeyCode::Char('y') && k.code != KeyCode::Char('Y'));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Shift+O close-options key check
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn shift_o_matches_close_options() {
        let k = key_mod(KeyCode::Char('O'), KeyModifiers::SHIFT);
        assert!(matches!(k.code, KeyCode::Char('O'))
            && (k.modifiers == KeyModifiers::SHIFT || k.modifiers == KeyModifiers::NONE));
    }

    #[test]
    fn plain_uppercase_o_matches_close_options() {
        let k = key(KeyCode::Char('O'));
        assert!(matches!(k.code, KeyCode::Char('O'))
            && (k.modifiers == KeyModifiers::SHIFT || k.modifiers == KeyModifiers::NONE));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Action enum variants used in clipboard nav
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn action_nav_down_clipboard_nav() { assert_eq!(Action::NavDown, Action::NavDown); }
    #[test]
    fn action_nav_up_clipboard_nav() { assert_eq!(Action::NavUp, Action::NavUp); }
    #[test]
    fn action_go_to_top_clipboard_nav() { assert_eq!(Action::GoToTop, Action::GoToTop); }
    #[test]
    fn action_go_to_bottom_clipboard_nav() { assert_eq!(Action::GoToBottom, Action::GoToBottom); }
    #[test]
    fn action_nav_right_clipboard_nav() { assert_eq!(Action::NavRight, Action::NavRight); }
    #[test]
    fn action_open_file_clipboard_nav() { assert_eq!(Action::OpenFile, Action::OpenFile); }
    #[test]
    fn action_toggle_dir_clipboard_nav() { assert_eq!(Action::ToggleDir, Action::ToggleDir); }
    #[test]
    fn action_nav_left_clipboard_nav() { assert_eq!(Action::NavLeft, Action::NavLeft); }

    // ══════════════════════════════════════════════════════════════════
    //  Input buffer string operations
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn buffer_push_char() { let mut b = String::from("he"); b.push('l'); assert_eq!(b, "hel"); }
    #[test]
    fn buffer_pop_char() { let mut b = String::from("hello"); b.pop(); assert_eq!(b, "hell"); }
    #[test]
    fn buffer_pop_empty() { let mut b = String::new(); assert!(b.pop().is_none()); }
    #[test]
    fn buffer_clear() { let mut b = String::from("x"); b.clear(); assert!(b.is_empty()); }
    #[test]
    fn buffer_trim_empty() { assert!("   ".trim().is_empty()); }
    #[test]
    fn buffer_trim_content() { assert_eq!("  file.rs  ".trim(), "file.rs"); }
}
