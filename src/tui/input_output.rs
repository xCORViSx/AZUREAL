//! Output panel input handling

use anyhow::Result;
use crossterm::event;

use crate::app::{App, ViewMode};
use super::input_rebase::handle_rebase_input;

/// Handle keyboard input when Output pane is focused.
/// ALL keybindings are resolved by lookup_action() in event_loop.rs BEFORE this
/// is called. This handler only receives keys that weren't mapped — meaning only
/// session list overlay and rebase mode input reach here.
pub fn handle_output_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    // Session list overlay: j/k navigate, Enter selects, s/Esc closes
    if app.show_session_list {
        return handle_session_list_input(key, app);
    }

    // Rebase mode has its own handler
    if app.view_mode == ViewMode::Rebase {
        return handle_rebase_input(key, app);
    }

    // All output keybindings resolved upstream — nothing to handle here
    Ok(())
}

/// Handle keyboard input for the session list overlay
fn handle_session_list_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    use event::{KeyCode, KeyModifiers};

    // Build flat count of total rows (same structure as draw_session_list)
    let total_rows: usize = app.sessions.iter().map(|s| {
        app.session_files.get(&s.branch_name).map(|f| f.len().max(1)).unwrap_or(1)
    }).sum();

    match (key.modifiers, key.code) {
        // j/↓: next row
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            if app.session_list_selected + 1 < total_rows {
                app.session_list_selected += 1;
            }
        }
        // k/↑: prev row
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            app.session_list_selected = app.session_list_selected.saturating_sub(1);
        }
        // J: page down
        (KeyModifiers::NONE, KeyCode::Char('J')) => {
            let page = app.output_viewport_height.saturating_sub(2);
            app.session_list_selected = (app.session_list_selected + page).min(total_rows.saturating_sub(1));
        }
        // K: page up
        (KeyModifiers::NONE, KeyCode::Char('K')) => {
            let page = app.output_viewport_height.saturating_sub(2);
            app.session_list_selected = app.session_list_selected.saturating_sub(page);
        }
        // Enter: load the selected session file
        (KeyModifiers::NONE, KeyCode::Enter) => {
            // Walk the flat list to find which (session_idx, file_idx) corresponds to selection
            let mut row = 0;
            for (sess_idx, session) in app.sessions.iter().enumerate() {
                let files = app.session_files.get(&session.branch_name);
                let file_count = files.map(|f| f.len()).unwrap_or(0).max(1);
                if app.session_list_selected < row + file_count {
                    let file_idx = app.session_list_selected - row;
                    if files.map(|f| f.len()).unwrap_or(0) > 0 {
                        // Select the session and file
                        let branch = session.branch_name.clone();
                        app.save_current_terminal();
                        app.selected_worktree = Some(sess_idx);
                        app.select_session_file(&branch, file_idx);
                        app.show_session_list = false;
                        app.invalidate_sidebar();
                    }
                    break;
                }
                row += file_count;
            }
        }
        // s or Esc: close overlay
        (KeyModifiers::NONE, KeyCode::Char('s')) | (_, KeyCode::Esc) => {
            app.show_session_list = false;
        }
        _ => {}
    }
    Ok(())
}
