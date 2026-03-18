//! Staged prompt sending and compaction lifecycle
//!
//! Handles sending staged prompts to agents, spawning/retrying compaction
//! agents, and auto-continuing after mid-turn compaction.

use crate::app::App;
use crate::backend::AgentProcess;

use super::agent_events;

/// Send staged prompt when no agent is running and no dialog is blocking.
/// Returns true if a prompt was sent (needs redraw).
pub fn send_staged_prompt(app: &mut App, claude_process: &AgentProcess) -> bool {
    if app.staged_prompt.is_none()
        || app.is_active_slot_running()
        || app.new_session_dialog_active
    {
        return false;
    }

    if let Some(prompt) = app.staged_prompt.take() {
        if let Some(wt_path) = app.current_worktree().and_then(|s| s.worktree_path.clone()) {
            let branch = app
                .current_worktree()
                .map(|s| s.branch_name.clone())
                .unwrap_or_default();
            let events_offset = app.display_events.len();
            app.add_user_message(prompt.clone());
            app.process_session_chunk(&format!("You: {}\n", prompt));
            app.current_todos.clear();
            let send_prompt = app
                .current_session_id
                .and_then(|sid| app.session_store.as_ref().map(|s| (sid, s)))
                .and_then(|(sid, store)| store.build_context(sid).ok().flatten())
                .map(|payload| {
                    crate::app::context_injection::build_context_prompt(&payload, &prompt)
                })
                .unwrap_or_else(|| prompt.clone());
            let selected_model = app.selected_model.clone();
            match claude_process.spawn(
                &wt_path,
                &send_prompt,
                None,
                selected_model.as_deref(),
            ) {
                Ok((rx, pid)) => {
                    if let Some(sid) = app.current_session_id {
                        app.pid_session_target.insert(
                            pid.to_string(),
                            (sid, wt_path.clone(), events_offset, app.session_file_size),
                        );
                    }
                    app.register_claude(branch, pid, rx, selected_model.as_deref());
                    app.update_title_session_name();
                    app.set_status("Running...");
                }
                Err(e) => app.set_status(format!("Failed to start: {}", e)),
            }
            return true;
        }
    }
    false
}

/// Manage compaction lifecycle: poll existing agents, spawn new ones when
/// threshold crossed, and handle retries. Returns true if needs redraw.
pub fn manage_compaction(app: &mut App, claude_process: &AgentProcess) -> bool {
    let mut redraw = false;

    // Poll compaction agents (background summarization, invisible to UI)
    agent_events::poll_compaction_agents(app);

    // Spawn compaction agent when threshold is crossed (mid-turn or post-exit).
    // Only consume the trigger if spawn succeeds — failed spawns set
    // compaction_spawn_deferred to avoid retrying every tick.
    if !app.compaction_spawn_deferred {
        if let Some((session_id, wt_path)) = app.compaction_needed.as_ref() {
            let (sid, wtp) = (*session_id, wt_path.clone());
            if agent_events::spawn_compaction_agent(app, claude_process, sid, &wtp) {
                app.compaction_needed = None;
            } else {
                app.compaction_spawn_deferred = true;
            }
        }
    }

    // Retry compaction if the primary produced no output
    if let Some((session_id, wt_path)) = app.compaction_retry_needed.as_ref() {
        let (sid, wtp) = (*session_id, wt_path.clone());
        if agent_events::spawn_compaction_agent(app, claude_process, sid, &wtp) {
            app.compaction_retry_needed = None;
        }
    }

    // Auto-continue after mid-turn compaction: once all compaction agents finish
    // (no receivers, no retry pending), spawn a hidden "continue" prompt with
    // fresh context injection (includes the new compaction summary). No user
    // bubble — the conversation resumes transparently.
    if app.auto_continue_after_compaction
        && app.compaction_receivers.is_empty()
        && app.compaction_retry_needed.is_none()
    {
        app.auto_continue_after_compaction = false;
        if let Some(wt_path) = app.current_worktree().and_then(|s| s.worktree_path.clone()) {
            let branch = app
                .current_worktree()
                .map(|s| s.branch_name.clone())
                .unwrap_or_default();
            let events_offset = app.display_events.len();
            let prompt = "Continue.".to_string();
            // Build context with compaction summary — no add_user_message (no bubble)
            let send_prompt = app
                .current_session_id
                .and_then(|sid| app.session_store.as_ref().map(|s| (sid, s)))
                .and_then(|(sid, store)| store.build_context(sid).ok().flatten())
                .map(|payload| {
                    crate::app::context_injection::build_context_prompt(&payload, &prompt)
                })
                .unwrap_or_else(|| prompt.clone());
            let selected_model = app.selected_model.clone();
            match claude_process.spawn(&wt_path, &send_prompt, None, selected_model.as_deref())
            {
                Ok((rx, pid)) => {
                    if let Some(sid) = app.current_session_id {
                        app.pid_session_target.insert(
                            pid.to_string(),
                            (sid, wt_path.clone(), events_offset, app.session_file_size),
                        );
                    }
                    app.register_claude(branch, pid, rx, selected_model.as_deref());
                    app.set_status("Auto-continuing after compaction...");
                    redraw = true;
                }
                Err(e) => app.set_status(format!("Auto-continue failed: {}", e)),
            }
        }
    }

    redraw
}
