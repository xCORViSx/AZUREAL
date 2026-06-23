//! Event processing for live agent output
//!
//! Handles incoming events from both the background `AgentProcessor` thread
//! (`apply_parsed_output`) and the legacy direct-output path
//! (`handle_claude_output`). Updates tool call tracking, todo state,
//! compaction counters, and display events.

use crate::app::util::display_text_from_json;
use crate::events::DisplayEvent;
use crate::models::OutputType;

use super::parse_todos_from_input;
use crate::app::state::App;

/// Remove parsed user-message confirmations that match optimistic prompts
/// already inserted for the current turn.
fn drop_confirmed_optimistic_user_messages(
    mut events: Vec<DisplayEvent>,
    existing_events: &[DisplayEvent],
    turn_events_offset: Option<usize>,
) -> Vec<DisplayEvent> {
    let Some(turn_events_offset) = turn_events_offset else {
        return events;
    };
    if !events
        .iter()
        .any(|event| matches!(event, DisplayEvent::UserMessage { .. }))
    {
        return events;
    }

    let optimistic_user_messages: std::collections::HashSet<&str> = existing_events
        .iter()
        .skip(turn_events_offset.min(existing_events.len()))
        .filter_map(|event| match event {
            DisplayEvent::UserMessage { content, .. } => Some(content.as_str()),
            _ => None,
        })
        .collect();
    if optimistic_user_messages.is_empty() {
        return events;
    }

    events.retain(|event| match event {
        DisplayEvent::UserMessage { content, .. } => {
            !optimistic_user_messages.contains(content.as_str())
        }
        _ => true,
    });
    events
}

/// Live event handling methods for foreground and background agent output.
impl App {
    /// Check if a slot's output should be displayed (active slot of viewed branch)
    pub fn is_viewing_slot(&self, slot_id: &str) -> bool {
        let is_rcr_slot = self
            .rcr_session
            .as_ref()
            .map(|r| r.slot_id == slot_id)
            .unwrap_or(false);
        !self.viewing_historic_session
            && (is_rcr_slot
                || self
                    .current_worktree()
                    .map(|s| {
                        self.active_slot
                            .get(&s.branch_name)
                            .map(|a| a == slot_id)
                            .unwrap_or(false)
                    })
                    .unwrap_or(false))
    }

