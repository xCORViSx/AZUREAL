//! Context injection for session resumption
//!
//! Builds a conversation transcript from cached DisplayEvents and injects it
//! into prompts so the agent has prior context without needing `--resume`.
//! Also provides stripping logic to remove the injected context from parsed
//! results before appending to the session store.

use crate::app::session_store::ContextPayload;
use crate::events::DisplayEvent;

pub const CONTEXT_OPEN: &str = "<azureal-session-context>";
pub const CONTEXT_CLOSE: &str = "</azureal-session-context>";

/// Build a context-injected prompt. If the payload has no content, returns
/// the original prompt unchanged.
pub fn build_context_prompt(payload: &ContextPayload, user_prompt: &str) -> String {
    let transcript = build_transcript(payload);
    if transcript.is_empty() {
        return user_prompt.to_string();
    }
    format!(
        "{CONTEXT_OPEN}\n{transcript}\n{CONTEXT_CLOSE}\n\n{user_prompt}"
    )
}

/// Strip injected context from a user message content string.
/// Returns the actual user prompt (everything after the closing tag).
/// If no context tags are found, returns the original content unchanged.
pub fn strip_injected_context(content: &str) -> &str {
    if let Some(close_pos) = content.find(CONTEXT_CLOSE) {
        let after = &content[close_pos + CONTEXT_CLOSE.len()..];
        after.trim_start_matches('\n').trim_start_matches('\n')
    } else {
        content
    }
}

/// Build a transcript string from a ContextPayload.
fn build_transcript(payload: &ContextPayload) -> String {
    let mut out = String::new();

    if let Some(ref summary) = payload.compaction_summary {
        out.push_str("[Previous conversation summary]\n");
        out.push_str(summary);
        out.push_str("\n\n[Conversation continues]\n\n");
    }

    for event in &payload.events {
        if let Some(line) = format_event(event) {
            out.push_str(&line);
            out.push('\n');
        }
    }

    out.trim_end().to_string()
}

/// Format a single DisplayEvent into a transcript line for context injection.
fn format_event(event: &DisplayEvent) -> Option<String> {
    match event {
        DisplayEvent::UserMessage { content, .. } => {
            Some(format!("## User\n{content}\n"))
        }
        DisplayEvent::AssistantText { text, .. } => {
            Some(format!("## Assistant\n{text}\n"))
        }
        DisplayEvent::ToolCall { tool_name, input, .. } => {
            let param = extract_key_param(tool_name, input);
            if param.is_empty() {
                Some(format!("## Tool: {tool_name}\n"))
            } else {
                Some(format!("## Tool: {tool_name} ({param})\n"))
            }
        }
        DisplayEvent::ToolResult { tool_name, content, is_error, .. } => {
            let prefix = if *is_error { "Error" } else { "Result" };
            let compact = compact_result(content);
            Some(format!("[{prefix}: {tool_name}] {compact}\n"))
        }
        DisplayEvent::Plan { name, content, .. } => {
            Some(format!("## Plan: {name}\n{content}\n"))
        }
        DisplayEvent::Command { name } => {
            Some(format!("## Command: {name}\n"))
        }
        DisplayEvent::Complete { duration_ms, cost_usd, .. } => {
            Some(format!("[Session complete: {:.1}s, ${:.4}]\n", *duration_ms as f64 / 1000.0, cost_usd))
        }
        // Omit non-content events
        DisplayEvent::Init { .. }
        | DisplayEvent::Hook { .. }
        | DisplayEvent::Compacting
        | DisplayEvent::Compacted
        | DisplayEvent::MayBeCompacting
        | DisplayEvent::Filtered => None,
    }
}

