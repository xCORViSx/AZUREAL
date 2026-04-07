//! Event parser for Codex CLI `--json` JSONL output
//!
//! Parses Codex stdout JSONL events into DisplayEvents.
//! Supports both the legacy streaming schema (`thread.started`, `item.*`) and
//! the newer OpenAI-style schema (`session_meta`, `response_item`, `event_msg`).

use super::display::DisplayEvent;
use std::collections::HashMap;

/// Parser for Codex CLI JSONL streaming events
pub struct CodexEventParser {
    buffer: String,
    /// Track items by ID for matching started → completed
    items: HashMap<String, String>,
    /// Track tool calls so later outputs keep the original display metadata.
    tool_calls: HashMap<String, (String, Option<String>)>,
    /// Model ID to embed in Init events (e.g. "gpt-5.4")
    model: String,
    /// Prevent duplicate Init events when both legacy and modern schemas appear.
    init_emitted: bool,
}

impl CodexEventParser {
    pub fn new(model: String) -> Self {
        Self {
            buffer: String::new(),
            items: HashMap::new(),
            tool_calls: HashMap::new(),
            model,
            init_emitted: false,
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
            if trimmed.is_empty() {
                continue;
            }
            let (line_events, json) = self.parse_line(trimmed);
            events.extend(line_events);
            if json.is_some() {
                last_json = json;
            }
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
            "session_meta" => self.parse_session_meta(&json),
            "thread.started" => self.parse_thread_started(&json),
            "turn.started" => Vec::new(),
            "turn_context" => self.parse_turn_context(&json),
            "response_item" => self.parse_response_item(&json),
            "event_msg" => self.parse_event_msg(&json),
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

    fn emit_init(&mut self, session_id: String, cwd: String) -> Vec<DisplayEvent> {
        if self.init_emitted {
            return Vec::new();
        }
        self.init_emitted = true;
        vec![DisplayEvent::Init {
            _session_id: session_id,
            cwd,
            model: self.model.clone(),
        }]
    }

    fn parse_session_meta(&mut self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        let payload = match json.get("payload") {
            Some(p) => p,
            None => return Vec::new(),
        };
        let session_id = payload
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let cwd = payload
            .get("cwd")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        self.emit_init(session_id, cwd)
    }

    fn parse_thread_started(&mut self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        let thread_id = json
            .get("thread_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        self.emit_init(thread_id, String::new())
    }

    fn parse_turn_context(&mut self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        if let Some(model) = json
            .get("payload")
            .and_then(|p| p.get("model"))
            .and_then(|v| v.as_str())
            .filter(|m| !m.is_empty())
        {
            self.model = model.to_string();
        }
        Vec::new()
    }

    fn parse_response_item(&mut self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        let payload = match json.get("payload") {
            Some(p) => p,
            None => return Vec::new(),
        };
        let item_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match item_type {
            "message" => self.parse_response_message(payload),
            "function_call" | "shell_command" => {
                self.parse_response_tool_call(payload, "shell_command")
            }
            "custom_tool_call" => self.parse_response_tool_call(payload, "custom_tool"),
            "function_call_output" => self.parse_response_tool_output(payload, "unknown"),
            "custom_tool_call_output" => self.parse_response_tool_output(payload, "custom_tool"),
            "reasoning" => self.parse_response_reasoning(payload),
            _ => Vec::new(),
        }
    }

    fn parse_response_message(&self, payload: &serde_json::Value) -> Vec<DisplayEvent> {
        let role = payload.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let text = extract_message_text(payload.get("content"));
        if text.is_empty() {
            return Vec::new();
        }

        match role {
            "user" | "developer" => vec![DisplayEvent::UserMessage {
                _uuid: String::new(),
                content: text,
            }],
            "assistant" => vec![DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text,
            }],
            _ => Vec::new(),
        }
    }

    fn parse_response_tool_call(
        &mut self,
        payload: &serde_json::Value,
        default_name: &str,
    ) -> Vec<DisplayEvent> {
        let call_id = payload
            .get("call_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let name = payload
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(default_name);
        let (tool_name, file_path) = map_codex_tool(name, payload);
        let input = build_tool_input(name, payload);

        self.tool_calls
            .insert(call_id.clone(), (tool_name.clone(), file_path.clone()));

        vec![DisplayEvent::ToolCall {
            _uuid: String::new(),
            tool_use_id: call_id,
            tool_name,
            file_path,
            input,
        }]
    }

    fn parse_response_tool_output(
        &mut self,
        payload: &serde_json::Value,
        fallback_tool_name: &str,
    ) -> Vec<DisplayEvent> {
        let call_id = payload
            .get("call_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let raw_output = payload
            .get("output")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let (tool_name, file_path) = self
            .tool_calls
            .remove(&call_id)
            .unwrap_or((fallback_tool_name.to_string(), None));
        let output = normalize_tool_output(&tool_name, raw_output);
        let is_error = output.starts_with("Error")
            || output.starts_with("write_stdin failed:")
            || output.starts_with("exec_command failed:")
            || (output.starts_with("Exit code: ") && !output.starts_with("Exit code: 0"));

        vec![DisplayEvent::ToolResult {
            tool_use_id: call_id,
            tool_name,
            file_path,
            content: output,
            is_error,
        }]
    }

    fn parse_response_reasoning(&self, payload: &serde_json::Value) -> Vec<DisplayEvent> {
        let mut events = Vec::new();
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
        events
    }

    fn parse_event_msg(&mut self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        let payload = match json.get("payload") {
            Some(p) => p,
            None => return Vec::new(),
        };
        let msg_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match msg_type {
            "user_message" => {
                let text = payload
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if text.is_empty() {
                    Vec::new()
                } else {
                    vec![DisplayEvent::UserMessage {
                        _uuid: String::new(),
                        content: text,
                    }]
                }
            }
            "agent_message" => {
                let text = payload
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
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
            "agent_reasoning" => {
                let text = payload
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
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
            "task_complete" => vec![DisplayEvent::Complete {
                _session_id: String::new(),
                success: true,
                duration_ms: 0,
                cost_usd: 0.0,
            }],
            "function_call_output" => self.parse_response_tool_output(payload, "Bash"),
            _ => Vec::new(),
        }
    }

    fn parse_item_started(&mut self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        let item = match json.get("item") {
            Some(i) => i,
            None => return Vec::new(),
        };

        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let item_id = item
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        match item_type {
            "command_execution" => {
                let command = item
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
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
        let item_id = item
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        match item_type {
            "reasoning" => {
                let text = item
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
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
                let text = item
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                vec![DisplayEvent::AssistantText {
                    _uuid: String::new(),
                    _message_id: String::new(),
                    text,
                }]
            }
            "command_execution" => {
                let command = item
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let output = item
                    .get("aggregated_output")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
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
                        let path = change
                            .get("path")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let kind = change
                            .get("kind")
                            .and_then(|v| v.as_str())
                            .unwrap_or("update")
                            .to_string();
                        let move_path = change
                            .get("move_path")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                        let unified_diff = change
                            .get("unified_diff")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                            .map(str::to_string);
                        let change_id = format!("{}-{}", item_id, path);
                        let mut input = serde_json::json!({
                            "file_path": path,
                            "kind": kind,
                        });
                        if let Some(ref move_path) = move_path {
                            input["move_path"] = serde_json::Value::String(move_path.clone());
                        }
                        if let Some(ref unified_diff) = unified_diff {
                            input["unified_diff"] = serde_json::Value::String(unified_diff.clone());
                        }
                        let result_content = match move_path {
                            Some(ref dst) => format!("File {}: {} -> {}", kind, path, dst),
                            None => format!("File {}: {}", kind, path),
                        };

                        events.push(DisplayEvent::ToolCall {
                            _uuid: String::new(),
                            tool_use_id: change_id.clone(),
                            tool_name: "Edit".to_string(),
                            file_path: Some(path.clone()),
                            input,
                        });
                        events.push(DisplayEvent::ToolResult {
                            tool_use_id: change_id,
                            tool_name: "Edit".to_string(),
                            file_path: Some(path.clone()),
                            content: result_content,
                            is_error: false,
                        });
                    }
                }
                events
            }
            "mcp_tool_call" => {
                let server = item.get("server").and_then(|v| v.as_str()).unwrap_or("mcp");
                let tool = item
                    .get("tool")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let tool_name = format!("{}:{}", server, tool);
                let result = item
                    .get("result")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let error = item.get("error").and_then(|v| v.as_str());

                vec![
                    DisplayEvent::ToolCall {
                        _uuid: String::new(),
                        tool_use_id: item_id.clone(),
                        tool_name: tool_name.clone(),
                        file_path: None,
                        input: item
                            .get("arguments")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null),
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
        let input_tokens = usage
            .and_then(|u| u.get("input_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output_tokens = usage
            .and_then(|u| u.get("output_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        vec![DisplayEvent::Complete {
            _session_id: String::new(),
            success: true,
            duration_ms: 0,
            cost_usd: estimate_codex_cost(input_tokens, output_tokens),
        }]
    }

    fn parse_error(&self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        let message = json
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error")
            .to_string();
        vec![DisplayEvent::AssistantText {
            _uuid: String::new(),
            _message_id: String::new(),
            text: format!("Error: {}", message),
        }]
    }

    fn parse_turn_failed(&self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        let message = json
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("Turn failed");

        vec![
            DisplayEvent::Complete {
                _session_id: String::new(),
                success: false,
                duration_ms: 0,
                cost_usd: 0.0,
            },
            DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text: format!("Error: {}", message),
            },
        ]
    }
}

/// Rough cost estimate for Codex models (o3-level pricing)
fn estimate_codex_cost(input_tokens: u64, output_tokens: u64) -> f64 {
    // Approximate: $10/M input, $30/M output (o3 pricing)
    (input_tokens as f64 * 10.0 + output_tokens as f64 * 30.0) / 1_000_000.0
}

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

fn map_codex_tool(name: &str, payload: &serde_json::Value) -> (String, Option<String>) {
    match name {
        "shell_command" => {
            let args = parse_tool_args(payload);
            let workdir = args
                .as_object()
                .and_then(|a| a.get("workdir"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            ("Bash".to_string(), workdir)
        }
        "exec_command" | "write_stdin" => ("Bash".to_string(), None),
        "apply_patch" => {
            let args = payload
                .get("arguments")
                .and_then(|v| v.as_str())
                .or_else(|| payload.get("input").and_then(|v| v.as_str()));
            let file_path = args.and_then(extract_patch_file_path);
            ("Edit".to_string(), file_path)
        }
        _ => (name.to_string(), None),
    }
}

fn build_tool_input(name: &str, payload: &serde_json::Value) -> serde_json::Value {
    match name {
        "shell_command" => parse_tool_args(payload),
        "exec_command" => normalize_exec_command_input(parse_tool_args(payload)),
        "write_stdin" => normalize_write_stdin_input(parse_tool_args(payload)),
        "apply_patch" => {
            let patch = payload
                .get("arguments")
                .and_then(|v| v.as_str())
                .or_else(|| payload.get("input").and_then(|v| v.as_str()))
                .unwrap_or("");
            serde_json::json!({ "patch": patch })
        }
        _ => parse_tool_args(payload),
    }
}

fn extract_patch_file_path(patch: &str) -> Option<String> {
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("*** Update File: ") {
            return Some(rest.trim().to_string());
        }
        if let Some(rest) = line.strip_prefix("*** Add File: ") {
            return Some(rest.trim().to_string());
        }
        if let Some(rest) = line.strip_prefix("*** Delete File: ") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

fn parse_tool_args(payload: &serde_json::Value) -> serde_json::Value {
    let args_str = payload
        .get("arguments")
        .and_then(|v| v.as_str())
        .or_else(|| payload.get("input").and_then(|v| v.as_str()))
        .unwrap_or("{}");
    serde_json::from_str(args_str).unwrap_or(serde_json::json!({}))
}

fn normalize_exec_command_input(mut args: serde_json::Value) -> serde_json::Value {
    let command = args
        .get("command")
        .or_else(|| args.get("cmd"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    insert_command_field(&mut args, command);
    args
}

fn normalize_write_stdin_input(mut args: serde_json::Value) -> serde_json::Value {
    let command = describe_write_stdin_action(&args);
    insert_command_field(&mut args, command);
    args
}

fn insert_command_field(args: &mut serde_json::Value, command: String) {
    match args {
        serde_json::Value::Object(map) => {
            map.insert("command".into(), serde_json::json!(command));
        }
        _ => {
            *args = serde_json::json!({ "command": command });
        }
    }
}

fn describe_write_stdin_action(args: &serde_json::Value) -> String {
    let session_suffix = args
        .get("session_id")
        .map(|v| match v {
            serde_json::Value::String(s) => format!(" {}", s),
            serde_json::Value::Number(n) => format!(" {}", n),
            _ => String::new(),
        })
        .unwrap_or_default();
    let chars = args.get("chars").and_then(|v| v.as_str()).unwrap_or("");
    if chars.is_empty() {
        return format!("poll session{session_suffix}");
    }
    if chars == "\u{3}" {
        return format!("send Ctrl-C to session{session_suffix}");
    }
    let escaped = chars.escape_default().to_string();
    let preview = if escaped.chars().count() > 32 {
        format!("{}...", escaped.chars().take(29).collect::<String>())
    } else {
        escaped
    };
    format!("send \"{preview}\" to session{session_suffix}")
}

fn normalize_tool_output(tool_name: &str, output: String) -> String {
    let output = unwrap_tool_output_envelope(&output).unwrap_or(output);

    if !matches!(tool_name, "Bash" | "bash") || !output.starts_with("Chunk ID:") {
        return output;
    }

    if let Some((_, tail)) = output.split_once("\nOutput:\n") {
        let actual = tail.trim_end_matches('\n');
        if !actual.trim().is_empty() {
            return actual.to_string();
        }
    }

    if let Some(code) = output
        .lines()
        .find_map(|line| line.strip_prefix("Process exited with code "))
    {
        let code = code.trim();
        return if code == "0" {
            String::new()
        } else {
            format!("Exit code: {code}")
        };
    }

    if let Some(session_id) = output
        .lines()
        .find_map(|line| line.strip_prefix("Process running with session ID "))
    {
        return format!("Process running with session ID {}", session_id.trim());
    }

    output
}

fn unwrap_tool_output_envelope(output: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(output).ok()?;
    let inner = json.get("output").and_then(|v| v.as_str())?;
    let exit_code = json
        .get("metadata")
        .and_then(|m| m.get("exit_code"))
        .and_then(|v| v.as_i64());

    if inner.trim().is_empty() {
        return exit_code.map(|code| {
            if code == 0 {
                String::new()
            } else {
                format!("Exit code: {code}")
            }
        });
    }

    Some(inner.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── CodexEventParser construction ──

    #[test]
    fn parser_new() {
        let p = CodexEventParser::new("gpt-5.4".to_string());
        assert!(p.buffer.is_empty());
        assert!(p.items.is_empty());
        assert!(p.tool_calls.is_empty());
        assert!(!p.init_emitted);
    }

    // ── thread.started ──

    #[test]
    fn parse_thread_started() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let (events, json) = p.parse(r#"{"type":"thread.started","thread_id":"abc-123"}"#);
        // No newline yet — should be buffered
        assert!(events.is_empty());
        assert!(json.is_none());

        // Feed newline
        let (events, json) = p.parse("\n");
        assert_eq!(events.len(), 1);
        assert!(json.is_some());
        match &events[0] {
            DisplayEvent::Init {
                _session_id, model, ..
            } => {
                assert_eq!(_session_id, "abc-123");
                assert_eq!(model, "gpt-5.4");
            }
            _ => panic!("expected Init"),
        }
    }

    #[test]
    fn parse_thread_started_missing_thread_id() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let (events, _) = p.parse("{\"type\":\"thread.started\"}\n");
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Init { _session_id, .. } => assert!(_session_id.is_empty()),
            _ => panic!("expected Init"),
        }
    }

    #[test]
    fn parse_session_meta_emits_init() {
        let mut p = CodexEventParser::new("gpt-5.1-codex-mini".to_string());
        let line = r#"{"type":"session_meta","payload":{"id":"sess-1","cwd":"/tmp/project"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Init {
                _session_id,
                cwd,
                model,
            } => {
                assert_eq!(_session_id, "sess-1");
                assert_eq!(cwd, "/tmp/project");
                assert_eq!(model, "gpt-5.1-codex-mini");
            }
            _ => panic!("expected Init"),
        }
    }

    // ── turn.started (no-op) ──

    #[test]
    fn parse_turn_started_is_noop() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let (events, _) = p.parse("{\"type\":\"turn.started\"}\n");
        assert!(events.is_empty());
    }

    // ── item.started + item.completed (command_execution) ──

    #[test]
    fn parse_command_execution_started() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line = r#"{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"ls -la","aggregated_output":"","exit_code":null,"status":"in_progress"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
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
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let started = r#"{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"ls","aggregated_output":"","exit_code":null,"status":"in_progress"}}"#;
        let completed = r#"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"ls","aggregated_output":"file1\nfile2\n","exit_code":0,"status":"completed"}}"#;
        let (_, _) = p.parse(&format!("{}\n", started));
        let (events, _) = p.parse(&format!("{}\n", completed));
        // Should only have ToolResult (no duplicate ToolCall)
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolResult {
                tool_name,
                content,
                is_error,
                ..
            } => {
                assert_eq!(tool_name, "Bash");
                assert!(content.contains("file1"));
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn parse_command_execution_completed_without_started() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let completed = r#"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"ls","aggregated_output":"output","exit_code":0,"status":"completed"}}"#;
        let (events, _) = p.parse(&format!("{}\n", completed));
        // Should emit both ToolCall and ToolResult
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], DisplayEvent::ToolCall { .. }));
        assert!(matches!(&events[1], DisplayEvent::ToolResult { .. }));
    }

    #[test]
    fn parse_command_execution_failed() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"false","aggregated_output":"","exit_code":1,"status":"completed"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        let result = events
            .iter()
            .find(|e| matches!(e, DisplayEvent::ToolResult { .. }));
        match result.unwrap() {
            DisplayEvent::ToolResult { is_error, .. } => assert!(is_error),
            _ => unreachable!(),
        }
    }

    #[test]
    fn parse_command_execution_empty_output() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"true","aggregated_output":"","exit_code":0,"status":"completed"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        let result = events
            .iter()
            .find(|e| matches!(e, DisplayEvent::ToolResult { .. }));
        match result.unwrap() {
            DisplayEvent::ToolResult { content, .. } => assert!(content.contains("Exit code: 0")),
            _ => unreachable!(),
        }
    }

    // ── item.completed (reasoning) ──

    #[test]
    fn parse_reasoning() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
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
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line =
            r#"{"type":"item.completed","item":{"id":"item_0","type":"reasoning","text":""}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert!(events.is_empty());
    }

    // ── item.completed (agent_message) ──

    #[test]
    fn parse_agent_message() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
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
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
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

    #[test]
    fn parse_response_item_custom_tool_call_apply_patch_preserves_patch() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line = r#"{"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","call_id":"call_patch","input":"*** Begin Patch\n*** Update File: /tmp/probe.txt\n@@\n-old\n+new\n*** End Patch"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolCall {
                tool_name,
                tool_use_id,
                file_path,
                input,
                ..
            } => {
                assert_eq!(tool_name, "Edit");
                assert_eq!(tool_use_id, "call_patch");
                assert_eq!(file_path.as_deref(), Some("/tmp/probe.txt"));
                assert_eq!(
                    input.get("patch").and_then(|v| v.as_str()),
                    Some(
                        "*** Begin Patch\n*** Update File: /tmp/probe.txt\n@@\n-old\n+new\n*** End Patch"
                    )
                );
            }
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn parse_response_item_custom_tool_call_output_uses_edit_metadata() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let call = r#"{"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","call_id":"call_patch","input":"*** Begin Patch\n*** Update File: /tmp/probe.txt\n@@\n-old\n+new\n*** End Patch"}}"#;
        let output = r#"{"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"call_patch","output":"{\"output\":\"Success. Updated the following files:\\nM /tmp/probe.txt\\n\",\"metadata\":{\"exit_code\":0}}"}}"#;
        let (_, _) = p.parse(&format!("{}\n", call));
        let (events, _) = p.parse(&format!("{}\n", output));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolResult {
                tool_use_id,
                tool_name,
                file_path,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "call_patch");
                assert_eq!(tool_name, "Edit");
                assert_eq!(file_path.as_deref(), Some("/tmp/probe.txt"));
                assert_eq!(
                    content,
                    "Success. Updated the following files:\nM /tmp/probe.txt\n"
                );
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn parse_response_item_exec_command_maps_to_bash_command() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line = r#"{"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"call_exec","arguments":"{\"cmd\":\"pwd\",\"workdir\":\"/tmp\",\"yield_time_ms\":1000}"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolCall {
                tool_name,
                file_path,
                input,
                ..
            } => {
                assert_eq!(tool_name, "Bash");
                assert!(file_path.is_none());
                assert_eq!(input.get("command").and_then(|v| v.as_str()), Some("pwd"));
                assert_eq!(input.get("workdir").and_then(|v| v.as_str()), Some("/tmp"));
            }
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn parse_response_item_write_stdin_maps_to_bash_poll_command() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line = r#"{"type":"response_item","payload":{"type":"function_call","name":"write_stdin","call_id":"call_poll","arguments":"{\"session_id\":98333,\"chars\":\"\",\"yield_time_ms\":1000}"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolCall {
                tool_name, input, ..
            } => {
                assert_eq!(tool_name, "Bash");
                assert_eq!(
                    input.get("command").and_then(|v| v.as_str()),
                    Some("poll session 98333")
                );
            }
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn parse_response_item_exec_command_output_strips_exec_wrapper() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let call = r#"{"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"call_exec","arguments":"{\"cmd\":\"pwd\"}"}}"#;
        let output = r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"call_exec","output":"Chunk ID: 6bf9d8\nWall time: 0.0000 seconds\nProcess exited with code 0\nOriginal token count: 7\nOutput:\n/Users/macbookpro/AZUREAL\n"}}"#;
        let (_, _) = p.parse(&format!("{}\n", call));
        let (events, _) = p.parse(&format!("{}\n", output));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolResult {
                tool_name,
                content,
                is_error,
                ..
            } => {
                assert_eq!(tool_name, "Bash");
                assert_eq!(content, "/Users/macbookpro/AZUREAL");
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }

