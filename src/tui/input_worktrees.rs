//! Worktrees panel input handling

use anyhow::Result;
use crossterm::event::{self, KeyCode, KeyModifiers};

use crate::app::App;

/// Handle keyboard input when Worktrees pane is focused.
/// ALL command keybindings are resolved by lookup_action() in event_loop.rs BEFORE
/// this is called. This handler only receives unresolved keys — meaning only
/// sidebar filter text input and the 's' (stop tracking) command reach here.
/// Note: file tree overlay routing is also handled upstream (ToggleFileTree action).
pub fn handle_worktrees_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    // File tree overlay: if visible, route unresolved keys to file tree handler
    if app.show_file_tree {
        return super::input_file_tree::handle_file_tree_input(key, app);
    }

    // Sidebar filter text input — typing characters into the filter field
    if app.sidebar_filter_active {
        match key.code {
            KeyCode::Esc => {
                app.sidebar_filter.clear();
                app.sidebar_filter_active = false;
                app.invalidate_sidebar();
            }
            KeyCode::Enter => { app.sidebar_filter_active = false; }
            KeyCode::Backspace => {
                app.sidebar_filter.pop();
                if app.sidebar_filter.is_empty() { app.sidebar_filter_active = false; }
                app.snap_selection_to_filter();
                app.invalidate_sidebar();
            }
            KeyCode::Down => app.select_next_session(),
            KeyCode::Up => app.select_prev_session(),
            KeyCode::Char(c) => {
                app.sidebar_filter.push(c);
                app.snap_selection_to_filter();
                app.invalidate_sidebar();
            }
            _ => {}
        }
        return Ok(());
    }

    // 's' — stop tracking (not a navigation binding, so not in WORKTREES array).
    // This is the only worktree key that isn't in the centralized system — it's a
    // destructive action (removes receiver) that only makes sense contextually.
    if key.modifiers == KeyModifiers::NONE && key.code == KeyCode::Char('s') {
        if let Some(session) = app.current_worktree() {
            let branch_name = session.branch_name.clone();
            let session_name = session.name().to_string();
            if app.running_sessions.remove(&branch_name) {
                app.claude_receivers.remove(&branch_name);
                app.invalidate_sidebar();
                app.set_status(format!("Stopped tracking: {}", session_name));
            }
        }
    }

    Ok(())
}
