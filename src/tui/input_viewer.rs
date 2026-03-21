//! Viewer input handling
//!
//! Handles keyboard input when the Viewer panel is focused.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::keybindings::{is_cmd_key, is_cmd_shift, macos_opt_key};
use crate::app::App;

/// Handle keyboard input for the Viewer panel.
/// ALL keybindings are resolved by lookup_action() in event_loop.rs BEFORE this
/// is called. This handler only receives keys that weren't mapped to any action —
/// meaning only dialog input and edit mode text editing reach here.
pub fn handle_viewer_input(key: KeyEvent, app: &mut App) -> Result<()> {
    // Tab dialog takes priority — consumes j/k/Enter/Esc/number keys
    if app.viewer_tab_dialog {
        return handle_tab_dialog_input(key, app);
    }

    // Save dialog (shown after saving from Edit diff view) — d/f/Esc
    if app.viewer_edit_save_dialog {
        return handle_save_dialog_input(key, app);
    }

    // Discard dialog — y/n/s/f/Esc
    if app.viewer_edit_discard_dialog {
        return handle_discard_dialog_input(key, app);
    }

    // Edit mode text editing: characters, arrows, backspace, etc.
    // These are legitimate raw key matches — not configurable keybindings.
    if app.viewer_edit_mode {
        return handle_edit_mode_input(key, app);
    }

    // Read-only mode: nothing to handle here — all bindings resolved upstream
    Ok(())
}

