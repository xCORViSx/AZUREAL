//! Claude session handling and event processing

use std::sync::mpsc::Receiver;

use crate::app::util::display_text_from_json;
use crate::claude::ClaudeEvent;
use crate::events::DisplayEvent;
use crate::models::OutputType;

use super::App;

impl App {
    pub fn handle_claude_started(&mut self, branch_name: &str, pid: u32) {
        self.running_sessions.insert(branch_name.to_string());
        self.claude_pids.insert(branch_name.to_string(), pid);
        // Clear previous exit code — process is running again
        self.claude_exit_codes.remove(branch_name);
        self.invalidate_sidebar();
        self.set_status(format!("Claude started in {}", branch_name));
    }

    pub fn handle_claude_exited(&mut self, branch_name: &str, code: Option<i32>) {
        // Send macOS notification before cleaning up state (need session info still available)
        self.send_completion_notification(branch_name, code);

        self.running_sessions.remove(branch_name);
        self.claude_pids.remove(branch_name);
        self.claude_receivers.remove(branch_name);
        self.interactive_sessions.remove(branch_name);
        // Store exit code so the convo pane title can show it
        if let Some(c) = code {
            self.claude_exit_codes.insert(branch_name.to_string(), c);
        }
        self.invalidate_sidebar();

        // Force a full re-parse from the session file now that streaming is done.
        // During streaming, session file polling was skipped (to avoid duplicates).
        // The authoritative session file has hook extraction, rewrite handling, etc.
        // that the live EventParser doesn't — a full parse reconciles everything.
        let is_current = self.current_session().map(|s| s.branch_name == branch_name).unwrap_or(false);
        if is_current {
            self.session_file_parse_offset = 0;
            self.session_file_dirty = true;
        }

        // If this was a [NewRunCmd] session, auto-reload run_commands.json
        // so the newly generated command appears in the picker immediately.
        if is_current && self.title_session_name.starts_with("[NewRunCmd]") {
            self.load_run_commands();
        }

        // If a staged prompt exists, leave it for the event loop to auto-send.
        // Otherwise show exit status.
        if self.staged_prompt.is_some() {
            self.set_status("Sending staged prompt...");
        } else {
            let exit_str = match code {
                Some(0) => "exited OK".to_string(),
                Some(c) => format!("exited: {}", c),
                None => "exited".to_string(),
            };
            self.set_status(format!("{} {}", branch_name, exit_str));
        }
    }

    /// Send a macOS notification when Claude finishes. Runs terminal-notifier
    /// in a background thread so it never blocks the event loop. The notification
    /// shows worktree:session_name so the user knows which instance completed.
    /// terminal-notifier runs as its own .app bundle so notifications are NOT
    /// suppressed when Kitty is the frontmost app (unlike osascript/notify-rust).
    fn send_completion_notification(&self, branch_name: &str, code: Option<i32>) {
        // Worktree name = branch name without "azureal/" prefix
        let worktree = branch_name.strip_prefix("azureal/").unwrap_or(branch_name);

        // Resolve session display name: use cached title if this is the current
        // session, otherwise look up from session_files + session_names TOML.
        let is_current = self.current_session().map(|s| s.branch_name == branch_name).unwrap_or(false);
        let session_name = if is_current && !self.title_session_name.is_empty() {
            self.title_session_name.clone()
        } else {
            let session_id = self.session_selected_file_idx.get(branch_name)
                .and_then(|idx| self.session_files.get(branch_name).and_then(|f| f.get(*idx)))
                .map(|(id, _, _)| id.clone())
                .or_else(|| self.claude_session_ids.get(branch_name).cloned());
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

        // Build the notification label: "worktree:session" or just "worktree"
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

        // Fire-and-forget: spawn detached thread so the event loop never blocks.
        // notify-rust uses the native macOS NSUserNotification API. Notifications
        // appear attributed to Finder (no custom icon support on macOS).
        let title = format!("AZUREAL - {}", label);
        let body = body.to_string();
        std::thread::spawn(move || {
            let _ = notify_rust::Notification::new()
                .summary(&title)
                .body(&body)
                .sound_name("Glass")
                .show();
        });
    }

    /// Cancel the currently running Claude process for the current session
    pub fn cancel_current_claude(&mut self) {
        let branch_name = match self.current_session() {
            Some(s) => s.branch_name.clone(),
            None => return,
        };
        if let Some(pid) = self.claude_pids.get(&branch_name) {
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
            self.set_status("Cancelled Claude".to_string());
        }
    }

    pub fn handle_claude_output(&mut self, branch_name: &str, output_type: OutputType, data: String) {
        let is_viewing = self.current_session().map(|s| s.branch_name == branch_name).unwrap_or(false);
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
                            } else {
                                self.subagent_todos = parse_todos_from_input(input);
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

            // Clear pending user message when Claude starts responding.
            // stream-json does NOT include user events in stdout (only
            // system/assistant/result/progress), so we can't match on
            // UserMessage. Instead, any assistant or tool event proves
            // Claude received our prompt — the pending bubble is no
            // longer needed. Just clear the flag and invalidate; the next
            // background render will naturally exclude the bubble.
            // NOTE: we do NOT truncate the cache here — rendered_content_line_count
            // can be stale (from a previous render cycle) and truncating to a stale
            // value destroys real content, causing messages to vanish permanently.
            if self.pending_user_message.is_some() {
                let has_response = events.iter().any(|ev| matches!(ev,
                    DisplayEvent::AssistantText { .. }
                    | DisplayEvent::ToolCall { .. }
                    | DisplayEvent::ToolResult { .. }
                ));
                if has_response {
                    self.pending_user_message = None;
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

    pub fn handle_claude_error(&mut self, branch_name: &str, error: String) {
        let is_viewing = self.current_session().map(|s| s.branch_name == branch_name).unwrap_or(false);
        if is_viewing { self.add_output(format!("Error: {}", error)); }
        self.set_status(format!("{}: {}", branch_name, error));
    }

    pub fn register_claude(&mut self, branch_name: String, receiver: Receiver<ClaudeEvent>) {
        self.claude_receivers.insert(branch_name.clone(), receiver);
        self.running_sessions.insert(branch_name);
        self.invalidate_sidebar(); // Status indicator changed
    }

    pub fn set_claude_session_id(&mut self, branch_name: &str, claude_session_id: String) {
        // Check if there's a pending custom session name to save
        self.check_pending_session_name(branch_name, &claude_session_id);
        self.claude_session_ids.insert(branch_name.to_string(), claude_session_id);
    }

    pub fn get_claude_session_id(&self, branch_name: &str) -> Option<&String> {
        self.claude_session_ids.get(branch_name)
    }

    pub fn poll_interactive_sessions(&mut self) -> bool {
        let current_branch = self.current_session().map(|s| s.branch_name.clone());
        let Some(branch_name) = current_branch else { return false };

        let events = if let Some(interactive) = self.interactive_sessions.get_mut(&branch_name) {
            interactive.poll_events()
        } else {
            return false;
        };

        if events.is_empty() { return false; }

        for event in &events {
            match event {
                DisplayEvent::ToolCall { tool_use_id, .. } => {
                    self.pending_tool_calls.insert(tool_use_id.clone());
                }
                DisplayEvent::ToolResult { tool_use_id, content, .. } => {
                    self.pending_tool_calls.remove(tool_use_id);
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

        self.display_events.extend(events);
        self.invalidate_render_cache();
        true
    }

    pub fn cleanup_interactive_session(&mut self, branch_name: &str) {
        self.interactive_sessions.remove(branch_name);
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
