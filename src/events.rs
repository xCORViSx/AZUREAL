//! Claude Code stream-json event types and parser
//!
//! Parses the JSON events emitted by `claude -p --output-format stream-json`
//! into structured Rust types for custom rendering.

use serde::{Deserialize, Serialize};

/// Top-level event from Claude Code stream-json output
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeCodeEvent {
    /// System events (hooks, init, etc.)
    System(SystemEvent),
    /// User message
    User(UserEvent),
    /// Assistant response
    Assistant(AssistantEvent),
    /// Final result
    Result(ResultEvent),
}

/// System events (hooks, initialization)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemEvent {
    pub subtype: String,
    pub session_id: String,
    #[serde(default)]
    pub uuid: String,
    /// For init events
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub model: Option<String>,
    /// For hook events
    #[serde(default)]
    pub hook_name: Option<String>,
    #[serde(default)]
    pub output: Option<String>,
}

/// User message event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserEvent {
    pub message: UserMessage,
    pub session_id: String,
    #[serde(default)]
    pub uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    pub role: String,
    pub content: String,
}

/// Assistant response event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantEvent {
    pub message: AssistantMessage,
    pub session_id: String,
    #[serde(default)]
    pub uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub id: String,
    pub model: String,
    pub role: String,
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

/// Content block in assistant message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text response
    Text { text: String },
    /// Tool use request
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Tool result
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value,
    },
}

/// Token usage info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
}

/// Final result event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultEvent {
    pub subtype: String,
    pub session_id: String,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
    #[serde(default)]
    pub num_turns: Option<u32>,
    #[serde(default)]
    pub uuid: String,
}

/// Parsed and displayable event for the UI
#[derive(Debug, Clone)]
pub enum DisplayEvent {
    /// System initialization
    Init {
        session_id: String,
        cwd: String,
        model: String,
    },
    /// Hook output
    Hook {
        name: String,
        output: String,
    },
    /// User's message
    UserMessage {
        uuid: String,
        content: String,
    },
    /// Slash command (e.g., /compact, /crt)
    Command {
        name: String,
    },
    /// Context compaction starting indicator
    Compacting,
    /// Context compaction completed indicator
    Compacted,
    /// Assistant text response
    AssistantText {
        uuid: String,
        message_id: String,
        text: String,
    },
    /// Tool being called
    ToolCall {
        uuid: String,
        tool_use_id: String,
        tool_name: String,
        /// Extracted file path if applicable
        file_path: Option<String>,
        /// Full input for display
        input: serde_json::Value,
    },
    /// Tool result (output from a tool call)
    ToolResult {
        tool_use_id: String,
        tool_name: String,
        /// For file-based tools: the path that was operated on
        file_path: Option<String>,
        /// The raw output content from the tool
        content: String,
    },
    /// Session complete
    Complete {
        session_id: String,
        success: bool,
        duration_ms: u64,
        cost_usd: f64,
    },
    /// Error
    Error {
        message: String,
    },
    /// Filtered out (used for rewound/edited messages that were superseded)
    Filtered,
}

/// Parser for Claude Code stream-json events
pub struct EventParser {
    buffer: String,
    /// Track tool calls by ID so we can match results to calls
    tool_calls: std::collections::HashMap<String, (String, Option<String>)>,
}

