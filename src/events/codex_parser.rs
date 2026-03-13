//! Event parser for Codex CLI `--json` JSONL output
//!
//! Parses Codex stdout JSONL events into DisplayEvents.
//! Event types: thread.started, turn.started, item.started, item.completed,
//! turn.completed, error, turn.failed.

use std::collections::HashMap;
use super::display::DisplayEvent;

/// Parser for Codex CLI JSONL streaming events
pub struct CodexEventParser {
    buffer: String,
    /// Track items by ID for matching started → completed
    items: HashMap<String, String>,
}

impl CodexEventParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            items: HashMap::new(),
        }
    }

    /// Feed raw data and get parsed display events + the last parsed JSON value.
    /// Same interface as EventParser::parse() for interchangeability.
    pub fn parse(&mut self, data: &str) -> (Vec<DisplayEvent>, Option<serde_json::Value>) {
        self.buffer.push_str(data);
        let mut events = Vec::new();

        let last_newline = match self.buffer.rfind('\n') {
            Some(pos) => pos,
            None => return (events, None),
        };

        let complete: String = self.buffer.drain(..=last_newline).collect();

        let mut last_json = None;
        for line in complete.split('\n') {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            let (line_events, json) = self.parse_line(trimmed);
            events.extend(line_events);
            if json.is_some() { last_json = json; }
        }

        (events, last_json)
    }

    /// Parse a single Codex JSONL line into DisplayEvents
    fn parse_line(&mut self, line: &str) -> (Vec<DisplayEvent>, Option<serde_json::Value>) {
        if !line.starts_with('{') {
            return (Vec::new(), None);
        }

        let json: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => return (Vec::new(), None),
        };

        let event_type = match json.get("type").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return (Vec::new(), Some(json)),
        };

        let events = match event_type {
            "thread.started" => self.parse_thread_started(&json),
            "turn.started" => Vec::new(),
            "item.started" => self.parse_item_started(&json),
            "item.updated" => Vec::new(),
            "item.completed" => self.parse_item_completed(&json),
            "turn.completed" => self.parse_turn_completed(&json),
            "error" => self.parse_error(&json),
            "turn.failed" => self.parse_turn_failed(&json),
            _ => Vec::new(),
        };

        (events, Some(json))
    }

    fn parse_thread_started(&self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        let thread_id = json.get("thread_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        vec![DisplayEvent::Init {
            _session_id: thread_id,
            cwd: String::new(),
            model: "codex".to_string(),
        }]
    }

    fn parse_item_started(&mut self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        let item = match json.get("item") {
            Some(i) => i,
            None => return Vec::new(),
        };

        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();

        match item_type {
            "command_execution" => {
                let command = item.get("command").and_then(|v| v.as_str()).unwrap_or("").to_string();
                // Track for matching with completed
                self.items.insert(item_id.clone(), "Bash".to_string());
                vec![DisplayEvent::ToolCall {
                    _uuid: String::new(),
                    tool_use_id: item_id,
                    tool_name: "Bash".to_string(),
                    file_path: None,
                    input: serde_json::json!({ "command": command }),
                }]
            }
            _ => Vec::new(),
        }
    }

    fn parse_item_completed(&mut self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        let item = match json.get("item") {
            Some(i) => i,
            None => return Vec::new(),
        };

        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();

        match item_type {
            "reasoning" => {
                let text = item.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if text.is_empty() {
                    Vec::new()
                } else {
                    vec![DisplayEvent::AssistantText {
                        _uuid: String::new(),
                        _message_id: String::new(),
                        text,
                    }]
                }
            }
            "agent_message" => {
                let text = item.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                vec![DisplayEvent::AssistantText {
                    _uuid: String::new(),
                    _message_id: String::new(),
                    text,
                }]
            }
            "command_execution" => {
                let command = item.get("command").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let output = item.get("aggregated_output").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let exit_code = item.get("exit_code").and_then(|v| v.as_i64());
                let is_error = exit_code.map(|c| c != 0).unwrap_or(false);

                // If we didn't see an item.started for this, emit the ToolCall too
                let mut events = Vec::new();
                if !self.items.contains_key(&item_id) {
                    events.push(DisplayEvent::ToolCall {
                        _uuid: String::new(),
                        tool_use_id: item_id.clone(),
                        tool_name: "Bash".to_string(),
                        file_path: None,
                        input: serde_json::json!({ "command": command }),
                    });
                }
                self.items.remove(&item_id);

                let content = if output.is_empty() {
                    format!("Exit code: {}", exit_code.unwrap_or(0))
                } else {
                    output
                };

                events.push(DisplayEvent::ToolResult {
                    tool_use_id: item_id,
                    tool_name: "Bash".to_string(),
                    file_path: None,
                    content,
                    is_error,
                });
                events
            }
            "file_change" => {
                let changes = item.get("changes").and_then(|v| v.as_array());
                let mut events = Vec::new();

                if let Some(changes) = changes {
                    for change in changes {
                        let path = change.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let kind = change.get("kind").and_then(|v| v.as_str()).unwrap_or("update").to_string();
                        let change_id = format!("{}-{}", item_id, path);

                        events.push(DisplayEvent::ToolCall {
                            _uuid: String::new(),
                            tool_use_id: change_id.clone(),
                            tool_name: "Edit".to_string(),
                            file_path: Some(path.clone()),
                            input: serde_json::json!({ "file_path": path, "kind": kind }),
                        });
                        events.push(DisplayEvent::ToolResult {
                            tool_use_id: change_id,
                            tool_name: "Edit".to_string(),
                            file_path: Some(path.clone()),
                            content: format!("File {}: {}", kind, path),
                            is_error: false,
                        });
                    }
                }
                events
            }
            "mcp_tool_call" => {
                let server = item.get("server").and_then(|v| v.as_str()).unwrap_or("mcp");
                let tool = item.get("tool").and_then(|v| v.as_str()).unwrap_or("unknown");
                let tool_name = format!("{}:{}", server, tool);
                let result = item.get("result").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let error = item.get("error").and_then(|v| v.as_str());

                vec![
                    DisplayEvent::ToolCall {
                        _uuid: String::new(),
                        tool_use_id: item_id.clone(),
                        tool_name: tool_name.clone(),
                        file_path: None,
                        input: item.get("arguments").cloned().unwrap_or(serde_json::Value::Null),
                    },
                    DisplayEvent::ToolResult {
                        tool_use_id: item_id,
                        tool_name,
                        file_path: None,
                        content: error.map(|e| e.to_string()).unwrap_or(result),
                        is_error: error.is_some(),
                    },
                ]
            }
            _ => Vec::new(),
        }
    }

    fn parse_turn_completed(&self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        let usage = json.get("usage");
        let input_tokens = usage.and_then(|u| u.get("input_tokens")).and_then(|v| v.as_u64()).unwrap_or(0);
        let output_tokens = usage.and_then(|u| u.get("output_tokens")).and_then(|v| v.as_u64()).unwrap_or(0);

        vec![DisplayEvent::Complete {
            _session_id: String::new(),
            success: true,
            duration_ms: 0,
            cost_usd: estimate_codex_cost(input_tokens, output_tokens),
        }]
    }

    fn parse_error(&self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        let message = json.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error").to_string();
        vec![DisplayEvent::AssistantText {
            _uuid: String::new(),
            _message_id: String::new(),
            text: format!("Error: {}", message),
        }]
    }

    fn parse_turn_failed(&self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        let message = json.get("error")
            .and_then(|e| e.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("Turn failed");

        vec![DisplayEvent::Complete {
            _session_id: String::new(),
            success: false,
            duration_ms: 0,
            cost_usd: 0.0,
        }, DisplayEvent::AssistantText {
            _uuid: String::new(),
            _message_id: String::new(),
            text: format!("Error: {}", message),
        }]
    }
}

