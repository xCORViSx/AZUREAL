//! Staged prompt sending and compaction lifecycle
//!
//! Handles sending staged prompts to agents, spawning/retrying compaction
//! agents, and auto-continuing after mid-turn compaction.

use crate::app::state::{backend_for_model, default_model, AutoPromptKey, AutoPromptTarget};
use crate::app::App;
use crate::backend::AgentProcess;
use crate::backend::Backend;
use crate::events::DisplayEvent;

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

/// Build a context-injected prompt for a specific session target.
fn build_context_prompt_for_target(app: &App, target: &AutoPromptTarget, prompt: &str) -> String {
    let is_current_target = app
        .current_auto_prompt_target()
        .map(|current| current.key() == target.key())
        .unwrap_or(false);
    if is_current_target {
        return app.build_context_prompt_for_current_session(prompt);
    }

    let store = match crate::app::session_store::SessionStore::open(target.key().worktree_path()) {
        Ok(store) => store,
        Err(_) => return prompt.to_string(),
    };
    let payload = match store.build_context(target.key().session_id()) {
        Ok(Some(payload)) => payload,
        Ok(None) | Err(_) => return prompt.to_string(),
    };
    crate::app::context_injection::build_context_prompt(&payload, prompt)
}

/// Return true when the target session is the currently visible session pane.
fn target_is_visible(app: &App, target: &AutoPromptTarget) -> bool {
    app.current_auto_prompt_target()
        .map(|current| current.key() == target.key())
        .unwrap_or(false)
}

/// Return true when the target has a current project or snapshot to own the slot.
fn target_project_is_registered(app: &App, target: &AutoPromptTarget) -> bool {
    let is_current_project =
        target.project_path() == app.project.as_ref().map(|project| project.path.as_path());
    is_current_project
        || target
            .project_path()
            .map(|path| app.project_snapshots.contains_key(path))
            .unwrap_or(true)
}

/// Count already-stored events for a target session before a background send.
fn stored_event_count(target: &AutoPromptTarget) -> usize {
    crate::app::session_store::SessionStore::open(target.key().worktree_path())
        .and_then(|store| store.count_events(target.key().session_id(), None))
        .unwrap_or(0)
}

/// Register a spawned prompt against either the active project or a background snapshot.
fn register_prompt_process(
    app: &mut App,
    target: &AutoPromptTarget,
    outcome: SpawnOutcome,
    make_visible: bool,
    events_offset: usize,
    session_file_size: u64,
) -> u32 {
    let slot = outcome.pid.to_string();
    app.agent_receivers.insert(slot.clone(), outcome.rx);
    app.running_sessions.insert(slot.clone());
    let backend = outcome
        .registration_model
        .as_deref()
        .map(backend_for_model)
        .unwrap_or(Backend::Claude);
    app.agent_slot_models.insert(
        slot.clone(),
        outcome
            .registration_model
            .as_deref()
            .map(str::to_string)
            .unwrap_or_else(|| backend.to_string()),
    );
    match backend {
        Backend::Codex => {
            app.codex_slot_started_at
                .insert(slot.clone(), std::time::Instant::now());
        }
        Backend::Claude => {
            app.codex_slot_started_at.remove(&slot);
        }
    }

    if let Some(project_path) = target.project_path() {
        app.slot_to_project
            .insert(slot.clone(), project_path.to_path_buf());
    } else if let Some(project) = app.project.as_ref() {
        app.slot_to_project
            .insert(slot.clone(), project.path.clone());
    }

    let is_current_project =
        target.project_path() == app.project.as_ref().map(|project| project.path.as_path());
    if is_current_project {
        app.branch_slots
            .entry(target.branch().to_string())
            .or_default()
            .push(slot.clone());
        app.pid_session_target.insert(
            slot.clone(),
            (
                target.key().session_id(),
                target.key().worktree_path().to_path_buf(),
                events_offset,
                session_file_size,
            ),
        );
        if make_visible {
            app.active_slot
                .insert(target.branch().to_string(), slot.clone());
            app.viewing_historic_session = false;
            app.last_session_event_time = std::time::Instant::now();
            app.compaction_banner_injected = false;
        }
        app.invalidate_sidebar();
    } else if let Some(project_path) = target.project_path().map(std::path::Path::to_path_buf) {
        if let Some(snapshot) = app.project_snapshots.get_mut(&project_path) {
            snapshot
                .branch_slots
                .entry(target.branch().to_string())
                .or_default()
                .push(slot.clone());
            snapshot.pid_session_target.insert(
                slot.clone(),
                (
                    target.key().session_id(),
                    target.key().worktree_path().to_path_buf(),
                    events_offset,
                    session_file_size,
                ),
            );
        }
    }

    outcome.pid
}

