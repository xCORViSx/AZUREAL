//! Rebase view input handling

use anyhow::Result;
use crossterm::event::{self, KeyCode};

use crate::app::{App, Focus, ViewMode};
use crate::git::Git;
use crate::models::RebaseResult;

/// Handle keyboard input when in Rebase view mode
pub fn handle_rebase_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.select_next_conflict(),
        KeyCode::Char('k') | KeyCode::Up => app.select_prev_conflict(),
        KeyCode::Char('c') => {
            if let Some(session) = app.current_session() {
                if let Some(ref wt_path) = session.worktree_path {
                    match Git::rebase_continue(wt_path) {
                        Ok(RebaseResult::Success) => {
                            app.set_status("Rebase completed successfully");
                            app.clear_rebase_status();
                        }
                        Ok(RebaseResult::Conflicts(status)) => {
                            let conflict_count = status.conflicted_files.len();
                            app.set_rebase_status(status);
                            app.set_status(format!("More conflicts: {} file(s) need resolution", conflict_count));
                        }
                        Ok(RebaseResult::Failed(e)) => {
                            app.set_status(format!("Continue failed: {}", e));
                        }
                        Err(e) => {
                            app.set_status(format!("Error: {}", e));
                        }
                        _ => {}
                    }
                } else {
                    app.set_status("Session has no worktree (archived?)");
                }
            }
        }
        KeyCode::Char('A') => {
            if let Some(session) = app.current_session() {
                if let Some(ref wt_path) = session.worktree_path {
                    match Git::rebase_abort(wt_path) {
                        Ok(RebaseResult::Aborted) => {
                            app.set_status("Rebase aborted");
                            app.clear_rebase_status();
                        }
                        Ok(RebaseResult::Failed(e)) => {
                            app.set_status(format!("Abort failed: {}", e));
                        }
                        Err(e) => {
                            app.set_status(format!("Error: {}", e));
                        }
                        _ => {}
                    }
                } else {
                    app.set_status("Session has no worktree (archived?)");
                }
            }
        }
        KeyCode::Char('s') => {
            if let Some(session) = app.current_session() {
                if let Some(ref wt_path) = session.worktree_path {
                    match Git::rebase_skip(wt_path) {
                        Ok(RebaseResult::Success) => {
                            app.set_status("Rebase completed successfully");
                            app.clear_rebase_status();
                        }
                        Ok(RebaseResult::Conflicts(status)) => {
                            let conflict_count = status.conflicted_files.len();
                            app.set_rebase_status(status);
                            app.set_status(format!("More conflicts: {} file(s) need resolution", conflict_count));
                        }
                        Ok(RebaseResult::Failed(e)) => {
                            app.set_status(format!("Skip failed: {}", e));
                        }
                        Err(e) => {
                            app.set_status(format!("Error: {}", e));
                        }
                        _ => {}
                    }
                } else {
                    app.set_status("Session has no worktree (archived?)");
                }
            }
        }
        KeyCode::Char('o') => {
            let session_data = app.current_session().and_then(|s| {
                s.worktree_path.clone().map(|wt| (wt, app.current_conflict_file().map(|f| f.to_string())))
            });
            if let Some((wt_path, Some(file))) = session_data {
                match Git::resolve_using_ours(&wt_path, &file) {
                    Ok(()) => {
                        app.set_status(format!("Resolved {} using ours", file));
                        if let Ok(status) = Git::get_rebase_status(&wt_path) {
                            if status.conflicted_files.is_empty() {
                                app.set_status("All conflicts resolved. Press 'c' to continue rebase.");
                            }
                            app.set_rebase_status(status);
                        }
                    }
                    Err(e) => {
                        app.set_status(format!("Failed to resolve: {}", e));
                    }
                }
            } else if app.current_session().is_some() {
                app.set_status("Session has no worktree (archived?)");
            }
        }
        KeyCode::Char('t') => {
            let session_data = app.current_session().and_then(|s| {
                s.worktree_path.clone().map(|wt| (wt, app.current_conflict_file().map(|f| f.to_string())))
            });
            if let Some((wt_path, Some(file))) = session_data {
                match Git::resolve_using_theirs(&wt_path, &file) {
                    Ok(()) => {
                        app.set_status(format!("Resolved {} using theirs", file));
                        if let Ok(status) = Git::get_rebase_status(&wt_path) {
                            if status.conflicted_files.is_empty() {
                                app.set_status("All conflicts resolved. Press 'c' to continue rebase.");
                            }
                            app.set_rebase_status(status);
                        }
                    }
                    Err(e) => {
                        app.set_status(format!("Failed to resolve: {}", e));
                    }
                }
            } else if app.current_session().is_some() {
                app.set_status("Session has no worktree (archived?)");
            }
        }
        KeyCode::Enter => {
            if let Some(session) = app.current_session() {
                if let Some(ref wt_path) = session.worktree_path {
                    if let Some(file) = app.current_conflict_file() {
                        let file = file.to_string();
                        match Git::get_conflict_diff(wt_path, &file) {
                            Ok(diff) => {
                                app.diff_text = Some(diff);
                                app.diff_lines_dirty = true;
                                app.view_mode = ViewMode::Diff;
                            }
                            Err(e) => {
                                app.set_status(format!("Failed to get diff: {}", e));
                            }
                        }
                    }
                } else {
                    app.set_status("Session has no worktree (archived?)");
                }
            }
        }
        KeyCode::Esc => {
            app.view_mode = ViewMode::Output;
            app.focus = Focus::Worktrees;
        }
        _ => {}
    }
    Ok(())
}
