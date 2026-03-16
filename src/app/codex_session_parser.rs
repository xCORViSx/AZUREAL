//! Codex session file parsing
//!
//! Parses Codex CLI's JSONL session files into DisplayEvents for the TUI.
//! Codex uses OpenAI API format on disk: session_meta, response_item,
//! event_msg, turn_context. This is different from the streaming stdout
//! format handled by CodexEventParser in src/events/codex_parser.rs.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use crate::app::session_parser::ParsedSession;
use crate::events::DisplayEvent;

/// Parse a Codex session JSONL file into display events (full parse from byte 0)
#[allow(dead_code)]
pub fn parse_codex_session_file(session_file: &Path) -> ParsedSession {
    parse_from(session_file, 0, None)
}

/// Parse a Codex session JSONL file incrementally (only new bytes after start_offset)
pub fn parse_codex_session_file_incremental(
    session_file: &Path,
    start_offset: u64,
    existing_events: &[DisplayEvent],
    existing_pending: &HashSet<String>,
    existing_failed: &HashSet<String>,
) -> ParsedSession {
    if start_offset == 0 {
        return parse_from(session_file, 0, None);
    }

    // Rebuild tool call tracking from existing events
    let prior_state = IncrementalState::from_events(existing_events);

    let mut parsed = parse_from(session_file, start_offset, Some(prior_state));

    // Merge with existing events
    let mut merged_events = existing_events.to_vec();
    merged_events.append(&mut parsed.events);
    parsed.events = merged_events;

    // Merge pending/failed tools
    let mut pending = existing_pending.clone();
    for id in &parsed.pending_tools {
        pending.insert(id.clone());
    }
    // Remove completed tools from pending
    for id in &parsed.failed_tools {
        pending.remove(id);
    }
    let mut failed = existing_failed.clone();
    for id in parsed.failed_tools.drain() {
        failed.insert(id);
    }
    parsed.pending_tools = pending;
    parsed.failed_tools = failed;

    parsed
}

/// Tracks tool call IDs → names for resolving results
struct IncrementalState {
    tool_calls: HashMap<String, (String, Option<String>)>,
}

impl IncrementalState {
    fn from_events(events: &[DisplayEvent]) -> Self {
        let mut tool_calls = HashMap::new();
        for event in events {
            if let DisplayEvent::ToolCall {
                tool_use_id,
                tool_name,
                file_path,
                ..
            } = event
            {
                tool_calls.insert(tool_use_id.clone(), (tool_name.clone(), file_path.clone()));
            }
        }
        Self { tool_calls }
    }
}

/// Core parser: reads Codex JSONL from a byte offset
fn parse_from(
    session_file: &Path,
    start_offset: u64,
    prior_state: Option<IncrementalState>,
) -> ParsedSession {
    let mut events = Vec::new();
    let mut pending_tools: HashSet<String> = HashSet::new();
    let mut failed_tools: HashSet<String> = HashSet::new();
    let mut total_lines: usize = 0;
    let mut parse_errors: usize = 0;
    let mut model: Option<String> = None;
    let mut tool_calls = prior_state.map(|s| s.tool_calls).unwrap_or_default();

    let file = match File::open(session_file) {
        Ok(f) => f,
        Err(_) => return empty_parsed(),
    };
    let mut reader = BufReader::new(file);

    if start_offset > 0 {
        if reader.seek(SeekFrom::Start(start_offset)).is_err() {
            return empty_parsed();
        }
    }

    let mut line = String::new();
    let mut byte_offset = start_offset;

    loop {
        line.clear();
        let bytes_read = match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };

        byte_offset += bytes_read as u64;
        total_lines += 1;

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let json: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => {
                parse_errors += 1;
                continue;
            }
        };

        let event_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let payload = json.get("payload");

        match event_type {
            "session_meta" => {
                if let Some(p) = payload {
                    let session_id = p
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let cwd = p
                        .get("cwd")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    events.push(DisplayEvent::Init {
                        _session_id: session_id,
                        cwd,
                        model: "codex".to_string(),
                    });
                }
            }

            "turn_context" => {
                // Extract model name from turn_context
                if let Some(p) = payload {
                    if let Some(m) = p.get("model").and_then(|v| v.as_str()) {
                        model = Some(m.to_string());
                    }
                }
            }

            "response_item" => {
                if let Some(p) = payload {
                    parse_response_item(
                        p,
                        &mut events,
                        &mut pending_tools,
                        &mut failed_tools,
                        &mut tool_calls,
                    );
                }
            }

            "event_msg" => {
                if let Some(p) = payload {
                    parse_event_msg(p, &mut events);
                }
            }

            _ => {} // Ignore unknown types
        }
    }

    ParsedSession {
        events,
        pending_tools,
        failed_tools,
        total_lines,
        parse_errors,
        assistant_total: 0,
        assistant_no_message: 0,
        assistant_no_content_arr: 0,
        assistant_text_blocks: 0,
        awaiting_plan_approval: false,
        end_offset: byte_offset,
        session_tokens: None,
        context_window: None,
        model,
    }
}

