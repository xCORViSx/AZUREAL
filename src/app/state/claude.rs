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
        self.invalidate_sidebar(); // Status indicator changed
        self.set_status(format!("Claude started in {} (PID: {})", branch_name, pid));
    }

    pub fn handle_claude_exited(&mut self, branch_name: &str, code: Option<i32>) {
        self.running_sessions.remove(branch_name);
        self.claude_pids.remove(branch_name);
        self.claude_receivers.remove(branch_name);
        self.interactive_sessions.remove(branch_name);
        self.invalidate_sidebar(); // Status indicator changed

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
            self.set_status(format!("{} exited: {:?}", branch_name, code));
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
            self.set_status(format!("Cancelled Claude (PID: {})", pid));
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

            // Clear pending user message if the stream now contains it —
            // prevents the "pending" bubble from rendering alongside the
            // real UserMessage that just arrived from Claude's stream-json.
            // Use contains() because the streamed content may have
            // <system-reminder> tags prepended by hooks.
            if let Some(ref pending) = self.pending_user_message {
                for ev in &events {
                    if let DisplayEvent::UserMessage { content, .. } = ev {
                        if content == pending || content.contains(pending.as_str()) {
                            self.pending_user_message = None;
                            break;
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
