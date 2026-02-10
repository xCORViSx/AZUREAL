//! Claude process event handling
//!
//! Processes output, lifecycle, and error events from Claude subprocesses.
//! Also handles auto-sending staged prompts after Claude exits, and
//! advancing the god-file modularization queue.

use anyhow::Result;
use crate::app::App;
use crate::claude::{ClaudeEvent, ClaudeProcess};

/// Handle Claude process events for a specific session.
/// After an exit event, auto-sends any staged prompt (user hit Enter mid-convo
/// which cancelled the old run and staged the new prompt in one keystroke).
pub fn handle_claude_event(session_id: &str, event: ClaudeEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    let is_exit = matches!(event, ClaudeEvent::Exited { .. });
    match event {
        ClaudeEvent::Output(output) => app.handle_claude_output(session_id, output.output_type, output.data),
        ClaudeEvent::Started { pid } => app.handle_claude_started(session_id, pid),
        ClaudeEvent::SessionId(claude_session_id) => app.set_claude_session_id(session_id, claude_session_id),
        ClaudeEvent::Exited { code } => app.handle_claude_exited(session_id, code),
        ClaudeEvent::Error(e) => app.handle_claude_error(session_id, e),
    }

    // Auto-send staged prompt after Claude exits — no second Enter needed.
    // CRITICAL: force a session file re-parse BEFORE spawning the new process.
    // handle_claude_exited() sets parse_offset=0 + dirty=true, but once the new
    // process starts, is_current_session_running() returns true and poll_session_file()
    // skips the parse. Without this, user messages and responses from the previous
    // turn never get loaded from the JSONL (they only existed as live-stream events
    // which were cleared), causing messages to vanish.
    if is_exit {
        if app.staged_prompt.is_some() {
            // Session is NOT running right now (just exited) — parse will succeed
            app.check_session_file();
            app.poll_session_file();
        }
        if let Some(prompt) = app.staged_prompt.take() {
            if let Some(wt_path) = app.current_session().and_then(|s| s.worktree_path.clone()) {
                let branch = app.current_session().map(|s| s.branch_name.clone()).unwrap_or_default();
                app.add_user_message(prompt.clone());
                app.process_output_chunk(&format!("You: {}\n", prompt));
                app.current_todos.clear();
                let resume_id = app.get_claude_session_id(&branch).cloned();
                match claude_process.spawn(&wt_path, &prompt, resume_id.as_deref()) {
                    Ok(rx) => { app.register_claude(branch, rx); app.set_status("Running..."); }
                    Err(e) => app.set_status(format!("Failed to start: {}", e)),
                }
            }
        }
        // God file modularization queue: when main branch session exits,
        // auto-start the next queued file if any remain
        if !app.god_file_queue.is_empty() {
            let is_main = app.project.as_ref()
                .map(|p| p.main_branch == session_id)
                .unwrap_or(false);
            if is_main {
                app.god_file_advance_queue(claude_process);
            }
        }
    }
    Ok(())
}