/// Rough cost estimate for Codex models (o3-level pricing)
fn estimate_codex_cost(input_tokens: u64, output_tokens: u64) -> f64 {
    // Approximate: $10/M input, $30/M output (o3 pricing)
    (input_tokens as f64 * 10.0 + output_tokens as f64 * 30.0) / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── CodexEventParser construction ──

    #[test]
    fn parser_new() {
        let p = CodexEventParser::new();
        assert!(p.buffer.is_empty());
        assert!(p.items.is_empty());
    }

    // ── thread.started ──

    #[test]
    fn parse_thread_started() {
        let mut p = CodexEventParser::new();
        let (events, json) = p.parse(r#"{"type":"thread.started","thread_id":"abc-123"}"#);
        // No newline yet — should be buffered
        assert!(events.is_empty());
        assert!(json.is_none());

        // Feed newline
        let (events, json) = p.parse("\n");
        assert_eq!(events.len(), 1);
        assert!(json.is_some());
        match &events[0] {
            DisplayEvent::Init { _session_id, model, .. } => {
                assert_eq!(_session_id, "abc-123");
                assert_eq!(model, "codex");
            }
            _ => panic!("expected Init"),
        }
    }

    #[test]
    fn parse_thread_started_missing_thread_id() {
        let mut p = CodexEventParser::new();
        let (events, _) = p.parse("{\"type\":\"thread.started\"}\n");
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Init { _session_id, .. } => assert!(_session_id.is_empty()),
            _ => panic!("expected Init"),
        }
    }

    // ── turn.started (no-op) ──

    #[test]
    fn parse_turn_started_is_noop() {
        let mut p = CodexEventParser::new();
        let (events, _) = p.parse("{\"type\":\"turn.started\"}\n");
        assert!(events.is_empty());
    }

    // ── item.started + item.completed (command_execution) ──

    #[test]
    fn parse_command_execution_started() {
        let mut p = CodexEventParser::new();
        let line = r#"{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"ls -la","aggregated_output":"","exit_code":null,"status":"in_progress"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolCall { tool_name, tool_use_id, input, .. } => {
                assert_eq!(tool_name, "Bash");
                assert_eq!(tool_use_id, "item_1");
                assert_eq!(input["command"], "ls -la");
            }
            _ => panic!("expected ToolCall"),
        }
        assert!(p.items.contains_key("item_1"));
    }

    #[test]
    fn parse_command_execution_completed_with_started() {
        let mut p = CodexEventParser::new();
        let started = r#"{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"ls","aggregated_output":"","exit_code":null,"status":"in_progress"}}"#;
        let completed = r#"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"ls","aggregated_output":"file1\nfile2\n","exit_code":0,"status":"completed"}}"#;
        let (_, _) = p.parse(&format!("{}\n", started));
        let (events, _) = p.parse(&format!("{}\n", completed));
        // Should only have ToolResult (no duplicate ToolCall)
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolResult { tool_name, content, is_error, .. } => {
                assert_eq!(tool_name, "Bash");
                assert!(content.contains("file1"));
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn parse_command_execution_completed_without_started() {
        let mut p = CodexEventParser::new();
        let completed = r#"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"ls","aggregated_output":"output","exit_code":0,"status":"completed"}}"#;
        let (events, _) = p.parse(&format!("{}\n", completed));
        // Should emit both ToolCall and ToolResult
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], DisplayEvent::ToolCall { .. }));
        assert!(matches!(&events[1], DisplayEvent::ToolResult { .. }));
    }

    #[test]
    fn parse_command_execution_failed() {
        let mut p = CodexEventParser::new();
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"false","aggregated_output":"","exit_code":1,"status":"completed"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        let result = events.iter().find(|e| matches!(e, DisplayEvent::ToolResult { .. }));
        match result.unwrap() {
            DisplayEvent::ToolResult { is_error, .. } => assert!(is_error),
            _ => unreachable!(),
        }
    }

    #[test]
    fn parse_command_execution_empty_output() {
        let mut p = CodexEventParser::new();
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"true","aggregated_output":"","exit_code":0,"status":"completed"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        let result = events.iter().find(|e| matches!(e, DisplayEvent::ToolResult { .. }));
        match result.unwrap() {
            DisplayEvent::ToolResult { content, .. } => assert!(content.contains("Exit code: 0")),
            _ => unreachable!(),
        }
    }

    // ── item.completed (reasoning) ──

    #[test]
    fn parse_reasoning() {
        let mut p = CodexEventParser::new();
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"reasoning","text":"Thinking about it..."}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::AssistantText { text, .. } => assert_eq!(text, "Thinking about it..."),
            _ => panic!("expected AssistantText"),
        }
    }

    #[test]
    fn parse_reasoning_empty_text() {
        let mut p = CodexEventParser::new();
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"reasoning","text":""}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert!(events.is_empty());
    }

    // ── item.completed (agent_message) ──

    #[test]
    fn parse_agent_message() {
        let mut p = CodexEventParser::new();
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"Hello world"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::AssistantText { text, .. } => assert_eq!(text, "Hello world"),
            _ => panic!("expected AssistantText"),
        }
    }

    #[test]
    fn parse_agent_message_with_markdown() {
        let mut p = CodexEventParser::new();
        let line = r##"{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"# Title\n\n```rust\nfn main() {}\n```"}}"##;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::AssistantText { text, .. } => {
                assert!(text.contains("# Title"));
            }
            _ => panic!("expected AssistantText"),
        }
    }

    // ── item.completed (file_change) ──

    #[test]
    fn parse_file_change_single() {
        let mut p = CodexEventParser::new();
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"file_change","changes":[{"path":"src/main.rs","kind":"update"}],"status":"completed"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 2); // ToolCall + ToolResult
        match &events[0] {
            DisplayEvent::ToolCall { tool_name, file_path, .. } => {
                assert_eq!(tool_name, "Edit");
                assert_eq!(file_path.as_deref(), Some("src/main.rs"));
            }
            _ => panic!("expected ToolCall"),
        }
        match &events[1] {
            DisplayEvent::ToolResult { content, file_path, .. } => {
                assert!(content.contains("update"));
                assert_eq!(file_path.as_deref(), Some("src/main.rs"));
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn parse_file_change_multiple() {
        let mut p = CodexEventParser::new();
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"file_change","changes":[{"path":"a.rs","kind":"create"},{"path":"b.rs","kind":"update"}],"status":"completed"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 4); // 2 ToolCall + 2 ToolResult
    }

    #[test]
    fn parse_file_change_empty_changes() {
        let mut p = CodexEventParser::new();
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"file_change","changes":[],"status":"completed"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert!(events.is_empty());
    }

    // ── item.completed (mcp_tool_call) ──

    #[test]
    fn parse_mcp_tool_call() {
        let mut p = CodexEventParser::new();
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"mcp_tool_call","server":"docs","tool":"search","arguments":{"query":"help"},"result":"Found docs","error":null}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 2);
        match &events[0] {
            DisplayEvent::ToolCall { tool_name, input, .. } => {
                assert_eq!(tool_name, "docs:search");
                assert_eq!(input["query"], "help");
            }
            _ => panic!("expected ToolCall"),
        }
        match &events[1] {
            DisplayEvent::ToolResult { content, is_error, .. } => {
                assert_eq!(content, "Found docs");
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn parse_mcp_tool_call_with_error() {
        let mut p = CodexEventParser::new();
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"mcp_tool_call","server":"s","tool":"t","arguments":null,"result":"","error":"timeout"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        let result = events.iter().find(|e| matches!(e, DisplayEvent::ToolResult { .. }));
        match result.unwrap() {
            DisplayEvent::ToolResult { content, is_error, .. } => {
                assert_eq!(content, "timeout");
                assert!(is_error);
            }
            _ => unreachable!(),
        }
    }

    // ── turn.completed ──

    #[test]
    fn parse_turn_completed() {
        let mut p = CodexEventParser::new();
        let line = r#"{"type":"turn.completed","usage":{"input_tokens":32607,"cached_input_tokens":32384,"output_tokens":87}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Complete { success, cost_usd, .. } => {
                assert!(success);
                assert!(*cost_usd > 0.0);
            }
            _ => panic!("expected Complete"),
        }
    }

    #[test]
    fn parse_turn_completed_no_usage() {
        let mut p = CodexEventParser::new();
        let line = r#"{"type":"turn.completed"}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Complete { cost_usd, .. } => assert_eq!(*cost_usd, 0.0),
            _ => panic!("expected Complete"),
        }
    }

    // ── error ──

    #[test]
    fn parse_error_event() {
        let mut p = CodexEventParser::new();
        let line = r#"{"type":"error","message":"Model not supported"}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::AssistantText { text, .. } => assert!(text.contains("Model not supported")),
            _ => panic!("expected AssistantText"),
        }
    }

    // ── turn.failed ──

    #[test]
    fn parse_turn_failed() {
        let mut p = CodexEventParser::new();
        let line = r#"{"type":"turn.failed","error":{"message":"API error"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], DisplayEvent::Complete { success: false, .. }));
        match &events[1] {
            DisplayEvent::AssistantText { text, .. } => assert!(text.contains("API error")),
            _ => panic!("expected AssistantText"),
        }
    }

    // ── Invalid / edge cases ──

    #[test]
    fn parse_invalid_json() {
        let mut p = CodexEventParser::new();
        let (events, _) = p.parse("not json\n");
        assert!(events.is_empty());
    }

    #[test]
    fn parse_json_without_type() {
        let mut p = CodexEventParser::new();
        let (events, json) = p.parse("{\"foo\":\"bar\"}\n");
        assert!(events.is_empty());
        assert!(json.is_some());
    }

    #[test]
    fn parse_unknown_event_type() {
        let mut p = CodexEventParser::new();
        let (events, _) = p.parse("{\"type\":\"future.event\"}\n");
        assert!(events.is_empty());
    }

    #[test]
    fn parse_empty_input() {
        let mut p = CodexEventParser::new();
        let (events, json) = p.parse("");
        assert!(events.is_empty());
        assert!(json.is_none());
    }

    #[test]
    fn parse_multiple_lines() {
        let mut p = CodexEventParser::new();
        let input = concat!(
            "{\"type\":\"thread.started\",\"thread_id\":\"t1\"}\n",
            "{\"type\":\"turn.started\"}\n",
            "{\"type\":\"item.completed\",\"item\":{\"id\":\"i0\",\"type\":\"agent_message\",\"text\":\"hi\"}}\n",
            "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":100,\"output_tokens\":10}}\n"
        );
        let (events, _) = p.parse(input);
        assert_eq!(events.len(), 3); // Init + AssistantText + Complete
    }

    #[test]
    fn parse_partial_then_complete() {
        let mut p = CodexEventParser::new();
        // Feed partial line
        let (events, _) = p.parse("{\"type\":\"thread.");
        assert!(events.is_empty());
        // Complete it
        let (events, _) = p.parse("started\",\"thread_id\":\"x\"}\n");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], DisplayEvent::Init { .. }));
    }

    // ── Full session simulation ──

    #[test]
    fn parse_full_codex_session() {
        let mut p = CodexEventParser::new();

        // thread.started
        let (e, _) = p.parse("{\"type\":\"thread.started\",\"thread_id\":\"uuid-1\"}\n");
        assert_eq!(e.len(), 1);

        // turn.started
        let (e, _) = p.parse("{\"type\":\"turn.started\"}\n");
        assert!(e.is_empty());

        // reasoning
        let (e, _) = p.parse("{\"type\":\"item.completed\",\"item\":{\"id\":\"i0\",\"type\":\"reasoning\",\"text\":\"Planning...\"}}\n");
        assert_eq!(e.len(), 1);

        // command started
        let (e, _) = p.parse("{\"type\":\"item.started\",\"item\":{\"id\":\"i1\",\"type\":\"command_execution\",\"command\":\"ls\",\"aggregated_output\":\"\",\"exit_code\":null,\"status\":\"in_progress\"}}\n");
        assert_eq!(e.len(), 1);

        // command completed
        let (e, _) = p.parse("{\"type\":\"item.completed\",\"item\":{\"id\":\"i1\",\"type\":\"command_execution\",\"command\":\"ls\",\"aggregated_output\":\"file.rs\\n\",\"exit_code\":0,\"status\":\"completed\"}}\n");
        assert_eq!(e.len(), 1);

        // file change
        let (e, _) = p.parse("{\"type\":\"item.completed\",\"item\":{\"id\":\"i2\",\"type\":\"file_change\",\"changes\":[{\"path\":\"src/main.rs\",\"kind\":\"update\"}],\"status\":\"completed\"}}\n");
        assert_eq!(e.len(), 2);

        // agent message
        let (e, _) = p.parse("{\"type\":\"item.completed\",\"item\":{\"id\":\"i3\",\"type\":\"agent_message\",\"text\":\"Done!\"}}\n");
        assert_eq!(e.len(), 1);

        // turn completed
        let (e, _) = p.parse("{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":1000,\"output_tokens\":50}}\n");
        assert_eq!(e.len(), 1);
    }

    // ── Cost estimation ──

    #[test]
    fn estimate_cost_zero() {
        assert_eq!(estimate_codex_cost(0, 0), 0.0);
    }

    #[test]
    fn estimate_cost_nonzero() {
        let cost = estimate_codex_cost(1_000_000, 1_000_000);
        assert!((cost - 40.0).abs() < 0.01); // $10 input + $30 output
    }

    #[test]
    fn estimate_cost_typical() {
        let cost = estimate_codex_cost(32607, 87);
        assert!(cost > 0.0);
        assert!(cost < 1.0);
    }

    // ── item.completed with unknown item type ──

    #[test]
    fn parse_unknown_item_type() {
        let mut p = CodexEventParser::new();
        let line = r#"{"type":"item.completed","item":{"id":"i99","type":"future_tool","data":"x"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert!(events.is_empty());
    }

    // ── item.started with no item field ──

    #[test]
    fn parse_item_started_no_item() {
        let mut p = CodexEventParser::new();
        let (events, _) = p.parse("{\"type\":\"item.started\"}\n");
        assert!(events.is_empty());
    }

    // ── item.completed with no item field ──

    #[test]
    fn parse_item_completed_no_item() {
        let mut p = CodexEventParser::new();
        let (events, _) = p.parse("{\"type\":\"item.completed\"}\n");
        assert!(events.is_empty());
    }

    // ── Buffer handling ──

    #[test]
    fn parse_preserves_buffer_across_calls() {
        let mut p = CodexEventParser::new();
        p.parse("{\"type\":\"thread.star");
        p.parse("ted\",\"thread_id\":\"x\"}\n");
        // Buffer should be empty after consuming complete line
        assert!(p.buffer.is_empty());
    }

    #[test]
    fn parse_handles_multiple_newlines() {
        let mut p = CodexEventParser::new();
        let (events, _) = p.parse("\n\n{\"type\":\"turn.started\"}\n\n");
        assert!(events.is_empty()); // turn.started produces no events
    }
}
