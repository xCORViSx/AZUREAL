//! Regression coverage for context construction and hidden-message filtering.

use super::*;

/// Covers the empty payload regression case.
fn empty_payload() -> ContextPayload {
    ContextPayload {
        compaction_summary: None,
        events: vec![],
    }
}

/// Covers the simple payload regression case.
fn simple_payload() -> ContextPayload {
    ContextPayload {
        compaction_summary: None,
        events: vec![
            DisplayEvent::UserMessage {
                _uuid: String::new(),
                content: "fix the bug".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text: "I'll look at it.".into(),
            },
        ],
    }
}

/// Covers the hidden codex context regression case.
fn hidden_codex_context() -> String {
    concat!(
        "# AGENTS.md instructions for /tmp/project\n",
        "<INSTRUCTIONS>\n",
        "Keep this hidden.\n",
        "</INSTRUCTIONS>\n",
        "<environment_context>\n",
        "  <cwd>/tmp/project</cwd>\n",
        "  <shell>bash</shell>\n",
        "</environment_context>"
    )
    .to_string()
}

/// Builds the standalone skill envelope emitted by Codex context injection.
fn hidden_skill_context() -> String {
    concat!(
        "<skill>\n",
        "<name>testing-standards</name>\n",
        "<path>/tmp/testing-standards/SKILL.md</path>\n",
        "# Testing Standards\n",
        "Hidden workflow instructions.\n",
        "</skill>"
    )
    .to_string()
}

// ── build_context_prompt ──

#[test]
/// Covers the empty payload returns original prompt regression case.
fn empty_payload_returns_original_prompt() {
    let result = build_context_prompt(&empty_payload(), "hello");
    assert_eq!(result, "hello");
}

#[test]
/// Covers the non empty payload wraps with tags regression case.
fn non_empty_payload_wraps_with_tags() {
    let result = build_context_prompt(&simple_payload(), "now fix the tests");
    assert!(result.starts_with(CONTEXT_OPEN));
    assert!(result.contains(CONTEXT_CLOSE));
    assert!(result.ends_with("now fix the tests"));
}

#[test]
/// Covers the prompt appears after close tag regression case.
fn prompt_appears_after_close_tag() {
    let result = build_context_prompt(&simple_payload(), "my prompt");
    let after_close = result.split(CONTEXT_CLOSE).nth(1).unwrap();
    assert!(after_close.contains("my prompt"));
}

#[test]
/// Covers the context contains user message regression case.
fn context_contains_user_message() {
    let result = build_context_prompt(&simple_payload(), "x");
    assert!(result.contains("## User\nfix the bug"));
}

#[test]
/// Covers the context contains assistant text regression case.
fn context_contains_assistant_text() {
    let result = build_context_prompt(&simple_payload(), "x");
    assert!(result.contains("## Assistant\nI'll look at it."));
}

// ── strip_injected_context ──

#[test]
/// Covers the strip no context returns original regression case.
fn strip_no_context_returns_original() {
    assert_eq!(strip_injected_context("hello world"), "hello world");
}

#[test]
/// Covers the strip with context returns prompt regression case.
fn strip_with_context_returns_prompt() {
    let injected = format!("{CONTEXT_OPEN}\nsome context\n{CONTEXT_CLOSE}\n\nactual prompt");
    assert_eq!(strip_injected_context(&injected), "actual prompt");
}

#[test]
/// Covers the strip round trip regression case.
fn strip_round_trip() {
    let payload = simple_payload();
    let prompt = "do the thing";
    let injected = build_context_prompt(&payload, prompt);
    let stripped = strip_injected_context(&injected);
    assert_eq!(stripped, prompt);
}

#[test]
/// Covers the strip preserves multiline prompt regression case.
fn strip_preserves_multiline_prompt() {
    let prompt = "line 1\nline 2\nline 3";
    let injected = format!("{CONTEXT_OPEN}\nctx\n{CONTEXT_CLOSE}\n\n{prompt}");
    assert_eq!(strip_injected_context(&injected), prompt);
}

