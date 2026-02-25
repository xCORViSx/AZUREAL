//! Claude session handling and event processing

use std::sync::mpsc::Receiver;

use crate::app::util::display_text_from_json;
use crate::claude::ClaudeEvent;
use crate::events::DisplayEvent;
use crate::models::OutputType;

use super::App;

impl App {
    /// Called when a Claude process emits Started { pid }. The slot_id IS the
    /// PID string, already registered in register_claude() — this just confirms
    /// the process is alive and clears stale exit codes.
    pub fn handle_claude_started(&mut self, slot_id: &str, _pid: u32) {
        self.running_sessions.insert(slot_id.to_string());
        self.claude_exit_codes.remove(slot_id);
        self.invalidate_sidebar();
        let branch = self.branch_for_slot(slot_id).unwrap_or_else(|| slot_id.to_string());
        self.set_status(format!("Claude started in {}", branch));
    }

    /// Called when a Claude process exits. Cleans up slot state, switches active
    /// slot if needed, and triggers session file re-parse.
    pub fn handle_claude_exited(&mut self, slot_id: &str, code: Option<i32>) {
        // Resolve branch before cleanup removes the slot from branch_slots
        let branch = self.branch_for_slot(slot_id);

        // Send macOS notification before cleaning up state
        if let Some(ref branch) = branch {
            self.send_completion_notification(branch, slot_id, code);
        }

        // Remove slot from all process-tracking maps
        self.running_sessions.remove(slot_id);
        self.claude_receivers.remove(slot_id);
        if let Some(c) = code {
            self.claude_exit_codes.insert(slot_id.to_string(), c);
        }

        // Remove slot from its branch's slot list
        if let Some(ref branch) = branch {
            if let Some(slots) = self.branch_slots.get_mut(branch) {
                slots.retain(|s| s != slot_id);
                if slots.is_empty() { self.branch_slots.remove(branch); }
            }
        }

        // If this was the active slot, switch to next available slot or clear
        let was_active = branch.as_ref().and_then(|b| self.active_slot.get(b))
            .map(|a| a == slot_id).unwrap_or(false);

        if was_active {
            if let Some(ref branch) = branch {
                // Pick another running slot on this branch, or remove active
                let next = self.branch_slots.get(branch)
                    .and_then(|slots| slots.last().cloned());
                match next {
                    Some(next_slot) => { self.active_slot.insert(branch.clone(), next_slot); }
                    None => { self.active_slot.remove(branch); }
                }
            }
        }

        self.invalidate_sidebar();

        // RCR exit intercept — when the RCR Claude process exits, show the approval
        // dialog instead of re-parsing (which would clobber the streaming output the
        // user is currently viewing). The session file lives under main's path, not
        // the feature branch's, so a normal re-parse would load the wrong data.
        if let Some(ref mut rcr) = self.rcr_session {
            if rcr.slot_id == slot_id {
                rcr.approval_pending = true;
                let display = branch.as_deref().unwrap_or(slot_id);
                let exit_str = match code {
                    Some(0) => "finished".to_string(),
                    Some(c) => format!("exited: {}", c),
                    None => "exited".to_string(),
                };
                self.set_status(format!("[RCR] {} — {}", display, exit_str));
                return;
            }
        }

        // Force a full re-parse from the session file now that streaming is done.
        // Skip re-parse if the exiting slot's session file isn't in the current
        // worktree's directory (e.g. merge resolution spawned from main's repo
        // root — its session file lives under main's path, not the feature branch's).
        // Without this guard, the re-parse would reload the OLD session file and
        // clobber the streaming output that the user is viewing.
        let is_current = branch.as_ref().and_then(|b| self.current_worktree().map(|s| s.branch_name == *b)).unwrap_or(false);
        if is_current && was_active {
            let session_file_exists = self.claude_session_ids.get(slot_id)
                .and_then(|sid| self.current_worktree().and_then(|wt| wt.worktree_path.as_deref().map(|p| (sid, p))))
                .and_then(|(sid, path)| crate::config::claude_session_file(path, sid))
                .is_some();
            if session_file_exists {
                self.session_file_parse_offset = 0;
                self.session_file_dirty = true;
            }
        }

        // If this was a [NewRunCmd] session, auto-reload runcmds
        if is_current && self.title_session_name.starts_with("[NewRunCmd]") {
            self.load_run_commands();
        }

        // If a staged prompt exists, leave it for the event loop to auto-send.
        if self.staged_prompt.is_some() {
            self.set_status("Sending staged prompt...");
        } else {
            let display = branch.as_deref().unwrap_or(slot_id);
            let exit_str = match code {
                Some(0) => "exited OK".to_string(),
                Some(c) => format!("exited: {}", c),
                None => "exited".to_string(),
            };
            self.set_status(format!("{} {}", display, exit_str));
        }
    }