/// Parse a response_item payload
fn parse_response_item(
    payload: &serde_json::Value,
    events: &mut Vec<DisplayEvent>,
    pending_tools: &mut HashSet<String>,
    failed_tools: &mut HashSet<String>,
    tool_calls: &mut HashMap<String, (String, Option<String>)>,
) {
    let item_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match item_type {
        "message" => {
            let role = payload.get("role").and_then(|v| v.as_str()).unwrap_or("");
            let content = payload.get("content");

            match role {
                "user" | "developer" => {
                    // User or developer (system) message
                    let text = extract_message_text(content);
                    if !text.is_empty() {
                        events.push(DisplayEvent::UserMessage {
                            _uuid: String::new(),
                            content: text,
                        });
                    }
                }
                "assistant" => {
                    // Assistant response text
                    let text = extract_message_text(content);
                    if !text.is_empty() {
                        events.push(DisplayEvent::AssistantText {
                            _uuid: String::new(),
                            _message_id: String::new(),
                            text,
                        });
                    }
                }
                _ => {}
            }
        }

        "function_call" | "shell_command" => {
            // Tool call: name + arguments
            let call_id = payload
                .get("call_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = payload
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("shell_command");

            // Map Codex tool names to display names
            let (tool_name, file_path) = map_codex_tool(name, payload);

            let input = build_tool_input(name, payload);

            events.push(DisplayEvent::ToolCall {
                _uuid: String::new(),
                tool_use_id: call_id.clone(),
                tool_name: tool_name.clone(),
                file_path: file_path.clone(),
                input,
            });

            tool_calls.insert(call_id.clone(), (tool_name, file_path));
            pending_tools.insert(call_id);
        }

        "function_call_output" => {
            // Tool result from the same response stream
            let call_id = payload
                .get("call_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let output = payload
                .get("output")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let (tool_name, file_path) = tool_calls
                .get(&call_id)
                .cloned()
                .unwrap_or(("unknown".to_string(), None));

            let is_error = output.starts_with("Error") || output.contains("Exit code: 1");

            events.push(DisplayEvent::ToolResult {
                tool_use_id: call_id.clone(),
                tool_name,
                file_path,
                content: output,
                is_error,
            });

            pending_tools.remove(&call_id);
            if is_error {
                failed_tools.insert(call_id);
            }
        }

        "custom_tool_call" => {
            // MCP or custom tool call
            let call_id = payload
                .get("call_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = payload
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("custom_tool");
            let (tool_name, file_path) = map_codex_tool(name, payload);
            let input = build_tool_input(name, payload);

            events.push(DisplayEvent::ToolCall {
                _uuid: String::new(),
                tool_use_id: call_id.clone(),
                tool_name: tool_name.clone(),
                file_path: file_path.clone(),
                input,
            });

            tool_calls.insert(call_id.clone(), (tool_name, file_path));
            pending_tools.insert(call_id);
        }

        "custom_tool_call_output" => {
            let call_id = payload
                .get("call_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let output = payload
                .get("output")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let (tool_name, file_path) = tool_calls
                .get(&call_id)
                .cloned()
                .unwrap_or(("custom_tool".to_string(), None));

            let is_error = output.starts_with("Error");

            events.push(DisplayEvent::ToolResult {
                tool_use_id: call_id.clone(),
                tool_name,
                file_path,
                content: output,
                is_error,
            });

            pending_tools.remove(&call_id);
            if is_error {
                failed_tools.insert(call_id);
            }
        }

        "reasoning" => {
            // Reasoning content (shown as assistant text)
            if let Some(summary) = payload.get("summary").and_then(|v| v.as_array()) {
                for item in summary {
                    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            events.push(DisplayEvent::AssistantText {
                                _uuid: String::new(),
                                _message_id: String::new(),
                                text: text.to_string(),
                            });
                        }
                    }
                }
            }
        }

        _ => {} // ghost_snapshot, etc. — skip
    }
}

/// Parse an event_msg payload
fn parse_event_msg(payload: &serde_json::Value, events: &mut Vec<DisplayEvent>) {
    let msg_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        "user_message" => {
            // User message from the event stream
            let text = payload
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !text.is_empty() {
                events.push(DisplayEvent::UserMessage {
                    _uuid: String::new(),
                    content: text.to_string(),
                });
            }
        }

        "agent_message" => {
            // Final agent summary message
            let text = payload
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !text.is_empty() {
                events.push(DisplayEvent::AssistantText {
                    _uuid: String::new(),
                    _message_id: String::new(),
                    text: text.to_string(),
                });
            }
        }

        "agent_reasoning" => {
            // Reasoning text
            let text = payload.get("text").and_then(|v| v.as_str()).unwrap_or("");
            if !text.is_empty() {
                events.push(DisplayEvent::AssistantText {
                    _uuid: String::new(),
                    _message_id: String::new(),
                    text: text.to_string(),
                });
            }
        }

        "task_complete" => {
            events.push(DisplayEvent::Complete {
                _session_id: String::new(),
                success: true,
                duration_ms: 0,
                cost_usd: 0.0,
            });
        }

        "task_started" => {
            // Ignore — session start already handled by session_meta
        }

        "function_call_output" => {
            // Same structure as response_item/function_call_output but in event_msg wrapper
            let call_id = payload
                .get("call_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let output = payload.get("output").and_then(|v| v.as_str()).unwrap_or("");
            if !call_id.is_empty() {
                let is_error = output.starts_with("Error") || output.contains("Exit code: 1");
                events.push(DisplayEvent::ToolResult {
                    tool_use_id: call_id.to_string(),
                    tool_name: "shell_command".to_string(),
                    file_path: None,
                    content: output.to_string(),
                    is_error,
                });
            }
        }

        _ => {} // token_count, etc. — skip
    }
}

/// Extract text content from a Codex message content field.
/// Content can be a string or an array of objects with text fields.
fn extract_message_text(content: Option<&serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(arr)) => {
            let mut texts = Vec::new();
            for item in arr {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    texts.push(text);
                }
            }
            texts.join("\n")
        }
        _ => String::new(),
    }
}