#[test]
/// Covers the strip malformed context returns empty regression case.
fn strip_malformed_context_returns_empty() {
    let malformed = format!("{CONTEXT_OPEN}\nctx without close");
    assert_eq!(strip_injected_context(&malformed), "");
    assert!(sanitize_user_message_content(&malformed).is_none());
}

#[test]
/// Covers the sanitize drops hidden codex context regression case.
fn sanitize_drops_hidden_codex_context() {
    let hidden = hidden_codex_context();
    assert!(is_hidden_codex_context(&hidden));
    assert!(sanitize_user_message_content(&hidden).is_none());
}

#[test]
/// Covers the sanitize drops truncated hidden codex context regression case.
fn sanitize_drops_truncated_hidden_codex_context() {
    let hidden = concat!(
        "# AGENTS.md instructions for /tmp/project\n",
        "<INSTRUCTIONS>\n",
        "Keep this hidden.\n"
    );

    assert!(is_hidden_codex_context(hidden));
    assert!(sanitize_user_message_content(hidden).is_none());
}

/// Standalone skill definitions are hidden instead of becoming user prompt bubbles.
#[test]
fn sanitize_drops_standalone_skill_context() {
    let hidden = hidden_skill_context();

    assert!(is_hidden_skill_context(&hidden));
    assert!(sanitize_user_message_content(&hidden).is_none());
}

/// Ordinary user prose that mentions a skill tag remains visible.
#[test]
fn sanitize_preserves_user_text_that_mentions_skill_context() {
    let content = "Please explain why <skill> blocks appeared in my session pane.";

    assert!(!is_hidden_skill_context(content));
    assert_eq!(
        sanitize_user_message_content(content).as_deref(),
        Some(content)
    );
}

/// Store reload filtering removes skill bubbles that older builds persisted.
#[test]
fn strip_events_removes_standalone_skill_context() {
    let events = vec![
        DisplayEvent::UserMessage {
            _uuid: "skill".into(),
            content: hidden_skill_context(),
        },
        DisplayEvent::UserMessage {
            _uuid: "prompt".into(),
            content: "real request".into(),
        },
    ];

    let stripped = strip_injected_context_from_events(events);

    assert_eq!(stripped.len(), 1);
    assert!(matches!(
        &stripped[0],
        DisplayEvent::UserMessage { content, .. } if content == "real request"
    ));
}

#[test]
/// Covers the sanitize drops internal auto continue prompt regression case.
fn sanitize_drops_internal_auto_continue_prompt() {
    assert!(is_internal_auto_continue_prompt(AUTO_CONTINUE_PROMPT));
    assert!(sanitize_user_message_content(AUTO_CONTINUE_PROMPT).is_none());
}

#[test]
/// Covers the sanitize drops context wrapped internal auto continue prompt regression case.
fn sanitize_drops_context_wrapped_internal_auto_continue_prompt() {
    let injected =
        format!("{CONTEXT_OPEN}\nprior context\n{CONTEXT_CLOSE}\n\n{AUTO_CONTINUE_PROMPT}");
    assert_eq!(strip_injected_context(&injected), AUTO_CONTINUE_PROMPT);
    assert!(sanitize_user_message_content(&injected).is_none());
}

#[test]
/// Covers the strip events removes context from user messages only regression case.
fn strip_events_removes_context_from_user_messages_only() {
    let injected = format!("{CONTEXT_OPEN}\nctx\n{CONTEXT_CLOSE}\n\nreal prompt");
    let events = vec![
        DisplayEvent::UserMessage {
            _uuid: "u".into(),
            content: injected,
        },
        DisplayEvent::AssistantText {
            _uuid: "a".into(),
            _message_id: "m".into(),
            text: "answer".into(),
        },
    ];

    let stripped = strip_injected_context_from_events(events);

    assert!(matches!(
        &stripped[0],
        DisplayEvent::UserMessage { content, .. } if content == "real prompt"
    ));
    assert!(matches!(
        &stripped[1],
        DisplayEvent::AssistantText { text, .. } if text == "answer"
    ));
}