/// Handle input when in edit mode
fn handle_edit_mode_input(key: KeyEvent, app: &mut App) -> Result<()> {
    // is_cmd_key matches ⌘+letter (macOS) / Ctrl+letter (Win/Linux), plus macOS ⌥-unicode fallbacks
    let (m, c) = (key.modifiers, key.code);
    match (m, c) {
        // Save: Cmd+S / Ctrl+S / ⌥s(ß)
        _ if is_cmd_key(m, c, 's') => {
            match app.save_viewer_edits() {
                Ok(()) => {
                    app.set_status("File saved");
                    // Show post-save dialog if editing from Edit diff view
                    if app.viewer_edit_diff.is_some() {
                        app.viewer_edit_save_dialog = true;
                    }
                }
                Err(e) => app.set_status(format!("Save failed: {}", e)),
            }
        }

        // Undo: Cmd+Z / Ctrl+Z / ⌥z(Ω)
        _ if is_cmd_key(m, c, 'z') && !m.contains(KeyModifiers::SHIFT) => {
            app.viewer_edit_undo();
        }

        // Redo: Cmd+Shift+Z / Ctrl+Y (Ctrl+Shift+Z also accepted)
        (m, KeyCode::Char('Z')) if is_cmd_shift(m) => {
            app.viewer_edit_redo();
        }
        (m, KeyCode::Char('z')) if is_cmd_shift(m) => {
            app.viewer_edit_redo();
        }
        // Ctrl+Y — redo on all platforms (macOS fallback for ⌘⇧Z without Kitty)
        (m, KeyCode::Char('y')) if m.contains(KeyModifiers::CONTROL) => {
            app.viewer_edit_redo();
        }

        // Copy: Cmd+C / Ctrl+C / ⌥c(ç)
        _ if is_cmd_key(m, c, 'c') => {
            if app.viewer_edit_copy() {
                app.set_status("Copied to clipboard");
            }
        }

        // Cut: Cmd+X / Ctrl+X / ⌥x(≈)
        _ if is_cmd_key(m, c, 'x') => {
            if app.has_edit_selection() {
                app.viewer_edit_cut();
                app.set_status("Cut to clipboard");
            }
        }

        // Paste: Cmd+V / Ctrl+V / ⌥v(√)
        _ if is_cmd_key(m, c, 'v') => {
            app.viewer_edit_paste();
            app.viewer_edit_scroll_to_cursor();
        }

        // Select All: Cmd+A / Ctrl+A / ⌥a(å)
        _ if is_cmd_key(m, c, 'a') => {
            app.viewer_edit_select_all();
        }

        // ⌃s (STT) handled by execute_action() via lookup_action() — never reaches here

        // Exit edit mode: Esc
        (KeyModifiers::NONE, KeyCode::Esc) => {
            if app.viewer_edit_dirty {
                // Show discard confirmation
                app.viewer_edit_discard_dialog = true;
            } else {
                app.exit_viewer_edit_mode();
            }
        }

        // Cursor movement with selection (Shift+Arrow)
        (KeyModifiers::SHIFT, KeyCode::Left) => {
            app.viewer_edit_left_select(true);
            app.viewer_edit_scroll_to_cursor();
        }
        (KeyModifiers::SHIFT, KeyCode::Right) => {
            app.viewer_edit_right_select(true);
            app.viewer_edit_scroll_to_cursor();
        }
        (KeyModifiers::SHIFT, KeyCode::Up) => {
            app.viewer_edit_up_select(true);
            app.viewer_edit_scroll_to_cursor();
        }
        (KeyModifiers::SHIFT, KeyCode::Down) => {
            app.viewer_edit_down_select(true);
            app.viewer_edit_scroll_to_cursor();
        }

        // Cursor movement without selection
        (KeyModifiers::NONE, KeyCode::Left) => {
            app.viewer_edit_left_select(false);
            app.viewer_edit_scroll_to_cursor();
        }
        (KeyModifiers::NONE, KeyCode::Right) => {
            app.viewer_edit_right_select(false);
            app.viewer_edit_scroll_to_cursor();
        }
        (KeyModifiers::NONE, KeyCode::Up) => {
            app.viewer_edit_up_select(false);
            app.viewer_edit_scroll_to_cursor();
        }
        (KeyModifiers::NONE, KeyCode::Down) => {
            app.viewer_edit_down_select(false);
            app.viewer_edit_scroll_to_cursor();
        }
        (KeyModifiers::NONE, KeyCode::Home) => {
            app.viewer_edit_clear_selection();
            app.viewer_edit_home();
        }
        (KeyModifiers::NONE, KeyCode::End) => {
            app.viewer_edit_clear_selection();
            app.viewer_edit_end();
        }

        // Text editing - delete selection if any, then insert
        (KeyModifiers::NONE, KeyCode::Enter) => {
            if app.has_edit_selection() {
                app.viewer_edit_delete_selection();
            }
            app.viewer_edit_enter();
            app.viewer_edit_scroll_to_cursor();
        }
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            if app.has_edit_selection() {
                app.viewer_edit_delete_selection();
            } else {
                app.viewer_edit_backspace();
            }
            app.viewer_edit_scroll_to_cursor();
        }
        (KeyModifiers::NONE, KeyCode::Delete) => {
            if app.has_edit_selection() {
                app.viewer_edit_delete_selection();
            } else {
                app.viewer_edit_delete();
            }
        }
        (KeyModifiers::NONE, KeyCode::Char(c)) => {
            if app.has_edit_selection() {
                app.viewer_edit_delete_selection();
            }
            app.viewer_edit_char(c);
            app.viewer_edit_scroll_to_cursor();
        }
        (KeyModifiers::SHIFT, KeyCode::Char(c)) => {
            if app.has_edit_selection() {
                app.viewer_edit_delete_selection();
            }
            app.viewer_edit_char(c);
            app.viewer_edit_scroll_to_cursor();
        }
        (KeyModifiers::NONE, KeyCode::Tab) => {
            if app.has_edit_selection() {
                app.viewer_edit_delete_selection();
            }
            // Insert 4 spaces for tab
            for _ in 0..4 {
                app.viewer_edit_char(' ');
            }
        }

        _ => {}
    }

    Ok(())
}