    /// Send a macOS notification when Claude finishes.
    fn send_completion_notification(&self, branch_name: &str, slot_id: &str, code: Option<i32>) {
        let worktree = crate::models::strip_branch_prefix(branch_name);

        // Resolve session display name
        let is_current = self.current_worktree().map(|s| s.branch_name == branch_name).unwrap_or(false);
        let session_name = if is_current && !self.title_session_name.is_empty() {
            self.title_session_name.clone()
        } else {
            // Try to find Claude session UUID for this slot, then look up its name
            let session_id = self.claude_session_ids.get(slot_id).cloned();
            match session_id {
                Some(id) => {
                    let names = self.load_all_session_names();
                    names.get(&id).cloned().unwrap_or_else(|| {
                        if id.len() > 8 { id[..8].to_string() } else { id }
                    })
                }
                None => String::new(),
            }
        };

        let label = if session_name.is_empty() {
            worktree.to_string()
        } else {
            format!("{}:{}", worktree, session_name)
        };

        let body = match code {
            Some(0) => "Response complete",
            Some(_) => "Exited with error",
            None => "Process terminated",
        };

        let title = label;
        let body = body.to_string();
        std::thread::spawn(move || {
            let _ = notify_rust::Notification::new()
                .summary(&title)
                .body(&body)
                .sound_name("Glass")
                .show();
        });
    }

    /// Cancel the active Claude process for the current session.
    /// Only kills the active slot — other concurrent sessions keep running.
    pub fn cancel_current_claude(&mut self) {
        let branch_name = match self.current_worktree() {
            Some(s) => s.branch_name.clone(),
            None => return,
        };
        // The active slot's key IS the PID string — parse it back to u32
        if let Some(slot) = self.active_slot.get(&branch_name).cloned() {
            if let Ok(pid) = slot.parse::<u32>() {
                #[cfg(unix)]
                {
                    use std::process::Command;
                    let _ = Command::new("kill").arg(pid.to_string()).status();
                }
                #[cfg(windows)]
                {
                    use std::process::Command;
                    let _ = Command::new("taskkill").args(["/PID", &pid.to_string(), "/F"]).status();
                }
                self.set_status("Cancelled Claude");
            }
        }
    }