#[test]
/// Covers the strip events drops malformed context user message regression case.
fn strip_events_drops_malformed_context_user_message() {
    let events = vec![
        DisplayEvent::UserMessage {
            _uuid: "u".into(),
            content: format!("{CONTEXT_OPEN}\nctx without close"),
        },
        DisplayEvent::AssistantText {
            _uuid: "a".into(),
            _message_id: "m".into(),
            text: "answer".into(),
        },
    ];

    let stripped = strip_injected_context_from_events(events);

    assert_eq!(stripped.len(), 1);
    assert!(matches!(
        &stripped[0],
        DisplayEvent::AssistantText { text, .. } if text == "answer"
    ));
}

#[test]
/// Covers the strip events drops legacy hidden codex context user message regression case.
fn strip_events_drops_legacy_hidden_codex_context_user_message() {
    let events = vec![
        DisplayEvent::UserMessage {
            _uuid: "u".into(),
            content: hidden_codex_context(),
        },
        DisplayEvent::AssistantText {
            _uuid: "a".into(),
            _message_id: "m".into(),
            text: "answer".into(),
        },
    ];

    let stripped = strip_injected_context_from_events(events);

    assert_eq!(stripped.len(), 1);
    assert!(matches!(
        &stripped[0],
        DisplayEvent::AssistantText { text, .. } if text == "answer"
    ));
}

#[test]
/// Covers the strip events removes split context user messages regression case.
fn strip_events_removes_split_context_user_messages() {
    let events = vec![
        DisplayEvent::UserMessage {
            _uuid: "u1".into(),
            content: "<permissions instructions>hidden</permissions instructions>".into(),
        },
        DisplayEvent::UserMessage {
            _uuid: "u2".into(),
            content: "older transcript fragment".into(),
        },
        DisplayEvent::UserMessage {
            _uuid: "u3".into(),
            content: format!("last hidden chunk\n{CONTEXT_CLOSE}\n\nreal prompt"),
        },
        DisplayEvent::AssistantText {
            _uuid: "a".into(),
            _message_id: "m".into(),
            text: "answer".into(),
        },
    ];

    let stripped = strip_injected_context_from_events(events);

    assert_eq!(stripped.len(), 2);
    assert!(matches!(
        &stripped[0],
        DisplayEvent::UserMessage { content, .. } if content == "real prompt"
    ));
    assert!(matches!(
        &stripped[1],
        DisplayEvent::AssistantText { text, .. } if text == "answer"
    ));
}

#[test]
/// Covers the strip events preserves consecutive real user messages regression case.
fn strip_events_preserves_consecutive_real_user_messages() {
    let events = vec![
        DisplayEvent::UserMessage {
            _uuid: "u1".into(),
            content: "first".into(),
        },
        DisplayEvent::UserMessage {
            _uuid: "u2".into(),
            content: "second".into(),
        },
        DisplayEvent::AssistantText {
            _uuid: "a".into(),
            _message_id: "m".into(),
            text: "answer".into(),
        },
    ];

    let stripped = strip_injected_context_from_events(events);

    assert_eq!(stripped.len(), 3);
    assert!(matches!(
        &stripped[0],
        DisplayEvent::UserMessage { content, .. } if content == "first"
    ));
    assert!(matches!(
        &stripped[1],
        DisplayEvent::UserMessage { content, .. } if content == "second"
    ));
}

// ── format_event ──

#[test]
/// Covers the format user message regression case.
fn format_user_message() {
    let ev = DisplayEvent::UserMessage {
        _uuid: String::new(),
        content: "hello".into(),
    };
    let line = format_event(&ev).unwrap();
    assert!(line.starts_with("## User\n"));
    assert!(line.contains("hello"));
}

#[test]
/// Covers the format assistant text regression case.
fn format_assistant_text() {
    let ev = DisplayEvent::AssistantText {
        _uuid: String::new(),
        _message_id: String::new(),
        text: "hi".into(),
    };
    let line = format_event(&ev).unwrap();
    assert!(line.starts_with("## Assistant\n"));
}

#[test]
/// Covers the format tool call with param regression case.
fn format_tool_call_with_param() {
    let ev = DisplayEvent::ToolCall {
        _uuid: String::new(),
        tool_use_id: String::new(),
        tool_name: "Read".into(),
        file_path: Some("/src/main.rs".into()),
        input: serde_json::json!({"file_path": "/src/main.rs"}),
    };
    let line = format_event(&ev).unwrap();
    assert!(line.contains("## Tool: Read (/src/main.rs)"));
}