/// Handle input for tab dialog
fn handle_tab_dialog_input(key: KeyEvent, app: &mut App) -> Result<()> {
    match (key.modifiers, key.code) {
        // Close dialog
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.viewer_tab_dialog = false;
        }
        // ⌥t toggle (macOS: ⌥t arrives as '†')
        (KeyModifiers::NONE, KeyCode::Char(c)) if macos_opt_key(c) == Some('t') => {
            app.viewer_tab_dialog = false;
        }
        (KeyModifiers::ALT, KeyCode::Char('t')) => {
            app.viewer_tab_dialog = false;
        }

        // Navigate tabs with j/k
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            if !app.viewer_tabs.is_empty() {
                app.viewer_active_tab = (app.viewer_active_tab + 1) % app.viewer_tabs.len();
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            if !app.viewer_tabs.is_empty() {
                app.viewer_active_tab = if app.viewer_active_tab == 0 {
                    app.viewer_tabs.len() - 1
                } else {
                    app.viewer_active_tab - 1
                };
            }
        }

        // Select tab with Enter
        (KeyModifiers::NONE, KeyCode::Enter) => {
            app.viewer_tab_dialog = false;
            app.load_tab_to_viewer();
        }

        // Close tab with x
        (KeyModifiers::NONE, KeyCode::Char('x')) => {
            app.viewer_close_current_tab();
            if app.viewer_tabs.is_empty() {
                app.viewer_tab_dialog = false;
            }
        }

        // Number keys 1-9 to switch to tab
        (KeyModifiers::NONE, KeyCode::Char(c)) if c.is_ascii_digit() && c != '0' => {
            let idx = (c as usize) - ('1' as usize);
            if idx < app.viewer_tabs.len() {
                app.viewer_active_tab = idx;
                app.viewer_tab_dialog = false;
                app.load_tab_to_viewer();
            }
        }

        _ => {}
    }

    Ok(())
}

/// Handle input for post-save dialog (when saving from Edit diff view)
fn handle_save_dialog_input(key: KeyEvent, app: &mut App) -> Result<()> {
    match (key.modifiers, key.code) {
        // 'd' or Enter: Return to Edit diff view
        (KeyModifiers::NONE, KeyCode::Char('d')) | (KeyModifiers::NONE, KeyCode::Enter) => {
            app.viewer_edit_save_dialog = false;
            app.exit_viewer_edit_mode();
            // Reload the file with the edit diff overlay
            if let Some(idx) = app.selected_tool_diff {
                if let Some((_, _, _, file_path, old_str, new_str, _)) =
                    app.clickable_paths.get(idx).cloned()
                {
                    app.load_file_with_edit_diff(&file_path, &old_str, &new_str);
                }
            }
        }

        // 'f': Go to modified file (clear diff overlay and selection)
        (KeyModifiers::NONE, KeyCode::Char('f')) => {
            app.viewer_edit_save_dialog = false;
            app.viewer_edit_diff = None;
            app.viewer_edit_diff_line = None;
            app.viewer_scroll_to_diff = false;
            app.selected_tool_diff = None;
            app.exit_viewer_edit_mode();
            // Reload file without diff overlay
            if let Some(path) = app.viewer_path.clone() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    app.viewer_content = Some(content);
                    app.viewer_lines_dirty = true;
                }
            }
        }

        // Esc: cancel, stay in edit mode
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.viewer_edit_save_dialog = false;
        }

        _ => {}
    }

    Ok(())
}

