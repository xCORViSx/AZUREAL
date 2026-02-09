//! Output panel input handling

use anyhow::Result;
use crossterm::event;

use crate::app::{App, Focus, ViewMode};
use crate::git::Git;
use super::input_rebase::handle_rebase_input;
use super::keybindings::{Action, lookup_action};

/// Handle keyboard input when Output pane is focused
pub fn handle_output_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    // Rebase mode has its own handler
    if app.view_mode == ViewMode::Rebase {
        return handle_rebase_input(key, app);
    }

    // Route through centralized keybinding lookup
    let action = lookup_action(Focus::Output, key.modifiers, key.code, false, false, false);

    match action {
        // j/k/↓/↑ line-by-line scroll (NavDown/NavUp only fires for j/k since ↓/↑ are mapped to JumpNext/PrevBubble)
        Some(Action::NavDown) => {
            match app.view_mode {
                ViewMode::Output => { app.scroll_output_down(1); }
                ViewMode::Diff => { app.scroll_diff_down(1); }
                _ => {}
            }
        }
        Some(Action::NavUp) => {
            match app.view_mode {
                ViewMode::Output => { app.scroll_output_up(1); }
                ViewMode::Diff => { app.scroll_diff_up(1); }
                _ => {}
            }
        }
        // ↓/↑ jump between user prompts (convo mode only)
        Some(Action::JumpNextBubble) => {
            if app.view_mode == ViewMode::Output { app.jump_to_next_bubble(false); }
        }
        Some(Action::JumpPrevBubble) => {
            if app.view_mode == ViewMode::Output { app.jump_to_prev_bubble(false); }
        }
        // ⇧↓/⇧↑ jump between all messages including assistant
        Some(Action::JumpNextMessage) => {
            if app.view_mode == ViewMode::Output { app.jump_to_next_bubble(true); }
        }
        Some(Action::JumpPrevMessage) => {
            if app.view_mode == ViewMode::Output { app.jump_to_prev_bubble(true); }
        }
        // J/K page scroll (full viewport minus 2 for overlap)
        Some(Action::PageDown) => {
            let page = app.output_viewport_height.saturating_sub(2);
            match app.view_mode {
                ViewMode::Output => { app.scroll_output_down(page); }
                ViewMode::Diff => { app.scroll_diff_down(page); }
                _ => {}
            }
        }
        Some(Action::PageUp) => {
            let page = app.output_viewport_height.saturating_sub(2);
            match app.view_mode {
                ViewMode::Output => { app.scroll_output_up(page); }
                ViewMode::Diff => { app.scroll_diff_up(page); }
                _ => {}
            }
        }
        // g/⌥↑ scroll to top
        Some(Action::GoToTop) => {
            match app.view_mode {
                ViewMode::Output => app.output_scroll = 0,
                ViewMode::Diff => app.diff_scroll = 0,
                _ => {}
            }
        }
        // G/⌥↓ scroll to bottom
        Some(Action::GoToBottom) => {
            match app.view_mode {
                ViewMode::Output => app.scroll_output_to_bottom(),
                ViewMode::Diff => app.scroll_diff_to_bottom(),
                _ => {}
            }
        }
        // o: switch to output view
        Some(Action::SwitchToOutput) => {
            app.view_mode = ViewMode::Output;
            app.output_scroll = usize::MAX; // follow bottom
        }
        // d: view diff
        Some(Action::ViewDiff) => {
            if let Err(e) = app.load_diff() {
                app.set_status(format!("Failed to get diff: {}", e));
            } else {
                app.diff_scroll = 0;
            }
        }
        // R: show rebase status
        Some(Action::RebaseStatus) => {
            if let Some(session) = app.current_session() {
                if let Some(ref wt_path) = session.worktree_path {
                    if Git::is_rebase_in_progress(wt_path) {
                        if let Ok(status) = Git::get_rebase_status(wt_path) {
                            app.set_rebase_status(status);
                        }
                    }
                }
            }
        }
        // Tab: switch focus to input (handled by global, but catch if it leaks through)
        Some(Action::CycleFocusForward) => app.focus = Focus::Input,
        // Esc: back to worktrees sidebar
        Some(Action::Escape) => app.focus = Focus::Worktrees,
        _ => {}
    }
    Ok(())
}
