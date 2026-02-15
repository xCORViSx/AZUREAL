//! Claude process event handling
//!
//! Processes output, lifecycle, and error events from Claude subprocesses.
//! slot_id is the PID string — each Claude process gets a unique slot.
//! Also handles auto-sending staged prompts after Claude exits.

use anyhow::Result;
use crate::app::App;
use crate::claude::{ClaudeEvent, ClaudeProcess};

/// Handle Claude process events for a specific slot (PID string).
/// After an exit event, auto-sends any staged prompt.
pub fn handle_claude_event(slot_id: &str, event: ClaudeEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    let is_exit = matches!(event, ClaudeEvent::Exited { .. });

    match event {
        ClaudeEvent::Output(output) => app.handle_claude_output(slot_id, output.output_type, output.data),
        ClaudeEvent::Started { pid } => app.handle_claude_started(slot_id, pid),
        ClaudeEvent::SessionId(claude_session_id) => app.set_claude_session_id(slot_id, claude_session_id),
        ClaudeEvent::Exited { code } => app.handle_claude_exited(slot_id, code),
        ClaudeEvent::Error(e) => app.handle_claude_error(slot_id, e),
    }

    // Auto-send staged prompt after Claude exits
    if is_exit {
        // Auto-rebase: if enabled for this slot's branch, rebase from main after exit
        auto_rebase_on_exit(slot_id, app);

        if app.staged_prompt.is_some() {
            app.check_session_file();
            app.poll_session_file();
        }
        if let Some(prompt) = app.staged_prompt.take() {
            if let Some(wt_path) = app.current_worktree().and_then(|s| s.worktree_path.clone()) {
                let branch = app.current_worktree().map(|s| s.branch_name.clone()).unwrap_or_default();
                app.add_user_message(prompt.clone());
                app.process_output_chunk(&format!("You: {}\n", prompt));
                app.current_todos.clear();
                let resume_id = app.get_claude_session_id(&branch).cloned();
                match claude_process.spawn(&wt_path, &prompt, resume_id.as_deref()) {
                    Ok((rx, pid)) => { app.register_claude(branch, pid, rx); app.set_status("Running..."); }
                    Err(e) => app.set_status(format!("Failed to start: {}", e)),
                }
            }
        }
    }
    Ok(())
}

/// If auto-rebase is enabled for the exiting slot's worktree, run rebase from main.
/// Reads `[git] auto-rebase` from the worktree's own `.azureal/azufig.toml`.
fn auto_rebase_on_exit(slot_id: &str, app: &mut App) {
    // Find which branch this slot belongs to
    let branch = match app.branch_for_slot(slot_id) {
        Some(b) => b,
        None => return,
    };
    // Find the session to get worktree path + main branch
    let (wt_path, main_branch) = match app.worktrees.iter().find(|s| s.branch_name == branch) {
        Some(s) => match (s.worktree_path.as_ref(), app.project.as_ref()) {
            (Some(wt), Some(proj)) => (wt.clone(), proj.main_branch.clone()),
            _ => return,
        },
        None => return,
    };
    // Check worktree-local config
    if !crate::azufig::is_autorebase_enabled(&wt_path) { return; }

    // Don't rebase the main branch onto itself
    if branch == main_branch { return; }

    match crate::git::Git::rebase_onto_main(&wt_path, &main_branch) {
        Ok(crate::models::RebaseResult::Success) => {
            app.set_status(format!("Auto-rebase: {} rebased", branch));
        }
        Ok(crate::models::RebaseResult::UpToDate) => {}
        Ok(crate::models::RebaseResult::Conflicts(s)) => {
            app.set_status(format!("Auto-rebase: {} has {} conflicts", branch, s.conflicted_files.len()));
        }
        Ok(crate::models::RebaseResult::Failed(e)) => {
            app.set_status(format!("Auto-rebase failed: {}", e));
        }
        Ok(crate::models::RebaseResult::Aborted) => {
            app.set_status(format!("Auto-rebase aborted: {}", branch));
        }
        Err(e) => {
            app.set_status(format!("Auto-rebase error: {}", e));
        }
    }
}
