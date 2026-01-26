//! Claude Code stream-json event types and parser
//!
//! Parses the JSON events emitted by `claude -p --output-format stream-json`
//! into structured Rust types for custom rendering.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

            if let Some(event) = self.parse_line(&line) {
                events.push(event);
            }
        }

        events
    }

    fn parse_line(&self, line: &str) -> Option<DisplayEvent> {
        // Try to parse as JSON
        let json: serde_json::Value = serde_json::from_str(line).ok()?;

        let event_type = json.get("type")?.as_str()?;

        match event_type {
            "system" => self.parse_system_event(&json),
            "user" => self.parse_user_event(&json),
            "assistant" => self.parse_assistant_event(&json),
            "result" => self.parse_result_event(&json),
            _ => None,
        }
    }

    fn parse_system_event(&self, json: &serde_json::Value) -> Option<DisplayEvent> {
        let subtype = json.get("subtype")?.as_str()?;

        match subtype {
            "init" => Some(DisplayEvent::Init {
                session_id: json.get("session_id")?.as_str()?.to_string(),
                cwd: json.get("cwd").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                model: json.get("model").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
            }),
            "hook_response" => {
                let output = json.get("output").and_then(|v| v.as_str()).unwrap_or("");
                if output.is_empty() {
                    None
                } else {
                    Some(DisplayEvent::Hook {
                        name: json.get("hook_name").and_then(|v| v.as_str()).unwrap_or("hook").to_string(),
                        output: output.to_string(),
                    })
                }
            }
            _ => None, // Skip other system events
        }
    }

    fn parse_user_event(&self, json: &serde_json::Value) -> Option<DisplayEvent> {
        let message = json.get("message")?;
        let content = message.get("content")?.as_str()?;

        Some(DisplayEvent::UserMessage {
            uuid: json.get("uuid").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            content: content.to_string(),
        })
    }

    fn parse_assistant_event(&self, json: &serde_json::Value) -> Option<DisplayEvent> {
        let message = json.get("message")?;
        let message_id = message.get("id")?.as_str()?.to_string();
        let uuid = json.get("uuid").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let content = message.get("content")?.as_array()?;

        // Process content blocks
        for block in content {
            let block_type = block.get("type")?.as_str()?;

            match block_type {
                "text" => {
                    let text = block.get("text")?.as_str()?;
                    return Some(DisplayEvent::AssistantText {
                        uuid: uuid.clone(),
                        message_id: message_id.clone(),
                        text: text.to_string(),
                    });
                }
                "tool_use" => {
                    let tool_name = block.get("name")?.as_str()?.to_string();
                    let input = block.get("input")?.clone();
                    let tool_use_id = block.get("id")?.as_str()?.to_string();

                    // Extract file path if present
                    let file_path = input.get("file_path")
                        .or_else(|| input.get("path"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    return Some(DisplayEvent::ToolCall {
                        uuid: uuid.clone(),
                        tool_use_id,
                        tool_name,
                        file_path,
                        input,
                    });
                }
                _ => {}
            }
        }

        None
    }

    fn parse_result_event(&self, json: &serde_json::Value) -> Option<DisplayEvent> {
        Some(DisplayEvent::Complete {
            session_id: json.get("session_id")?.as_str()?.to_string(),
            success: !json.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false),
            duration_ms: json.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0),
            cost_usd: json.get("total_cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0),
        })
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
}
