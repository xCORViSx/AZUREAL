//! Worktrees panel input handling

use anyhow::Result;
use crossterm::event::{self, KeyCode, KeyModifiers};

use crate::app::{App, Focus};
use crate::git::Git;
use crate::models::{RebaseResult, SessionStatus};

/// Handle keyboard input when Worktrees pane is focused
pub fn handle_worktrees_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    // When sidebar filter is active, typing goes to filter input (not commands)
    if app.sidebar_filter_active {
        match key.code {
            KeyCode::Esc => {
                // Clear filter and deactivate
                app.sidebar_filter.clear();
                app.sidebar_filter_active = false;
                app.invalidate_sidebar();
            }
            KeyCode::Enter => {
                // Accept filter (keep text, exit filter input mode)
                app.sidebar_filter_active = false;
            }
            KeyCode::Backspace => {
                app.sidebar_filter.pop();
                if app.sidebar_filter.is_empty() {
                    app.sidebar_filter_active = false;
                }
                app.snap_selection_to_filter();
                app.invalidate_sidebar();
            }
            // Navigate filtered results while typing
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

    // Check if current session is expanded (dropdown mode)
    let is_expanded = app.is_current_session_expanded();

    match (key.modifiers, key.code) {
        // / activates sidebar search filter
        (KeyModifiers::NONE, KeyCode::Char('/')) => {
            app.sidebar_filter_active = true;
            app.sidebar_filter.clear();
            app.invalidate_sidebar();
        }
        // ⌥↑/⌥↓: jump to first/last within current context (must come before plain ↑/↓)
        (KeyModifiers::ALT, KeyCode::Up) => {
            if is_expanded { app.session_file_first(); } else { app.select_first_session(); }
        }
        (KeyModifiers::ALT, KeyCode::Down) => {
            if is_expanded { app.session_file_last(); } else { app.select_last_session(); }
        }
        // Right: Expand dropdown to show session files
        (_, KeyCode::Right) | (_, KeyCode::Char('l')) if !is_expanded => {
            if let Some(session) = app.current_session() {
                let branch = session.branch_name.clone();
                app.expand_session(&branch);
            }
        }
        // Left: Collapse dropdown
        (_, KeyCode::Left) | (_, KeyCode::Char('h')) if is_expanded => {
            if let Some(session) = app.current_session() {
                let branch = session.branch_name.clone();
                app.collapse_session(&branch);
            }
        }
        // j/k: Navigate within dropdown when expanded, otherwise navigate sessions
        (_, KeyCode::Char('j')) | (_, KeyCode::Down) => {
            if is_expanded {
                app.session_file_next();
            } else {
                app.select_next_session();
            }
        }
        (_, KeyCode::Char('k')) | (_, KeyCode::Up) => {
            if is_expanded {
                app.session_file_prev();
            } else {
                app.select_prev_session();
            }
        }
        (_, KeyCode::Tab) => app.focus = Focus::Output,
        (_, KeyCode::Char(' ')) | (_, KeyCode::Char('?')) => app.open_context_menu(),
        (_, KeyCode::Char('n')) => app.start_wizard(),
        (_, KeyCode::Char('b')) => {
            if let Some(project) = app.current_project() {
                match Git::list_available_branches(&project.path) {
                    Ok(branches) => app.open_branch_dialog(branches),
                    Err(e) => app.set_status(format!("Failed to list branches: {}", e)),
                }
            }
        }
        (_, KeyCode::Char('d')) => {
            if let Err(e) = app.load_diff() {
                app.set_status(format!("Failed to get diff: {}", e));
            }
        }
        // r: open run command picker (or execute directly if only 1 command)
        (KeyModifiers::NONE, KeyCode::Char('r')) => {
            app.open_run_command_picker();
        }
        // ⌥r: open dialog to add a new run command
        (KeyModifiers::ALT, KeyCode::Char('r')) => {
            app.open_run_command_dialog();
        }
        // macOS ⌥+letter produces unicode — use macos_opt_key() lookup
        (_, KeyCode::Char(c)) if super::keybindings::macos_opt_key(c) == Some('r') => {
            app.open_run_command_dialog();
        }
        // R (Shift+R): rebase current worktree onto main
        (_, KeyCode::Char('R')) => {
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
                                "Rebase conflicts: {} file(s) need resolution",
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
        (_, KeyCode::Char('a')) => {
            if let Err(e) = app.archive_current_session() {
                app.set_status(format!("Failed to archive: {}", e));
            }
        }
        (_, KeyCode::Enter) => {
            if is_expanded {
                // Select the highlighted session file and load it
                if let Some(session) = app.current_session() {
                    let branch = session.branch_name.clone();
                    let idx = *app.session_selected_file_idx.get(&branch).unwrap_or(&0);
                    app.select_session_file(&branch, idx);
                    app.collapse_session(&branch);
                    app.set_status("Loaded selected session file");
                }
            } else if let Some(session) = app.current_session() {
                let status = session.status(&app.running_sessions);
                if status == SessionStatus::Pending || status == SessionStatus::Stopped
                    || status == SessionStatus::Completed || status == SessionStatus::Failed
                    || status == SessionStatus::Waiting
                {
                    app.focus = Focus::Input;
                    app.prompt_mode = true;
                    app.set_status("Type your prompt and press Enter to send");
                }
            }
        }
        (_, KeyCode::Char('i')) => {
            if app.is_current_session_running() {
                app.focus = Focus::Input;
                app.set_status("Enter input to send to Claude:");
            } else {
                app.set_status("No Claude running in this session");
            }
        }
        (_, KeyCode::Char('s')) => {
            if let Some(session) = app.current_session() {
                let branch_name = session.branch_name.clone();
                let session_name = session.name().to_string();
                if app.running_sessions.remove(&branch_name) {
                    app.claude_receivers.remove(&branch_name);
                    app.invalidate_sidebar(); // Status indicator changed
                    app.set_status(format!("Stopped tracking: {}", session_name));
                }
            }
        }
        _ => {}
    }
    Ok(())
}