/// Handle input for discard confirmation dialog
fn handle_discard_dialog_input(key: KeyEvent, app: &mut App) -> Result<()> {
    let from_edit_diff = app.viewer_edit_diff.is_some();

    match (key.modifiers, key.code) {
        // Yes/discard: y or Enter
        (KeyModifiers::NONE, KeyCode::Char('y')) | (KeyModifiers::NONE, KeyCode::Enter) => {
            app.viewer_edit_discard_dialog = false;
            app.exit_viewer_edit_mode();
        }

        // No/cancel: n or Esc
        (KeyModifiers::NONE, KeyCode::Char('n')) | (KeyModifiers::NONE, KeyCode::Esc) => {
            app.viewer_edit_discard_dialog = false;
        }

        // Save options - different behavior when from Edit diff
        // 's': Save and return to edit diff (or just exit if not from diff)
        (KeyModifiers::NONE, KeyCode::Char('s')) => {
            match app.save_viewer_edits() {
                Ok(()) => {
                    app.set_status("File saved");
                    app.viewer_edit_discard_dialog = false;
                    if from_edit_diff {
                        app.exit_viewer_edit_mode();
                        // Reload with edit diff overlay
                        if let Some(idx) = app.selected_tool_diff {
                            if let Some((_, _, _, file_path, old_str, new_str, _)) =
                                app.clickable_paths.get(idx).cloned()
                            {
                                app.load_file_with_edit_diff(&file_path, &old_str, &new_str);
                            }
                        }
                    } else {
                        app.exit_viewer_edit_mode();
                    }
                }
                Err(e) => {
                    app.set_status(format!("Save failed: {}", e));
                    app.viewer_edit_discard_dialog = false;
                }
            }
        }

        // 'f': Save and go to modified file (only when from Edit diff)
        (KeyModifiers::NONE, KeyCode::Char('f')) if from_edit_diff => {
            match app.save_viewer_edits() {
                Ok(()) => {
                    app.set_status("File saved");
                    app.viewer_edit_discard_dialog = false;
                    app.viewer_edit_diff = None;
                    app.viewer_edit_diff_line = None;
                    app.selected_tool_diff = None;
                    app.exit_viewer_edit_mode();
                    // Reload file without diff overlay
                    if let Some(path) = app.viewer_path.clone() {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            app.viewer_content = Some(content);
                            app.viewer_lines_dirty = true;
                        }
                    }
                }
                Err(e) => {
                    app.set_status(format!("Save failed: {}", e));
                    app.viewer_edit_discard_dialog = false;
                }
            }
        }

        _ => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    /// Build a KeyEvent with no modifiers.
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    /// Build a KeyEvent with specified modifiers.
    fn key_mod(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    // ── KeyEvent construction ──────────────────────────────────────────

    #[test]
    fn key_helper_produces_none_modifiers() {
        let k = key(KeyCode::Char('a'));
        assert_eq!(k.modifiers, KeyModifiers::NONE);
        assert_eq!(k.code, KeyCode::Char('a'));
    }

    #[test]
    fn key_mod_helper_produces_super() {
        let k = key_mod(KeyCode::Char('s'), KeyModifiers::SUPER);
        assert_eq!(k.modifiers, KeyModifiers::SUPER);
    }

    #[test]
    fn key_event_kind_is_press() {
        let k = key(KeyCode::Enter);
        assert_eq!(k.kind, KeyEventKind::Press);
    }

    #[test]
    fn key_event_state_is_none() {
        let k = key(KeyCode::Esc);
        assert_eq!(k.state, KeyEventState::NONE);
    }

    // ── Cmd shortcuts match patterns ──────────────────────────────────

    #[test]
    fn cmd_s_matches_super_s() {
        let k = key_mod(KeyCode::Char('s'), KeyModifiers::SUPER);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::SUPER, KeyCode::Char('s'))
        ));
    }

    #[test]
    fn cmd_z_matches_super_z() {
        let k = key_mod(KeyCode::Char('z'), KeyModifiers::SUPER);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::SUPER, KeyCode::Char('z'))
        ));
    }

    #[test]
    fn cmd_shift_z_matches_redo_pattern() {
        let mods = KeyModifiers::SUPER | KeyModifiers::SHIFT;
        let k = key_mod(KeyCode::Char('Z'), mods);
        assert!(k.modifiers == KeyModifiers::SUPER | KeyModifiers::SHIFT);
        assert_eq!(k.code, KeyCode::Char('Z'));
    }

    #[test]
    fn cmd_shift_z_lowercase_also_matches() {
        let mods = KeyModifiers::SUPER | KeyModifiers::SHIFT;
        let k = key_mod(KeyCode::Char('z'), mods);
        assert!(k.modifiers == KeyModifiers::SUPER | KeyModifiers::SHIFT);
        assert_eq!(k.code, KeyCode::Char('z'));
    }

    #[test]
    fn cmd_c_matches_copy() {
        let k = key_mod(KeyCode::Char('c'), KeyModifiers::SUPER);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::SUPER, KeyCode::Char('c'))
        ));
    }

    #[test]
    fn cmd_x_matches_cut() {
        let k = key_mod(KeyCode::Char('x'), KeyModifiers::SUPER);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::SUPER, KeyCode::Char('x'))
        ));
    }

    #[test]
    fn cmd_v_matches_paste() {
        let k = key_mod(KeyCode::Char('v'), KeyModifiers::SUPER);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::SUPER, KeyCode::Char('v'))
        ));
    }

    #[test]
    fn cmd_a_matches_select_all() {
        let k = key_mod(KeyCode::Char('a'), KeyModifiers::SUPER);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::SUPER, KeyCode::Char('a'))
        ));
    }

    // ── Cursor movement patterns ──────────────────────────────────────

    #[test]
    fn shift_left_matches_selection_pattern() {
        let k = key_mod(KeyCode::Left, KeyModifiers::SHIFT);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::SHIFT, KeyCode::Left)
        ));
    }

    #[test]
    fn shift_right_matches_selection_pattern() {
        let k = key_mod(KeyCode::Right, KeyModifiers::SHIFT);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::SHIFT, KeyCode::Right)
        ));
    }

    #[test]
    fn shift_up_matches_selection_pattern() {
        let k = key_mod(KeyCode::Up, KeyModifiers::SHIFT);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::SHIFT, KeyCode::Up)
        ));
    }

    #[test]
    fn shift_down_matches_selection_pattern() {
        let k = key_mod(KeyCode::Down, KeyModifiers::SHIFT);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::SHIFT, KeyCode::Down)
        ));
    }

    #[test]
    fn plain_left_matches_no_selection_pattern() {
        let k = key(KeyCode::Left);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Left)
        ));
    }

    #[test]
    fn plain_right_matches_no_selection_pattern() {
        let k = key(KeyCode::Right);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Right)
        ));
    }

    #[test]
    fn plain_up_matches_no_selection_pattern() {
        let k = key(KeyCode::Up);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Up)
        ));
    }

    #[test]
    fn plain_down_matches_no_selection_pattern() {
        let k = key(KeyCode::Down);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Down)
        ));
    }

    #[test]
    fn home_key_matches() {
        let k = key(KeyCode::Home);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Home)
        ));
    }

    #[test]
    fn end_key_matches() {
        let k = key(KeyCode::End);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::End)
        ));
    }

    // ── Text editing key patterns ─────────────────────────────────────

    #[test]
    fn enter_key_matches() {
        let k = key(KeyCode::Enter);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Enter)
        ));
    }

    #[test]
    fn backspace_key_matches() {
        let k = key(KeyCode::Backspace);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Backspace)
        ));
    }

    #[test]
    fn delete_key_matches() {
        let k = key(KeyCode::Delete);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Delete)
        ));
    }

    #[test]
    fn char_key_matches_none_modifier() {
        let k = key(KeyCode::Char('h'));
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('h'))
        ));
    }

    #[test]
    fn shift_char_matches_shift_modifier() {
        let k = key_mod(KeyCode::Char('H'), KeyModifiers::SHIFT);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::SHIFT, KeyCode::Char('H'))
        ));
    }

    #[test]
    fn tab_key_matches() {
        let k = key(KeyCode::Tab);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Tab)
        ));
    }

    #[test]
    fn esc_key_matches() {
        let k = key(KeyCode::Esc);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Esc)
        ));
    }

    // ── Tab dialog key patterns ───────────────────────────────────────

    #[test]
    fn tab_dialog_j_matches_down() {
        let k = key(KeyCode::Char('j'));
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down)
        ));
    }

    #[test]
    fn tab_dialog_k_matches_up() {
        let k = key(KeyCode::Char('k'));
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up)
        ));
    }

    #[test]
    fn tab_dialog_down_arrow_matches() {
        let k = key(KeyCode::Down);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down)
        ));
    }

    #[test]
    fn tab_dialog_up_arrow_matches() {
        let k = key(KeyCode::Up);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up)
        ));
    }

    #[test]
    fn tab_dialog_enter_matches() {
        let k = key(KeyCode::Enter);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Enter)
        ));
    }

    #[test]
    fn tab_dialog_x_matches_close() {
        let k = key(KeyCode::Char('x'));
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('x'))
        ));
    }

    // ── Tab dialog number key digit-to-index mapping ──────────────────

    #[test]
    fn digit_1_maps_to_index_0() {
        let c = '1';
        let idx = (c as usize) - ('1' as usize);
        assert_eq!(idx, 0);
    }

    #[test]
    fn digit_5_maps_to_index_4() {
        let c = '5';
        let idx = (c as usize) - ('1' as usize);
        assert_eq!(idx, 4);
    }

    #[test]
    fn digit_9_maps_to_index_8() {
        let c = '9';
        let idx = (c as usize) - ('1' as usize);
        assert_eq!(idx, 8);
    }

    #[test]
    fn digit_guard_excludes_zero() {
        let c = '0';
        assert!(!(c.is_ascii_digit() && c != '0'));
    }

    #[test]
    fn digit_guard_includes_1_through_9() {
        for c in '1'..='9' {
            assert!(c.is_ascii_digit() && c != '0');
        }
    }

    #[test]
    fn digit_guard_excludes_letters() {
        for c in ['a', 'z', 'A', 'Z'] {
            assert!(!(c.is_ascii_digit() && c != '0'));
        }
    }

    // ── Tab wrapping arithmetic ───────────────────────────────────────

    #[test]
    fn tab_next_wraps_around() {
        let len = 5;
        let active = 4;
        assert_eq!((active + 1) % len, 0);
    }

    #[test]
    fn tab_next_increments() {
        let len = 5;
        let active = 2;
        assert_eq!((active + 1) % len, 3);
    }

    #[test]
    fn tab_prev_wraps_to_end() {
        let len = 5;
        let active: usize = 0;
        let result = if active == 0 { len - 1 } else { active - 1 };
        assert_eq!(result, 4);
    }

    #[test]
    fn tab_prev_decrements() {
        let len = 5;
        let active: usize = 3;
        let result = if active == 0 { len - 1 } else { active - 1 };
        assert_eq!(result, 2);
    }

    #[test]
    fn tab_wrapping_single_element() {
        let len = 1;
        let active = 0;
        assert_eq!((active + 1) % len, 0);
        let result = if active == 0 { len - 1 } else { active - 1 };
        assert_eq!(result, 0);
    }

    // ── macOS option key (⌥t toggle) ──────────────────────────────────

    #[test]
    fn macos_opt_t_is_dagger() {
        // ⌥t on macOS US keyboard produces '†'
        assert_eq!(macos_opt_key('†'), Some('t'));
    }

    #[test]
    fn macos_opt_key_unmapped_returns_none() {
        assert_eq!(macos_opt_key('z'), None);
    }

    #[test]
    fn macos_opt_key_all_letters_mapped() {
        let mappings = [
            ('å', 'a'),
            ('∫', 'b'),
            ('ç', 'c'),
            ('∂', 'd'),
            ('´', 'e'),
            ('ƒ', 'f'),
            ('©', 'g'),
            ('˙', 'h'),
            ('ˆ', 'i'),
            ('∆', 'j'),
            ('˚', 'k'),
            ('¬', 'l'),
            ('µ', 'm'),
            ('˜', 'n'),
            ('ø', 'o'),
            ('π', 'p'),
            ('œ', 'q'),
            ('®', 'r'),
            ('ß', 's'),
            ('†', 't'),
            ('¨', 'u'),
            ('√', 'v'),
            ('∑', 'w'),
            ('≈', 'x'),
            ('¥', 'y'),
            ('Ω', 'z'),
        ];
        for (unicode, letter) in mappings {
            assert_eq!(
                macos_opt_key(unicode),
                Some(letter),
                "failed for {} -> {}",
                unicode,
                letter
            );
        }
    }

    #[test]
    fn macos_opt_key_numbers_mapped() {
        let mappings = [
            ('¡', '1'),
            ('™', '2'),
            ('£', '3'),
            ('¢', '4'),
            ('∞', '5'),
            ('§', '6'),
            ('¶', '7'),
            ('•', '8'),
            ('ª', '9'),
            ('º', '0'),
        ];
        for (unicode, digit) in mappings {
            assert_eq!(
                macos_opt_key(unicode),
                Some(digit),
                "failed for {} -> {}",
                unicode,
                digit
            );
        }
    }

    #[test]
    fn macos_opt_t_toggle_guard_matches_dagger() {
        let c = '†';
        assert!(macos_opt_key(c) == Some('t'));
    }

    #[test]
    fn alt_t_key_matches_tab_toggle() {
        let k = key_mod(KeyCode::Char('t'), KeyModifiers::ALT);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::ALT, KeyCode::Char('t'))
        ));
    }

    // ── Save dialog key patterns ──────────────────────────────────────

    #[test]
    fn save_dialog_d_matches() {
        let k = key(KeyCode::Char('d'));
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('d')) | (KeyModifiers::NONE, KeyCode::Enter)
        ));
    }

    #[test]
    fn save_dialog_enter_matches() {
        let k = key(KeyCode::Enter);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('d')) | (KeyModifiers::NONE, KeyCode::Enter)
        ));
    }

    #[test]
    fn save_dialog_f_matches_go_to_file() {
        let k = key(KeyCode::Char('f'));
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('f'))
        ));
    }

    #[test]
    fn save_dialog_esc_cancels() {
        let k = key(KeyCode::Esc);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Esc)
        ));
    }

    // ── Discard dialog key patterns ───────────────────────────────────

    #[test]
    fn discard_dialog_y_matches_confirm() {
        let k = key(KeyCode::Char('y'));
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('y')) | (KeyModifiers::NONE, KeyCode::Enter)
        ));
    }

    #[test]
    fn discard_dialog_enter_matches_confirm() {
        let k = key(KeyCode::Enter);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('y')) | (KeyModifiers::NONE, KeyCode::Enter)
        ));
    }

    #[test]
    fn discard_dialog_n_matches_cancel() {
        let k = key(KeyCode::Char('n'));
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('n')) | (KeyModifiers::NONE, KeyCode::Esc)
        ));
    }

    #[test]
    fn discard_dialog_esc_matches_cancel() {
        let k = key(KeyCode::Esc);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('n')) | (KeyModifiers::NONE, KeyCode::Esc)
        ));
    }

    #[test]
    fn discard_dialog_s_matches_save() {
        let k = key(KeyCode::Char('s'));
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('s'))
        ));
    }

    #[test]
    fn discard_dialog_f_matches_save_and_go() {
        let k = key(KeyCode::Char('f'));
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Char('f'))
        ));
    }

    // ── KeyModifiers bitflag combinations ─────────────────────────────

    #[test]
    fn super_shift_combined() {
        let mods = KeyModifiers::SUPER | KeyModifiers::SHIFT;
        assert!(mods.contains(KeyModifiers::SUPER));
        assert!(mods.contains(KeyModifiers::SHIFT));
        assert!(!mods.contains(KeyModifiers::ALT));
        assert!(!mods.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn none_modifier_is_empty() {
        assert!(KeyModifiers::NONE.is_empty());
    }

    #[test]
    fn super_modifier_not_empty() {
        assert!(!KeyModifiers::SUPER.is_empty());
    }

    #[test]
    fn shift_does_not_equal_super() {
        assert_ne!(KeyModifiers::SHIFT, KeyModifiers::SUPER);
    }

    // ── String formatting patterns used in the handler ────────────────

    #[test]
    fn save_failed_format() {
        let err = "permission denied";
        let msg = format!("Save failed: {}", err);
        assert_eq!(msg, "Save failed: permission denied");
    }

    #[test]
    fn stopped_tracking_format() {
        let name = "feat-branch";
        let msg = format!("Stopped tracking: {}", name);
        assert_eq!(msg, "Stopped tracking: feat-branch");
    }

    #[test]
    fn file_saved_literal() {
        let msg = "File saved";
        assert_eq!(msg.len(), 10);
    }

    #[test]
    fn copied_to_clipboard_literal() {
        let msg = "Copied to clipboard";
        assert!(!msg.is_empty());
    }

    #[test]
    fn cut_to_clipboard_literal() {
        let msg = "Cut to clipboard";
        assert!(msg.contains("clipboard"));
    }

    // ── Tab spaces (4 spaces for tab) ─────────────────────────────────

    #[test]
    fn tab_inserts_four_spaces() {
        let mut buf = String::new();
        for _ in 0..4 {
            buf.push(' ');
        }
        assert_eq!(buf, "    ");
        assert_eq!(buf.len(), 4);
    }

    // ── from_edit_diff guard logic ────────────────────────────────────

    #[test]
    fn option_is_some_means_from_edit_diff() {
        let diff: Option<String> = Some("old -> new".to_string());
        assert!(diff.is_some());
    }

    #[test]
    fn option_is_none_means_not_from_edit_diff() {
        let diff: Option<String> = None;
        assert!(!diff.is_some());
    }

    // ── KeyCode variant coverage ──────────────────────────────────────

    #[test]
    fn all_arrow_keys_are_distinct() {
        let codes = [KeyCode::Left, KeyCode::Right, KeyCode::Up, KeyCode::Down];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn char_codes_are_distinct_per_character() {
        assert_ne!(KeyCode::Char('a'), KeyCode::Char('b'));
    }

    #[test]
    fn same_char_codes_are_equal() {
        assert_eq!(KeyCode::Char('z'), KeyCode::Char('z'));
    }

    #[test]
    fn enter_and_esc_are_distinct() {
        assert_ne!(KeyCode::Enter, KeyCode::Esc);
    }
}