    /// Handle Claude output. Only processes events from the active slot (the one
    /// displayed in the convo pane). Non-active slots' output is silently drained
    /// by the event loop to prevent channel backup.
    pub fn handle_claude_output(&mut self, slot_id: &str, output_type: OutputType, data: String) {
        // Only display output from the active slot of the currently viewed branch
        let is_viewing = self.current_worktree().map(|s| {
            self.active_slot.get(&s.branch_name).map(|a| a == slot_id).unwrap_or(false)
        }).unwrap_or(false);
        if is_viewing {
            // Single JSON parse: EventParser returns both events AND the raw parsed
            // JSON value. We reuse that value for token/model extraction below instead
            // of calling serde_json::from_str again (was the #1 remaining CPU cost).
            let (events, parsed_json) = self.event_parser.parse(&data);

            for event in &events {
                match event {
                    DisplayEvent::ToolCall { tool_use_id, tool_name, input, .. } => {
                        self.pending_tool_calls.insert(tool_use_id.clone());
                        // Track subagent (Task) tool calls — while active, TodoWrite
                        // events go to subagent_todos instead of overwriting main todos.
                        // On first Task spawn, snapshot which main todo is in_progress
                        // so subtasks render directly beneath that parent item.
                        if tool_name == "Task" {
                            if self.active_task_tool_ids.is_empty() {
                                self.subagent_parent_idx = self.current_todos.iter()
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
                    DisplayEvent::ToolResult { tool_use_id, content, .. } => {
                        self.pending_tool_calls.remove(tool_use_id);
                        // When a Task (subagent) completes, clear subagent state
                        if self.active_task_tool_ids.remove(tool_use_id) && self.active_task_tool_ids.is_empty() {
                            self.subagent_todos.clear();
                            self.subagent_parent_idx = None;
                        }
                        let lower = content.to_lowercase();
                        if lower.contains("error:") || lower.contains("failed")
                            || lower.starts_with("error") || content.contains("ENOENT")
                            || content.contains("permission denied") {
                            self.failed_tool_calls.insert(tool_use_id.clone());
                        }
                    }
                    _ => {}
                }
            }

            // Reuse the JSON value that EventParser already parsed — zero additional
            // serde_json::from_str calls. EventParser returns it alongside events.

            // Extract token usage, model, and context window from live stream events.
            // assistant events give us token counts + model heuristic (available mid-turn).
            // result events give us the authoritative contextWindow from the API (end of turn).
            if let Some(ref json) = parsed_json {
                let mut tokens_changed = false;
                match json.get("type").and_then(|t| t.as_str()) {
                    Some("assistant") => if let Some(msg) = json.get("message") {
                        if let Some(usage) = msg.get("usage") {
                            let input = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                            let output = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                            let cache_read = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                            let cache_create = usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                            self.session_tokens = Some((input + cache_read + cache_create, output));
                            tokens_changed = true;
                        }
                        // Heuristic fallback — result event will overwrite with exact value
                        if self.model_context_window.is_none() {
                            if let Some(model) = msg.get("model").and_then(|m| m.as_str()) {
                                self.model_context_window = Some(
                                    crate::app::session_parser::context_window_for_model(model)
                                );
                                tokens_changed = true;
                            }
                        }
                    },
                    // result event contains modelUsage.<model_id>.contextWindow — the
                    // exact context window from the API, overriding any heuristic guess
                    Some("result") => {
                        if let Some(obj) = json.get("model_usage")
                            .or_else(|| json.get("modelUsage"))
                            .and_then(|v| v.as_object())
                        {
                            for (_model, usage) in obj {
                                if let Some(cw) = usage.get("context_window")
                                    .or_else(|| usage.get("contextWindow"))
                                    .and_then(|v| v.as_u64())
                                {
                                    self.model_context_window = Some(cw);
                                    tokens_changed = true;
                                }
                            }
                        }
                    },
                    _ => {}
                }
                if tokens_changed { self.update_token_badge(); }
            }

            // Only extend + invalidate when we actually got events. Many stdout lines
            // (progress, hook_started) produce 0 events — skip the work entirely.
            if !events.is_empty() {
                self.display_events.extend(events);
                self.invalidate_render_cache();
                // Activity detected — reset compaction inactivity watcher
                self.last_convo_event_time = std::time::Instant::now();
                self.compaction_banner_injected = false;
            }

            // Feed the fallback output_lines only when the rendered cache is empty
            // (before first render completes). Once we have rendered content, the convo
            // pane draws from rendered_lines_cache and output_lines is never read —
            // skip the display_text_from_json + process_output_chunk work entirely.
            if self.rendered_lines_cache.is_empty() {
                if let Some(json) = parsed_json {
                    if let Some(display_text) = display_text_from_json(&json) {
                        self.process_output_chunk(&display_text);
                    }
                } else if output_type != OutputType::Stdout && output_type != OutputType::Json {
                    self.process_output_chunk(&data);
                }
            }

        }
    }

    /// Register a newly spawned Claude process. The PID is used as the slot key.
    /// Newest spawn becomes the active slot (its output appears in convo pane).
    pub fn register_claude(&mut self, branch_name: String, pid: u32, receiver: Receiver<ClaudeEvent>) {
        let slot = pid.to_string();
        self.claude_receivers.insert(slot.clone(), receiver);
        self.running_sessions.insert(slot.clone());
        // Track this slot under its branch (append = spawn order preserved)
        self.branch_slots.entry(branch_name.clone()).or_default().push(slot.clone());
        // Newest spawn becomes active — its output shows in convo pane
        self.active_slot.insert(branch_name, slot);
        self.invalidate_sidebar();
    }

    /// Store Claude's real session UUID, keyed by slot_id (PID string).
    /// Also propagates to RcrSession if this slot is the active RCR process.
    pub fn set_claude_session_id(&mut self, slot_id: &str, claude_session_id: String) {
        self.check_pending_session_name(slot_id, &claude_session_id);
        // Keep RCR session_id in sync so we can --resume and clean up the file
        if let Some(ref mut rcr) = self.rcr_session {
            if rcr.slot_id == slot_id {
                rcr.session_id = Some(claude_session_id.clone());
            }
        }
        self.claude_session_ids.insert(slot_id.to_string(), claude_session_id);
    }

    /// Get the Claude session UUID for the active slot of a branch (for --resume)
    pub fn get_claude_session_id(&self, branch_name: &str) -> Option<&String> {
        // Look up the active slot's Claude session UUID
        self.active_slot.get(branch_name)
            .and_then(|slot| self.claude_session_ids.get(slot))
            // Fallback: check if there's a session_id stored directly by branch
            // (from load_worktrees at startup, before any slot was created)
            .or_else(|| self.claude_session_ids.get(branch_name))
    }

}

/// Parse TodoWrite input JSON into TodoItem vec.
/// Input structure: { "todos": [{ "content": "...", "status": "pending"|"in_progress"|"completed", "activeForm": "..." }] }
pub fn parse_todos_from_input(input: &serde_json::Value) -> Vec<super::app::TodoItem> {
    let Some(todos) = input.get("todos").and_then(|v| v.as_array()) else { return Vec::new() };
    todos.iter().filter_map(|t| {
        let content = t.get("content")?.as_str()?.to_string();
        let active_form = t.get("activeForm").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let status = match t.get("status").and_then(|v| v.as_str()).unwrap_or("pending") {
            "in_progress" => super::app::TodoStatus::InProgress,
            "completed" => super::app::TodoStatus::Completed,
            _ => super::app::TodoStatus::Pending,
        };
        Some(super::app::TodoItem { content, status, active_form })
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::app::TodoStatus;
    use serde_json::json;

    /// Verifies parse_todos_from_input correctly parses a real TodoWrite input
    /// with mixed statuses (in_progress, pending, completed).
    /// This test exists because TodoWrite JSON has a specific structure from
    /// Claude Code's tool calls — getting the field names or status strings wrong
    /// would silently produce empty results.
    #[test]
    fn test_parse_todos_real_data_mixed_statuses() {
        let input = json!({
            "todos": [
                {
                    "content": "Add all terminal keybindings to title bar hints",
                    "status": "in_progress",
                    "activeForm": "Adding terminal keybindings to title bar"
                },
                {
                    "content": "Remove Terminal section from help_sections()",
                    "status": "pending",
                    "activeForm": "Removing Terminal from help panel"
                },
                {
                    "content": "Build check and verify",
                    "status": "completed",
                    "activeForm": "Verifying build"
                }
            ]
        });
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos.len(), 3, "Should parse all 3 todos");
        assert_eq!(todos[0].content, "Add all terminal keybindings to title bar hints");
        assert_eq!(todos[0].status, TodoStatus::InProgress);
        assert_eq!(todos[0].active_form, "Adding terminal keybindings to title bar");
        assert_eq!(todos[1].status, TodoStatus::Pending);
        assert_eq!(todos[2].status, TodoStatus::Completed);
    }

    /// Verifies empty or missing "todos" array returns empty Vec (no panic).
    /// Without this, a missing "todos" field would need to be handled gracefully.
    #[test]
    fn test_parse_todos_empty_input() {
        assert!(parse_todos_from_input(&json!({})).is_empty());
        assert!(parse_todos_from_input(&json!({"todos": []})).is_empty());
        assert!(parse_todos_from_input(&json!({"todos": "not_array"})).is_empty());
    }

    /// Verifies that missing optional fields don't cause panics.
    /// activeForm is optional in the Claude schema — should default to empty string.
    #[test]
    fn test_parse_todos_missing_active_form() {
        let input = json!({
            "todos": [{"content": "Test item", "status": "pending"}]
        });
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].active_form, "");
        assert_eq!(todos[0].status, TodoStatus::Pending);
    }

    /// Verifies unknown status strings default to Pending (defensive parsing).
    /// Claude might add new statuses in the future — should not panic.
    #[test]
    fn test_parse_todos_unknown_status_defaults_pending() {
        let input = json!({
            "todos": [{"content": "x", "status": "blocked", "activeForm": ""}]
        });
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos[0].status, TodoStatus::Pending);
    }

    /// Verifies todos with missing content field are skipped (filter_map returns None).
    #[test]
    fn test_parse_todos_missing_content_skipped() {
        let input = json!({
            "todos": [
                {"status": "pending", "activeForm": "No content"},
                {"content": "Has content", "status": "pending", "activeForm": ""}
            ]
        });
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].content, "Has content");
    }
}
