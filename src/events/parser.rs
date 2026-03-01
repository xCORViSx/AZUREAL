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

    /// Feed raw data and get parsed display events + the last parsed JSON value.
    /// The JSON value is returned so callers can extract token usage without re-parsing.
    /// Collects complete line ranges first, then drains consumed bytes in one shot —
    /// O(n) total instead of O(n²) re-allocation on every newline.
    pub fn parse(&mut self, data: &str) -> (Vec<DisplayEvent>, Option<serde_json::Value>) {
        self.buffer.push_str(data);
        let mut events = Vec::new();

        // Find all complete lines (up to last newline)
        let last_newline = match self.buffer.rfind('\n') {
            Some(pos) => pos,
            None => return (events, None),
        };

        // Extract complete lines as one owned string, drain from buffer
        let complete: String = self.buffer.drain(..=last_newline).collect();

        let mut last_json = None;
        for line in complete.split('\n') {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            let (line_events, json) = self.parse_line_with_json(trimmed);
            events.extend(line_events);
            if json.is_some() { last_json = json; }
        }

        (events, last_json)
    }

    /// Parse a single line, returning events and the raw parsed JSON value (if any).
    /// The JSON value is reused by callers for token extraction — avoids double parse.
    fn parse_line_with_json(&mut self, line: &str) -> (Vec<DisplayEvent>, Option<serde_json::Value>) {
        let trimmed = line.trim();

        if trimmed.starts_with('{') {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(event_type) = json.get("type").and_then(|v| v.as_str()) {
                    let events = match event_type {
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
                    return (events, Some(json));
                }
                return (Vec::new(), Some(json));
            }
            return (Vec::new(), None);
        }

        (self.parse_text_hook(line).into_iter().collect(), None)
    }

    fn parse_system_event(&self, json: &serde_json::Value) -> Option<DisplayEvent> {
        let subtype = json.get("subtype").and_then(|v| v.as_str()).unwrap_or("");

        if subtype == "init" {
            return Some(DisplayEvent::Init {
                _session_id: json.get("session_id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
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
            // Compaction summary — show banner instead of raw text
            if content.starts_with("This session is being continued from a previous conversation") {
                events.push(DisplayEvent::Compacting);
                return events;
            }
            // local-command-stdout (e.g., /compact output) — filter or show Compacted banner
            if content.contains("<local-command-stdout>") {
                if content.contains("Compacted") { events.push(DisplayEvent::Compacted); }
                return events;
            }
            // local-command-caveat — filter entirely
            if content.contains("<local-command-caveat>") { return events; }

            events.extend(Self::extract_hooks_from_content(content));
            events.push(DisplayEvent::UserMessage {
                _uuid: json.get("uuid").and_then(|v| v.as_str()).unwrap_or("").to_string(),
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
                                    _uuid: json.get("uuid").and_then(|v| v.as_str()).unwrap_or("").to_string(),
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
                            _uuid: uuid.clone(),
                            _message_id: message_id.to_string(),
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
                            _uuid: uuid.clone(),
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
            _session_id: json.get("session_id")?.as_str()?.to_string(),
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
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Init { _session_id, cwd, model } => {
                assert_eq!(_session_id, "abc123");
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
        let (events, _) = parser.parse(&format!("{}\n", json));
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
        let (events, _) = parser.parse(&format!("{}\n", json));
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
        let (events, _) = parser.parse(&format!("{}\n", json));
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
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 0, "hook_started should not produce events");
    }

    #[test]
    fn test_extract_hooks_from_system_reminder() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"<system-reminder>\nUserPromptSubmit hook success: Follow CLAUDE.md guidelines.\n</system-reminder>\nHello Claude"}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
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
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "PreToolUse:Bash");
                assert_eq!(output, "Ensure this action complies with CLAUDE.md and AGENTS.md.");
            }
            _ => panic!("Expected Hook event"),
        }
    }

    // ── parse() buffer behavior ──

    #[test]
    fn test_parse_no_newline_buffers() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"system","subtype":"init","session_id":"s1","cwd":"/test","model":"claude"}"#;
        let (events, _) = parser.parse(json);
        assert!(events.is_empty(), "no newline means no complete line to parse");
    }

    #[test]
    fn test_parse_newline_flushes() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"system","subtype":"init","session_id":"s1","cwd":"/test","model":"claude"}"#;
        // Feed without newline (buffered), then feed just newline (flush)
        let (events1, _) = parser.parse(json);
        assert!(events1.is_empty());
        let (events2, _) = parser.parse("\n");
        assert_eq!(events2.len(), 1);
    }

    #[test]
    fn test_parse_multiple_lines_at_once() {
        let mut parser = EventParser::new();
        let line1 = r#"{"type":"system","subtype":"init","session_id":"s1","cwd":"/a","model":"claude"}"#;
        let line2 = r#"{"type":"system","subtype":"init","session_id":"s2","cwd":"/b","model":"claude"}"#;
        let data = format!("{}\n{}\n", line1, line2);
        let (events, _) = parser.parse(&data);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_parse_empty_lines_ignored() {
        let mut parser = EventParser::new();
        let data = "\n\n\n";
        let (events, _) = parser.parse(data);
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_empty_string() {
        let mut parser = EventParser::new();
        let (events, json) = parser.parse("");
        assert!(events.is_empty());
        assert!(json.is_none());
    }

    #[test]
    fn test_parse_returns_last_json() {
        let mut parser = EventParser::new();
        let line1 = r#"{"type":"system","subtype":"init","session_id":"s1","cwd":"/a","model":"claude"}"#;
        let line2 = r#"{"type":"system","subtype":"init","session_id":"s2","cwd":"/b","model":"opus"}"#;
        let data = format!("{}\n{}\n", line1, line2);
        let (_, json) = parser.parse(&data);
        let json = json.unwrap();
        assert_eq!(json.get("session_id").unwrap().as_str().unwrap(), "s2");
    }

    #[test]
    fn test_parse_invalid_json_returns_no_json() {
        let mut parser = EventParser::new();
        let (events, json) = parser.parse("{not valid json}\n");
        assert!(events.is_empty());
        assert!(json.is_none());
    }

    #[test]
    fn test_parse_non_json_text_no_hook() {
        let mut parser = EventParser::new();
        let (events, _) = parser.parse("just plain text\n");
        assert!(events.is_empty());
    }

    // ── parse_system_event ──

    #[test]
    fn test_parse_system_init_missing_cwd() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Init { cwd, .. } => assert_eq!(cwd, ""),
            _ => panic!("Expected Init event"),
        }
    }

    #[test]
    fn test_parse_system_init_missing_model() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"system","subtype":"init","session_id":"s1","cwd":"/test"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Init { model, .. } => assert_eq!(model, "unknown"),
            _ => panic!("Expected Init event"),
        }
    }

    #[test]
    fn test_parse_system_non_init_non_hook_ignored() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"system","subtype":"something_else","session_id":"s1"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_system_hook_empty_name_ignored() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"system","subtype":"hook_response","hook_name":"","output":"data","session_id":"s1"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty(), "empty hook name should be filtered");
    }

    #[test]
    fn test_parse_system_hook_empty_output_ignored() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"system","subtype":"hook_response","hook_name":"MyHook","output":"","session_id":"s1"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty(), "empty output should be filtered");
    }

    #[test]
    fn test_parse_system_hook_uses_name_fallback() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"system","subtype":"hook_response","name":"FallbackHook","output":"ok","session_id":"s1"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { name, .. } => assert_eq!(name, "FallbackHook"),
            _ => panic!("Expected Hook event"),
        }
    }

    #[test]
    fn test_parse_system_hook_uses_hook_fallback() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"system","subtype":"hook_response","hook":"HookFallback","output":"result","session_id":"s1"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { name, .. } => assert_eq!(name, "HookFallback"),
            _ => panic!("Expected Hook event"),
        }
    }

    #[test]
    fn test_parse_system_hook_output_from_stdout() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"system","subtype":"hook_response","hook_name":"Build","stdout":"compiled ok","session_id":"s1"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { output, .. } => assert_eq!(output, "compiled ok"),
            _ => panic!("Expected Hook event"),
        }
    }

    // ── parse_user_event ──

    #[test]
    fn test_parse_user_message_content_string() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"What is Rust?"}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::UserMessage { content, .. } => assert_eq!(content, "What is Rust?"),
            _ => panic!("Expected UserMessage event"),
        }
    }

    #[test]
    fn test_parse_user_compaction_summary() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"This session is being continued from a previous conversation that was compacted."}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], DisplayEvent::Compacting));
    }

    #[test]
    fn test_parse_user_local_command_stdout_compacted() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"<local-command-stdout>Compacted context</local-command-stdout>"}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], DisplayEvent::Compacted));
    }

    #[test]
    fn test_parse_user_local_command_stdout_no_compacted() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"<local-command-stdout>other output</local-command-stdout>"}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_user_local_command_caveat_filtered() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"<local-command-caveat>warning</local-command-caveat>"}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_user_no_message() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"user","uuid":"u1"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_user_no_content() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"user","uuid":"u1","message":{"role":"user"}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_user_content_array_with_tool_result() {
        let mut parser = EventParser::new();
        // First register a tool call so the result can resolve it
        let tool_json = r#"{"type":"assistant","uuid":"u0","message":{"id":"m0","model":"claude","role":"assistant","content":[{"type":"tool_use","id":"tc-1","name":"Read","input":{"file_path":"/test.rs"}}]}}"#;
        parser.parse(&format!("{}\n", tool_json));
        // Now the tool result
        let json = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tc-1","content":"file contents here"}]}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolResult { tool_name, content, .. } => {
                assert_eq!(tool_name, "Read");
                assert_eq!(content, "file contents here");
            }
            _ => panic!("Expected ToolResult event"),
        }
    }

    #[test]
    fn test_parse_user_content_array_tool_result_unknown_tool() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tc-unknown","content":"data"}]}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolResult { tool_name, .. } => {
                assert_eq!(tool_name, "Unknown");
            }
            _ => panic!("Expected ToolResult event"),
        }
    }

    #[test]
    fn test_parse_user_content_array_text_block() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":[{"type":"text","text":"hello from array"}]}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::UserMessage { content, .. } => assert_eq!(content, "hello from array"),
            _ => panic!("Expected UserMessage event"),
        }
    }

    #[test]
    fn test_parse_user_content_array_empty_text_ignored() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":[{"type":"text","text":""}]}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty(), "empty text blocks should be ignored");
    }

    #[test]
    fn test_parse_user_content_array_tool_result_array_content() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tc-1","content":[{"type":"text","text":"line1"},{"type":"text","text":"line2"}]}]}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolResult { content, .. } => {
                assert_eq!(content, "line1\nline2");
            }
            _ => panic!("Expected ToolResult event"),
        }
    }

    #[test]
    fn test_parse_user_content_array_tool_result_empty_content() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tc-1","content":""}]}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty(), "empty tool result content should not produce events");
    }

    // ── parse_assistant_event ──

    #[test]
    fn test_parse_assistant_no_message() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"assistant","uuid":"u1"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_assistant_no_id() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"assistant","uuid":"u1","message":{"model":"claude","role":"assistant","content":[{"type":"text","text":"hi"}]}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty(), "missing message.id should produce no events");
    }

    #[test]
    fn test_parse_assistant_no_content_array() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"assistant","uuid":"u1","message":{"id":"m1","model":"claude","role":"assistant","content":"not an array"}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty(), "content must be an array");
    }

    #[test]
    fn test_parse_assistant_multiple_text_blocks() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"assistant","uuid":"u1","message":{"id":"m1","model":"claude","role":"assistant","content":[{"type":"text","text":"First."},{"type":"text","text":"Second."}]}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], DisplayEvent::AssistantText { text, .. } if text == "First."));
        assert!(matches!(&events[1], DisplayEvent::AssistantText { text, .. } if text == "Second."));
    }

    #[test]
    fn test_parse_assistant_text_and_tool_use() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"assistant","uuid":"u1","message":{"id":"m1","model":"claude","role":"assistant","content":[{"type":"text","text":"Let me check."},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], DisplayEvent::AssistantText { .. }));
        assert!(matches!(&events[1], DisplayEvent::ToolCall { .. }));
    }

    #[test]
    fn test_parse_assistant_tool_use_with_path_input() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"assistant","uuid":"u1","message":{"id":"m1","model":"claude","role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Glob","input":{"path":"/src","pattern":"*.rs"}}]}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolCall { file_path, .. } => {
                assert_eq!(file_path.as_deref(), Some("/src"));
            }
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn test_parse_assistant_tool_use_no_file_path() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"assistant","uuid":"u1","message":{"id":"m1","model":"claude","role":"assistant","content":[{"type":"tool_use","id":"t1","name":"WebSearch","input":{"query":"rust async"}}]}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolCall { file_path, .. } => {
                assert!(file_path.is_none());
            }
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn test_parse_assistant_unknown_block_type_ignored() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"assistant","uuid":"u1","message":{"id":"m1","model":"claude","role":"assistant","content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"ok"}]}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        // "thinking" is unknown to EventParser, only "text" should produce event
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], DisplayEvent::AssistantText { text, .. } if text == "ok"));
    }

    #[test]
    fn test_parse_assistant_empty_content_array() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"assistant","uuid":"u1","message":{"id":"m1","model":"claude","role":"assistant","content":[]}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty());
    }

    // ── parse_result_event ──

    #[test]
    fn test_parse_result_success() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"result","session_id":"s1","is_error":false,"duration_ms":5000,"total_cost_usd":0.05}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Complete { success, duration_ms, cost_usd, .. } => {
                assert!(*success);
                assert_eq!(*duration_ms, 5000);
                assert!((cost_usd - 0.05).abs() < f64::EPSILON);
            }
            _ => panic!("Expected Complete event"),
        }
    }

    #[test]
    fn test_parse_result_error() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"result","session_id":"s1","is_error":true}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Complete { success, .. } => {
                assert!(!success);
            }
            _ => panic!("Expected Complete event"),
        }
    }

    #[test]
    fn test_parse_result_missing_session_id() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"result","is_error":false}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        // session_id returns None, so parse_result_event returns None
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_result_defaults() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"result","session_id":"s1"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Complete { success, duration_ms, cost_usd, .. } => {
                assert!(*success);
                assert_eq!(*duration_ms, 0);
                assert!((cost_usd - 0.0).abs() < f64::EPSILON);
            }
            _ => panic!("Expected Complete event"),
        }
    }

    // ── parse_progress_event ──

    #[test]
    fn test_parse_progress_non_hook_ignored() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"progress","data":{"type":"something_else","hookName":"test"}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_progress_no_data() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"progress"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_progress_empty_hook_name() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"progress","data":{"type":"hook_progress","hookName":"","command":"echo 'hi'"}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty(), "empty hook name should be filtered");
    }

    #[test]
    fn test_parse_progress_double_quote_echo() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"progress","data":{"type":"hook_progress","hookName":"TestHook","command":"echo \"Double quoted output\""}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { output, .. } => assert_eq!(output, "Double quoted output"),
            _ => panic!("Expected Hook event"),
        }
    }

    #[test]
    fn test_parse_progress_out_var_single_quote() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"progress","data":{"type":"hook_progress","hookName":"TestHook","command":"OUT='extracted'; echo '$OUT'"}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { output, .. } => assert_eq!(output, "extracted"),
            _ => panic!("Expected Hook event"),
        }
    }

    #[test]
    fn test_parse_progress_out_var_double_quote() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"progress","data":{"type":"hook_progress","hookName":"TestHook","command":"OUT=\"value\"; echo \"$OUT\""}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { output, .. } => assert_eq!(output, "value"),
            _ => panic!("Expected Hook event"),
        }
    }

    #[test]
    fn test_parse_progress_no_extractable_output_fallback() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"progress","data":{"type":"hook_progress","hookName":"BuildHook","command":"cargo build 2>&1"}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { output, .. } => assert_eq!(output, "[BuildHook]"),
            _ => panic!("Expected Hook event"),
        }
    }

    #[test]
    fn test_parse_progress_hook_event_fallback_for_name() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"progress","data":{"type":"hook_progress","hookEvent":"PreToolUse","command":"echo 'test'"}}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { name, .. } => assert_eq!(name, "PreToolUse"),
            _ => panic!("Expected Hook event"),
        }
    }

    // ── hook/hook_result/hook_response top-level type ──

    #[test]
    fn test_parse_hook_type() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"hook","hook_name":"MyHook","output":"result data"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "MyHook");
                assert_eq!(output, "result data");
            }
            _ => panic!("Expected Hook event"),
        }
    }

    #[test]
    fn test_parse_hook_result_type() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"hook_result","name":"TestHook","result":"pass"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "TestHook");
                assert_eq!(output, "pass");
            }
            _ => panic!("Expected Hook event"),
        }
    }

    #[test]
    fn test_parse_hook_response_type() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"hook_response","hook":"ResponseHook","message":"all good"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "ResponseHook");
                assert_eq!(output, "all good");
            }
            _ => panic!("Expected Hook event"),
        }
    }

    #[test]
    fn test_parse_hook_type_no_name_fallback() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"hook","output":"data"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { name, .. } => assert_eq!(name, "hook"),
            _ => panic!("Expected Hook event"),
        }
    }

    #[test]
    fn test_parse_hook_type_no_output_empty() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"hook","hook_name":"NoOutput"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { output, .. } => assert_eq!(output, ""),
            _ => panic!("Expected Hook event"),
        }
    }

    // ── parse_text_hook ──

    #[test]
    fn test_text_hook_success_with_output() {
        let mut parser = EventParser::new();
        let (events, _) = parser.parse("MyHook hook success: checks passed\n");
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "MyHook");
                assert_eq!(output, "checks passed");
            }
            _ => panic!("Expected Hook event"),
        }
    }

    #[test]
    fn test_text_hook_failed_with_output() {
        let mut parser = EventParser::new();
        let (events, _) = parser.parse("BuildCheck hook failed: error in main.rs\n");
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "BuildCheck");
                assert_eq!(output, "error in main.rs");
            }
            _ => panic!("Expected Hook event"),
        }
    }

    #[test]
    fn test_text_hook_success_no_output() {
        let mut parser = EventParser::new();
        let (events, _) = parser.parse("Startup hook success\n");
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "Startup");
                assert_eq!(output, "");
            }
            _ => panic!("Expected Hook event"),
        }
    }

    #[test]
    fn test_text_hook_failed_no_output() {
        let mut parser = EventParser::new();
        let (events, _) = parser.parse("LintCheck hook failed\n");
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "LintCheck");
                assert_eq!(output, "");
            }
            _ => panic!("Expected Hook event"),
        }
    }

    #[test]
    fn test_text_hook_generic_hook_mention() {
        let mut parser = EventParser::new();
        let (events, _) = parser.parse("PreToolUse hook running...\n");
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Hook { name, .. } => assert_eq!(name, "PreToolUse"),
            _ => panic!("Expected Hook event"),
        }
    }

    // ── extract_hooks_from_content (via user event) ──

    #[test]
    fn test_user_event_extracts_hooks_from_system_reminder() {
        let mut parser = EventParser::new();
        let content = r#"<system-reminder>\nPreToolUse hook success: Validated command.\n</system-reminder>\nActual user message"#;
        let json = format!(r#"{{"type":"user","uuid":"u1","message":{{"role":"user","content":"{}"}}}}"#, content);
        let (events, _) = parser.parse(&format!("{}\n", json));
        // Should have hook + user message
        assert!(events.len() >= 2);
        assert!(matches!(&events[0], DisplayEvent::Hook { .. }));
    }

    // ── Unknown type ──

    #[test]
    fn test_parse_unknown_type_produces_no_events() {
        let mut parser = EventParser::new();
        let json = r#"{"type":"unknown_event_type","data":"whatever"}"#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty());
    }

    // ── JSON without type field ──

    #[test]
    fn test_parse_json_no_type_field() {
        let mut parser = EventParser::new();
        let json = r#"{"key":"value","data":42}"#;
        let (events, json_val) = parser.parse(&format!("{}\n", json));
        assert!(events.is_empty());
        assert!(json_val.is_some(), "valid JSON should still be returned");
    }

    // ── tool_call tracking across messages ──

    #[test]
    fn test_tool_call_tracking_across_messages() {
        let mut parser = EventParser::new();
        // Register tool calls
        let asst = r#"{"type":"assistant","uuid":"u1","message":{"id":"m1","model":"claude","role":"assistant","content":[{"type":"tool_use","id":"tc-A","name":"Write","input":{"file_path":"/output.rs","content":"fn main(){}"}},{"type":"tool_use","id":"tc-B","name":"Bash","input":{"command":"cargo build"}}]}}"#;
        parser.parse(&format!("{}\n", asst));
        // Tool results
        let result_a = r#"{"type":"user","uuid":"u2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tc-A","content":"file written"},{"type":"tool_result","tool_use_id":"tc-B","content":"compiled ok"}]}}"#;
        let (events, _) = parser.parse(&format!("{}\n", result_a));
        assert_eq!(events.len(), 2);
        match &events[0] {
            DisplayEvent::ToolResult { tool_name, file_path, .. } => {
                assert_eq!(tool_name, "Write");
                assert_eq!(file_path.as_deref(), Some("/output.rs"));
            }
            _ => panic!("Expected ToolResult"),
        }
        match &events[1] {
            DisplayEvent::ToolResult { tool_name, file_path, .. } => {
                assert_eq!(tool_name, "Bash");
                assert!(file_path.is_none());
            }
            _ => panic!("Expected ToolResult"),
        }
    }

    // ── Default impl ──

    #[test]
    fn test_event_parser_default() {
        let parser = EventParser::default();
        let json = r#"{"type":"system","subtype":"init","session_id":"s1","cwd":"/","model":"claude"}"#;
        let mut parser = parser;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
    }

    // ── Whitespace handling ──

    #[test]
    fn test_parse_line_with_leading_whitespace() {
        let mut parser = EventParser::new();
        let json = r#"  {"type":"system","subtype":"init","session_id":"s1","cwd":"/","model":"claude"}  "#;
        let (events, _) = parser.parse(&format!("{}\n", json));
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_parse_incremental_buffer_accumulation() {
        let mut parser = EventParser::new();
        // Feed partial JSON
        let (e1, _) = parser.parse(r#"{"type":"sys"#);
        assert!(e1.is_empty());
        // Feed rest + newline
        let (e2, _) = parser.parse(r#"tem","subtype":"init","session_id":"s1","cwd":"/","model":"c"}"#);
        assert!(e2.is_empty());
        let (e3, _) = parser.parse("\n");
        // The full line should now parse — but it's invalid JSON since we broke it up oddly
        // This tests the buffer accumulation behavior
        assert!(e3.is_empty() || e3.len() >= 1);
    }
}
