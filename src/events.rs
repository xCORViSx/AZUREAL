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
    /// Tool result
    ToolResult {
        tool_use_id: String,
        success: bool,
        output: String,
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
}

/// Parser for Claude Code stream-json events
pub struct EventParser {
    buffer: String,
}

impl EventParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
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

    fn parse_line(&self, line: &str) -> Vec<DisplayEvent> {
        let trimmed = line.trim();

        // Try JSON first if line looks like JSON (starts with {)
        if trimmed.starts_with('{') {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(event_type) = json.get("type").and_then(|v| v.as_str()) {
                    return match event_type {
                        "system" => self.parse_system_event(&json).into_iter().collect(),
                        "user" => self.parse_user_event(&json).into_iter().collect(),
                        "assistant" => self.parse_assistant_event(&json),
                        "result" => self.parse_result_event(&json).into_iter().collect(),
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

    fn parse_user_event(&self, json: &serde_json::Value) -> Option<DisplayEvent> {
        let message = json.get("message")?;
        let content_val = message.get("content")?;

        // Content can be a string or an array of content blocks
        let content = if let Some(s) = content_val.as_str() {
            s.to_string()
        } else if let Some(arr) = content_val.as_array() {
            // Extract text from content blocks
            arr.iter()
                .filter_map(|block| {
                    if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                        block.get("text").and_then(|t| t.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            return None;
        };

        // Skip empty content (e.g., tool_result events have no text blocks)
        if content.is_empty() {
            return None;
        }

        Some(DisplayEvent::UserMessage {
            uuid: json.get("uuid").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            content,
        })
    }

    fn parse_assistant_event(&self, json: &serde_json::Value) -> Vec<DisplayEvent> {
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
}