#[test]
/// Covers the format tool call no param regression case.
fn format_tool_call_no_param() {
    let ev = DisplayEvent::ToolCall {
        _uuid: String::new(),
        tool_use_id: String::new(),
        tool_name: "Custom".into(),
        file_path: None,
        input: serde_json::json!({}),
    };
    let line = format_event(&ev).unwrap();
    assert_eq!(line.trim(), "## Tool: Custom");
}

#[test]
/// Covers the format tool result ok regression case.
fn format_tool_result_ok() {
    let ev = DisplayEvent::ToolResult {
        tool_use_id: String::new(),
        tool_name: "Bash".into(),
        file_path: None,
        content: "OK".into(),
        is_error: false,
    };
    let line = format_event(&ev).unwrap();
    assert!(line.contains("[Result: Bash]"));
}

#[test]
/// Covers the format tool result error regression case.
fn format_tool_result_error() {
    let ev = DisplayEvent::ToolResult {
        tool_use_id: String::new(),
        tool_name: "Bash".into(),
        file_path: None,
        content: "not found".into(),
        is_error: true,
    };
    let line = format_event(&ev).unwrap();
    assert!(line.contains("[Error: Bash]"));
}

#[test]
/// Covers the format init returns none regression case.
fn format_init_returns_none() {
    let ev = DisplayEvent::Init {
        _session_id: String::new(),
        cwd: String::new(),
        model: String::new(),
    };
    assert!(format_event(&ev).is_none());
}

#[test]
/// Covers the format filtered returns none regression case.
fn format_filtered_returns_none() {
    assert!(format_event(&DisplayEvent::Filtered).is_none());
}

#[test]
/// Covers the format compacting returns none regression case.
fn format_compacting_returns_none() {
    assert!(format_event(&DisplayEvent::Compacting).is_none());
}

#[test]
/// Covers the format hook returns none regression case.
fn format_hook_returns_none() {
    let ev = DisplayEvent::Hook {
        name: "x".into(),
        output: "y".into(),
    };
    assert!(format_event(&ev).is_none());
}

#[test]
/// Covers the format model switch returns none regression case.
fn format_model_switch_returns_none() {
    let ev = DisplayEvent::ModelSwitch {
        model: "gpt-5.4".into(),
    };
    assert!(format_event(&ev).is_none());
}

#[test]
/// Covers the format plan regression case.
fn format_plan() {
    let ev = DisplayEvent::Plan {
        name: "refactor".into(),
        content: "step 1".into(),
    };
    let line = format_event(&ev).unwrap();
    assert!(line.contains("## Plan: refactor"));
    assert!(line.contains("step 1"));
}

#[test]
/// Covers the format command regression case.
fn format_command() {
    let ev = DisplayEvent::Command {
        name: "/compact".into(),
    };
    let line = format_event(&ev).unwrap();
    assert!(line.contains("## Command: /compact"));
}

#[test]
/// Covers the format complete regression case.
fn format_complete() {
    let ev = DisplayEvent::Complete {
        _session_id: String::new(),
        success: true,
        duration_ms: 5000,
        cost_usd: 0.05,
    };
    let line = format_event(&ev).unwrap();
    assert!(line.contains("5.0s"));
    assert!(line.contains("$0.0500"));
}

// ── compact_result ──

#[test]
/// Covers the compact result short regression case.
fn compact_result_short() {
    assert_eq!(compact_result("one\ntwo\nthree"), "one\ntwo\nthree");
}

#[test]
/// Covers the compact result long regression case.
fn compact_result_long() {
    let long = "a\nb\nc\nd\ne\nf\ng\nh";
    let result = compact_result(long);
    assert!(result.contains("a\nb\nc"));
    assert!(result.contains("(+2 more lines)"));
    assert!(result.contains("f\ng\nh"));
    assert!(!result.contains("\nd\ne\n"));
}

