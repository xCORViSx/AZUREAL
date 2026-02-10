//! Event parser for Claude Code stream-json
//!
//! Parses raw stream-json output into DisplayEvents.

use std::collections::HashMap;
use super::display::DisplayEvent;

/// Parser for Claude Code stream-json events
pub struct EventParser {
    buffer: String,
    /// Track tool calls by ID so we can match results to calls
    tool_calls: HashMap<String, (String, Option<String>)>,
}

impl EventParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            tool_calls: HashMap::new(),
        }
    }

    /// Feed raw data and get parsed display events.
    /// Collects complete line ranges first, then drains consumed bytes in one shot —
    /// O(n) total instead of O(n²) re-allocation on every newline.
    pub fn parse(&mut self, data: &str) -> Vec<DisplayEvent> {
        self.buffer.push_str(data);
        let mut events = Vec::new();

        // Find all complete lines (up to last newline)
        let last_newline = match self.buffer.rfind('\n') {
            Some(pos) => pos,
            None => return events,
        };

        // Extract complete lines as one owned string, drain from buffer
        let complete: String = self.buffer.drain(..=last_newline).collect();

        for line in complete.split('\n') {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            events.extend(self.parse_line(trimmed));
        }

        events
    }

    fn parse_line(&mut self, line: &str) -> Vec<DisplayEvent> {
        let trimmed = line.trim();

        if trimmed.starts_with('{') {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(event_type) = json.get("type").and_then(|v| v.as_str()) {
                    return match event_type {
                        "system" => self.parse_system_event(&json).into_iter().collect(),
                        "user" => self.parse_user_event(&json),
                        "assistant" => self.parse_assistant_event(&json),
                        "result" => self.parse_result_event(&json).into_iter().collect(),
                        "progress" => self.parse_progress_event(&json).into_iter().collect(),
                        "hook" | "hook_result" | "hook_response" => {
                            let name = json.get("hook_name").or_else(|| json.get("name")).or_else(|| json.get("hook"))
                                .and_then(|v| v.as_str()).unwrap_or("hook").to_string();
                            let output = json.get("output").or_else(|| json.get("result")).or_else(|| json.get("message"))
                                .and_then(|v| v.as_str()).unwrap_or("").to_string();
                            vec![DisplayEvent::Hook { name, output }]
                        }
                        _ => Vec::new(),
                    };
                }
            }
            return Vec::new();
        }

        self.parse_text_hook(line).into_iter().collect()
    }

    fn parse_system_event(&self, json: &serde_json::Value) -> Option<DisplayEvent> {
        let subtype = json.get("subtype").and_then(|v| v.as_str()).unwrap_or("");

        if subtype == "init" {
            return Some(DisplayEvent::Init {
                session_id: json.get("session_id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                cwd: json.get("cwd").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                model: json.get("model").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
            });
        }

        if subtype != "hook_response" { return None; }

        let hook_name = json.get("hook_name").or_else(|| json.get("name")).or_else(|| json.get("hook"))
            .and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_default();
        let output = json.get("output").or_else(|| json.get("stdout"))
            .and_then(|v| v.as_str()).unwrap_or("").trim().to_string();

        if !hook_name.is_empty() && !output.is_empty() {
            return Some(DisplayEvent::Hook { name: hook_name, output });
        }
        None
    }

    fn parse_progress_event(&self, json: &serde_json::Value) -> Option<DisplayEvent> {
        let data = json.get("data")?;
        if data.get("type").and_then(|v| v.as_str()) != Some("hook_progress") { return None; }

        let hook_event = data.get("hookEvent").and_then(|v| v.as_str()).unwrap_or("");
        let hook_name = data.get("hookName").and_then(|v| v.as_str()).unwrap_or(hook_event);
        let command = data.get("command").and_then(|v| v.as_str()).unwrap_or("");

        if hook_name.is_empty() { return None; }

        let output = if command.starts_with("echo '") && command.ends_with('\'') {
            command[6..command.len()-1].to_string()
        } else if command.starts_with("echo \"") && command.ends_with('"') {
            command[6..command.len()-1].to_string()
        } else if command.contains("; echo \"$OUT\"") || command.contains("; echo '$OUT'") {
            if let Some(start) = command.find("OUT='") {
                let rest = &command[start + 5..];
                rest.find('\'').map(|end| rest[..end].to_string()).unwrap_or_default()
            } else if let Some(start) = command.find("OUT=\"") {
                let rest = &command[start + 5..];
                rest.find('"').map(|end| rest[..end].to_string()).unwrap_or_default()
            } else { String::new() }
        } else { String::new() };

        // Always show hooks - use [hookName] as fallback when no output extracted
        let display_output = if output.is_empty() {
            format!("[{}]", hook_name)
        } else {
            output
        };
        Some(DisplayEvent::Hook { name: hook_name.to_string(), output: display_output })
    }

    fn parse_user_event(&self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        let mut events = Vec::new();
        let Some(message) = json.get("message") else { return events };
        let Some(content_val) = message.get("content") else { return events };

        if let Some(content) = content_val.as_str() {
            events.extend(Self::extract_hooks_from_content(content));
            events.push(DisplayEvent::UserMessage {
                uuid: json.get("uuid").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                content: content.to_string(),
            });
        } else if let Some(arr) = content_val.as_array() {
            for block in arr {
                let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match block_type {
                    "tool_result" => {
                        let tool_use_id = block.get("tool_use_id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                        let (tool_name, file_path) = self.tool_calls.get(&tool_use_id).cloned().unwrap_or(("Unknown".to_string(), None));
                        let content = if let Some(s) = block.get("content").and_then(|c| c.as_str()) {
                            s.to_string()
                        } else if let Some(arr) = block.get("content").and_then(|c| c.as_array()) {
                            arr.iter().filter_map(|b| {
                                if b.get("type").and_then(|t| t.as_str()) == Some("text") { b.get("text").and_then(|t| t.as_str()) } else { None }
                            }).collect::<Vec<_>>().join("\n")
                        } else { String::new() };

                        events.extend(Self::extract_hooks_from_content(&content));
                        if !content.is_empty() {
                            events.push(DisplayEvent::ToolResult { tool_use_id, tool_name, file_path, content });
                        }
                    }
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            events.extend(Self::extract_hooks_from_content(text));
                            if !text.is_empty() {
                                events.push(DisplayEvent::UserMessage {
                                    uuid: json.get("uuid").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    content: text.to_string(),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        events
    }

    fn parse_assistant_event(&mut self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        let mut events = Vec::new();
        let Some(message) = json.get("message") else { return events };
        let Some(message_id) = message.get("id").and_then(|v| v.as_str()) else { return events };
        let uuid = json.get("uuid").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let Some(content) = message.get("content").and_then(|v| v.as_array()) else { return events };

        for block in content {
            let Some(block_type) = block.get("type").and_then(|v| v.as_str()) else { continue };
            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        events.push(DisplayEvent::AssistantText {
                            uuid: uuid.clone(),
                            message_id: message_id.to_string(),
                            text: text.to_string(),
                        });
                    }
                }
                "tool_use" => {
                    if let (Some(tool_name), Some(input), Some(tool_use_id)) = (
                        block.get("name").and_then(|v| v.as_str()),
                        block.get("input"),
                        block.get("id").and_then(|v| v.as_str()),
                    ) {
                        let file_path = input.get("file_path").or_else(|| input.get("path"))
                            .and_then(|v| v.as_str()).map(|s| s.to_string());
                        self.tool_calls.insert(tool_use_id.to_string(), (tool_name.to_string(), file_path.clone()));
                        events.push(DisplayEvent::ToolCall {
                            uuid: uuid.clone(),
                            tool_use_id: tool_use_id.to_string(),
                            tool_name: tool_name.to_string(),
                            file_path,
                            input: input.clone(),
                        });
                    }
                }
                _ => {}
            }
        }
        events
    }

    fn parse_result_event(&self, json: &serde_json::Value) -> Option<DisplayEvent> {
        Some(DisplayEvent::Complete {
            session_id: json.get("session_id")?.as_str()?.to_string(),
            success: !json.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false),
            duration_ms: json.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0),
            cost_usd: json.get("total_cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0),
        })
    }

    fn parse_text_hook(&self, line: &str) -> Option<DisplayEvent> {
        let line = line.trim();
        if let Some(pos) = line.find(" hook success:") {
            return Some(DisplayEvent::Hook { name: line[..pos].to_string(), output: line[pos + 14..].trim().to_string() });
        }
        if let Some(pos) = line.find(" hook failed:") {
            return Some(DisplayEvent::Hook { name: line[..pos].to_string(), output: line[pos + 13..].trim().to_string() });
        }
        if line.ends_with(" hook success") {
            return Some(DisplayEvent::Hook { name: line.trim_end_matches(" hook success").to_string(), output: String::new() });
        }
        if line.ends_with(" hook failed") {
            return Some(DisplayEvent::Hook { name: line.trim_end_matches(" hook failed").to_string(), output: String::new() });
        }
        if line.contains(" hook ") || line.contains("Hook") {
            if let Some(pos) = line.find(" hook") {
                return Some(DisplayEvent::Hook { name: line[..pos].to_string(), output: line[pos..].to_string() });
            }
        }
        None
    }

    fn extract_hooks_from_content(content: &str) -> Vec<DisplayEvent> {
        let mut hooks = Vec::new();
        let mut search_start = 0;
        while let Some(start) = content[search_start..].find("<system-reminder>") {
            let abs_start = search_start + start + 17;
            if let Some(end) = content[abs_start..].find("</system-reminder>") {
                let reminder_content = &content[abs_start..abs_start + end];
                if let Some(hook_pos) = reminder_content.find(" hook success:") {
                    let name = reminder_content[..hook_pos].trim().to_string();
                    let output = reminder_content[hook_pos + 14..].trim().to_string();
                    if !output.is_empty() { hooks.push(DisplayEvent::Hook { name, output }); }
                } else if let Some(hook_pos) = reminder_content.find(" hook failed:") {
                    let name = reminder_content[..hook_pos].trim().to_string();
                    let output = reminder_content[hook_pos + 13..].trim().to_string();
                    hooks.push(DisplayEvent::Hook { name, output: format!("FAILED: {}", output) });
                }
                search_start = abs_start + end + 18;
            } else { break; }
        }
        hooks
    }
}

impl Default for EventParser {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_init_event() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"system","subtype":"init","session_id":"abc123","cwd":"/test","model":"claude-3"}"#;
        let events = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Init { session_id, cwd, model } => {
                assert_eq!(session_id, "abc123");
                assert_eq!(cwd, "/test");
                assert_eq!(model, "claude-3");
            }
            _ => panic!("Expected Init event"),
        }
    }

    #[test]
    fn test_parse_assistant_text() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"assistant","uuid":"u1","message":{"id":"msg1","model":"claude","role":"assistant","content":[{"type":"text","text":"Hello!"}]}}"#;
        let events = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::AssistantText { text, .. } => assert_eq!(text, "Hello!"),
            _ => panic!("Expected AssistantText event"),
        }
    }

    #[test]
    fn test_parse_tool_call() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"assistant","uuid":"u1","message":{"id":"msg1","model":"claude","role":"assistant","content":[{"type":"tool_use","id":"tool1","name":"Read","input":{"file_path":"/test/file.rs"}}]}}"#;
        let events = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolCall { tool_name, file_path, .. } => {
                assert_eq!(tool_name, "Read");
                assert_eq!(file_path.as_deref(), Some("/test/file.rs"));
            }
            _ => panic!("Expected ToolCall event"),
        }
    }

    #[test]
    fn test_parse_hook_response() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"system","subtype":"hook_response","hook_name":"SessionStart:startup","output":"Read CLAUDE.md before proceeding.\n","session_id":"abc123"}"#;
        let events = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "SessionStart:startup");
                assert_eq!(output, "Read CLAUDE.md before proceeding.");
            }
            _ => panic!("Expected Hook event"),
        }
    }

    #[test]
    fn test_parse_hook_started_ignored() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"system","subtype":"hook_started","hook_name":"SessionStart:startup","session_id":"abc123"}"#;
        let events = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 0, "hook_started should not produce events");
    }

    #[test]
    fn test_extract_hooks_from_system_reminder() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"<system-reminder>\nUserPromptSubmit hook success: Follow CLAUDE.md guidelines.\n</system-reminder>\nHello Claude"}}"#;
        let events = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 2);
        match &events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "UserPromptSubmit");
                assert_eq!(output, "Follow CLAUDE.md guidelines.");
            }
            _ => panic!("Expected Hook event"),
        }
    }

    #[test]
    fn test_parse_hook_progress_event() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"progress","data":{"type":"hook_progress","hookEvent":"PreToolUse","hookName":"PreToolUse:Bash","command":"echo 'Ensure this action complies with CLAUDE.md and AGENTS.md.'"}}"#;
        let events = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "PreToolUse:Bash");
                assert_eq!(output, "Ensure this action complies with CLAUDE.md and AGENTS.md.");
            }
            _ => panic!("Expected Hook event"),
        }
    }
}