/// Extract the most relevant parameter from a tool input for the transcript.
fn extract_key_param(tool_name: &str, input: &serde_json::Value) -> String {
    let key = match tool_name {
        "Bash" | "bash" => "command",
        "Read" | "read" => "file_path",
        "Edit" | "edit" => "file_path",
        "Write" | "write" => "file_path",
        "Glob" | "glob" => "pattern",
        "Grep" | "grep" => "pattern",
        "WebFetch" | "webfetch" => "url",
        "WebSearch" | "websearch" => "query",
        "Agent" | "agent" | "Task" | "task" => "description",
        "LSP" | "lsp" => "operation",
        _ => "file_path",
    };
    input.get(key)
        .or_else(|| input.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// Compact a tool result to a reasonable length for context injection.
/// Keeps first 3 lines + count for long results.
fn compact_result(content: &str) -> String {
    let content = content.split("<system-reminder>").next().unwrap_or(content).trim_end();
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= 3 {
        content.to_string()
    } else {
        format!("{}\n(+{} more lines)", lines[..3].join("\n"), lines.len() - 3)
    }
}

/// Build the prompt for a background compaction agent. The agent receives the
/// full conversation transcript since the last compaction and returns a summary.
pub fn build_compaction_prompt(payload: &ContextPayload) -> String {
    let transcript = build_transcript(payload);
    format!(
        "You are summarizing a conversation for future context injection. \
The transcript below represents the conversation since the last summary point.\n\n\
<transcript>\n{transcript}\n</transcript>\n\n\
Produce a concise summary (2000-4000 characters) that preserves:\n\
1. Key decisions made and their rationale\n\
2. Files created, modified, or deleted (with paths)\n\
3. Important technical context (architecture, patterns, constraints)\n\
4. Current state of any in-progress work\n\
5. Unresolved issues or agreed next steps\n\n\
Write in third person, past tense. Focus on information needed to continue \
this conversation effectively. Output ONLY the summary text, no preamble."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_payload() -> ContextPayload {
        ContextPayload { compaction_summary: None, events: vec![] }
    }

    fn simple_payload() -> ContextPayload {
        ContextPayload {
            compaction_summary: None,
            events: vec![
                DisplayEvent::UserMessage { _uuid: String::new(), content: "fix the bug".into() },
                DisplayEvent::AssistantText { _uuid: String::new(), _message_id: String::new(), text: "I'll look at it.".into() },
            ],
        }
    }

    // ── build_context_prompt ──

    #[test]
    fn empty_payload_returns_original_prompt() {
        let result = build_context_prompt(&empty_payload(), "hello");
        assert_eq!(result, "hello");
    }

    #[test]
    fn non_empty_payload_wraps_with_tags() {
        let result = build_context_prompt(&simple_payload(), "now fix the tests");
        assert!(result.starts_with(CONTEXT_OPEN));
        assert!(result.contains(CONTEXT_CLOSE));
        assert!(result.ends_with("now fix the tests"));
    }

    #[test]
    fn prompt_appears_after_close_tag() {
        let result = build_context_prompt(&simple_payload(), "my prompt");
        let after_close = result.split(CONTEXT_CLOSE).nth(1).unwrap();
        assert!(after_close.contains("my prompt"));
    }

    #[test]
    fn context_contains_user_message() {
        let result = build_context_prompt(&simple_payload(), "x");
        assert!(result.contains("## User\nfix the bug"));
    }

    #[test]
    fn context_contains_assistant_text() {
        let result = build_context_prompt(&simple_payload(), "x");
        assert!(result.contains("## Assistant\nI'll look at it."));
    }

    // ── strip_injected_context ──

    #[test]
    fn strip_no_context_returns_original() {
        assert_eq!(strip_injected_context("hello world"), "hello world");
    }

    #[test]
    fn strip_with_context_returns_prompt() {
        let injected = format!("{CONTEXT_OPEN}\nsome context\n{CONTEXT_CLOSE}\n\nactual prompt");
        assert_eq!(strip_injected_context(&injected), "actual prompt");
    }

    #[test]
    fn strip_round_trip() {
        let payload = simple_payload();
        let prompt = "do the thing";
        let injected = build_context_prompt(&payload, prompt);
        let stripped = strip_injected_context(&injected);
        assert_eq!(stripped, prompt);
    }

    #[test]
    fn strip_preserves_multiline_prompt() {
        let prompt = "line 1\nline 2\nline 3";
        let injected = format!("{CONTEXT_OPEN}\nctx\n{CONTEXT_CLOSE}\n\n{prompt}");
        assert_eq!(strip_injected_context(&injected), prompt);
    }

    // ── format_event ──

    #[test]
    fn format_user_message() {
        let ev = DisplayEvent::UserMessage { _uuid: String::new(), content: "hello".into() };
        let line = format_event(&ev).unwrap();
        assert!(line.starts_with("## User\n"));
        assert!(line.contains("hello"));
    }

    #[test]
    fn format_assistant_text() {
        let ev = DisplayEvent::AssistantText { _uuid: String::new(), _message_id: String::new(), text: "hi".into() };
        let line = format_event(&ev).unwrap();
        assert!(line.starts_with("## Assistant\n"));
    }

    #[test]
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
    fn format_init_returns_none() {
        let ev = DisplayEvent::Init { _session_id: String::new(), cwd: String::new(), model: String::new() };
        assert!(format_event(&ev).is_none());
    }

    #[test]
    fn format_filtered_returns_none() {
        assert!(format_event(&DisplayEvent::Filtered).is_none());
    }

    #[test]
    fn format_compacting_returns_none() {
        assert!(format_event(&DisplayEvent::Compacting).is_none());
    }

    #[test]
    fn format_hook_returns_none() {
        let ev = DisplayEvent::Hook { name: "x".into(), output: "y".into() };
        assert!(format_event(&ev).is_none());
    }

    #[test]
    fn format_plan() {
        let ev = DisplayEvent::Plan { name: "refactor".into(), content: "step 1".into() };
        let line = format_event(&ev).unwrap();
        assert!(line.contains("## Plan: refactor"));
        assert!(line.contains("step 1"));
    }

    #[test]
    fn format_command() {
        let ev = DisplayEvent::Command { name: "/compact".into() };
        let line = format_event(&ev).unwrap();
        assert!(line.contains("## Command: /compact"));
    }

    #[test]
    fn format_complete() {
        let ev = DisplayEvent::Complete {
            _session_id: String::new(), success: true, duration_ms: 5000, cost_usd: 0.05
        };
        let line = format_event(&ev).unwrap();
        assert!(line.contains("5.0s"));
        assert!(line.contains("$0.0500"));
    }

    // ── compact_result ──

    #[test]
    fn compact_result_short() {
        assert_eq!(compact_result("one\ntwo\nthree"), "one\ntwo\nthree");
    }

    #[test]
    fn compact_result_long() {
        let long = "a\nb\nc\nd\ne\nf";
        let result = compact_result(long);
        assert!(result.contains("a\nb\nc"));
        assert!(result.contains("(+3 more lines)"));
    }

    #[test]
    fn compact_result_strips_system_reminder() {
        let content = "actual content<system-reminder>secret stuff</system-reminder>";
        assert_eq!(compact_result(content), "actual content");
    }

    // ── extract_key_param ──

    #[test]
    fn extract_key_param_bash() {
        let input = serde_json::json!({"command": "cargo test"});
        assert_eq!(extract_key_param("Bash", &input), "cargo test");
    }

    #[test]
    fn extract_key_param_read() {
        let input = serde_json::json!({"file_path": "/src/main.rs"});
        assert_eq!(extract_key_param("Read", &input), "/src/main.rs");
    }

    #[test]
    fn extract_key_param_grep() {
        let input = serde_json::json!({"pattern": "fn main"});
        assert_eq!(extract_key_param("Grep", &input), "fn main");
    }

    #[test]
    fn extract_key_param_missing() {
        let input = serde_json::json!({});
        assert_eq!(extract_key_param("Read", &input), "");
    }

    #[test]
    fn extract_key_param_path_fallback() {
        let input = serde_json::json!({"path": "/fallback"});
        assert_eq!(extract_key_param("Unknown", &input), "/fallback");
    }

    // ── build_transcript with compaction ──

    #[test]
    fn transcript_with_compaction_summary() {
        let payload = ContextPayload {
            compaction_summary: Some("Previously: fixed auth bug, added tests.".into()),
            events: vec![
                DisplayEvent::UserMessage { _uuid: String::new(), content: "now what?".into() },
            ],
        };
        let transcript = build_transcript(&payload);
        assert!(transcript.contains("[Previous conversation summary]"));
        assert!(transcript.contains("fixed auth bug"));
        assert!(transcript.contains("[Conversation continues]"));
        assert!(transcript.contains("## User\nnow what?"));
    }

    #[test]
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
    fn transcript_no_compaction_no_events_empty() {
        let transcript = build_transcript(&empty_payload());
        assert!(transcript.is_empty());
    }

    // ── build_compaction_prompt ──

    #[test]
    fn compaction_prompt_contains_transcript() {
        let prompt = build_compaction_prompt(&simple_payload());
        assert!(prompt.contains("<transcript>"));
        assert!(prompt.contains("</transcript>"));
        assert!(prompt.contains("fix the bug"));
    }

    #[test]
    fn compaction_prompt_instructions() {
        let prompt = build_compaction_prompt(&simple_payload());
        assert!(prompt.contains("Key decisions"));
        assert!(prompt.contains("Files created"));
        assert!(prompt.contains("third person"));
        assert!(prompt.contains("ONLY the summary"));
    }

    #[test]
    fn compaction_prompt_empty_payload_still_valid() {
        let prompt = build_compaction_prompt(&empty_payload());
        assert!(prompt.contains("<transcript>"));
        assert!(prompt.contains("</transcript>"));
    }

    #[test]
    fn compaction_prompt_with_compaction_summary() {
        let payload = ContextPayload {
            compaction_summary: Some("Previously fixed auth.".into()),
            events: vec![
                DisplayEvent::UserMessage { _uuid: String::new(), content: "next task".into() },
            ],
        };
        let prompt = build_compaction_prompt(&payload);
        assert!(prompt.contains("[Previous conversation summary]"));
        assert!(prompt.contains("Previously fixed auth."));
        assert!(prompt.contains("next task"));
    }
}
