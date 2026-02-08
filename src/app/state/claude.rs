//! Claude session handling and event processing

use std::sync::mpsc::Receiver;

use crate::app::util::parse_stream_json_for_display;
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

        // If there's a staged prompt, restore it to the input field
        if let Some(prompt) = self.staged_prompt.take() {
            self.input = prompt;
            self.input_cursor = self.input.len();
            self.set_status("Ready - staged prompt restored");
        } else {
            let exit_str = match code {
                Some(0) => "exited OK".to_string(),
                Some(c) => format!("exited: {}", c),
                None => "exited".to_string(),
            };
            self.set_status(format!("{} {}", branch_name, exit_str));
        }
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
            let events = self.event_parser.parse(&data);

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

            // Clear pending user message when Claude starts responding.
            // stream-json does NOT include user events in stdout (only
            // system/assistant/result/progress), so we can't match on
            // UserMessage. Instead, any assistant or tool event proves
            // Claude received our prompt — the pending bubble is no
            // longer needed. Trim the stale bubble from the cache
            // immediately so it doesn't linger while the background
            // render thread processes the re-render.
            if self.pending_user_message.is_some() {
                let has_response = events.iter().any(|ev| matches!(ev,
                    DisplayEvent::AssistantText { .. }
                    | DisplayEvent::ToolCall { .. }
                    | DisplayEvent::ToolResult { .. }
                ));
                if has_response {
                    self.pending_user_message = None;
                    let trim = self.rendered_content_line_count;
                    if trim < self.rendered_lines_cache.len() {
                        self.rendered_lines_cache.truncate(trim);
                        self.animation_line_indices.retain(|&(idx, _)| idx < trim);
                        if let Some(&(line_idx, _)) = self.message_bubble_positions.last() {
                            if line_idx >= trim { self.message_bubble_positions.pop(); }
                        }
                    }
                }
            }

            self.display_events.extend(events);
            self.invalidate_render_cache();

            if output_type == OutputType::Stdout || output_type == OutputType::Json {
                if let Some(display_text) = parse_stream_json_for_display(&data) {
                    self.process_output_chunk(&display_text);
                }
            } else {
                self.process_output_chunk(&data);
            }

            self.output_scroll = usize::MAX;
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
        self.output_scroll = usize::MAX;
        true
    }

    pub fn cleanup_interactive_session(&mut self, branch_name: &str) {
        self.interactive_sessions.remove(branch_name);
    }
}
