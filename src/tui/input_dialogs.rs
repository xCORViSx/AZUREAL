//! Context menu and branch dialog input handling

use anyhow::Result;
use crossterm::event::{self, KeyCode};

use crate::app::{App, Focus, SessionAction};
use crate::claude::ClaudeProcess;
use crate::git::Git;

/// Handle keyboard input when context menu is open
pub fn handle_context_menu_input(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.context_menu_next(),
        KeyCode::Char('k') | KeyCode::Up => app.context_menu_prev(),
        KeyCode::Enter => {
            if let Some(action) = app.selected_action() {
                execute_action(app, claude_process, action)?;
            }
            app.close_context_menu();
        }
        KeyCode::Esc => app.close_context_menu(),
        _ => {}
    }
    Ok(())
}

/// Execute a session action from the context menu
fn execute_action(app: &mut App, _claude_process: &ClaudeProcess, action: SessionAction) -> Result<()> {
    match action {
        SessionAction::Start => {
            if let Some(session) = app.current_session() {
                if app.is_session_running(&session.branch_name) {
                    app.set_status("Claude already running in this session");
                } else {
                    app.focus = Focus::Input;
                    app.set_status("Enter your prompt");
                }
            }
        }
        SessionAction::Stop => {
            app.set_status("Stop action not yet implemented");
        }
        SessionAction::Archive => {
            if let Err(e) = app.archive_current_session() {
                app.set_status(format!("Failed to archive: {}", e));
            }
        }
        SessionAction::Delete => {
            app.set_status("Delete action not yet implemented - use CLI: azureal session delete");
        }
        SessionAction::ViewDiff => {
            if let Err(e) = app.load_diff() {
                app.set_status(format!("Failed to get diff: {}", e));
            }
        }
        SessionAction::RebaseFromMain => {
            if let Err(e) = app.rebase_current_session() {
                app.set_status(format!("Rebase failed: {}", e));
            }
        }
        SessionAction::OpenInEditor => {
            if let Some(session) = app.current_session() {
                if let Some(ref wt_path) = session.worktree_path {
                    let path = wt_path.display().to_string();
                    app.set_status(format!("Editor integration not implemented. Path: {}", path));
                }
            }
        }
        SessionAction::CopyWorktreePath => {
            if let Some(session) = app.current_session() {
                if let Some(ref wt_path) = session.worktree_path {
                    let path = wt_path.display().to_string();
                    app.set_status(format!("Copied to clipboard (not implemented): {}", path));
                }
            }
        }
    }
    Ok(())
}

/// Handle keyboard input when Branch dialog is focused
pub fn handle_branch_dialog_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    if let Some(ref mut dialog) = app.branch_dialog {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => dialog.select_next(),
            KeyCode::Up | KeyCode::Char('k') => dialog.select_prev(),
            KeyCode::Backspace => dialog.filter_backspace(),
            KeyCode::Enter => {
                if let Some(branch) = dialog.selected_branch().cloned() {
                    if let Some(project) = app.current_project().cloned() {
                        // Create worktree from existing branch
                        let worktree_name = branch.strip_prefix("azureal/").unwrap_or(&branch);
                        let worktree_path = project.worktrees_dir().join(worktree_name);

                        match Git::create_worktree(&project.path, &worktree_path, &branch) {
                            Ok(()) => {
                                app.set_status(format!("Created worktree: {}", worktree_name));
                                let _ = app.refresh_sessions();
                            }
                            Err(e) => app.set_status(format!("Failed to create worktree: {}", e)),
                        }
                    }
                    app.close_branch_dialog();
                }
            }
            KeyCode::Esc => app.close_branch_dialog(),
            KeyCode::Char(c) => dialog.filter_char(c),
            _ => {}
        }
    } else {
        app.focus = Focus::Worktrees;
    }
    Ok(())
}