    /// Apply pre-parsed Claude output to app state. Called with results from
    /// the background AgentProcessor thread — all JSON parsing already done.
    pub fn apply_parsed_output(
        &mut self,
        slot_id: &str,
        events: Vec<DisplayEvent>,
        parsed_json: Option<serde_json::Value>,
        output_type: OutputType,
        data: &str,
    ) {
        let turn_events_offset = self
            .pid_session_target
            .get(slot_id)
            .map(|(_, _, events_offset, _)| *events_offset);
        let mut events = drop_confirmed_optimistic_user_messages(
            events,
            &self.display_events,
            turn_events_offset,
        );
        self.apply_slot_turn_duration(slot_id, &mut events);
        for event in &events {
            match event {
                DisplayEvent::ToolCall {
                    tool_use_id,
                    tool_name,
                    input,
                    ..
                } => {
                    self.pending_tool_calls.insert(tool_use_id.clone());
                    self.tool_status_generation += 1;
                    if tool_name == "Task" {
                        if self.active_task_tool_ids.is_empty() {
                            self.subagent_parent_idx = self
                                .current_todos
                                .iter()
                                .position(|t| t.status == crate::app::TodoStatus::InProgress);
                        }
                        self.active_task_tool_ids.insert(tool_use_id.clone());
                    }
                    if tool_name == "TodoWrite" {
                        if self.active_task_tool_ids.is_empty() {
                            self.current_todos = parse_todos_from_input(input);
                            self.todo_scroll = 0;
                        } else {
                            self.subagent_todos = parse_todos_from_input(input);
                            self.todo_scroll = 0;
                        }
                    }
                    if tool_name == "AskUserQuestion" {
                        self.awaiting_ask_user_question = true;
                        self.ask_user_questions_cache = Some(input.clone());
                    }
                }
                DisplayEvent::ToolResult {
                    tool_use_id,
                    is_error,
                    ..
                } => {
                    self.pending_tool_calls.remove(tool_use_id);
                    self.tool_status_generation += 1;
                    if self.active_task_tool_ids.remove(tool_use_id)
                        && self.active_task_tool_ids.is_empty()
                    {
                        self.subagent_todos.clear();
                        self.subagent_parent_idx = None;
                    }
                    if *is_error {
                        self.failed_tool_calls.insert(tool_use_id.clone());
                        self.tool_status_generation += 1;
                    }
                }
                _ => {}
            }
        }

        if !events.is_empty() {
            let added_chars: usize = events
                .iter()
                .map(crate::app::session_store::event_char_len)
                .sum();
            self.chars_since_compaction += added_chars;
            self.display_events.extend(events);
            self.invalidate_render_cache();
            self.last_session_event_time = std::time::Instant::now();
            self.compaction_banner_injected = false;
            self.update_token_badge_live();

            if self.compaction_needed.is_none()
                && self.compaction_receivers.is_empty()
                && self.chars_since_compaction >= crate::app::session_store::COMPACTION_THRESHOLD
            {
                if let Some(sid) = self.current_session_id {
                    if let Some(wt_path) = self
                        .current_worktree()
                        .and_then(|s| s.worktree_path.clone())
                    {
                        if self.store_append_from_display(slot_id) {
                            self.compaction_needed = Some((sid, wt_path));
                            // Only auto-continue if the agent hasn't already completed.
                            // If the batch that crossed the threshold contains a Complete
                            // event, the agent finished — compaction still runs but there's
                            // nothing to continue.
                            let session_completed =
                                self.display_events.iter().rev().take(20).any(|e| {
                                    matches!(e, crate::events::DisplayEvent::Complete { .. })
                                });
                            if !session_completed {
                                self.auto_continue_after_compaction = true;
                                self.cancel_current_claude();
                                self.set_status("Compacting context — will auto-continue...");
                            } else {
                                self.set_status(
                                    "Context full — compacting (session already complete)...",
                                );
                            }
                        } else {
                            self.set_status(
                                "Context compaction deferred: failed to store current turn.",
                            );
                        }
                    }
                }
            }
        }

        if self.rendered_lines_cache.is_empty() {
            if let Some(json) = parsed_json {
                if let Some(display_text) = display_text_from_json(&json) {
                    self.process_session_chunk(&display_text);
                }
            } else if output_type != OutputType::Stdout && output_type != OutputType::Json {
                self.process_session_chunk(data);
            }
        }
    }

    /// Apply parsed output for a slot that is not currently displayed. The
    /// events are still kept in that worktree's live cache so switching back to
    /// it shows the current turn without waiting for process exit or JSONL
    /// recovery.
    pub fn apply_background_parsed_output(&mut self, slot_id: &str, events: Vec<DisplayEvent>) {
        let mut events = crate::app::context_injection::strip_injected_context_from_events(events);
        let turn_events_offset = self
            .pid_session_target
            .get(slot_id)
            .map(|(_, _, events_offset, _)| *events_offset);
        self.apply_slot_turn_duration(slot_id, &mut events);
        if events.is_empty() {
            return;
        }

        if let Some(branch) = self.branch_for_slot(slot_id) {
            let is_active_for_branch = self
                .active_slot
                .get(&branch)
                .map(|active| active == slot_id)
                .unwrap_or(false);
            if is_active_for_branch {
                let existing_events = self
                    .live_display_events_cache
                    .get(&branch)
                    .map(|events| events.as_slice())
                    .unwrap_or(&[]);
                let events = drop_confirmed_optimistic_user_messages(
                    events,
                    existing_events,
                    turn_events_offset,
                );
                if events.is_empty() {
                    return;
                }
                self.live_display_events_cache
                    .entry(branch)
                    .or_default()
                    .extend(events);
            }
            return;
        }

        let Some(project_path) = self.slot_to_project.get(slot_id).cloned() else {
            return;
        };
        let Some(snapshot) = self.project_snapshots.get_mut(&project_path) else {
            return;
        };
        let branch = snapshot
            .branch_slots
            .iter()
            .find(|(_, slots)| slots.iter().any(|slot| slot == slot_id))
            .map(|(branch, _)| branch.clone());
        let Some(branch) = branch else {
            return;
        };
        let is_active_for_branch = snapshot
            .active_slot
            .get(&branch)
            .map(|active| active == slot_id)
            .unwrap_or(false);
        if is_active_for_branch {
            let existing_events = snapshot
                .live_display_events_cache
                .get(&branch)
                .filter(|events| !events.is_empty())
                .map(|events| events.as_slice())
                .unwrap_or(snapshot.display_events.as_slice());
            let events = drop_confirmed_optimistic_user_messages(
                events,
                existing_events,
                turn_events_offset,
            );
            if events.is_empty() {
                return;
            }
            snapshot
                .live_display_events_cache
                .entry(branch)
                .or_default()
                .extend(events);
        }
    }