    // ── item.completed (file_change) ──

    #[test]
    fn parse_file_change_single() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"file_change","changes":[{"path":"src/main.rs","kind":"update"}],"status":"completed"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 2); // ToolCall + ToolResult
        match &events[0] {
            DisplayEvent::ToolCall {
                tool_name,
                file_path,
                ..
            } => {
                assert_eq!(tool_name, "Edit");
                assert_eq!(file_path.as_deref(), Some("src/main.rs"));
            }
            _ => panic!("expected ToolCall"),
        }
        match &events[1] {
            DisplayEvent::ToolResult {
                content, file_path, ..
            } => {
                assert!(content.contains("update"));
                assert_eq!(file_path.as_deref(), Some("src/main.rs"));
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn parse_file_change_multiple() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"file_change","changes":[{"path":"a.rs","kind":"create"},{"path":"b.rs","kind":"update"}],"status":"completed"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 4); // 2 ToolCall + 2 ToolResult
    }

    #[test]
    fn parse_file_change_empty_changes() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"file_change","changes":[],"status":"completed"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert!(events.is_empty());
    }

    #[test]
    fn parse_file_change_preserves_unified_diff() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"file_change","changes":[{"path":"src/main.rs","kind":"update","unified_diff":"diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n"}],"status":"completed"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 2);
        match &events[0] {
            DisplayEvent::ToolCall { input, .. } => {
                assert_eq!(
                    input.get("unified_diff").and_then(|v| v.as_str()),
                    Some(
                        "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n"
                    )
                );
            }
            _ => panic!("expected ToolCall"),
        }
    }

    // ── item.completed (mcp_tool_call) ──

    #[test]
    fn parse_mcp_tool_call() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"mcp_tool_call","server":"docs","tool":"search","arguments":{"query":"help"},"result":"Found docs","error":null}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 2);
        match &events[0] {
            DisplayEvent::ToolCall {
                tool_name, input, ..
            } => {
                assert_eq!(tool_name, "docs:search");
                assert_eq!(input["query"], "help");
            }
            _ => panic!("expected ToolCall"),
        }
        match &events[1] {
            DisplayEvent::ToolResult {
                content, is_error, ..
            } => {
                assert_eq!(content, "Found docs");
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn parse_mcp_tool_call_with_error() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"mcp_tool_call","server":"s","tool":"t","arguments":null,"result":"","error":"timeout"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        let result = events
            .iter()
            .find(|e| matches!(e, DisplayEvent::ToolResult { .. }));
        match result.unwrap() {
            DisplayEvent::ToolResult {
                content, is_error, ..
            } => {
                assert_eq!(content, "timeout");
                assert!(is_error);
            }
            _ => unreachable!(),
        }
    }

    // ── turn.completed ──

    #[test]
    fn parse_turn_completed() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line = r#"{"type":"turn.completed","usage":{"input_tokens":32607,"cached_input_tokens":32384,"output_tokens":87}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::Complete {
                success, cost_usd, ..
            } => {
                assert!(success);
                assert!(*cost_usd > 0.0);
            }
            _ => panic!("expected Complete"),
        }
    }

    #[test]
    fn parse_turn_completed_no_usage() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
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
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line = r#"{"type":"error","message":"Model not supported"}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::AssistantText { text, .. } => {
                assert!(text.contains("Model not supported"))
            }
            _ => panic!("expected AssistantText"),
        }
    }

    // ── turn.failed ──

    #[test]
    fn parse_turn_failed() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line = r#"{"type":"turn.failed","error":{"message":"API error"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            DisplayEvent::Complete { success: false, .. }
        ));
        match &events[1] {
            DisplayEvent::AssistantText { text, .. } => assert!(text.contains("API error")),
            _ => panic!("expected AssistantText"),
        }
    }

    // ── Invalid / edge cases ──

    #[test]
    fn parse_invalid_json() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let (events, _) = p.parse("not json\n");
        assert!(events.is_empty());
    }

    #[test]
    fn parse_json_without_type() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let (events, json) = p.parse("{\"foo\":\"bar\"}\n");
        assert!(events.is_empty());
        assert!(json.is_some());
    }

    #[test]
    fn parse_unknown_event_type() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let (events, _) = p.parse("{\"type\":\"future.event\"}\n");
        assert!(events.is_empty());
    }

    #[test]
    fn parse_empty_input() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let (events, json) = p.parse("");
        assert!(events.is_empty());
        assert!(json.is_none());
    }

    #[test]
    fn parse_multiple_lines() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
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
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
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
        let mut p = CodexEventParser::new("gpt-5.4".to_string());

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
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let line =
            r#"{"type":"item.completed","item":{"id":"i99","type":"future_tool","data":"x"}}"#;
        let (events, _) = p.parse(&format!("{}\n", line));
        assert!(events.is_empty());
    }

    // ── item.started with no item field ──

    #[test]
    fn parse_item_started_no_item() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let (events, _) = p.parse("{\"type\":\"item.started\"}\n");
        assert!(events.is_empty());
    }

    // ── item.completed with no item field ──

    #[test]
    fn parse_item_completed_no_item() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let (events, _) = p.parse("{\"type\":\"item.completed\"}\n");
        assert!(events.is_empty());
    }

    // ── Buffer handling ──

    #[test]
    fn parse_preserves_buffer_across_calls() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        p.parse("{\"type\":\"thread.star");
        p.parse("ted\",\"thread_id\":\"x\"}\n");
        // Buffer should be empty after consuming complete line
        assert!(p.buffer.is_empty());
    }

    #[test]
    fn parse_handles_multiple_newlines() {
        let mut p = CodexEventParser::new("gpt-5.4".to_string());
        let (events, _) = p.parse("\n\n{\"type\":\"turn.started\"}\n\n");
        assert!(events.is_empty()); // turn.started produces no events
    }
}
