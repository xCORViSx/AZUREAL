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
        self.invalidate_sidebar(); // Status indicator changed
        self.set_status(format!("Claude started in {} (PID: {})", branch_name, pid));
    }

    pub fn handle_claude_exited(&mut self, branch_name: &str, code: Option<i32>) {
        self.running_sessions.remove(branch_name);
        self.claude_receivers.remove(branch_name);
        self.interactive_sessions.remove(branch_name);
        self.invalidate_sidebar(); // Status indicator changed
        self.set_status(format!("{} exited: {:?}", branch_name, code));
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