/// Track a successfully spawned prompt for auto-prompt repeat decisions.
fn track_auto_prompt_spawn(
    app: &mut App,
    target: &AutoPromptTarget,
    display_prompt: Option<&str>,
    actual_prompt: &str,
    pid: u32,
) {
    let slot = pid.to_string();
    if let Some(prompt) = display_prompt {
        app.auto_prompt.capture_prompt(target.clone(), prompt, slot);
    } else if actual_prompt == crate::app::context_injection::AUTO_CONTINUE_PROMPT {
        app.auto_prompt.track_continuation_slot(target.key(), slot);
    }
}

/// Cancel the current auto-prompt loop when a manual prompt changes the repeat text.
fn cancel_auto_prompt_if_prompt_changed(
    app: &mut App,
    target: &AutoPromptTarget,
    display_prompt: Option<&str>,
) -> bool {
    display_prompt
        .map(|prompt| {
            app.auto_prompt
                .cancel_if_prompt_differs(target.key(), prompt)
        })
        .unwrap_or(false)
}

/// Return true when events contain a pause tool call without a later user response.
fn events_have_unanswered_pause(events: &[DisplayEvent]) -> bool {
    let mut pending_pause = false;
    for event in events {
        match event {
            DisplayEvent::ToolCall { tool_name, .. }
                if tool_name == "ExitPlanMode" || tool_name == "AskUserQuestion" =>
            {
                pending_pause = true;
            }
            DisplayEvent::UserMessage { .. } => pending_pause = false,
            _ => {}
        }
    }
    pending_pause
}

/// Return true when the target session is waiting on a user-answer pause.
fn target_has_unanswered_pause(app: &App, key: &AutoPromptKey) -> bool {
    let is_current_target = app
        .current_auto_prompt_target()
        .map(|target| target.key() == key)
        .unwrap_or(false);
    if is_current_target {
        return app.awaiting_plan_approval
            || app.awaiting_ask_user_question
            || events_have_unanswered_pause(&app.display_events);
    }

    crate::app::session_store::SessionStore::open(key.worktree_path())
        .and_then(|store| store.load_events(key.session_id()))
        .map(|events| events_have_unanswered_pause(&events))
        .unwrap_or(false)
}

/// Return true when a special session pause owns the tracked slot.
fn special_pause_owns_slot(app: &App, tracked_slot: &str) -> bool {
    app.rcr_session
        .as_ref()
        .map(|rcr| rcr.slot_id == tracked_slot)
        .unwrap_or(false)
        || app
            .issue_session
            .as_ref()
            .map(|issue| issue.slot_id == tracked_slot)
            .unwrap_or(false)
}

/// Return true when a stopped slot is only waiting on a target-local pause workflow.
fn auto_prompt_blocked_by_pause(app: &App, key: &AutoPromptKey, tracked_slot: &str) -> bool {
    special_pause_owns_slot(app, tracked_slot) || target_has_unanswered_pause(app, key)
}

/// Return true when compaction belongs to the target session.
fn compaction_blocks_auto_prompt(app: &App, key: &AutoPromptKey) -> bool {
    let pending = app
        .compaction_needed
        .as_ref()
        .map(|(sid, path)| *sid == key.session_id() && path.as_path() == key.worktree_path())
        .unwrap_or(false);
    let retry = app
        .compaction_retry_needed
        .as_ref()
        .map(|(sid, path)| *sid == key.session_id() && path.as_path() == key.worktree_path())
        .unwrap_or(false);
    let running = app.compaction_receivers.values().any(|job| {
        job.session_id == key.session_id() && job.wt_path.as_path() == key.worktree_path()
    });
    pending || retry || running
}

