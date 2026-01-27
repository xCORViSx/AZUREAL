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

    let viewport_height = 20; // Estimate; actual clamping happens in draw_output

    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            match app.view_mode {
                ViewMode::Output => app.scroll_output_down(1, viewport_height),
                ViewMode::Diff => app.scroll_diff_down(1, viewport_height),
                _ => {}
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            match app.view_mode {
                ViewMode::Output => app.scroll_output_up(1),
                ViewMode::Diff => app.scroll_diff_up(1),
                _ => {}
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('G')) => {
            match app.view_mode {
                ViewMode::Output => app.scroll_output_to_bottom(viewport_height),
                ViewMode::Diff => app.scroll_diff_to_bottom(viewport_height),
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
                ViewMode::Output => app.scroll_output_down(10, viewport_height),
                ViewMode::Diff => app.scroll_diff_down(10, viewport_height),
                _ => {}
            }
        }
        (KeyModifiers::NONE, KeyCode::PageUp) => {
            match app.view_mode {
                ViewMode::Output => app.scroll_output_up(10),
                ViewMode::Diff => app.scroll_diff_up(10),
                _ => {}
            }
        }
        (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
            match app.view_mode {
                ViewMode::Output => app.scroll_output_down(20, viewport_height),
                ViewMode::Diff => app.scroll_diff_down(20, viewport_height),
                _ => {}
            }
        }
        (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
            match app.view_mode {
                ViewMode::Output => app.scroll_output_up(20),
                ViewMode::Diff => app.scroll_diff_up(20),
                _ => {}
            }
        }
        (KeyModifiers::CONTROL, KeyCode::Char('f')) => {
            match app.view_mode {
                ViewMode::Output => app.scroll_output_down(40, viewport_height),
                ViewMode::Diff => app.scroll_diff_down(40, viewport_height),
                _ => {}
            }
        }
        (KeyModifiers::CONTROL, KeyCode::Char('b')) => {
            match app.view_mode {
                ViewMode::Output => app.scroll_output_up(40),
                ViewMode::Diff => app.scroll_diff_up(40),
                _ => {}
            }
        }
        (KeyModifiers::NONE, KeyCode::Tab) => app.focus = Focus::Input,
        (KeyModifiers::NONE, KeyCode::Char('o')) => {
            app.view_mode = ViewMode::Output;
            app.output_scroll = 0;
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
                let worktree_path = session.worktree_path.clone();
                if Git::is_rebase_in_progress(&worktree_path) {
                    if let Ok(status) = Git::get_rebase_status(&worktree_path) {
                        app.set_rebase_status(status);
                    }
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Esc) => app.focus = Focus::Sessions,
        _ => {}
    }
    Ok(())
}
