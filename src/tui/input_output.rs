//! Output panel input handling

use anyhow::Result;
use crossterm::event::{self, KeyCode, KeyModifiers};

use crate::app::{App, Focus, ViewMode};
use crate::git::Git;
use super::input_rebase::handle_rebase_input;

/// Handle keyboard input when Output pane is focused
pub fn handle_output_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    // Handle rebase mode separately
    if app.view_mode == ViewMode::Rebase {
        return handle_rebase_input(key, app);
    }

    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char('j')) => {
            match app.view_mode {
                ViewMode::Output => { app.scroll_output_down(1); }
                ViewMode::Diff => { app.scroll_diff_down(1); }
                _ => {}
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) => {
            match app.view_mode {
                ViewMode::Output => { app.scroll_output_up(1); }
                ViewMode::Diff => { app.scroll_diff_up(1); }
                _ => {}
            }
        }
        // Arrow keys for bubble navigation: Down/Up = user prompts, Shift = include assistant
        (KeyModifiers::NONE, KeyCode::Down) => {
            if app.view_mode == ViewMode::Output {
                app.jump_to_next_bubble(false);
            }
        }
        (KeyModifiers::NONE, KeyCode::Up) => {
            if app.view_mode == ViewMode::Output {
                app.jump_to_prev_bubble(false);
            }
        }
        (KeyModifiers::SHIFT, KeyCode::Down) => {
            if app.view_mode == ViewMode::Output {
                app.jump_to_next_bubble(true);
            }
        }
        (KeyModifiers::SHIFT, KeyCode::Up) => {
            if app.view_mode == ViewMode::Output {
                app.jump_to_prev_bubble(true);
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('G')) => {
            match app.view_mode {
                ViewMode::Output => app.scroll_output_to_bottom(),
                ViewMode::Diff => app.scroll_diff_to_bottom(),
                _ => {}
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('g')) => {
            match app.view_mode {
                ViewMode::Output => app.output_scroll = 0,
                ViewMode::Diff => app.diff_scroll = 0,
                _ => {}
            }
        }
        (KeyModifiers::NONE, KeyCode::PageDown) => {
            match app.view_mode {
                ViewMode::Output => { app.scroll_output_down(10); }
                ViewMode::Diff => { app.scroll_diff_down(10); }
                _ => {}
            }
        }
        (KeyModifiers::NONE, KeyCode::PageUp) => {
            match app.view_mode {
                ViewMode::Output => { app.scroll_output_up(10); }
                ViewMode::Diff => { app.scroll_diff_up(10); }
                _ => {}
            }
        }
        // Half-page scroll (uses cached viewport height)
        (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
            let half = app.output_viewport_height / 2;
            match app.view_mode {
                ViewMode::Output => { app.scroll_output_down(half); }
                ViewMode::Diff => { app.scroll_diff_down(half); }
                _ => {}
            }
        }
        (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
            let half = app.output_viewport_height / 2;
            match app.view_mode {
                ViewMode::Output => { app.scroll_output_up(half); }
                ViewMode::Diff => { app.scroll_diff_up(half); }
                _ => {}
            }
        }
        // Full-page scroll (uses cached viewport height)
        (KeyModifiers::CONTROL, KeyCode::Char('f')) => {
            let page = app.output_viewport_height.saturating_sub(2);
            match app.view_mode {
                ViewMode::Output => { app.scroll_output_down(page); }
                ViewMode::Diff => { app.scroll_diff_down(page); }
                _ => {}
            }
        }
        (KeyModifiers::CONTROL, KeyCode::Char('b')) => {
            let page = app.output_viewport_height.saturating_sub(2);
            match app.view_mode {
                ViewMode::Output => { app.scroll_output_up(page); }
                ViewMode::Diff => { app.scroll_diff_up(page); }
                _ => {}
            }
        }
        (KeyModifiers::NONE, KeyCode::Tab) => app.focus = Focus::Input,
        (KeyModifiers::NONE, KeyCode::Char('o')) => {
            app.view_mode = ViewMode::Output;
            app.output_scroll = usize::MAX; // Scroll to bottom (most recent)
        }
        (KeyModifiers::NONE, KeyCode::Char('d')) => {
            if let Err(e) = app.load_diff() {
                app.set_status(format!("Failed to get diff: {}", e));
            } else {
                app.diff_scroll = 0;
            }
        }
        (KeyModifiers::SHIFT, KeyCode::Char('R')) => {
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
        (KeyModifiers::NONE, KeyCode::Esc) => app.focus = Focus::Worktrees,
        _ => {}
    }
    Ok(())
}
