//! Sessions panel input handling

use anyhow::Result;
use crossterm::event::{self, KeyCode};

use crate::app::{App, Focus};
use crate::git::Git;
use crate::models::{RebaseResult, SessionStatus};

/// Handle keyboard input when Sessions pane is focused
pub fn handle_sessions_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.select_next_session(),
        KeyCode::Char('k') | KeyCode::Up => app.select_prev_session(),
        KeyCode::Char('J') => app.select_next_project(),
        KeyCode::Char('K') => app.select_prev_project(),
        KeyCode::Tab => app.focus = Focus::Output,
        KeyCode::Char(' ') | KeyCode::Char('?') => app.open_context_menu(),
        KeyCode::Char('n') => app.start_wizard(),
        KeyCode::Char('b') => {
            if let Some(project) = app.current_project() {
                match Git::list_available_branches(&project.path) {
                    Ok(branches) => app.open_branch_dialog(branches),
                    Err(e) => app.set_status(format!("Failed to list branches: {}", e)),
                }
            }
        }
        KeyCode::Char('d') => {
            if let Err(e) = app.load_diff() {
                app.set_status(format!("Failed to get diff: {}", e));
            }
        }
        KeyCode::Char('r') => {
            if let Some(session) = app.current_session() {
                if let Some(project) = app.current_project() {
                    let worktree_path = session.worktree_path.clone();
                    let main_branch = project.main_branch.clone();
                    match Git::rebase_onto_main(&worktree_path, &main_branch) {
                        Ok(RebaseResult::Success) => {
                            app.set_status("Rebase completed successfully");
                            app.clear_rebase_status();
                        }
                        Ok(RebaseResult::UpToDate) => {
                            app.set_status("Already up to date with main branch");
                        }
                        Ok(RebaseResult::Conflicts(status)) => {
                            let conflict_count = status.conflicted_files.len();
                            app.set_rebase_status(status);
                            app.set_status(format!(
                                "Rebase conflicts: {} file(s) need resolution. Press 'R' for rebase menu.",
                                conflict_count
                            ));
                        }
                        Ok(RebaseResult::Aborted) => {
                            app.set_status("Rebase was aborted");
                            app.clear_rebase_status();
                        }
                        Ok(RebaseResult::Failed(e)) => {
                            app.set_status(format!("Rebase failed: {}", e));
                        }
                        Err(e) => {
                            app.set_status(format!("Rebase error: {}", e));
                        }
                    }
                }
            }
        }
        KeyCode::Char('R') => {
            if let Some(session) = app.current_session() {
                let worktree_path = session.worktree_path.clone();
                if Git::is_rebase_in_progress(&worktree_path) {
                    match Git::get_rebase_status(&worktree_path) {
                        Ok(status) => app.set_rebase_status(status),
                        Err(e) => app.set_status(format!("Failed to get rebase status: {}", e)),
                    }
                } else {
                    app.set_status("No rebase in progress");
                }
            }
        }
        KeyCode::Char('a') => {
            if let Err(e) = app.archive_current_session() {
                app.set_status(format!("Failed to archive: {}", e));
            }
        }
        KeyCode::Enter => {
            let session_data = app.current_session().map(|s| (s.id.clone(), s.worktree_path.clone(), s.status.clone()));
            if let Some((_, _, status)) = session_data {
                if status == SessionStatus::Pending || status == SessionStatus::Stopped || status == SessionStatus::Completed || status == SessionStatus::Failed {
                    app.focus = Focus::Input;
                    app.insert_mode = true;
                    app.set_status("Type your prompt and press Enter to send");
                }
            }
        }
        KeyCode::Char('i') => {
            if app.is_current_session_running() {
                app.focus = Focus::Input;
                app.set_status("Enter input to send to Claude:");
            } else {
                app.set_status("No Claude running in this session");
            }
        }
        KeyCode::Char('s') => {
            if let Some(session) = app.current_session() {
                let session_id = session.id.clone();
                if app.running_sessions.remove(&session_id) {
                    app.claude_receivers.remove(&session_id);
                    app.set_status(format!("Stopped tracking: {}", session_id));
                }
            }
        }
        _ => {}
    }
    Ok(())
}
