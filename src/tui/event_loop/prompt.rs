//! Staged prompt sending and compaction lifecycle
//!
//! Handles sending staged prompts to agents, spawning/retrying compaction
//! agents, and auto-continuing after mid-turn compaction.

use crate::app::state::backend_for_model;
use crate::app::state::default_model;
use crate::app::App;
use crate::backend::AgentProcess;
use crate::backend::Backend;

/// Result of spawning an agent process, including data needed for registration.
struct SpawnOutcome {
    rx: std::sync::mpsc::Receiver<crate::claude::AgentEvent>,
    pid: u32,
    registration_model: Option<String>,
    success_notice: Option<String>,
}

/// Format a spawn failure that includes primary retries and fallback failure details.
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

/// Format a status notice when fallback backend spawning succeeds.
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

/// Spawn the selected backend with retry, falling back to the alternate backend on failure.
fn spawn_with_retry_and_fallback(
    claude_process: &AgentProcess,
    wt_path: &std::path::Path,
    prompt: &str,
    resume_session_id: Option<&str>,
    selected_model: Option<&str>,
    action: &str,
) -> Result<SpawnOutcome, String> {
    let selected_model = match selected_model {
        Some(model) => model,
        None => default_model(),
    };
    let primary_backend = backend_for_model(selected_model);
    let primary_label = selected_model.to_string();

    let mut primary_errors = Vec::new();
    for _ in 0..2 {
        match claude_process.spawn_on_backend(
            primary_backend,
            wt_path,
            prompt,
            resume_session_id,
            Some(selected_model),
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
                    registration_model: Some(selected_model.to_string()),
                    success_notice,
                });
            }
            Err(err) => primary_errors.push(err.to_string()),
        }
    }

    let fallback_backend = primary_backend.alternate();
    let fallback_model = match fallback_backend {
        Backend::Claude => None,
        Backend::Codex => Some(default_model()),
    };
    match claude_process.spawn_on_backend(
        fallback_backend,
        wt_path,
        prompt,
        resume_session_id,
        fallback_model,
    ) {
        Ok((rx, pid)) => Ok(SpawnOutcome {
            rx,
            pid,
            registration_model: fallback_model.map(str::to_string),
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

/// Return true when the prompt is a response to an agent pause/approval event.
fn is_pause_response_prompt(actual_prompt: &str) -> bool {
    actual_prompt.starts_with("[SYSTEM: You just called ExitPlanMode.")
        || actual_prompt.starts_with("[SYSTEM: You just called AskUserQuestion.")
}

/// Track a successfully spawned prompt for auto-prompt repeat decisions.
fn track_auto_prompt_spawn(
    app: &mut App,
    display_prompt: Option<&str>,
    actual_prompt: &str,
    pid: u32,
    branch: &str,
) {
    let slot = pid.to_string();
    if let Some(prompt) = display_prompt {
        if !is_pause_response_prompt(actual_prompt) {
            app.auto_prompt
                .capture_prompt(prompt, slot, branch, app.current_session_id);
        }
    } else if actual_prompt == crate::app::context_injection::AUTO_CONTINUE_PROMPT {
        app.auto_prompt.track_continuation_slot(slot);
    }
}

/// Return true when a stopped slot is only waiting on an agent pause workflow.
fn auto_prompt_blocked_by_pause(app: &App) -> bool {
    app.awaiting_plan_approval
        || app.awaiting_ask_user_question
        || app.rcr_session.is_some()
        || app.issue_session.is_some()
}

/// Stage the captured auto prompt when the tracked turn has really completed.
fn stage_auto_prompt_if_ready(app: &mut App) -> bool {
    if !app.auto_prompt.is_enabled() || app.staged_prompt.is_some() {
        return false;
    }

    let Some(tracked_slot) = app.auto_prompt.tracked_slot().map(str::to_string) else {
        return false;
    };
    let Some(tracked_branch) = app.auto_prompt.branch().map(str::to_string) else {
        app.auto_prompt.clear_tracked_turn();
        return false;
    };
    let tracked_session_id = app.auto_prompt.session_id();
    let current_branch = app.current_worktree().map(|wt| wt.branch_name.clone());
    if current_branch.as_deref() != Some(tracked_branch.as_str())
        || app.current_session_id != tracked_session_id
        || app.viewing_historic_session
    {
        return false;
    }
    if app.running_sessions.contains(&tracked_slot) {
        return false;
    }

    let Some(exit_code) = app.agent_exit_codes.get(&tracked_slot).copied() else {
        return false;
    };
    if exit_code != 0 {
        app.auto_prompt.clear_tracked_turn();
        return false;
    }
    if app.auto_continue_after_compaction {
        return false;
    }
    if auto_prompt_blocked_by_pause(app) {
        app.auto_prompt.clear_tracked_turn();
        return false;
    }
    if app.compaction_needed.is_some()
        || !app.compaction_receivers.is_empty()
        || app.compaction_retry_needed.is_some()
    {
        app.auto_prompt.defer_for_compaction();
        return false;
    }

    let Some(prompt) = app.auto_prompt.take_repeat_prompt() else {
        return false;
    };
    app.staged_prompt = Some(prompt);
    true
}

/// Send a prompt to the current worktree and optionally show it as a user message.
pub(crate) fn send_prompt_to_current_worktree(
    app: &mut App,
    claude_process: &AgentProcess,
    display_prompt: Option<&str>,
    actual_prompt: &str,
    action: &str,
    default_status: &str,
) -> bool {
    let Some(wt_path) = app.current_worktree().and_then(|s| s.worktree_path.clone()) else {
        app.set_status("Session has no worktree (archived?)");
        return false;
    };
    let branch = app
        .current_worktree()
        .map(|s| s.branch_name.clone())
        .unwrap_or_default();
    let events_offset = app.display_events.len();
    let send_prompt = app.build_context_prompt_for_current_session(actual_prompt);

    if let Some(prompt) = display_prompt {
        app.record_prompt_history(prompt);
        app.add_user_message(prompt.to_string());
        app.process_session_chunk(&format!("You: {}\n", prompt));
        app.current_todos.clear();
    }

    let selected_model = app.selected_model.clone();
    match spawn_with_retry_and_fallback(
        claude_process,
        &wt_path,
        &send_prompt,
        None,
        selected_model.as_deref(),
        action,
    ) {
        Ok(outcome) => {
            if let Some(sid) = app.current_session_id {
                app.pid_session_target.insert(
                    outcome.pid.to_string(),
                    (sid, wt_path.clone(), events_offset, app.session_file_size),
                );
            }
            app.register_claude(
                branch.clone(),
                outcome.pid,
                outcome.rx,
                outcome.registration_model.as_deref(),
            );
            track_auto_prompt_spawn(app, display_prompt, actual_prompt, outcome.pid, &branch);
            app.update_title_session_name();
            app.set_status(
                outcome
                    .success_notice
                    .unwrap_or_else(|| default_status.to_string()),
            );
        }
        Err(e) => app.set_status(e),
    }
    true
}

use super::agent_events;

/// Send staged prompt when no agent is running and no dialog is blocking.
/// Returns true if a prompt was sent (needs redraw).
pub fn send_staged_prompt(app: &mut App, claude_process: &AgentProcess) -> bool {
    if app.is_active_slot_running() || app.new_session_dialog_active {
        return false;
    }
    if app.staged_prompt.is_none() {
        stage_auto_prompt_if_ready(app);
    }
    if app.staged_prompt.is_none() {
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
        return send_prompt_to_current_worktree(
            app,
            claude_process,
            Some(&prompt),
            &prompt,
            "prompt start",
            "Running...",
        );
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
        redraw |= send_prompt_to_current_worktree(
            app,
            claude_process,
            None,
            crate::app::context_injection::AUTO_CONTINUE_PROMPT,
            "auto-continue after compaction",
            "Auto-continuing after compaction...",
        );
    }

    redraw
}

#[cfg(test)]
/// Tests for prompt spawning, staging, and auto-prompt scheduling.
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Build an app whose tracked auto-prompt slot has exited successfully.
    fn app_with_tracked_auto_prompt() -> App {
        let mut app = App::new();
        app.worktrees.push(crate::models::Worktree {
            branch_name: "feature".into(),
            worktree_path: Some(PathBuf::from("/tmp/feature")),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.current_session_id = Some(7);
        app.auto_prompt.toggle();
        app.auto_prompt
            .capture_prompt("repeat me", "42", "feature", Some(7));
        app.agent_exit_codes.insert("42".into(), 0);
        app
    }

    /// Spawn failure messages include retry details and fallback failure text.
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

    /// Fallback notices report the backend that actually spawned successfully.
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

    /// A zero-exit tracked turn queues the captured prompt for repeat.
    #[test]
    fn stage_auto_prompt_queues_after_completed_tracked_turn() {
        let mut app = app_with_tracked_auto_prompt();

        assert!(stage_auto_prompt_if_ready(&mut app));

        assert_eq!(app.staged_prompt.as_deref(), Some("repeat me"));
        assert!(app.auto_prompt.tracked_slot().is_none());
    }

    /// Compaction delays the repeat until the compaction request clears.
    #[test]
    fn stage_auto_prompt_defers_while_compaction_is_pending() {
        let mut app = app_with_tracked_auto_prompt();
        app.compaction_needed = Some((7, PathBuf::from("/tmp/feature")));

        assert!(!stage_auto_prompt_if_ready(&mut app));

        assert!(app.staged_prompt.is_none());
        assert!(app.auto_prompt.is_pending_after_compaction());

        app.compaction_needed = None;
        assert!(stage_auto_prompt_if_ready(&mut app));
        assert_eq!(app.staged_prompt.as_deref(), Some("repeat me"));
    }

    /// Agent pause workflows suppress repeats instead of treating the pause as completion.
    #[test]
    fn stage_auto_prompt_suppresses_agent_pause_turns() {
        let mut app = app_with_tracked_auto_prompt();
        app.awaiting_plan_approval = true;

        assert!(!stage_auto_prompt_if_ready(&mut app));

        assert!(app.staged_prompt.is_none());
        assert!(app.auto_prompt.tracked_slot().is_none());
    }
}