    /// Handle already-filtered output for the slot currently displayed in the
    /// session pane. Background slot output is parsed by the event loop and stored
    /// in that worktree's live cache instead.
    pub fn handle_claude_output(&mut self, slot_id: &str, output_type: OutputType, data: String) {
        // Only display output from the active slot of the currently viewed branch.
        // Also suppress when the user is viewing a different session file (historic).
        // During RCR, always show output if the slot matches the RCR session — the
        // worktree's branch_name may be empty (detached HEAD during rebase).
        let is_rcr_slot = self
            .rcr_session
            .as_ref()
            .map(|r| r.slot_id == slot_id)
            .unwrap_or(false);
        let is_viewing = !self.viewing_historic_session
            && (is_rcr_slot
                || self
                    .current_worktree()
                    .map(|s| {
                        self.active_slot
                            .get(&s.branch_name)
                            .map(|a| a == slot_id)
                            .unwrap_or(false)
                    })
                    .unwrap_or(false));
        if is_viewing {
            // Single JSON parse: EventParser returns both events AND the raw parsed
            // JSON value. We reuse that value for token/model extraction below instead
            // of calling serde_json::from_str again (was the #1 remaining CPU cost).
            let (mut events, parsed_json) = self.event_parser.parse(&data);
            let turn_events_offset = self
                .pid_session_target
                .get(slot_id)
                .map(|(_, _, events_offset, _)| *events_offset);
            events = drop_confirmed_optimistic_user_messages(
                events,
                &self.display_events,
                turn_events_offset,
            );
            self.apply_slot_turn_duration(slot_id, &mut events);

            for event in &events {
                match event {
                    DisplayEvent::ToolCall {
                        tool_use_id,
                        tool_name,
                        input,
                        ..
                    } => {
                        self.pending_tool_calls.insert(tool_use_id.clone());
                        self.tool_status_generation += 1;
                        // Track subagent (Task) tool calls — while active, TodoWrite
                        // events go to subagent_todos instead of overwriting main todos.
                        // On first Task spawn, snapshot which main todo is in_progress
                        // so subtasks render directly beneath that parent item.
                        if tool_name == "Agent" || tool_name == "Task" {
                            if self.active_task_tool_ids.is_empty() {
                                self.subagent_parent_idx = self
                                    .current_todos
                                    .iter()
                                    .position(|t| t.status == crate::app::TodoStatus::InProgress);
                            }
                            self.active_task_tool_ids.insert(tool_use_id.clone());
                        }
                        // TodoWrite: route to subagent_todos if a Task is active,
                        // otherwise update the main agent's current_todos
                        if tool_name == "TodoWrite" {
                            if self.active_task_tool_ids.is_empty() {
                                self.current_todos = parse_todos_from_input(input);
                                self.todo_scroll = 0;
                            } else {
                                self.subagent_todos = parse_todos_from_input(input);
                                self.todo_scroll = 0;
                            }
                        }
                        // AskUserQuestion: flag for special input handling
                        if tool_name == "AskUserQuestion" {
                            self.awaiting_ask_user_question = true;
                            self.ask_user_questions_cache = Some(input.clone());
                        }
                    }
                    DisplayEvent::ToolResult {
                        tool_use_id,
                        is_error,
                        ..
                    } => {
                        self.pending_tool_calls.remove(tool_use_id);
                        self.tool_status_generation += 1;
                        // When a Task (subagent) completes, clear subagent state
                        if self.active_task_tool_ids.remove(tool_use_id)
                            && self.active_task_tool_ids.is_empty()
                        {
                            self.subagent_todos.clear();
                            self.subagent_parent_idx = None;
                        }
                        if *is_error {
                            self.failed_tool_calls.insert(tool_use_id.clone());
                            self.tool_status_generation += 1;
                        }
                    }
                    _ => {}
                }
            }

            // Only extend + invalidate when we actually got events. Many stdout lines
            // (progress, hook_started) produce 0 events — skip the work entirely.
            if !events.is_empty() {
                // Update live char counter for mid-turn compaction detection
                let added_chars: usize = events
                    .iter()
                    .map(crate::app::session_store::event_char_len)
                    .sum();
                self.chars_since_compaction += added_chars;
                self.update_token_badge_live();

                // Extend display_events BEFORE the threshold check so that
                // store_append_from_display captures the triggering batch.
                self.display_events.extend(events);
                self.invalidate_render_cache();

                // Mid-turn compaction trigger: fire as soon as threshold is crossed.
                // Also kill the active process — letting it continue would just pile
                // more uncompacted content onto an already-full context window.
                // After compaction completes, the event loop auto-sends a hidden
                // "continue" prompt with the fresh compaction summary.
                if self.compaction_needed.is_none()
                    && self.compaction_receivers.is_empty()
                    && self.chars_since_compaction
                        >= crate::app::session_store::COMPACTION_THRESHOLD
                {
                    if let Some(sid) = self.current_session_id {
                        if let Some(wt_path) = self
                            .current_worktree()
                            .and_then(|s| s.worktree_path.clone())
                        {
                            // Store partial turn events before killing so compaction
                            // has the latest data. Uses store_append_from_display
                            // which removes the slot from pid_session_target only
                            // after a successful write.
                            if self.store_append_from_display(slot_id) {
                                self.compaction_needed = Some((sid, wt_path));
                                // Only auto-continue if the agent hasn't already completed.
                                let session_completed =
                                    self.display_events.iter().rev().take(20).any(|e| {
                                        matches!(e, crate::events::DisplayEvent::Complete { .. })
                                    });
                                if !session_completed {
                                    self.auto_continue_after_compaction = true;
                                    self.cancel_current_claude();
                                    self.set_status("Compacting context — will auto-continue...");
                                } else {
                                    self.set_status(
                                        "Context full — compacting (session already complete)...",
                                    );
                                }
                            } else {
                                self.set_status(
                                    "Context compaction deferred: failed to store current turn.",
                                );
                            }
                        }
                    }
                }
                // Activity detected — reset compaction inactivity watcher
                self.last_session_event_time = std::time::Instant::now();
                self.compaction_banner_injected = false;
            }

            // Feed the fallback session_lines only when the rendered cache is empty
            // (before first render completes). Once we have rendered content, the session
            // pane draws from rendered_lines_cache and session_lines is never read —
            // skip the display_text_from_json + process_session_chunk work entirely.
            if self.rendered_lines_cache.is_empty() {
                if let Some(json) = parsed_json {
                    if let Some(display_text) = display_text_from_json(&json) {
                        self.process_session_chunk(&display_text);
                    }
                } else if output_type != OutputType::Stdout && output_type != OutputType::Json {
                    self.process_session_chunk(&data);
                }
            }
        }
    }

