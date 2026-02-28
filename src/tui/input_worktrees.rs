//! Worktrees panel input handling

use anyhow::Result;
use crossterm::event::{self, KeyCode, KeyModifiers};

use crate::app::App;

/// Handle keyboard input when Worktree tab row is focused.
/// ALL command keybindings are resolved by lookup_action() in event_loop.rs BEFORE
/// this is called. This handler only receives unresolved keys.
pub fn handle_worktrees_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
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