/// Return true when the target entry's tracked turn has really completed.
fn auto_prompt_ready_for_key(app: &mut App, key: &AutoPromptKey) -> bool {
    let Some(tracked_slot) = app
        .auto_prompt
        .entry_for(key)
        .and_then(|entry| entry.tracked_slot())
        .map(str::to_string)
    else {
        return false;
    };
    if app.running_sessions.contains(&tracked_slot) {
        return false;
    }

    let Some(exit_code) = app.agent_exit_codes.get(&tracked_slot).copied() else {
        return false;
    };
    if exit_code != 0 {
        app.auto_prompt.clear_tracked_turn(key);
        return false;
    }
    if auto_prompt_blocked_by_pause(app, key, &tracked_slot) {
        return false;
    }
    if compaction_blocks_auto_prompt(app, key) {
        app.auto_prompt.defer_for_compaction(key);
        return false;
    }

    true
}

/// Send a prompt to a session target and optionally show it as a user message.
fn send_prompt_to_target(
    app: &mut App,
    claude_process: &AgentProcess,
    target: AutoPromptTarget,
    display_prompt: Option<&str>,
    actual_prompt: &str,
    action: &str,
    default_status: &str,
) -> bool {
    if !target_project_is_registered(app, &target) {
        app.set_status("Auto prompt target project is no longer loaded.");
        return false;
    }
    let visible = target_is_visible(app, &target);
    let events_offset = if visible {
        app.display_events.len()
    } else {
        stored_event_count(&target)
    };
    let session_file_size = if visible { app.session_file_size } else { 0 };
    let send_prompt = build_context_prompt_for_target(app, &target, actual_prompt);

    if visible {
        if let Some(prompt) = display_prompt {
            app.record_prompt_history(prompt);
            app.add_user_message(prompt.to_string());
            app.process_session_chunk(&format!("You: {}\n", prompt));
            app.current_todos.clear();
        }
    } else if let Some(prompt) = display_prompt {
        app.record_prompt_history(prompt);
    }

    let selected_model = app.selected_model.clone();
    match spawn_with_retry_and_fallback(
        claude_process,
        target.key().worktree_path(),
        &send_prompt,
        None,
        selected_model.as_deref(),
        action,
    ) {
        Ok(outcome) => {
            let success_notice = outcome.success_notice.clone();
            let pid = register_prompt_process(
                app,
                &target,
                outcome,
                visible,
                events_offset,
                session_file_size,
            );
            track_auto_prompt_spawn(app, &target, display_prompt, actual_prompt, pid);
            if visible {
                app.update_title_session_name();
            }
            app.set_status(success_notice.unwrap_or_else(|| default_status.to_string()));
        }
        Err(e) => app.set_status(e),
    }
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
    if let Some(target) = app.current_auto_prompt_target() {
        let cancelled_auto_prompt =
            cancel_auto_prompt_if_prompt_changed(app, &target, display_prompt);
        let default_status = if cancelled_auto_prompt {
            "Auto prompt cancelled; running..."
        } else {
            default_status
        };
        return send_prompt_to_target(
            app,
            claude_process,
            target,
            display_prompt,
            actual_prompt,
            action,
            default_status,
        );
    }
    send_prompt_to_current_worktree_without_store(
        app,
        claude_process,
        display_prompt,
        actual_prompt,
        action,
        default_status,
    )
}

