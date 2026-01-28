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
                if let (Some(ref wt_path), Some(project)) = (&session.worktree_path, app.current_project()) {
                    let wt = wt_path.clone();
                    let main_branch = project.main_branch.clone();
                    match Git::rebase_onto_main(&wt, &main_branch) {
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
                } else {
                    app.set_status("Session has no worktree");
                }
            }
        }
        KeyCode::Char('R') => {
            if let Some(session) = app.current_session() {
                if let Some(ref wt_path) = session.worktree_path {
                    if Git::is_rebase_in_progress(wt_path) {
                        match Git::get_rebase_status(wt_path) {
                            Ok(status) => app.set_rebase_status(status),
                            Err(e) => app.set_status(format!("Failed to get rebase status: {}", e)),
                        }
                    } else {
                        app.set_status("No rebase in progress");
                    }
                }
            }
        }
        KeyCode::Char('a') => {
            if let Err(e) = app.archive_current_session() {
                app.set_status(format!("Failed to archive: {}", e));
            }
        }
        KeyCode::Enter => {
            if let Some(session) = app.current_session() {
                let status = session.status(&app.running_sessions);
                if status == SessionStatus::Pending || status == SessionStatus::Stopped
                    || status == SessionStatus::Completed || status == SessionStatus::Failed
                    || status == SessionStatus::Waiting
                {
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
                let branch_name = session.branch_name.clone();
                let session_name = session.name().to_string();
                if app.running_sessions.remove(&branch_name) {
                    app.claude_receivers.remove(&branch_name);
                    app.set_status(format!("Stopped tracking: {}", session_name));
                }
            }
        }
        _ => {}
    }
    Ok(())
}