    /// Fill missing Codex completion durations from the slot start time.
    fn apply_slot_turn_duration(&self, slot_id: &str, events: &mut [DisplayEvent]) {
        let Some(started_at) = self.codex_slot_started_at.get(slot_id) else {
            return;
        };
        let elapsed_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
        for event in events {
            if let DisplayEvent::Complete { duration_ms, .. } = event {
                if *duration_ms == 0 {
                    *duration_ms = elapsed_ms;
                }
            }
        }
    }
}

#[cfg(test)]
/// Tests for live event handling and optimistic prompt reconciliation.
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Build a minimal user-message event for reconciliation tests.
    fn user(content: &str) -> DisplayEvent {
        DisplayEvent::UserMessage {
            _uuid: String::new(),
            content: content.into(),
        }
    }

    /// Build a minimal assistant-message event for reconciliation tests.
    fn assistant(text: &str) -> DisplayEvent {
        DisplayEvent::AssistantText {
            _uuid: String::new(),
            _message_id: String::new(),
            text: text.into(),
        }
    }

    /// Parsed foreground output drops a prompt confirmation already shown optimistically.
    #[test]
    fn apply_parsed_output_drops_confirmed_optimistic_user_message() {
        let mut app = App::new();
        let slot = "slot-1";
        app.pid_session_target
            .insert(slot.into(), (1, PathBuf::from("/tmp/wt"), 0, 0));
        app.display_events.push(user("fix the failing test"));

        app.apply_parsed_output(
            slot,
            vec![
                assistant("looking"),
                user("fix the failing test"),
                assistant("done"),
            ],
            None,
            OutputType::Stdout,
            "",
        );

        let users: Vec<&str> = app
            .display_events
            .iter()
            .filter_map(|event| match event {
                DisplayEvent::UserMessage { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(users, vec!["fix the failing test"]);

        let assistant_texts: Vec<&str> = app
            .display_events
            .iter()
            .filter_map(|event| match event {
                DisplayEvent::AssistantText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(assistant_texts, vec!["looking", "done"]);
    }

    /// Matching prompt text from before the current turn boundary is preserved.
    #[test]
    fn apply_parsed_output_keeps_same_text_from_prior_turn_before_offset() {
        let mut app = App::new();
        let slot = "slot-1";
        app.display_events.push(user("repeat"));
        app.pid_session_target
            .insert(slot.into(), (1, PathBuf::from("/tmp/wt"), 1, 0));

        app.apply_parsed_output(slot, vec![user("repeat")], None, OutputType::Stdout, "");

        let user_count = app
            .display_events
            .iter()
            .filter(|event| matches!(event, DisplayEvent::UserMessage { content, .. } if content == "repeat"))
            .count();
        assert_eq!(user_count, 2);
    }

    /// Background live caches drop prompt confirmations the same way foreground output does.
    #[test]
    fn apply_background_parsed_output_drops_confirmation_from_live_cache() {
        let mut app = App::new();
        let branch = "main".to_string();
        let slot = "slot-1";
        app.branch_slots.insert(branch.clone(), vec![slot.into()]);
        app.active_slot.insert(branch.clone(), slot.into());
        app.pid_session_target
            .insert(slot.into(), (1, PathBuf::from("/tmp/wt"), 0, 0));
        app.live_display_events_cache
            .insert(branch.clone(), vec![user("ship it")]);

        app.apply_background_parsed_output(
            slot,
            vec![assistant("working"), user("ship it"), assistant("complete")],
        );

        let cached = app.live_display_events_cache.get(&branch).unwrap();
        let user_count = cached
            .iter()
            .filter(|event| matches!(event, DisplayEvent::UserMessage { content, .. } if content == "ship it"))
            .count();
        assert_eq!(user_count, 1);

        let assistant_texts: Vec<&str> = cached
            .iter()
            .filter_map(|event| match event {
                DisplayEvent::AssistantText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(assistant_texts, vec!["working", "complete"]);
    }
}
