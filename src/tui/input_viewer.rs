//! Viewer input handling
//!
//! Handles keyboard input when the Viewer panel is focused.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
    match (key.modifiers, key.code) {
        // Save: Cmd+S
        (KeyModifiers::SUPER, KeyCode::Char('s')) => {
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

        // Undo: Cmd+Z
        (KeyModifiers::SUPER, KeyCode::Char('z')) => {
            app.viewer_edit_undo();
        }

        // Redo: Cmd+Shift+Z
        (m, KeyCode::Char('Z')) if m == KeyModifiers::SUPER | KeyModifiers::SHIFT => {
            app.viewer_edit_redo();
        }
        (m, KeyCode::Char('z')) if m == KeyModifiers::SUPER | KeyModifiers::SHIFT => {
            app.viewer_edit_redo();
        }

        // Copy: Cmd+C
        (KeyModifiers::SUPER, KeyCode::Char('c')) => {
            if app.viewer_edit_copy() {
                app.set_status("Copied to clipboard");
            }
        }

        // Cut: Cmd+X
        (KeyModifiers::SUPER, KeyCode::Char('x')) => {
            if app.has_edit_selection() {
                app.viewer_edit_cut();
                app.set_status("Cut to clipboard");
            }
        }

        // Paste: Cmd+V (from system clipboard)
        (KeyModifiers::SUPER, KeyCode::Char('v')) => {
            app.viewer_edit_paste();
            app.viewer_edit_scroll_to_cursor();
        }

        // Select All: Cmd+A
        (KeyModifiers::SUPER, KeyCode::Char('a')) => {
            app.viewer_edit_select_all();
        }

        // Speech input: ⌃s — toggle speech-to-text recording
        (KeyModifiers::CONTROL, KeyCode::Char('s')) => {
            app.toggle_stt();
        }

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
        (KeyModifiers::NONE, KeyCode::Esc) | (KeyModifiers::NONE, KeyCode::Char('T')) => {
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
                if let Some((_, _, _, file_path, old_str, new_str, _)) = app.clickable_paths.get(idx).cloned() {
                    app.load_file_with_edit_diff(&file_path, &old_str, &new_str);
                }
            }
        }

        // 'f': Go to modified file (clear diff overlay and selection)
        (KeyModifiers::NONE, KeyCode::Char('f')) => {
            app.viewer_edit_save_dialog = false;
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
                            if let Some((_, _, _, file_path, old_str, new_str, _)) = app.clickable_paths.get(idx).cloned() {
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