#[test]
/// Covers the compact result strips system reminder regression case.
fn compact_result_strips_system_reminder() {
    let content = "actual content<system-reminder>secret stuff</system-reminder>";
    assert_eq!(compact_result(content), "actual content");
}

// ── extract_key_param ──

#[test]
/// Covers the extract key param bash regression case.
fn extract_key_param_bash() {
    let input = serde_json::json!({"command": "cargo test"});
    assert_eq!(extract_key_param("Bash", &input), "cargo test");
}

#[test]
/// Covers the extract key param read regression case.
fn extract_key_param_read() {
    let input = serde_json::json!({"file_path": "/src/main.rs"});
    assert_eq!(extract_key_param("Read", &input), "/src/main.rs");
}

#[test]
/// Covers the extract key param grep regression case.
fn extract_key_param_grep() {
    let input = serde_json::json!({"pattern": "fn main"});
    assert_eq!(extract_key_param("Grep", &input), "fn main");
}

#[test]
/// Covers the extract key param missing regression case.
fn extract_key_param_missing() {
    let input = serde_json::json!({});
    assert_eq!(extract_key_param("Read", &input), "");
}

#[test]
/// Covers the extract key param path fallback regression case.
fn extract_key_param_path_fallback() {
    let input = serde_json::json!({"path": "/fallback"});
    assert_eq!(extract_key_param("Unknown", &input), "/fallback");
}

// ── build_transcript with compaction ──

#[test]
/// Covers the transcript with compaction summary regression case.
fn transcript_with_compaction_summary() {
    let payload = ContextPayload {
        compaction_summary: Some("Previously: fixed auth bug, added tests.".into()),
        events: vec![DisplayEvent::UserMessage {
            _uuid: String::new(),
            content: "now what?".into(),
        }],
    };
    let transcript = build_transcript(&payload);
    assert!(transcript.contains("[Previous conversation summary]"));
    assert!(transcript.contains("fixed auth bug"));
    assert!(transcript.contains("[Conversation continues]"));
    assert!(transcript.contains("## User\nnow what?"));
}

#[test]
/// Covers the transcript compaction only no events regression case.
fn transcript_compaction_only_no_events() {
    let payload = ContextPayload {
        compaction_summary: Some("All summarized.".into()),
        events: vec![],
    };
    let transcript = build_transcript(&payload);
    assert!(transcript.contains("All summarized."));
    assert!(transcript.contains("[Conversation continues]"));
}

#[test]
/// Covers the transcript no compaction no events empty regression case.
fn transcript_no_compaction_no_events_empty() {
    let transcript = build_transcript(&empty_payload());
    assert!(transcript.is_empty());
}

// ── build_compaction_prompt ──

#[test]
/// Covers the compaction prompt contains transcript regression case.
fn compaction_prompt_contains_transcript() {
    let prompt = build_compaction_prompt(&simple_payload());
    assert!(prompt.contains("<transcript>"));
    assert!(prompt.contains("</transcript>"));
    assert!(prompt.contains("fix the bug"));
}

#[test]
/// Covers the compaction prompt instructions regression case.
fn compaction_prompt_instructions() {
    let prompt = build_compaction_prompt(&simple_payload());
    assert!(prompt.contains("Key decisions"));
    assert!(prompt.contains("Files created"));
    assert!(prompt.contains("third person"));
    assert!(prompt.contains("ONLY the summary"));
}

#[test]
/// Covers the compaction prompt empty payload still valid regression case.
fn compaction_prompt_empty_payload_still_valid() {
    let prompt = build_compaction_prompt(&empty_payload());
    assert!(prompt.contains("<transcript>"));
    assert!(prompt.contains("</transcript>"));
}

#[test]
/// Covers the compaction prompt with compaction summary regression case.
fn compaction_prompt_with_compaction_summary() {
    let payload = ContextPayload {
        compaction_summary: Some("Previously fixed auth.".into()),
        events: vec![DisplayEvent::UserMessage {
            _uuid: String::new(),
            content: "next task".into(),
        }],
    };
    let prompt = build_compaction_prompt(&payload);
    assert!(prompt.contains("[Previous conversation summary]"));
    assert!(prompt.contains("Previously fixed auth."));
    assert!(prompt.contains("next task"));
}