/// Map Codex tool names to Azureal display names
fn map_codex_tool(name: &str, payload: &serde_json::Value) -> (String, Option<String>) {
    match name {
        "shell_command" => {
            let args = payload
                .get("arguments")
                .and_then(|v| v.as_str())
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
            let workdir = args
                .as_ref()
                .and_then(|a| a.get("workdir"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            ("Bash".to_string(), workdir)
        }
        "apply_patch" => {
            // Extract file path from patch content if possible
            let args = payload
                .get("arguments")
                .and_then(|v| v.as_str())
                .or_else(|| payload.get("input").and_then(|v| v.as_str()));
            let file_path = args.and_then(|s| {
                // Look for "*** Update File: <path>" or "*** Add File: <path>"
                for line in s.lines() {
                    if let Some(rest) = line.strip_prefix("*** Update File: ") {
                        return Some(rest.trim().to_string());
                    }
                    if let Some(rest) = line.strip_prefix("*** Add File: ") {
                        return Some(rest.trim().to_string());
                    }
                }
                None
            });
            ("Edit".to_string(), file_path)
        }
        _ => (name.to_string(), None),
    }
}

/// Build a serde_json::Value input for tool display
fn build_tool_input(name: &str, payload: &serde_json::Value) -> serde_json::Value {
    match name {
        "shell_command" => {
            let args_str = payload
                .get("arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            serde_json::from_str(args_str).unwrap_or(serde_json::json!({}))
        }
        "apply_patch" => {
            let patch = payload
                .get("arguments")
                .and_then(|v| v.as_str())
                .or_else(|| payload.get("input").and_then(|v| v.as_str()))
                .unwrap_or("");
            serde_json::json!({ "patch": patch })
        }
        _ => {
            // For custom tools, try arguments or input field
            let args_str = payload
                .get("arguments")
                .and_then(|v| v.as_str())
                .or_else(|| payload.get("input").and_then(|v| v.as_str()))
                .unwrap_or("{}");
            serde_json::from_str(args_str).unwrap_or(serde_json::json!({}))
        }
    }
}

/// Empty result for error cases
fn empty_parsed() -> ParsedSession {
    ParsedSession {
        events: Vec::new(),
        pending_tools: HashSet::new(),
        failed_tools: HashSet::new(),
        total_lines: 0,
        parse_errors: 0,
        assistant_total: 0,
        assistant_no_message: 0,
        assistant_no_content_arr: 0,
        assistant_text_blocks: 0,
        awaiting_plan_approval: false,
        end_offset: 0,
        session_tokens: None,
        context_window: None,
        model: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Helper: write JSONL lines to a temp file and parse
    fn parse_lines(lines: &[&str]) -> ParsedSession {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        let mut f = File::create(&path).unwrap();
        for line in lines {
            writeln!(f, "{}", line).unwrap();
        }
        parse_codex_session_file(&path)
    }

    // ── session_meta ──

    #[test]
    fn test_session_meta_produces_init() {
        let result = parse_lines(&[
            r#"{"type":"session_meta","timestamp":"2026-01-01T00:00:00Z","payload":{"id":"abc-123","cwd":"/home/user/project","originator":"codex_cli","cli_version":"0.77.0"}}"#,
        ]);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::Init {
                _session_id,
                cwd,
                model,
            } => {
                assert_eq!(_session_id, "abc-123");
                assert_eq!(cwd, "/home/user/project");
                assert_eq!(model, "codex");
            }
            _ => panic!("Expected Init event"),
        }
    }

    #[test]
    fn test_session_meta_missing_cwd() {
        let result = parse_lines(&[
            r#"{"type":"session_meta","timestamp":"2026-01-01T00:00:00Z","payload":{"id":"abc"}}"#,
        ]);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::Init { cwd, .. } => assert_eq!(cwd, ""),
            _ => panic!("Expected Init"),
        }
    }

    // ── response_item/message ──

    #[test]
    fn test_user_message_string_content() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:01Z","payload":{"type":"message","role":"user","content":"hello world"}}"#,
        ]);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::UserMessage { content, .. } => assert_eq!(content, "hello world"),
            _ => panic!("Expected UserMessage"),
        }
    }

    #[test]
    fn test_user_message_array_content() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:01Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"fix the bug"},{"type":"input_text","text":"in main.rs"}]}}"#,
        ]);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::UserMessage { content, .. } => {
                assert_eq!(content, "fix the bug\nin main.rs")
            }
            _ => panic!("Expected UserMessage"),
        }
    }

    #[test]
    fn test_developer_message_treated_as_user() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:01Z","payload":{"type":"message","role":"developer","content":"system instructions"}}"#,
        ]);
        assert_eq!(result.events.len(), 1);
        assert!(matches!(
            &result.events[0],
            DisplayEvent::UserMessage { .. }
        ));
    }

    #[test]
    fn test_assistant_message() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:02Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Here is the fix."}]}}"#,
        ]);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::AssistantText { text, .. } => assert_eq!(text, "Here is the fix."),
            _ => panic!("Expected AssistantText"),
        }
    }

    #[test]
    fn test_empty_message_skipped() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:01Z","payload":{"type":"message","role":"user","content":""}}"#,
        ]);
        assert_eq!(result.events.len(), 0);
    }

    #[test]
    fn test_empty_array_content_skipped() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:01Z","payload":{"type":"message","role":"user","content":[]}}"#,
        ]);
        assert_eq!(result.events.len(), 0);
    }

    // ── function_call / function_call_output ──

    #[test]
    fn test_function_call_shell_command() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:03Z","payload":{"type":"function_call","name":"shell_command","call_id":"call_abc","arguments":"{\"command\":\"ls\",\"workdir\":\"/tmp\"}"}}"#,
        ]);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::ToolCall {
                tool_name,
                file_path,
                input,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "Bash");
                assert_eq!(file_path.as_deref(), Some("/tmp"));
                assert_eq!(tool_use_id, "call_abc");
                assert_eq!(input.get("command").and_then(|v| v.as_str()), Some("ls"));
            }
            _ => panic!("Expected ToolCall"),
        }
        assert!(result.pending_tools.contains("call_abc"));
    }

    #[test]
    fn test_function_call_output() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:03Z","payload":{"type":"function_call","name":"shell_command","call_id":"call_xyz","arguments":"{\"command\":\"echo hi\"}"}}"#,
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:04Z","payload":{"type":"function_call_output","call_id":"call_xyz","output":"Exit code: 0\nhi\n"}}"#,
        ]);
        assert_eq!(result.events.len(), 2);
        assert!(matches!(&result.events[0], DisplayEvent::ToolCall { .. }));
        match &result.events[1] {
            DisplayEvent::ToolResult {
                tool_use_id,
                tool_name,
                content,
                is_error,
                ..
            } => {
                assert_eq!(tool_use_id, "call_xyz");
                assert_eq!(tool_name, "Bash");
                assert!(content.contains("hi"));
                assert!(!is_error);
            }
            _ => panic!("Expected ToolResult"),
        }
        // call_xyz should be resolved (not pending)
        assert!(!result.pending_tools.contains("call_xyz"));
    }

    #[test]
    fn test_function_call_output_error() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:03Z","payload":{"type":"function_call","name":"shell_command","call_id":"call_err","arguments":"{\"command\":\"false\"}"}}"#,
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:04Z","payload":{"type":"function_call_output","call_id":"call_err","output":"Exit code: 1\ncommand failed"}}"#,
        ]);
        match &result.events[1] {
            DisplayEvent::ToolResult { is_error, .. } => assert!(is_error),
            _ => panic!("Expected ToolResult"),
        }
        assert!(result.failed_tools.contains("call_err"));
    }

    // ── apply_patch ──

    #[test]
    fn test_apply_patch_maps_to_edit() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:05Z","payload":{"type":"function_call","name":"apply_patch","call_id":"call_patch","arguments":"*** Begin Patch\n*** Update File: /src/main.rs\n@@\n-old\n+new"}}"#,
        ]);
        match &result.events[0] {
            DisplayEvent::ToolCall {
                tool_name,
                file_path,
                ..
            } => {
                assert_eq!(tool_name, "Edit");
                assert_eq!(file_path.as_deref(), Some("/src/main.rs"));
            }
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn test_apply_patch_add_file() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:05Z","payload":{"type":"function_call","name":"apply_patch","call_id":"call_add","arguments":"*** Begin Patch\n*** Add File: /src/new.rs\n+content"}}"#,
        ]);
        match &result.events[0] {
            DisplayEvent::ToolCall {
                tool_name,
                file_path,
                ..
            } => {
                assert_eq!(tool_name, "Edit");
                assert_eq!(file_path.as_deref(), Some("/src/new.rs"));
            }
            _ => panic!("Expected ToolCall"),
        }
    }

    // ── custom_tool_call ──

    #[test]
    fn test_custom_tool_call() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:06Z","payload":{"type":"custom_tool_call","name":"my_mcp_tool","call_id":"call_mcp","input":"some input"}}"#,
        ]);
        match &result.events[0] {
            DisplayEvent::ToolCall {
                tool_name,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "my_mcp_tool");
                assert_eq!(tool_use_id, "call_mcp");
            }
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn test_custom_tool_call_output() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:06Z","payload":{"type":"custom_tool_call","name":"my_tool","call_id":"call_ct","input":"{}"}}"#,
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:07Z","payload":{"type":"custom_tool_call_output","call_id":"call_ct","output":"result text"}}"#,
        ]);
        assert_eq!(result.events.len(), 2);
        match &result.events[1] {
            DisplayEvent::ToolResult {
                tool_name,
                content,
                is_error,
                ..
            } => {
                assert_eq!(tool_name, "my_tool");
                assert_eq!(content, "result text");
                assert!(!is_error);
            }
            _ => panic!("Expected ToolResult"),
        }
    }

    // ── event_msg types ──

    #[test]
    fn test_event_msg_user_message() {
        let result = parse_lines(&[
            r#"{"type":"event_msg","timestamp":"2026-01-01T00:00:08Z","payload":{"type":"user_message","message":"what does this do?"}}"#,
        ]);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::UserMessage { content, .. } => assert_eq!(content, "what does this do?"),
            _ => panic!("Expected UserMessage"),
        }
    }

    #[test]
    fn test_event_msg_agent_message() {
        let result = parse_lines(&[
            r#"{"type":"event_msg","timestamp":"2026-01-01T00:00:09Z","payload":{"type":"agent_message","message":"I'll fix that for you."}}"#,
        ]);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::AssistantText { text, .. } => assert_eq!(text, "I'll fix that for you."),
            _ => panic!("Expected AssistantText"),
        }
    }

    #[test]
    fn test_event_msg_agent_reasoning() {
        let result = parse_lines(&[
            r#"{"type":"event_msg","timestamp":"2026-01-01T00:00:10Z","payload":{"type":"agent_reasoning","text":"I need to read the file first."}}"#,
        ]);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::AssistantText { text, .. } => {
                assert_eq!(text, "I need to read the file first.")
            }
            _ => panic!("Expected AssistantText"),
        }
    }

    #[test]
    fn test_event_msg_task_complete() {
        let result = parse_lines(&[
            r#"{"type":"event_msg","timestamp":"2026-01-01T00:00:11Z","payload":{"type":"task_complete","turn_id":"turn_abc","last_agent_message":null}}"#,
        ]);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::Complete { success, .. } => assert!(success),
            _ => panic!("Expected Complete"),
        }
    }

    #[test]
    fn test_event_msg_empty_message_skipped() {
        let result = parse_lines(&[
            r#"{"type":"event_msg","timestamp":"2026-01-01T00:00:08Z","payload":{"type":"user_message","message":""}}"#,
        ]);
        assert_eq!(result.events.len(), 0);
    }

    #[test]
    fn test_event_msg_token_count_ignored() {
        let result = parse_lines(&[
            r#"{"type":"event_msg","timestamp":"2026-01-01T00:00:12Z","payload":{"type":"token_count","info":{"tokens":500}}}"#,
        ]);
        assert_eq!(result.events.len(), 0);
    }

    // ── turn_context ──

    #[test]
    fn test_turn_context_extracts_model() {
        let result = parse_lines(&[
            r#"{"type":"turn_context","timestamp":"2026-01-01T00:00:13Z","payload":{"model":"gpt-5.2-codex","cwd":"/tmp"}}"#,
        ]);
        assert_eq!(result.model.as_deref(), Some("gpt-5.2-codex"));
    }

    #[test]
    fn test_turn_context_model_updates() {
        let result = parse_lines(&[
            r#"{"type":"turn_context","timestamp":"2026-01-01T00:00:13Z","payload":{"model":"gpt-5.4"}}"#,
            r#"{"type":"turn_context","timestamp":"2026-01-01T00:00:14Z","payload":{"model":"gpt-5.3-codex"}}"#,
        ]);
        // Last model wins
        assert_eq!(result.model.as_deref(), Some("gpt-5.3-codex"));
    }

    // ── reasoning response_item ──

    #[test]
    fn test_reasoning_with_summary() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:15Z","payload":{"type":"reasoning","summary":[{"type":"text","text":"Thinking about the approach..."}],"content":"encrypted"}}"#,
        ]);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::AssistantText { text, .. } => {
                assert_eq!(text, "Thinking about the approach...")
            }
            _ => panic!("Expected AssistantText"),
        }
    }

    #[test]
    fn test_reasoning_empty_summary() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:15Z","payload":{"type":"reasoning","summary":[],"content":"encrypted"}}"#,
        ]);
        assert_eq!(result.events.len(), 0);
    }

    #[test]
    fn test_reasoning_no_summary() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:15Z","payload":{"type":"reasoning","content":"encrypted"}}"#,
        ]);
        assert_eq!(result.events.len(), 0);
    }

    // ── ghost_snapshot (should be ignored) ──

    #[test]
    fn test_ghost_snapshot_ignored() {
        let result = parse_lines(&[
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:16Z","payload":{"type":"ghost_snapshot","ghost_commit":"abc123"}}"#,
        ]);
        assert_eq!(result.events.len(), 0);
    }

    // ── Error handling ──

    #[test]
    fn test_invalid_json_counted_as_error() {
        let result = parse_lines(&[
            "not valid json",
            r#"{"type":"session_meta","timestamp":"2026-01-01T00:00:00Z","payload":{"id":"abc","cwd":"/tmp"}}"#,
        ]);
        assert_eq!(result.parse_errors, 1);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.total_lines, 2);
    }

    #[test]
    fn test_empty_lines_skipped() {
        let result = parse_lines(&[
            "",
            r#"{"type":"session_meta","timestamp":"2026-01-01T00:00:00Z","payload":{"id":"abc","cwd":"/tmp"}}"#,
            "",
        ]);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.parse_errors, 0);
    }

    #[test]
    fn test_nonexistent_file() {
        let result = parse_codex_session_file(Path::new("/nonexistent/path.jsonl"));
        assert_eq!(result.events.len(), 0);
        assert_eq!(result.end_offset, 0);
    }

    // ── Full session simulation ──

    #[test]
    fn test_full_codex_session() {
        let result = parse_lines(&[
            r#"{"type":"session_meta","timestamp":"2026-01-01T00:00:00Z","payload":{"id":"sess-1","cwd":"/home/user/project","originator":"codex_cli","cli_version":"0.77.0"}}"#,
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:01Z","payload":{"type":"message","role":"user","content":"fix the tests"}}"#,
            r#"{"type":"turn_context","timestamp":"2026-01-01T00:00:02Z","payload":{"model":"gpt-5.4","cwd":"/home/user/project"}}"#,
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:03Z","payload":{"type":"function_call","name":"shell_command","call_id":"call_1","arguments":"{\"command\":\"cargo test\",\"workdir\":\"/home/user/project\"}"}}"#,
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:04Z","payload":{"type":"function_call_output","call_id":"call_1","output":"Exit code: 0\ntest result: ok. 5 passed"}}"#,
            r#"{"type":"response_item","timestamp":"2026-01-01T00:00:05Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"All tests pass."}]}}"#,
            r#"{"type":"event_msg","timestamp":"2026-01-01T00:00:06Z","payload":{"type":"task_complete","turn_id":"turn_1"}}"#,
        ]);

        assert_eq!(result.events.len(), 6); // Init, UserMsg, ToolCall, ToolResult, AssistantText, Complete
        assert!(matches!(&result.events[0], DisplayEvent::Init { .. }));
        assert!(matches!(
            &result.events[1],
            DisplayEvent::UserMessage { .. }
        ));
        assert!(matches!(&result.events[2], DisplayEvent::ToolCall { .. }));
        assert!(matches!(&result.events[3], DisplayEvent::ToolResult { .. }));
        assert!(matches!(
            &result.events[4],
            DisplayEvent::AssistantText { .. }
        ));
        assert!(matches!(&result.events[5], DisplayEvent::Complete { .. }));

        assert_eq!(result.model.as_deref(), Some("gpt-5.4"));
        assert_eq!(result.parse_errors, 0);
        assert_eq!(result.total_lines, 7);
    }

    // ── Incremental parsing ──

    #[test]
    fn test_incremental_parse_appends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");

        // Write initial lines
        {
            let mut f = File::create(&path).unwrap();
            writeln!(f, r#"{{"type":"session_meta","timestamp":"2026-01-01T00:00:00Z","payload":{{"id":"s1","cwd":"/tmp"}}}}"#).unwrap();
            writeln!(f, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"message","role":"user","content":"hello"}}}}"#).unwrap();
        }

        // Full parse
        let first = parse_codex_session_file(&path);
        assert_eq!(first.events.len(), 2);
        let offset = first.end_offset;

        // Append more lines
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            writeln!(f, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"hi back"}}]}}}}"#).unwrap();
        }

        // Incremental parse
        let second = parse_codex_session_file_incremental(
            &path,
            offset,
            &first.events,
            &first.pending_tools,
            &first.failed_tools,
        );
        assert_eq!(second.events.len(), 3); // 2 existing + 1 new
        assert!(
            matches!(&second.events[2], DisplayEvent::AssistantText { text, .. } if text == "hi back")
        );
    }

    #[test]
    fn test_incremental_parse_zero_offset_is_full() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        {
            let mut f = File::create(&path).unwrap();
            writeln!(f, r#"{{"type":"session_meta","timestamp":"2026-01-01T00:00:00Z","payload":{{"id":"s1","cwd":"/tmp"}}}}"#).unwrap();
        }

        let result =
            parse_codex_session_file_incremental(&path, 0, &[], &HashSet::new(), &HashSet::new());
        assert_eq!(result.events.len(), 1);
    }

    #[test]
    fn test_incremental_tool_call_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");

        // First batch: tool call
        {
            let mut f = File::create(&path).unwrap();
            writeln!(f, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:03Z","payload":{{"type":"function_call","name":"shell_command","call_id":"call_inc","arguments":"{{\"command\":\"echo hi\"}}" }}}}"#).unwrap();
        }

        let first = parse_codex_session_file(&path);
        assert_eq!(first.events.len(), 1);
        assert!(first.pending_tools.contains("call_inc"));
        let offset = first.end_offset;

        // Second batch: tool result
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            writeln!(f, r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:04Z","payload":{{"type":"function_call_output","call_id":"call_inc","output":"hi"}}}}"#).unwrap();
        }

        let second = parse_codex_session_file_incremental(
            &path,
            offset,
            &first.events,
            &first.pending_tools,
            &first.failed_tools,
        );
        assert_eq!(second.events.len(), 2);
        // Tool call should be resolved (tool_name from first parse carried over)
        match &second.events[1] {
            DisplayEvent::ToolResult { tool_name, .. } => assert_eq!(tool_name, "Bash"),
            _ => panic!("Expected ToolResult"),
        }
    }

    // ── end_offset tracking ──

    #[test]
    fn test_end_offset_tracks_bytes() {
        let result = parse_lines(&[
            r#"{"type":"session_meta","timestamp":"2026-01-01T00:00:00Z","payload":{"id":"s1","cwd":"/tmp"}}"#,
        ]);
        assert!(result.end_offset > 0);
    }

    // ── extract_message_text ──

    #[test]
    fn test_extract_text_from_null() {
        assert_eq!(extract_message_text(None), "");
    }

    #[test]
    fn test_extract_text_from_string() {
        let val = serde_json::json!("hello");
        assert_eq!(extract_message_text(Some(&val)), "hello");
    }

    #[test]
    fn test_extract_text_from_array() {
        let val = serde_json::json!([{"text": "a"}, {"text": "b"}]);
        assert_eq!(extract_message_text(Some(&val)), "a\nb");
    }

    #[test]
    fn test_extract_text_from_mixed_array() {
        let val = serde_json::json!([{"type": "image"}, {"text": "only text"}]);
        assert_eq!(extract_message_text(Some(&val)), "only text");
    }

    // ── map_codex_tool ──

    #[test]
    fn test_map_shell_command() {
        let payload = serde_json::json!({"arguments": "{\"command\":\"ls\",\"workdir\":\"/tmp\"}"});
        let (name, path) = map_codex_tool("shell_command", &payload);
        assert_eq!(name, "Bash");
        assert_eq!(path.as_deref(), Some("/tmp"));
    }

    #[test]
    fn test_map_apply_patch() {
        let payload = serde_json::json!({"arguments": "*** Begin Patch\n*** Update File: /src/lib.rs\n@@\n-old\n+new"});
        let (name, path) = map_codex_tool("apply_patch", &payload);
        assert_eq!(name, "Edit");
        assert_eq!(path.as_deref(), Some("/src/lib.rs"));
    }

    #[test]
    fn test_map_unknown_tool() {
        let payload = serde_json::json!({});
        let (name, path) = map_codex_tool("my_custom_tool", &payload);
        assert_eq!(name, "my_custom_tool");
        assert!(path.is_none());
    }

    // ── Real session file integration tests ──

    #[test]
    fn test_parse_real_codex_session() {
        let path = Path::new("/Users/macbookpro/.codex/sessions/2026/03/12/rollout-2026-03-12T22-29-48-019ce53e-414d-7852-a2dd-71d7c376fdc2.jsonl");
        if !path.exists() {
            return;
        }
        let result = parse_codex_session_file(path);
        assert!(
            !result.events.is_empty(),
            "Real session should produce events"
        );
        assert_eq!(
            result.parse_errors, 0,
            "Real session should have no parse errors"
        );
        // First event should be Init
        assert!(matches!(&result.events[0], DisplayEvent::Init { .. }));
    }

    #[test]
    fn test_parse_real_codex_session_with_tools() {
        let path = Path::new("/Users/macbookpro/.codex/sessions/2026/01/01/rollout-2026-01-01T07-56-02-019b79d8-1364-79d0-a63d-9dcbfcf1c21e.jsonl");
        if !path.exists() {
            return;
        }
        let result = parse_codex_session_file(path);
        assert!(!result.events.is_empty());
        assert_eq!(result.parse_errors, 0);
        // Should have tool calls
        let tool_calls = result
            .events
            .iter()
            .filter(|e| matches!(e, DisplayEvent::ToolCall { .. }))
            .count();
        assert!(
            tool_calls > 0,
            "Session with tools should have ToolCall events"
        );
        let tool_results = result
            .events
            .iter()
            .filter(|e| matches!(e, DisplayEvent::ToolResult { .. }))
            .count();
        assert!(
            tool_results > 0,
            "Session with tools should have ToolResult events"
        );
    }

    #[test]
    fn test_real_session_incremental_matches_full() {
        let path = Path::new("/Users/macbookpro/.codex/sessions/2026/01/01/rollout-2026-01-01T07-56-02-019b79d8-1364-79d0-a63d-9dcbfcf1c21e.jsonl");
        if !path.exists() {
            return;
        }
        let full = parse_codex_session_file(path);
        // Re-parse incrementally from 0 should produce same event count
        let incremental =
            parse_codex_session_file_incremental(path, 0, &[], &HashSet::new(), &HashSet::new());
        assert_eq!(full.events.len(), incremental.events.len());
    }
}