/// Send a current-worktree prompt when no store session id exists yet.
fn send_prompt_to_current_worktree_without_store(
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
            app.register_claude(
                branch,
                outcome.pid,
                outcome.rx,
                outcome.registration_model.as_deref(),
            );
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

/// Send all ready auto-prompt repeats to their own target sessions.
fn send_ready_auto_prompts(app: &mut App, claude_process: &AgentProcess) -> bool {
    let mut sent = false;
    for key in app.auto_prompt.tracked_keys() {
        if !auto_prompt_ready_for_key(app, &key) {
            continue;
        }
        let Some(target) = app.auto_prompt.entry_for(&key).and_then(|entry| {
            if entry.prompt().is_some() {
                Some(entry.target().clone())
            } else {
                None
            }
        }) else {
            app.auto_prompt.clear_tracked_turn(&key);
            continue;
        };
        let Some(prompt) = app.auto_prompt.take_repeat_prompt(&key) else {
            continue;
        };
        sent |= send_prompt_to_target(
            app,
            claude_process,
            target,
            Some(&prompt),
            &prompt,
            "auto prompt repeat",
            "Auto prompt running...",
        );
    }
    sent
}

use super::agent_events;

/// Remove repeated trailing compaction banners caused by retry spawns.
fn collapse_trailing_compaction_banner(app: &mut App) {
    let mut removed = false;
    while app.display_events.len() >= 2 {
        let last = app.display_events.last();
        let prev = app
            .display_events
            .get(app.display_events.len().saturating_sub(2));
        if matches!(
            (prev, last),
            (
                Some(DisplayEvent::MayBeCompacting),
                Some(DisplayEvent::MayBeCompacting)
            )
        ) {
            app.display_events.pop();
            removed = true;
        } else {
            break;
        }
    }
    if removed {
        app.invalidate_render_cache();
    }
}

/// Clear a failed compaction retry and prevent unsafe hidden continuation.
fn stop_empty_compaction_retry(app: &mut App, status: impl Into<String>) {
    app.compaction_retry_needed = None;
    app.auto_continue_after_compaction = false;
    app.auto_continue_compaction_target = None;
    app.compaction_spawn_deferred = true;
    app.set_status(status.into());
}

/// Claim the single allowed empty-summary retry for a compaction request.
fn take_empty_compaction_retry(app: &mut App) -> Option<(i64, std::path::PathBuf)> {
    let retry = app.compaction_retry_needed.as_ref()?;
    if app.compaction_spawn_deferred {
        stop_empty_compaction_retry(
            app,
            "Compaction stopped: summary retry also produced no text. Switch models if needed, then send a new prompt to retry.",
        );
        return None;
    }

    let (session_id, wt_path) = (retry.0, retry.1.clone());
    app.compaction_spawn_deferred = true;
    Some((session_id, wt_path))
}

/// Release the empty-summary retry latch after a successful compaction.
fn clear_completed_compaction_retry_latch(app: &mut App) {
    if app.compaction_spawn_deferred
        && app.compaction_retry_needed.is_none()
        && app.compaction_receivers.is_empty()
        && app.chars_since_compaction < crate::app::session_store::COMPACTION_THRESHOLD
    {
        app.compaction_spawn_deferred = false;
        app.set_status("Compaction complete.");
    }
}

/// Return true when a mid-turn compaction is ready to resume the interrupted turn.
fn compaction_auto_continue_is_ready(app: &App) -> bool {
    app.auto_continue_after_compaction
        && app.auto_continue_compaction_target.is_some()
        && app.compaction_receivers.is_empty()
        && app.compaction_retry_needed.is_none()
}

/// Send staged prompt when no agent is running and no dialog is blocking.
/// Returns true if a prompt was sent (needs redraw).
pub fn send_staged_prompt(app: &mut App, claude_process: &AgentProcess) -> bool {
    if app.new_session_dialog_active {
        return false;
    }
    let mut sent = false;

    if !app.is_active_slot_running() && app.staged_prompt.is_some() {
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
                    sent = true;
                }
            }
        }

        if !sent {
            if let Some(prompt) = app.staged_prompt.take() {
                sent |= send_prompt_to_current_worktree(
                    app,
                    claude_process,
                    Some(&prompt),
                    &prompt,
                    "prompt start",
                    "Running...",
                );
            }
        }
    }

    let auto_sent = send_ready_auto_prompts(app, claude_process);
    sent || auto_sent
}

