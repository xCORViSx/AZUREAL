//! Staged prompt sending and compaction lifecycle
//!
//! Handles sending staged prompts to agents, spawning/retrying compaction
//! agents, and auto-continuing after mid-turn compaction.

use crate::app::App;
use crate::app::state::backend_for_model;
use crate::backend::AgentProcess;
use crate::backend::Backend;

struct SpawnOutcome {
    rx: std::sync::mpsc::Receiver<crate::claude::AgentEvent>,
    pid: u32,
    registration_model: Option<String>,
    success_notice: Option<String>,
}

fn format_spawn_failure(
    action: &str,
    primary_label: &str,
    primary_errors: &[String],
    fallback_backend: Backend,
    fallback_error: &str,
) -> String {
    let attempts = primary_errors
        .iter()
        .enumerate()
        .map(|(idx, err)| format!("attempt {}: {}", idx + 1, err))
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "{} failed: {} spawn failed ({}); {} fallback failed: {}",
        action, primary_label, attempts, fallback_backend, fallback_error
    )
}

fn format_fallback_notice(
    action: &str,
    primary_label: &str,
    primary_errors: &[String],
    fallback_backend: Backend,
) -> String {
    let last_error = primary_errors
        .last()
        .cloned()
        .unwrap_or_else(|| "unknown error".to_string());
    format!(
        "{} via {} after {} spawn failed ({}).",
        action, fallback_backend, primary_label, last_error
    )
}

fn spawn_with_retry_and_fallback(
    claude_process: &AgentProcess,
    wt_path: &std::path::Path,
    prompt: &str,
    resume_session_id: Option<&str>,
    selected_model: Option<&str>,
    action: &str,
) -> Result<SpawnOutcome, String> {
    let primary_backend = selected_model
        .map(backend_for_model)
        .unwrap_or(Backend::Claude);
    let primary_label = selected_model
        .map(str::to_string)
        .unwrap_or_else(|| primary_backend.to_string());

    let mut primary_errors = Vec::new();
    for _ in 0..2 {
        match claude_process.spawn_on_backend(
            primary_backend,
            wt_path,
            prompt,
            resume_session_id,
            selected_model,
        ) {
            Ok((rx, pid)) => {
                let success_notice = if primary_errors.is_empty() {
                    None
                } else {
                    Some(format!(
                        "{} succeeded on retry after {} spawn failed once ({})",
                        action,
                        primary_label,
                        primary_errors.last().unwrap_or(&String::new())
                    ))
                };
                return Ok(SpawnOutcome {
                    rx,
                    pid,
                    registration_model: selected_model.map(str::to_string),
                    success_notice,
                });
            }
            Err(err) => primary_errors.push(err.to_string()),
        }
    }

    let fallback_backend = primary_backend.alternate();
    match claude_process.spawn_on_backend(
        fallback_backend,
        wt_path,
        prompt,
        resume_session_id,
        None,
    ) {
        Ok((rx, pid)) => Ok(SpawnOutcome {
            rx,
            pid,
            registration_model: match fallback_backend {
                Backend::Claude => None,
                Backend::Codex => Some("codex".to_string()),
            },
            success_notice: Some(format_fallback_notice(
                action,
                &primary_label,
                &primary_errors,
                fallback_backend,
            )),
        }),
        Err(err) => Err(format_spawn_failure(
            action,
            &primary_label,
            &primary_errors,
            fallback_backend,
            &err.to_string(),
        )),
    }
}

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

    // Issue session: first prompt spawns the issue agent with hidden system prompt
    if let Some(ref issue) = app.issue_session {
        if issue.slot_id.is_empty() {
            if let Some(prompt) = app.staged_prompt.take() {
                let cached_json = app
                    .issue_session
                    .as_ref()
                    .map(|i| i.cached_issues_json.clone())
                    .unwrap_or_default();
                app.spawn_issue_session(&prompt, &cached_json, claude_process);
                return true;
            }
        }
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
            match spawn_with_retry_and_fallback(
                claude_process,
                &wt_path,
                &send_prompt,
                None,
                selected_model.as_deref(),
                "prompt start",
            ) {
                Ok(outcome) => {
                    if let Some(sid) = app.current_session_id {
                        app.pid_session_target.insert(
                            outcome.pid.to_string(),
                            (sid, wt_path.clone(), events_offset, app.session_file_size),
                        );
                    }
                    app.register_claude(
                        branch,
                        outcome.pid,
                        outcome.rx,
                        outcome.registration_model.as_deref(),
                    );
                    app.update_title_session_name();
                    app.set_status(outcome.success_notice.unwrap_or_else(|| "Running...".to_string()));
                }
                Err(e) => app.set_status(e),
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
    //
    // Safety net: if a Complete event appeared in display_events (the agent
    // finished before or during the compaction), skip the auto-continue —
    // there's nothing to continue.
    if app.auto_continue_after_compaction
        && app.compaction_receivers.is_empty()
        && app.compaction_retry_needed.is_none()
    {
        let session_completed = app
            .display_events
            .iter()
            .rev()
            .take(20)
            .any(|e| matches!(e, crate::events::DisplayEvent::Complete { .. }));
        if session_completed {
            app.auto_continue_after_compaction = false;
            app.set_status("Compaction complete — session already finished.");
            return true;
        }
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
            match spawn_with_retry_and_fallback(
                claude_process,
                &wt_path,
                &send_prompt,
                None,
                selected_model.as_deref(),
                "auto-continue after compaction",
            ) {
                Ok(outcome) => {
                    if let Some(sid) = app.current_session_id {
                        app.pid_session_target.insert(
                            outcome.pid.to_string(),
                            (sid, wt_path.clone(), events_offset, app.session_file_size),
                        );
                    }
                    app.register_claude(
                        branch,
                        outcome.pid,
                        outcome.rx,
                        outcome.registration_model.as_deref(),
                    );
                    app.set_status(
                        outcome.success_notice.unwrap_or_else(|| {
                            "Auto-continuing after compaction...".to_string()
                        }),
                    );
                    redraw = true;
                }
                Err(e) => app.set_status(e),
            }
        }
    }

    redraw
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_spawn_failure_includes_attempts_and_fallback_error() {
        let msg = format_spawn_failure(
            "auto-continue after compaction",
            "gpt-5.4",
            &["E2BIG".into(), "ENOENT".into()],
            Backend::Claude,
            "missing claude binary",
        );
        assert!(msg.contains("auto-continue after compaction failed"));
        assert!(msg.contains("gpt-5.4 spawn failed"));
        assert!(msg.contains("attempt 1: E2BIG"));
        assert!(msg.contains("attempt 2: ENOENT"));
        assert!(msg.contains("claude fallback failed: missing claude binary"));
    }

    #[test]
    fn format_fallback_notice_includes_real_backend_and_error() {
        let msg = format_fallback_notice(
            "auto-continue after compaction",
            "gpt-5.4",
            &["argument list too long".into()],
            Backend::Claude,
        );
        assert_eq!(
            msg,
            "auto-continue after compaction via claude after gpt-5.4 spawn failed (argument list too long)."
        );
    }
}
