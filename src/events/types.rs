//! Raw Claude Code stream-json event types
//!
//! Serde structs for deserializing the JSON events from `claude -p --output-format stream-json`.
//! Fields are populated by serde deserialization but not all are read yet — allow dead_code
//! to suppress warnings until we consume more of the parsed structure.

#![allow(dead_code)]

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

#[cfg(test)]
mod tests {
    use super::*;

    // ── ClaudeCodeEvent deserialization ──

    #[test]
    fn test_deserialize_system_init() {
        let json_str = r#"{
            "type": "system",
            "subtype": "init",
            "session_id": "abc-123",
            "uuid": "u-1",
            "cwd": "/home/user/project",
            "model": "claude-opus-4-6",
            "tools": ["Read", "Write", "Bash"]
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::System(sys) => {
                assert_eq!(sys.subtype, "init");
                assert_eq!(sys.session_id, "abc-123");
                assert_eq!(sys.cwd, Some("/home/user/project".to_string()));
                assert_eq!(sys.model, Some("claude-opus-4-6".to_string()));
                assert_eq!(sys.tools.as_ref().unwrap().len(), 3);
            }
            _ => panic!("expected System event"),
        }
    }

    #[test]
    fn test_deserialize_system_hook() {
        let json_str = r#"{
            "type": "system",
            "subtype": "hook_response",
            "session_id": "abc-123",
            "uuid": "u-2",
            "hook_name": "pre-commit",
            "output": "checks passed"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::System(sys) => {
                assert_eq!(sys.subtype, "hook_response");
                assert_eq!(sys.hook_name, Some("pre-commit".to_string()));
                assert_eq!(sys.output, Some("checks passed".to_string()));
            }
            _ => panic!("expected System event"),
        }
    }

    #[test]
    fn test_deserialize_user_event() {
        let json_str = r#"{
            "type": "user",
            "message": {
                "role": "user",
                "content": "Hello Claude, help me with Rust"
            },
            "session_id": "sess-1",
            "uuid": "u-3"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::User(user) => {
                assert_eq!(user.message.role, "user");
                assert_eq!(user.message.content, "Hello Claude, help me with Rust");
                assert_eq!(user.session_id, "sess-1");
            }
            _ => panic!("expected User event"),
        }
    }

    #[test]
    fn test_deserialize_assistant_text() {
        let json_str = r#"{
            "type": "assistant",
            "message": {
                "id": "msg-1",
                "model": "claude-opus-4-6",
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Here is the answer."}
                ],
                "stop_reason": "end_turn"
            },
            "session_id": "sess-1",
            "uuid": "u-4"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::Assistant(asst) => {
                assert_eq!(asst.message.model, "claude-opus-4-6");
                assert_eq!(asst.message.content.len(), 1);
                match &asst.message.content[0] {
                    ContentBlock::Text { text } => assert_eq!(text, "Here is the answer."),
                    _ => panic!("expected Text block"),
                }
                assert_eq!(asst.message.stop_reason, Some("end_turn".to_string()));
            }
            _ => panic!("expected Assistant event"),
        }
    }

    #[test]
    fn test_deserialize_assistant_tool_use() {
        let json_str = r#"{
            "type": "assistant",
            "message": {
                "id": "msg-2",
                "model": "claude-opus-4-6",
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "tu-1",
                        "name": "Read",
                        "input": {"file_path": "/src/main.rs"}
                    }
                ]
            },
            "session_id": "sess-1",
            "uuid": "u-5"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::Assistant(asst) => match &asst.message.content[0] {
                ContentBlock::ToolUse { id, name, input } => {
                    assert_eq!(id, "tu-1");
                    assert_eq!(name, "Read");
                    assert_eq!(
                        input.get("file_path").unwrap().as_str().unwrap(),
                        "/src/main.rs"
                    );
                }
                _ => panic!("expected ToolUse block"),
            },
            _ => panic!("expected Assistant event"),
        }
    }

    #[test]
    fn test_deserialize_assistant_with_usage() {
        let json_str = r#"{
            "type": "assistant",
            "message": {
                "id": "msg-3",
                "model": "claude-sonnet-4-5",
                "role": "assistant",
                "content": [{"type": "text", "text": "ok"}],
                "usage": {
                    "input_tokens": 5000,
                    "output_tokens": 2000,
                    "cache_read_input_tokens": 3000,
                    "cache_creation_input_tokens": 1000
                }
            },
            "session_id": "sess-1"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::Assistant(asst) => {
                let usage = asst.message.usage.unwrap();
                assert_eq!(usage.input_tokens, 5000);
                assert_eq!(usage.output_tokens, 2000);
                assert_eq!(usage.cache_read_input_tokens, Some(3000));
                assert_eq!(usage.cache_creation_input_tokens, Some(1000));
            }
            _ => panic!("expected Assistant event"),
        }
    }

    #[test]
    fn test_deserialize_result_success() {
        let json_str = r#"{
            "type": "result",
            "subtype": "success",
            "session_id": "sess-1",
            "result": "Task completed",
            "is_error": false,
            "duration_ms": 5000,
            "total_cost_usd": 0.0456,
            "num_turns": 3,
            "uuid": "u-6"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::Result(result) => {
                assert_eq!(result.subtype, "success");
                assert!(!result.is_error);
                assert_eq!(result.duration_ms, Some(5000));
                assert_eq!(result.total_cost_usd, Some(0.0456));
                assert_eq!(result.num_turns, Some(3));
            }
            _ => panic!("expected Result event"),
        }
    }

    #[test]
    fn test_deserialize_result_error() {
        let json_str = r#"{
            "type": "result",
            "subtype": "error",
            "session_id": "sess-1",
            "is_error": true,
            "uuid": "u-7"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::Result(result) => {
                assert!(result.is_error);
                assert!(result.result.is_none());
                assert!(result.duration_ms.is_none());
            }
            _ => panic!("expected Result event"),
        }
    }

    // ── Usage defaults ──

    #[test]
    fn test_usage_defaults() {
        let usage: Usage = serde_json::from_str("{}").unwrap();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert!(usage.cache_read_input_tokens.is_none());
        assert!(usage.cache_creation_input_tokens.is_none());
    }

    // ── ContentBlock variants ──

    #[test]
    fn test_content_block_tool_result() {
        let json_str = r#"{
            "type": "tool_result",
            "tool_use_id": "tu-99",
            "content": "file contents here"
        }"#;
        let block: ContentBlock = serde_json::from_str(json_str).unwrap();
        match block {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
            } => {
                assert_eq!(tool_use_id, "tu-99");
                assert_eq!(content.as_str().unwrap(), "file contents here");
            }
            _ => panic!("expected ToolResult block"),
        }
    }

    // ── SystemEvent optional fields default to None ──

    #[test]
    fn test_system_event_minimal() {
        let json_str = r#"{
            "type": "system",
            "subtype": "custom",
            "session_id": "s1"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::System(sys) => {
                assert!(sys.cwd.is_none());
                assert!(sys.tools.is_none());
                assert!(sys.model.is_none());
                assert!(sys.hook_name.is_none());
                assert!(sys.output.is_none());
                assert_eq!(sys.uuid, "");
            }
            _ => panic!("expected System event"),
        }
    }

    // ── Round-trip ──

    #[test]
    fn test_system_event_roundtrip() {
        let original = ClaudeCodeEvent::System(SystemEvent {
            subtype: "init".to_string(),
            session_id: "sess-rt".to_string(),
            uuid: "u-rt".to_string(),
            cwd: Some("/test".to_string()),
            tools: Some(vec!["Read".to_string()]),
            model: Some("opus".to_string()),
            hook_name: None,
            output: None,
        });
        let json_str = serde_json::to_string(&original).unwrap();
        let deserialized: ClaudeCodeEvent = serde_json::from_str(&json_str).unwrap();
        match deserialized {
            ClaudeCodeEvent::System(sys) => {
                assert_eq!(sys.subtype, "init");
                assert_eq!(sys.session_id, "sess-rt");
                assert_eq!(sys.cwd, Some("/test".to_string()));
            }
            _ => panic!("roundtrip failed"),
        }
    }

    #[test]
    fn test_user_event_roundtrip() {
        let original = ClaudeCodeEvent::User(UserEvent {
            message: UserMessage {
                role: "user".to_string(),
                content: "test prompt".to_string(),
            },
            session_id: "s-1".to_string(),
            uuid: "u-1".to_string(),
        });
        let json_str = serde_json::to_string(&original).unwrap();
        let deserialized: ClaudeCodeEvent = serde_json::from_str(&json_str).unwrap();
        match deserialized {
            ClaudeCodeEvent::User(u) => {
                assert_eq!(u.message.content, "test prompt");
            }
            _ => panic!("roundtrip failed"),
        }
    }

    // ── ContentBlock variant isolation ──

    #[test]
    fn test_content_block_text_isolation() {
        let json_str = r#"{"type": "text", "text": "Hello world"}"#;
        let block: ContentBlock = serde_json::from_str(json_str).unwrap();
        match block {
            ContentBlock::Text { text } => assert_eq!(text, "Hello world"),
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn test_content_block_text_empty() {
        let json_str = r#"{"type": "text", "text": ""}"#;
        let block: ContentBlock = serde_json::from_str(json_str).unwrap();
        match block {
            ContentBlock::Text { text } => assert_eq!(text, ""),
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn test_content_block_tool_use_isolation() {
        let json_str =
            r#"{"type": "tool_use", "id": "tu-abc", "name": "Bash", "input": {"command": "ls"}}"#;
        let block: ContentBlock = serde_json::from_str(json_str).unwrap();
        match block {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tu-abc");
                assert_eq!(name, "Bash");
                assert_eq!(input.get("command").unwrap().as_str().unwrap(), "ls");
            }
            _ => panic!("expected ToolUse block"),
        }
    }

    #[test]
    fn test_content_block_tool_use_empty_input() {
        let json_str = r#"{"type": "tool_use", "id": "tu-1", "name": "Glob", "input": {}}"#;
        let block: ContentBlock = serde_json::from_str(json_str).unwrap();
        match block {
            ContentBlock::ToolUse { input, .. } => {
                assert!(input.as_object().unwrap().is_empty());
            }
            _ => panic!("expected ToolUse block"),
        }
    }

    #[test]
    fn test_content_block_tool_result_object_content() {
        let json_str = r#"{"type": "tool_result", "tool_use_id": "tu-5", "content": {"key": "value", "count": 42}}"#;
        let block: ContentBlock = serde_json::from_str(json_str).unwrap();
        match block {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
            } => {
                assert_eq!(tool_use_id, "tu-5");
                assert_eq!(content.get("key").unwrap().as_str().unwrap(), "value");
                assert_eq!(content.get("count").unwrap().as_u64().unwrap(), 42);
            }
            _ => panic!("expected ToolResult block"),
        }
    }

    #[test]
    fn test_content_block_tool_result_array_content() {
        let json_str =
            r#"{"type": "tool_result", "tool_use_id": "tu-6", "content": ["line1", "line2"]}"#;
        let block: ContentBlock = serde_json::from_str(json_str).unwrap();
        match block {
            ContentBlock::ToolResult { content, .. } => {
                let arr = content.as_array().unwrap();
                assert_eq!(arr.len(), 2);
                assert_eq!(arr[0].as_str().unwrap(), "line1");
            }
            _ => panic!("expected ToolResult block"),
        }
    }

    #[test]
    fn test_content_block_tool_result_null_content() {
        let json_str = r#"{"type": "tool_result", "tool_use_id": "tu-7", "content": null}"#;
        let block: ContentBlock = serde_json::from_str(json_str).unwrap();
        match block {
            ContentBlock::ToolResult { content, .. } => {
                assert!(content.is_null());
            }
            _ => panic!("expected ToolResult block"),
        }
    }

    #[test]
    fn test_content_block_tool_result_numeric_content() {
        let json_str = r#"{"type": "tool_result", "tool_use_id": "tu-8", "content": 12345}"#;
        let block: ContentBlock = serde_json::from_str(json_str).unwrap();
        match block {
            ContentBlock::ToolResult { content, .. } => {
                assert_eq!(content.as_u64().unwrap(), 12345);
            }
            _ => panic!("expected ToolResult block"),
        }
    }

    // ── Deserialization with minimal required fields ──

    #[test]
    fn test_deserialize_user_event_minimal() {
        let json_str = r#"{
            "type": "user",
            "message": {"role": "user", "content": "hi"},
            "session_id": "s1"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::User(u) => {
                assert_eq!(u.uuid, "");
                assert_eq!(u.message.content, "hi");
            }
            _ => panic!("expected User event"),
        }
    }

    #[test]
    fn test_deserialize_assistant_minimal() {
        let json_str = r#"{
            "type": "assistant",
            "message": {
                "id": "m1",
                "model": "claude",
                "role": "assistant",
                "content": []
            },
            "session_id": "s1"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::Assistant(a) => {
                assert_eq!(a.uuid, "");
                assert!(a.message.stop_reason.is_none());
                assert!(a.message.usage.is_none());
                assert!(a.message.content.is_empty());
            }
            _ => panic!("expected Assistant event"),
        }
    }

    #[test]
    fn test_deserialize_result_minimal() {
        let json_str = r#"{
            "type": "result",
            "subtype": "success",
            "session_id": "s1"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::Result(r) => {
                assert_eq!(r.subtype, "success");
                assert!(r.result.is_none());
                assert!(!r.is_error);
                assert!(r.duration_ms.is_none());
                assert!(r.total_cost_usd.is_none());
                assert!(r.num_turns.is_none());
                assert_eq!(r.uuid, "");
            }
            _ => panic!("expected Result event"),
        }
    }

    // ── Extra unknown fields (serde should ignore them) ──

    #[test]
    fn test_system_event_extra_fields_ignored() {
        let json_str = r#"{
            "type": "system",
            "subtype": "init",
            "session_id": "s1",
            "unknown_field": "should be ignored",
            "another_extra": 42
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::System(sys) => assert_eq!(sys.subtype, "init"),
            _ => panic!("expected System event"),
        }
    }

    #[test]
    fn test_user_event_extra_fields_ignored() {
        let json_str = r#"{
            "type": "user",
            "message": {"role": "user", "content": "hello", "extra": true},
            "session_id": "s1",
            "bonus_field": [1,2,3]
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::User(u) => assert_eq!(u.message.content, "hello"),
            _ => panic!("expected User event"),
        }
    }

    #[test]
    fn test_assistant_event_extra_fields_ignored() {
        let json_str = r#"{
            "type": "assistant",
            "message": {
                "id": "m1",
                "model": "claude",
                "role": "assistant",
                "content": [{"type": "text", "text": "ok", "citations": []}],
                "extra_msg_field": "ignored"
            },
            "session_id": "s1",
            "extra_top_field": null
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::Assistant(a) => assert_eq!(a.message.content.len(), 1),
            _ => panic!("expected Assistant event"),
        }
    }

    // ── Serialization output format (verify "type" tag) ──

    #[test]
    fn test_serialize_system_has_type_tag() {
        let event = ClaudeCodeEvent::System(SystemEvent {
            subtype: "init".into(),
            session_id: "s".into(),
            uuid: "u".into(),
            cwd: None,
            tools: None,
            model: None,
            hook_name: None,
            output: None,
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json.get("type").unwrap().as_str().unwrap(), "system");
    }

    #[test]
    fn test_serialize_user_has_type_tag() {
        let event = ClaudeCodeEvent::User(UserEvent {
            message: UserMessage {
                role: "user".into(),
                content: "hi".into(),
            },
            session_id: "s".into(),
            uuid: "u".into(),
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json.get("type").unwrap().as_str().unwrap(), "user");
    }

    #[test]
    fn test_serialize_assistant_has_type_tag() {
        let event = ClaudeCodeEvent::Assistant(AssistantEvent {
            message: AssistantMessage {
                id: "m".into(),
                model: "claude".into(),
                role: "assistant".into(),
                content: vec![],
                stop_reason: None,
                usage: None,
            },
            session_id: "s".into(),
            uuid: "u".into(),
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json.get("type").unwrap().as_str().unwrap(), "assistant");
    }

    #[test]
    fn test_serialize_result_has_type_tag() {
        let event = ClaudeCodeEvent::Result(ResultEvent {
            subtype: "success".into(),
            session_id: "s".into(),
            result: None,
            is_error: false,
            duration_ms: None,
            total_cost_usd: None,
            num_turns: None,
            uuid: "u".into(),
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json.get("type").unwrap().as_str().unwrap(), "result");
    }

    // ── AssistantMessage with empty content array ──

    #[test]
    fn test_assistant_empty_content_array() {
        let json_str = r#"{
            "type": "assistant",
            "message": {
                "id": "m-empty",
                "model": "claude",
                "role": "assistant",
                "content": []
            },
            "session_id": "s1"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::Assistant(a) => {
                assert!(a.message.content.is_empty());
            }
            _ => panic!("expected Assistant event"),
        }
    }

    // ── AssistantMessage with multiple text blocks ──

    #[test]
    fn test_assistant_multiple_text_blocks() {
        let json_str = r#"{
            "type": "assistant",
            "message": {
                "id": "m-multi",
                "model": "claude",
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "First paragraph."},
                    {"type": "text", "text": "Second paragraph."},
                    {"type": "text", "text": "Third paragraph."}
                ]
            },
            "session_id": "s1"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::Assistant(a) => {
                assert_eq!(a.message.content.len(), 3);
                for block in &a.message.content {
                    match block {
                        ContentBlock::Text { .. } => {}
                        _ => panic!("expected all Text blocks"),
                    }
                }
            }
            _ => panic!("expected Assistant event"),
        }
    }

    // ── AssistantMessage with multiple tool_use blocks ──

    #[test]
    fn test_assistant_multiple_tool_use_blocks() {
        let json_str = r#"{
            "type": "assistant",
            "message": {
                "id": "m-tools",
                "model": "claude",
                "role": "assistant",
                "content": [
                    {"type": "tool_use", "id": "t1", "name": "Read", "input": {"file_path": "/a.rs"}},
                    {"type": "tool_use", "id": "t2", "name": "Grep", "input": {"pattern": "fn main"}}
                ]
            },
            "session_id": "s1"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::Assistant(a) => {
                assert_eq!(a.message.content.len(), 2);
                match &a.message.content[0] {
                    ContentBlock::ToolUse { name, .. } => assert_eq!(name, "Read"),
                    _ => panic!("expected ToolUse"),
                }
                match &a.message.content[1] {
                    ContentBlock::ToolUse { name, .. } => assert_eq!(name, "Grep"),
                    _ => panic!("expected ToolUse"),
                }
            }
            _ => panic!("expected Assistant event"),
        }
    }

    // ── AssistantMessage with mixed text and tool_use ──

    #[test]
    fn test_assistant_mixed_text_and_tool_use() {
        let json_str = r#"{
            "type": "assistant",
            "message": {
                "id": "m-mix",
                "model": "claude",
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Let me read that file."},
                    {"type": "tool_use", "id": "t1", "name": "Read", "input": {"file_path": "/x.rs"}},
                    {"type": "text", "text": "Now let me write."},
                    {"type": "tool_use", "id": "t2", "name": "Write", "input": {"file_path": "/y.rs", "content": "code"}}
                ]
            },
            "session_id": "s1"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::Assistant(a) => {
                assert_eq!(a.message.content.len(), 4);
                assert!(matches!(&a.message.content[0], ContentBlock::Text { .. }));
                assert!(matches!(
                    &a.message.content[1],
                    ContentBlock::ToolUse { .. }
                ));
                assert!(matches!(&a.message.content[2], ContentBlock::Text { .. }));
                assert!(matches!(
                    &a.message.content[3],
                    ContentBlock::ToolUse { .. }
                ));
            }
            _ => panic!("expected Assistant event"),
        }
    }

    // ── Usage with all zero values ──

    #[test]
    fn test_usage_all_zeros() {
        let json_str = r#"{
            "input_tokens": 0,
            "output_tokens": 0,
            "cache_read_input_tokens": 0,
            "cache_creation_input_tokens": 0
        }"#;
        let usage: Usage = serde_json::from_str(json_str).unwrap();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.cache_read_input_tokens, Some(0));
        assert_eq!(usage.cache_creation_input_tokens, Some(0));
    }

    // ── Usage with very large token counts ──

    #[test]
    fn test_usage_large_token_counts() {
        let json_str = r#"{
            "input_tokens": 18446744073709551615,
            "output_tokens": 999999999999,
            "cache_read_input_tokens": 500000000000,
            "cache_creation_input_tokens": 100000000000
        }"#;
        let usage: Usage = serde_json::from_str(json_str).unwrap();
        assert_eq!(usage.input_tokens, u64::MAX);
        assert_eq!(usage.output_tokens, 999999999999);
        assert_eq!(usage.cache_read_input_tokens, Some(500000000000));
        assert_eq!(usage.cache_creation_input_tokens, Some(100000000000));
    }

    #[test]
    fn test_usage_partial_fields() {
        let json_str = r#"{"input_tokens": 100, "output_tokens": 50}"#;
        let usage: Usage = serde_json::from_str(json_str).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert!(usage.cache_read_input_tokens.is_none());
        assert!(usage.cache_creation_input_tokens.is_none());
    }

    // ── ResultEvent with all optional fields set ──

    #[test]
    fn test_result_event_all_fields_set() {
        let json_str = r#"{
            "type": "result",
            "subtype": "success",
            "session_id": "sess-full",
            "result": "All tasks completed successfully.",
            "is_error": false,
            "duration_ms": 123456,
            "total_cost_usd": 1.2345,
            "num_turns": 15,
            "uuid": "u-full"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::Result(r) => {
                assert_eq!(r.subtype, "success");
                assert_eq!(r.session_id, "sess-full");
                assert_eq!(
                    r.result,
                    Some("All tasks completed successfully.".to_string())
                );
                assert!(!r.is_error);
                assert_eq!(r.duration_ms, Some(123456));
                assert_eq!(r.total_cost_usd, Some(1.2345));
                assert_eq!(r.num_turns, Some(15));
                assert_eq!(r.uuid, "u-full");
            }
            _ => panic!("expected Result event"),
        }
    }

    // ── ResultEvent with no optional fields ──

    #[test]
    fn test_result_event_no_optional_fields() {
        let json_str = r#"{
            "type": "result",
            "subtype": "error",
            "session_id": "sess-min"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::Result(r) => {
                assert!(r.result.is_none());
                assert!(!r.is_error);
                assert!(r.duration_ms.is_none());
                assert!(r.total_cost_usd.is_none());
                assert!(r.num_turns.is_none());
                assert_eq!(r.uuid, "");
            }
            _ => panic!("expected Result event"),
        }
    }

    // ── UserMessage content with special chars, newlines, unicode, long strings ──

    #[test]
    fn test_user_message_special_chars() {
        let json_str = r#"{
            "type": "user",
            "message": {"role": "user", "content": "Hello <world> & \"quotes\" 'apostrophes'"},
            "session_id": "s1"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::User(u) => {
                assert_eq!(
                    u.message.content,
                    "Hello <world> & \"quotes\" 'apostrophes'"
                );
            }
            _ => panic!("expected User event"),
        }
    }

    #[test]
    fn test_user_message_newlines() {
        let json_str = r#"{
            "type": "user",
            "message": {"role": "user", "content": "line1\nline2\nline3"},
            "session_id": "s1"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::User(u) => {
                assert!(u.message.content.contains('\n'));
                assert_eq!(u.message.content.lines().count(), 3);
            }
            _ => panic!("expected User event"),
        }
    }

    #[test]
    fn test_user_message_unicode() {
        let json_str = r#"{
            "type": "user",
            "message": {"role": "user", "content": "日本語テスト 🦀 Ñoño café résumé"},
            "session_id": "s1"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::User(u) => {
                assert!(u.message.content.contains("日本語"));
                assert!(u.message.content.contains("café"));
            }
            _ => panic!("expected User event"),
        }
    }

    #[test]
    fn test_user_message_very_long_string() {
        let long_content = "x".repeat(100_000);
        let json_str = format!(
            r#"{{"type":"user","message":{{"role":"user","content":"{}"}},"session_id":"s1"}}"#,
            long_content
        );
        let event: ClaudeCodeEvent = serde_json::from_str(&json_str).unwrap();
        match event {
            ClaudeCodeEvent::User(u) => {
                assert_eq!(u.message.content.len(), 100_000);
            }
            _ => panic!("expected User event"),
        }
    }

    #[test]
    fn test_user_message_empty_string() {
        let json_str = r#"{
            "type": "user",
            "message": {"role": "user", "content": ""},
            "session_id": "s1"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::User(u) => {
                assert_eq!(u.message.content, "");
            }
            _ => panic!("expected User event"),
        }
    }

    // ── SystemEvent with all tools ──

    #[test]
    fn test_system_event_all_tools() {
        let json_str = r#"{
            "type": "system",
            "subtype": "init",
            "session_id": "s1",
            "tools": ["Read", "Write", "Edit", "Bash", "Glob", "Grep", "WebFetch", "WebSearch", "NotebookEdit", "TodoWrite"]
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::System(sys) => {
                let tools = sys.tools.unwrap();
                assert_eq!(tools.len(), 10);
                assert!(tools.contains(&"Read".to_string()));
                assert!(tools.contains(&"WebSearch".to_string()));
            }
            _ => panic!("expected System event"),
        }
    }

    #[test]
    fn test_system_event_empty_tools() {
        let json_str = r#"{
            "type": "system",
            "subtype": "init",
            "session_id": "s1",
            "tools": []
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::System(sys) => {
                assert!(sys.tools.unwrap().is_empty());
            }
            _ => panic!("expected System event"),
        }
    }

    // ── Deserialization errors ──

    #[test]
    fn test_deserialize_invalid_type_field() {
        let json_str = r#"{"type": "nonexistent", "session_id": "s1"}"#;
        let result = serde_json::from_str::<ClaudeCodeEvent>(json_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_missing_type_field() {
        let json_str = r#"{"subtype": "init", "session_id": "s1"}"#;
        let result = serde_json::from_str::<ClaudeCodeEvent>(json_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_invalid_content_block_type() {
        let json_str = r#"{"type": "thinking", "thinking": "hmm"}"#;
        let result = serde_json::from_str::<ContentBlock>(json_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_system_missing_session_id() {
        let json_str = r#"{"type": "system", "subtype": "init"}"#;
        let result = serde_json::from_str::<ClaudeCodeEvent>(json_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_user_missing_message() {
        let json_str = r#"{"type": "user", "session_id": "s1"}"#;
        let result = serde_json::from_str::<ClaudeCodeEvent>(json_str);
        assert!(result.is_err());
    }

    // ── Result event roundtrip ──

    #[test]
    fn test_result_event_roundtrip() {
        let original = ClaudeCodeEvent::Result(ResultEvent {
            subtype: "success".into(),
            session_id: "sess-rt".into(),
            result: Some("done".into()),
            is_error: false,
            duration_ms: Some(1234),
            total_cost_usd: Some(0.05),
            num_turns: Some(5),
            uuid: "u-rt".into(),
        });
        let json_str = serde_json::to_string(&original).unwrap();
        let deserialized: ClaudeCodeEvent = serde_json::from_str(&json_str).unwrap();
        match deserialized {
            ClaudeCodeEvent::Result(r) => {
                assert_eq!(r.result, Some("done".to_string()));
                assert_eq!(r.duration_ms, Some(1234));
                assert_eq!(r.num_turns, Some(5));
            }
            _ => panic!("roundtrip failed"),
        }
    }

    // ── Assistant event roundtrip ──

    #[test]
    fn test_assistant_event_roundtrip() {
        let original = ClaudeCodeEvent::Assistant(AssistantEvent {
            message: AssistantMessage {
                id: "m-rt".into(),
                model: "claude-opus-4-6".into(),
                role: "assistant".into(),
                content: vec![
                    ContentBlock::Text {
                        text: "Hello".into(),
                    },
                    ContentBlock::ToolUse {
                        id: "t-rt".into(),
                        name: "Read".into(),
                        input: serde_json::json!({"file_path": "/test.rs"}),
                    },
                ],
                stop_reason: Some("end_turn".into()),
                usage: Some(Usage {
                    input_tokens: 100,
                    output_tokens: 50,
                    cache_read_input_tokens: Some(20),
                    cache_creation_input_tokens: None,
                }),
            },
            session_id: "s-rt".into(),
            uuid: "u-rt".into(),
        });
        let json_str = serde_json::to_string(&original).unwrap();
        let deserialized: ClaudeCodeEvent = serde_json::from_str(&json_str).unwrap();
        match deserialized {
            ClaudeCodeEvent::Assistant(a) => {
                assert_eq!(a.message.content.len(), 2);
                assert_eq!(a.message.model, "claude-opus-4-6");
                let usage = a.message.usage.unwrap();
                assert_eq!(usage.input_tokens, 100);
                assert_eq!(usage.cache_read_input_tokens, Some(20));
                assert!(usage.cache_creation_input_tokens.is_none());
            }
            _ => panic!("roundtrip failed"),
        }
    }

    // ── ContentBlock serialization format ──

    #[test]
    fn test_content_block_text_serialization() {
        let block = ContentBlock::Text {
            text: "hello".into(),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json.get("type").unwrap().as_str().unwrap(), "text");
        assert_eq!(json.get("text").unwrap().as_str().unwrap(), "hello");
    }

    #[test]
    fn test_content_block_tool_use_serialization() {
        let block = ContentBlock::ToolUse {
            id: "t1".into(),
            name: "Bash".into(),
            input: serde_json::json!({"cmd": "ls"}),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json.get("type").unwrap().as_str().unwrap(), "tool_use");
        assert_eq!(json.get("name").unwrap().as_str().unwrap(), "Bash");
    }

    #[test]
    fn test_content_block_tool_result_serialization() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: serde_json::json!("output"),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json.get("type").unwrap().as_str().unwrap(), "tool_result");
        assert_eq!(json.get("tool_use_id").unwrap().as_str().unwrap(), "t1");
    }

    // ── SystemEvent with hook fields ──

    #[test]
    fn test_system_event_hook_and_output() {
        let json_str = r#"{
            "type": "system",
            "subtype": "hook_response",
            "session_id": "s1",
            "hook_name": "PreToolUse:Bash",
            "output": "command validated"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::System(sys) => {
                assert_eq!(sys.hook_name, Some("PreToolUse:Bash".to_string()));
                assert_eq!(sys.output, Some("command validated".to_string()));
            }
            _ => panic!("expected System event"),
        }
    }

    #[test]
    fn test_system_event_all_fields_populated() {
        let json_str = r#"{
            "type": "system",
            "subtype": "init",
            "session_id": "sess-all",
            "uuid": "uuid-all",
            "cwd": "/home/user",
            "tools": ["Read"],
            "model": "claude-opus-4-6",
            "hook_name": "StartupHook",
            "output": "startup output"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::System(sys) => {
                assert_eq!(sys.subtype, "init");
                assert_eq!(sys.session_id, "sess-all");
                assert_eq!(sys.uuid, "uuid-all");
                assert_eq!(sys.cwd, Some("/home/user".to_string()));
                assert_eq!(sys.tools.as_ref().unwrap().len(), 1);
                assert_eq!(sys.model, Some("claude-opus-4-6".to_string()));
                assert_eq!(sys.hook_name, Some("StartupHook".to_string()));
                assert_eq!(sys.output, Some("startup output".to_string()));
            }
            _ => panic!("expected System event"),
        }
    }

    // ── Usage serialization ──

    #[test]
    fn test_usage_serialization_roundtrip() {
        let usage = Usage {
            input_tokens: 5000,
            output_tokens: 2000,
            cache_read_input_tokens: Some(3000),
            cache_creation_input_tokens: None,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let parsed: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.input_tokens, 5000);
        assert_eq!(parsed.output_tokens, 2000);
        assert_eq!(parsed.cache_read_input_tokens, Some(3000));
        assert!(parsed.cache_creation_input_tokens.is_none());
    }

    // ── ToolUse with complex input ──

    #[test]
    fn test_tool_use_nested_input() {
        let json_str = r#"{
            "type": "tool_use",
            "id": "tu-nested",
            "name": "WebSearch",
            "input": {
                "query": "rust async",
                "options": {"max_results": 10, "language": "en"},
                "filters": ["blog", "docs"]
            }
        }"#;
        let block: ContentBlock = serde_json::from_str(json_str).unwrap();
        match block {
            ContentBlock::ToolUse { input, .. } => {
                assert_eq!(input.get("query").unwrap().as_str().unwrap(), "rust async");
                assert_eq!(input["options"]["max_results"].as_u64().unwrap(), 10);
                assert_eq!(input["filters"].as_array().unwrap().len(), 2);
            }
            _ => panic!("expected ToolUse block"),
        }
    }

    // ── Result event with is_error true ──

    #[test]
    fn test_result_event_is_error_true_with_all_fields() {
        let json_str = r#"{
            "type": "result",
            "subtype": "error",
            "session_id": "s-err",
            "result": "Rate limit exceeded",
            "is_error": true,
            "duration_ms": 500,
            "total_cost_usd": 0.001,
            "num_turns": 1,
            "uuid": "u-err"
        }"#;
        let event: ClaudeCodeEvent = serde_json::from_str(json_str).unwrap();
        match event {
            ClaudeCodeEvent::Result(r) => {
                assert!(r.is_error);
                assert_eq!(r.result, Some("Rate limit exceeded".to_string()));
                assert_eq!(r.duration_ms, Some(500));
            }
            _ => panic!("expected Result event"),
        }
    }
}