/// Manage compaction lifecycle: poll existing agents, spawn new ones when
/// threshold crossed, and handle retries. Returns true if needs redraw.
pub fn manage_compaction(app: &mut App, claude_process: &AgentProcess) -> bool {
    let mut redraw = false;

    // Poll compaction agents (background summarization, invisible to UI)
    agent_events::poll_compaction_agents(app);
    clear_completed_compaction_retry_latch(app);

    // Spawn compaction agent when threshold is crossed (mid-turn or post-exit).
    // Only consume the trigger if spawn succeeds — failed spawns set
    // compaction_spawn_deferred to avoid retrying every tick.
    if !app.compaction_spawn_deferred {
        if let Some((session_id, wt_path)) = app.compaction_needed.as_ref() {
            let (sid, wtp) = (*session_id, wt_path.clone());
            if agent_events::spawn_compaction_agent(app, claude_process, sid, &wtp) {
                app.compaction_needed = None;
                if app.is_viewing_session_target(sid, &wtp) {
                    collapse_trailing_compaction_banner(app);
                }
            } else {
                app.compaction_spawn_deferred = true;
            }
        }
    }

    // Retry compaction once if the primary produced no output.
    if let Some((sid, wtp)) = take_empty_compaction_retry(app) {
        if agent_events::spawn_compaction_agent(app, claude_process, sid, &wtp) {
            app.compaction_retry_needed = None;
            if app.is_viewing_session_target(sid, &wtp) {
                collapse_trailing_compaction_banner(app);
            }
        } else {
            stop_empty_compaction_retry(
                app,
                "Compaction stopped: summary retry failed to spawn. Switch models if needed, then send a new prompt to retry.",
            );
        }
    }

    // Auto-continue after mid-turn compaction: once all compaction agents finish
    // (no receivers, no retry pending), spawn a hidden "continue" prompt with
    // fresh context injection (includes the new compaction summary). No user
    // bubble — the conversation resumes transparently. Do not treat a later
    // Complete banner as natural completion here: Codex can emit one while
    // finalizing the turn Azureal intentionally killed for compaction.
    if compaction_auto_continue_is_ready(app) {
        app.auto_continue_after_compaction = false;
        let target = app.auto_continue_compaction_target.take();
        let Some(target) = target else {
            return redraw;
        };
        redraw |= send_prompt_to_target(
            app,
            claude_process,
            target,
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
    fn app_with_tracked_auto_prompt() -> (App, AutoPromptKey) {
        let mut app = App::new();
        let wt_path = PathBuf::from("/tmp/feature");
        app.worktrees.push(crate::models::Worktree {
            branch_name: "feature".into(),
            worktree_path: Some(wt_path.clone()),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.current_session_id = Some(7);
        let target = app.current_auto_prompt_target().unwrap();
        let key = target.key().clone();
        app.auto_prompt.toggle(target.clone());
        app.auto_prompt.capture_prompt(target, "repeat me", "42");
        app.agent_exit_codes.insert("42".into(), 0);
        (app, key)
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

    /// Duplicate retry banners collapse to one visible compaction notice.
    #[test]
    fn collapse_trailing_compaction_banner_removes_retry_duplicate() {
        let mut app = App::new();
        app.display_events.push(DisplayEvent::UserMessage {
            _uuid: String::new(),
            content: "hello".into(),
        });
        app.display_events.push(DisplayEvent::MayBeCompacting);
        app.display_events.push(DisplayEvent::MayBeCompacting);

        collapse_trailing_compaction_banner(&mut app);

        assert_eq!(app.display_events.len(), 2);
        assert!(matches!(
            app.display_events.last(),
            Some(DisplayEvent::MayBeCompacting)
        ));
    }

    /// Empty compaction summaries get exactly one retry budget.
    #[test]
    fn take_empty_compaction_retry_stops_after_budget_spent() {
        let mut app = App::new();
        app.compaction_retry_needed = Some((7, PathBuf::from("/tmp/feature")));
        app.auto_continue_after_compaction = true;

        let retry = take_empty_compaction_retry(&mut app);

        assert_eq!(retry, Some((7, PathBuf::from("/tmp/feature"))));
        assert!(app.compaction_spawn_deferred);
        assert!(app.compaction_retry_needed.is_some());

        let retry = take_empty_compaction_retry(&mut app);

        assert!(retry.is_none());
        assert!(app.compaction_retry_needed.is_none());
        assert!(!app.auto_continue_after_compaction);
        assert!(app.compaction_spawn_deferred);
        assert!(app
            .status_message
            .as_deref()
            .is_some_and(|status| status.contains("Compaction stopped")));
    }

    /// A successful compaction releases the retry latch once the badge drops.
    #[test]
    fn clear_completed_compaction_retry_latch_releases_after_success() {
        let mut app = App::new();
        app.compaction_spawn_deferred = true;
        app.chars_since_compaction = crate::app::session_store::COMPACTION_THRESHOLD - 1;

        clear_completed_compaction_retry_latch(&mut app);

        assert!(!app.compaction_spawn_deferred);
        assert_eq!(app.status_message.as_deref(), Some("Compaction complete."));
    }

    /// A still-over-threshold session keeps the retry latch set.
    #[test]
    fn clear_completed_compaction_retry_latch_keeps_failed_high_context_latch() {
        let mut app = App::new();
        app.compaction_spawn_deferred = true;
        app.chars_since_compaction = crate::app::session_store::COMPACTION_THRESHOLD;

        clear_completed_compaction_retry_latch(&mut app);

        assert!(app.compaction_spawn_deferred);
    }

    /// Killed Codex turns can leave a completion banner but still need hidden resume.
    #[test]
    fn compaction_auto_continue_ignores_completion_banner_from_interrupted_turn() {
        let mut app = App::new();
        app.auto_continue_after_compaction = true;
        app.auto_continue_compaction_target = Some(AutoPromptTarget::new(
            PathBuf::from("/tmp/feature"),
            7,
            "feature",
            None,
        ));
        app.display_events.push(DisplayEvent::Complete {
            _session_id: String::new(),
            success: true,
            duration_ms: 0,
            cost_usd: 0.0,
        });

        assert!(compaction_auto_continue_is_ready(&app));
    }

    /// Pending compaction retries still block hidden continuation.
    #[test]
    fn compaction_auto_continue_waits_for_empty_summary_retry() {
        let mut app = App::new();
        app.auto_continue_after_compaction = true;
        app.auto_continue_compaction_target = Some(AutoPromptTarget::new(
            PathBuf::from("/tmp/feature"),
            7,
            "feature",
            None,
        ));
        app.compaction_retry_needed = Some((7, PathBuf::from("/tmp/feature")));

        assert!(!compaction_auto_continue_is_ready(&app));
    }

    /// A zero-exit tracked turn queues the captured prompt for repeat.
    #[test]
    fn auto_prompt_ready_after_completed_tracked_turn() {
        let (mut app, key) = app_with_tracked_auto_prompt();

        assert!(auto_prompt_ready_for_key(&mut app, &key));

        assert_eq!(
            app.auto_prompt.entry_for(&key).unwrap().prompt(),
            Some("repeat me")
        );
        assert_eq!(
            app.auto_prompt.entry_for(&key).unwrap().tracked_slot(),
            Some("42")
        );
    }

    /// Compaction delays the repeat until the compaction request clears.
    #[test]
    fn auto_prompt_defers_while_target_compaction_is_pending() {
        let (mut app, key) = app_with_tracked_auto_prompt();
        app.compaction_needed = Some((7, PathBuf::from("/tmp/feature")));

        assert!(!auto_prompt_ready_for_key(&mut app, &key));

        assert!(app
            .auto_prompt
            .entry_for(&key)
            .unwrap()
            .is_pending_after_compaction());

        app.compaction_needed = None;
        assert!(auto_prompt_ready_for_key(&mut app, &key));
    }

    /// Compaction in one session does not block a different auto-prompt key.
    #[test]
    fn auto_prompt_compaction_block_is_per_session() {
        let (mut app, first_key) = app_with_tracked_auto_prompt();
        let second_target = AutoPromptTarget::new(
            PathBuf::from("/tmp/feature"),
            8,
            "feature",
            app.project.as_ref().map(|project| project.path.clone()),
        );
        let second_key = second_target.key().clone();
        app.auto_prompt.toggle(second_target.clone());
        app.auto_prompt
            .capture_prompt(second_target, "other repeat", "43");
        app.agent_exit_codes.insert("43".into(), 0);
        app.compaction_needed = Some((7, PathBuf::from("/tmp/feature")));

        assert!(!auto_prompt_ready_for_key(&mut app, &first_key));
        assert!(auto_prompt_ready_for_key(&mut app, &second_key));
    }

    /// Build an ExitPlanMode tool call for pause-detection tests.
    fn exit_plan_tool_call() -> DisplayEvent {
        DisplayEvent::ToolCall {
            _uuid: String::new(),
            tool_use_id: "tool-1".into(),
            tool_name: "ExitPlanMode".into(),
            file_path: None,
            input: serde_json::json!({}),
        }
    }

    /// Agent pause workflows hold repeats instead of treating the pause as completion.
    #[test]
    fn auto_prompt_defers_agent_pause_turns() {
        let (mut app, key) = app_with_tracked_auto_prompt();
        app.display_events.push(exit_plan_tool_call());

        assert!(!auto_prompt_ready_for_key(&mut app, &key));

        assert_eq!(
            app.auto_prompt.entry_for(&key).unwrap().tracked_slot(),
            Some("42")
        );
    }

    /// A pause in the viewed session does not block another session's loop.
    #[test]
    fn auto_prompt_pause_block_is_per_session() {
        let (mut app, first_key) = app_with_tracked_auto_prompt();
        let second_target = AutoPromptTarget::new(
            PathBuf::from("/tmp/feature"),
            8,
            "feature",
            app.project.as_ref().map(|project| project.path.clone()),
        );
        let second_key = second_target.key().clone();
        app.auto_prompt.toggle(second_target.clone());
        app.auto_prompt
            .capture_prompt(second_target, "other repeat", "43");
        app.agent_exit_codes.insert("43".into(), 0);
        app.display_events.push(exit_plan_tool_call());

        assert!(!auto_prompt_ready_for_key(&mut app, &first_key));
        assert!(auto_prompt_ready_for_key(&mut app, &second_key));
    }

    /// A manual prompt that changes the loop text disables auto prompt before spawn.
    #[test]
    fn changed_manual_prompt_cancels_auto_prompt_loop() {
        let (mut app, key) = app_with_tracked_auto_prompt();
        let target = app.current_auto_prompt_target().unwrap();

        assert!(cancel_auto_prompt_if_prompt_changed(
            &mut app,
            &target,
            Some("manual override")
        ));

        assert!(!app.auto_prompt.is_enabled_for(&key));
    }

    /// Resending the loop prompt keeps auto prompt enabled for the next repeat.
    #[test]
    fn repeated_manual_prompt_keeps_auto_prompt_loop() {
        let (mut app, key) = app_with_tracked_auto_prompt();
        let target = app.current_auto_prompt_target().unwrap();

        assert!(!cancel_auto_prompt_if_prompt_changed(
            &mut app,
            &target,
            Some("repeat me")
        ));

        assert!(app.auto_prompt.is_enabled_for(&key));
        assert_eq!(
            app.auto_prompt.entry_for(&key).unwrap().tracked_slot(),
            Some("42")
        );
    }

    /// Hidden continuation prompts do not cancel the visible auto-prompt loop.
    #[test]
    fn hidden_prompt_does_not_cancel_auto_prompt_loop() {
        let (mut app, key) = app_with_tracked_auto_prompt();
        let target = app.current_auto_prompt_target().unwrap();

        assert!(!cancel_auto_prompt_if_prompt_changed(
            &mut app, &target, None
        ));

        assert!(app.auto_prompt.is_enabled_for(&key));
    }
}