impl EventParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            tool_calls: std::collections::HashMap::new(),
        }
    }

    /// Feed raw data and get parsed display events
    pub fn parse(&mut self, data: &str) -> Vec<DisplayEvent> {
        self.buffer.push_str(data);
        let mut events = Vec::new();

        // Process complete lines
        while let Some(newline_pos) = self.buffer.find('\n') {
            let line = self.buffer[..newline_pos].trim().to_string();
            self.buffer = self.buffer[newline_pos + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            events.extend(self.parse_line(&line));
        }

        events
    }

    fn parse_line(&mut self, line: &str) -> Vec<DisplayEvent> {
        let trimmed = line.trim();

        // Try JSON first if line looks like JSON (starts with {)
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
                            let name = json.get("hook_name")
                                .or_else(|| json.get("name"))
                                .or_else(|| json.get("hook"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("hook")
                                .to_string();
                            let output = json.get("output")
                                .or_else(|| json.get("result"))
                                .or_else(|| json.get("message"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            vec![DisplayEvent::Hook { name, output }]
                        }
                        _ => Vec::new(),
                    };
                }
            }
            // Invalid JSON starting with { - skip it
            return Vec::new();
        }

        // For non-JSON lines, try text hook patterns (e.g., "UserPromptSubmit hook success: ...")
        self.parse_text_hook(line).into_iter().collect()
    }

    fn parse_system_event(&self, json: &serde_json::Value) -> Option<DisplayEvent> {
        let subtype = json.get("subtype").and_then(|v| v.as_str()).unwrap_or("");

        // Init event
        if subtype == "init" {
            return Some(DisplayEvent::Init {
                session_id: json.get("session_id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                cwd: json.get("cwd").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                model: json.get("model").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
            });
        }

        // Only process hook_response events (hook_started has no output)
        // Note: Claude Code only emits hook_response for SessionStart hooks in stream-json
        // PreToolUse/PostToolUse hooks are injected into context but not streamed
        if subtype != "hook_response" {
            return None;
        }

        let hook_name = json.get("hook_name")
            .or_else(|| json.get("name"))
            .or_else(|| json.get("hook"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();

        let output = json.get("output")
            .or_else(|| json.get("stdout"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        // Only show hooks that have actual output
        if !hook_name.is_empty() && !output.is_empty() {
            return Some(DisplayEvent::Hook { name: hook_name, output });
        }

        None
    }

    /// Parse progress events (includes hook_progress for PreToolUse/PostToolUse hooks)
    fn parse_progress_event(&self, json: &serde_json::Value) -> Option<DisplayEvent> {
        let data = json.get("data")?;
        let data_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");

        // Only process hook_progress events
        if data_type != "hook_progress" {
            return None;
        }

        let hook_event = data.get("hookEvent").and_then(|v| v.as_str()).unwrap_or("");
        let hook_name = data.get("hookName").and_then(|v| v.as_str()).unwrap_or(hook_event);
        let command = data.get("command").and_then(|v| v.as_str()).unwrap_or("");

        // Skip if no hook name
        if hook_name.is_empty() {
            return None;
        }

        // Try to extract output from simple echo commands (e.g., "echo 'message'" or "echo \"message\"")
        // This handles the common pattern where hooks just echo a message
        let output = if command.starts_with("echo '") && command.ends_with('\'') {
            command[6..command.len()-1].to_string()
        } else if command.starts_with("echo \"") && command.ends_with('"') {
            command[6..command.len()-1].to_string()
        } else if command.contains("; echo \"$OUT\"") || command.contains("; echo '$OUT'") {
            // Pattern: OUT='message'; ...; echo "$OUT" - extract the OUT value
            if let Some(start) = command.find("OUT='") {
                let rest = &command[start + 5..];
                if let Some(end) = rest.find('\'') {
                    rest[..end].to_string()
                } else {
                    String::new()
                }
            } else if let Some(start) = command.find("OUT=\"") {
                let rest = &command[start + 5..];
                if let Some(end) = rest.find('"') {
                    rest[..end].to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // Only show hooks that have meaningful output (skip verbose command-only hooks)
        if output.is_empty() {
            return None;
        }

        Some(DisplayEvent::Hook {
            name: hook_name.to_string(),
            output,
        })
    }

    fn parse_user_event(&self, json: &serde_json::Value) -> Vec<DisplayEvent> {
        let mut events = Vec::new();
        let message = match json.get("message") {
            Some(m) => m,
            None => return events,
        };
        let content_val = match message.get("content") {
            Some(c) => c,
            None => return events,
        };

        // String content = user prompt (may contain system-reminder tags with hook output)
        if let Some(content) = content_val.as_str() {
            // Extract hooks from system-reminder tags (e.g., "UserPromptSubmit hook success: ...")
            events.extend(Self::extract_hooks_from_content(content));

            events.push(DisplayEvent::UserMessage {
                uuid: json.get("uuid").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                content: content.to_string(),
            });
        }
        // Array content = could be text blocks or tool_result blocks
        else if let Some(arr) = content_val.as_array() {
            for block in arr {
                let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match block_type {
                    "tool_result" => {
                        let tool_use_id = block.get("tool_use_id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                        let (tool_name, file_path) = self.tool_calls.get(&tool_use_id).cloned().unwrap_or(("Unknown".to_string(), None));

                        let content = if let Some(s) = block.get("content").and_then(|c| c.as_str()) {
                            s.to_string()
                        } else if let Some(arr) = block.get("content").and_then(|c| c.as_array()) {
                            arr.iter()
                                .filter_map(|b| if b.get("type").and_then(|t| t.as_str()) == Some("text") { b.get("text").and_then(|t| t.as_str()) } else { None })
                                .collect::<Vec<_>>().join("\n")
                        } else { String::new() };

                        // Extract hooks from system-reminder tags in tool result content
                        events.extend(Self::extract_hooks_from_content(&content));

                        if !content.is_empty() {
                            events.push(DisplayEvent::ToolResult { tool_use_id, tool_name, file_path, content });
                        }
                    }
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            // Extract hooks from system-reminder tags in text content
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

        let message = match json.get("message") {
            Some(m) => m,
            None => return events,
        };
        let message_id = match message.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => return events,
        };
        let uuid = json.get("uuid").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let content = match message.get("content").and_then(|v| v.as_array()) {
            Some(c) => c,
            None => return events,
        };

        // Process ALL content blocks, not just the first
        for block in content {
            let block_type = match block.get("type").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => continue,
            };

            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        events.push(DisplayEvent::AssistantText {
                            uuid: uuid.clone(),
                            message_id: message_id.clone(),
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
                        let file_path = input.get("file_path")
                            .or_else(|| input.get("path"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());

                        // Track this tool call so we can match it with the result later
                        self.tool_calls.insert(
                            tool_use_id.to_string(),
                            (tool_name.to_string(), file_path.clone()),
                        );

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

    /// Parse plain text hook patterns
    fn parse_text_hook(&self, line: &str) -> Option<DisplayEvent> {
        let line = line.trim();

        // Pattern: "HookName hook success: output" or "HookName hook failed: output"
        if let Some(pos) = line.find(" hook success:") {
            let name = line[..pos].to_string();
            let output = line[pos + 14..].trim().to_string();
            return Some(DisplayEvent::Hook { name, output });
        }
        if let Some(pos) = line.find(" hook failed:") {
            let name = line[..pos].to_string();
            let output = line[pos + 13..].trim().to_string();
            return Some(DisplayEvent::Hook { name, output });
        }
        // Pattern: "HookName hook success" (no colon/output)
        if line.ends_with(" hook success") {
            let name = line.trim_end_matches(" hook success").to_string();
            return Some(DisplayEvent::Hook { name, output: String::new() });
        }
        if line.ends_with(" hook failed") {
            let name = line.trim_end_matches(" hook failed").to_string();
            return Some(DisplayEvent::Hook { name, output: String::new() });
        }
        // Pattern: contains "hook" somewhere - more aggressive matching
        if line.contains(" hook ") || line.contains("Hook") {
            // Try to extract a meaningful name
            if let Some(pos) = line.find(" hook") {
                let name = line[..pos].to_string();
                let output = line[pos..].to_string();
                return Some(DisplayEvent::Hook { name, output });
            }
        }
        None
    }

    /// Extract hook events from system-reminder tags in content
    /// Parses patterns like "<system-reminder>HookName hook success: output</system-reminder>"
    fn extract_hooks_from_content(content: &str) -> Vec<DisplayEvent> {
        let mut hooks = Vec::new();

        // Find all system-reminder blocks
        let mut search_start = 0;
        while let Some(start) = content[search_start..].find("<system-reminder>") {
            let abs_start = search_start + start + 17; // skip the opening tag
            if let Some(end) = content[abs_start..].find("</system-reminder>") {
                let reminder_content = &content[abs_start..abs_start + end];

                // Parse "HookName hook success: output" or "HookName hook failed: output"
                if let Some(hook_pos) = reminder_content.find(" hook success:") {
                    let name = reminder_content[..hook_pos].trim().to_string();
                    let output = reminder_content[hook_pos + 14..].trim().to_string();
                    if !output.is_empty() {
                        hooks.push(DisplayEvent::Hook { name, output });
                    }
                } else if let Some(hook_pos) = reminder_content.find(" hook failed:") {
                    let name = reminder_content[..hook_pos].trim().to_string();
                    let output = reminder_content[hook_pos + 13..].trim().to_string();
                    hooks.push(DisplayEvent::Hook { name, output: format!("FAILED: {}", output) });
                }

                search_start = abs_start + end + 18; // skip past </system-reminder>
            } else {
                break;
            }
        }

        hooks
    }
}

impl Default for EventParser {
    fn default() -> Self {
        Self::new()
    }
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
            DisplayEvent::AssistantText { text, .. } => {
                assert_eq!(text, "Hello!");
            }
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
        // hook_started events should be ignored (no output)
        let json = r#"{"type":"system","subtype":"hook_started","hook_name":"SessionStart:startup","session_id":"abc123"}"#;
        let events = parser.parse(&format!("{}\n", json));

        assert_eq!(events.len(), 0, "hook_started should not produce events");
    }

    #[test]
    fn test_parse_hook_response_empty_output_ignored() {
        let mut parser = EventParser::new();
        // hook_response with empty output should be ignored
        let json = r#"{"type":"system","subtype":"hook_response","hook_name":"SessionStart:startup","output":"","session_id":"abc123"}"#;
        let events = parser.parse(&format!("{}\n", json));

        assert_eq!(events.len(), 0, "hook_response with empty output should not produce events");
    }

    #[test]
    fn test_parse_tool_result_matches_tool_call() {
        let mut parser = EventParser::new();

        // First, send a tool_use event
        let tool_call_json = r#"{"type":"assistant","uuid":"u1","message":{"id":"msg1","model":"claude","role":"assistant","content":[{"type":"tool_use","id":"tool123","name":"Read","input":{"file_path":"/test/file.rs"}}]}}"#;
        let events = parser.parse(&format!("{}\n", tool_call_json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolCall { tool_name, file_path, tool_use_id, .. } => {
                assert_eq!(tool_name, "Read");
                assert_eq!(file_path.as_deref(), Some("/test/file.rs"));
                assert_eq!(tool_use_id, "tool123");
            }
            _ => panic!("Expected ToolCall event"),
        }

        // Then, send a tool_result event in a user message
        let tool_result_json = r#"{"type":"user","uuid":"u2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool123","content":"File contents here"}]}}"#;
        let events = parser.parse(&format!("{}\n", tool_result_json));
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::ToolResult { tool_name, file_path, content, tool_use_id } => {
                assert_eq!(tool_name, "Read");
                assert_eq!(file_path.as_deref(), Some("/test/file.rs"));
                assert_eq!(content, "File contents here");
                assert_eq!(tool_use_id, "tool123");
            }
            _ => panic!("Expected ToolResult event, got {:?}", events[0]),
        }
    }

    #[test]
    fn test_extract_hooks_from_system_reminder() {
        let mut parser = EventParser::new();

        // User message with system-reminder containing hook output
        let json = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"<system-reminder>\nUserPromptSubmit hook success: Follow CLAUDE.md guidelines.\n</system-reminder>\nHello Claude"}}"#;
        let events = parser.parse(&format!("{}\n", json));

        // Should have Hook event AND UserMessage
        assert_eq!(events.len(), 2);

        match &events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "UserPromptSubmit");
                assert_eq!(output, "Follow CLAUDE.md guidelines.");
            }
            _ => panic!("Expected Hook event, got {:?}", events[0]),
        }

        match &events[1] {
            DisplayEvent::UserMessage { content, .. } => {
                assert!(content.contains("Hello Claude"));
            }
            _ => panic!("Expected UserMessage event"),
        }
    }

    #[test]
    fn test_extract_hooks_from_tool_result() {
        let mut parser = EventParser::new();

        // First register a tool call
        let tool_call_json = r#"{"type":"assistant","uuid":"a1","message":{"id":"m1","role":"assistant","content":[{"type":"tool_use","id":"tool456","name":"Read","input":{"file_path":"/test.rs"}}]}}"#;
        let _ = parser.parse(&format!("{}\n", tool_call_json));

        // Tool result with system-reminder containing hook output
        let tool_result_json = r#"{"type":"user","uuid":"u2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool456","content":"<system-reminder>\nPostToolUse hook success: If this changed code, consider updating AGENTS.md.\n</system-reminder>\nFile contents here"}]}}"#;
        let events = parser.parse(&format!("{}\n", tool_result_json));

        // Should have Hook event AND ToolResult
        assert_eq!(events.len(), 2, "Expected 2 events, got {:?}", events);

        match &events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "PostToolUse");
                assert_eq!(output, "If this changed code, consider updating AGENTS.md.");
            }
            _ => panic!("Expected Hook event, got {:?}", events[0]),
        }

        match &events[1] {
            DisplayEvent::ToolResult { tool_name, content, .. } => {
                assert_eq!(tool_name, "Read");
                assert!(content.contains("File contents here"));
            }
            _ => panic!("Expected ToolResult event, got {:?}", events[1]),
        }
    }

    #[test]
    fn test_parse_hook_progress_event() {
        let mut parser = EventParser::new();

        // Simple echo command
        let json = r#"{"type":"progress","data":{"type":"hook_progress","hookEvent":"PreToolUse","hookName":"PreToolUse:Bash","command":"echo 'Ensure this action complies with CLAUDE.md and AGENTS.md.'"}}"#;
        let events = parser.parse(&format!("{}\n", json));

        assert_eq!(events.len(), 1, "Expected 1 event, got {:?}", events);
        match &events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "PreToolUse:Bash");
                assert_eq!(output, "Ensure this action complies with CLAUDE.md and AGENTS.md.");
            }
            _ => panic!("Expected Hook event, got {:?}", events[0]),
        }
    }

    #[test]
    fn test_parse_hook_progress_with_out_variable() {
        let mut parser = EventParser::new();

        // OUT variable pattern
        let json = r#"{"type":"progress","data":{"type":"hook_progress","hookEvent":"PostToolUse","hookName":"PostToolUse:Read","command":"OUT='If this changed code, consider updating AGENTS.md.'; ~/.claude/scripts/log-hook.sh PostToolUse \"$OUT\"; echo \"$OUT\""}}"#;
        let events = parser.parse(&format!("{}\n", json));

        assert_eq!(events.len(), 1, "Expected 1 event, got {:?}", events);
        match &events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "PostToolUse:Read");
                assert_eq!(output, "If this changed code, consider updating AGENTS.md.");
            }
            _ => panic!("Expected Hook event, got {:?}", events[0]),
        }
    }
}
