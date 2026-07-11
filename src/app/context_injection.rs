//! Context injection for session resumption
//!
//! Builds a conversation transcript from cached DisplayEvents and injects it
//! into prompts so the agent has prior context without needing `--resume`.
//! Also provides stripping logic to remove the injected context from parsed
//! results before appending to the session store.

use crate::app::session_store::ContextPayload;
use crate::events::DisplayEvent;

/// Opening delimiter for Azureal's hidden resumed-session transcript.
pub const CONTEXT_OPEN: &str = "<azureal-session-context>";
/// Closing delimiter for Azureal's hidden resumed-session transcript.
pub const CONTEXT_CLOSE: &str = "</azureal-session-context>";
/// Opening delimiter for a hidden continuation request after compaction.
pub const AUTO_CONTINUE_OPEN: &str = "<azureal-internal-auto-continue>";
/// Closing delimiter for a hidden continuation request after compaction.
pub const AUTO_CONTINUE_CLOSE: &str = "</azureal-internal-auto-continue>";
/// Internal prompt that resumes interrupted work without creating a visible user turn.
pub const AUTO_CONTINUE_PROMPT: &str =
    "<azureal-internal-auto-continue>\nContinue the in-progress work from the supplied Azureal session context. Do not treat this as a new user request, do not change objectives, and keep following the user's latest instructions.\n</azureal-internal-auto-continue>";
/// Prefix used to recognize Codex-injected repository instructions.
const AGENTS_INSTRUCTIONS_PREFIX: &str = "# AGENTS.md instructions for ";
/// Opening delimiter used by Codex for an injected skill definition.
const SKILL_CONTEXT_OPEN: &str = "<skill>";
/// Closing delimiter used by Codex for an injected skill definition.
const SKILL_CONTEXT_CLOSE: &str = "</skill>";

/// Build a context-injected prompt. If the payload has no content, returns
/// the original prompt unchanged.
pub fn build_context_prompt(payload: &ContextPayload, user_prompt: &str) -> String {
    let transcript = build_transcript(payload);
    if transcript.is_empty() {
        return user_prompt.to_string();
    }
    format!("{CONTEXT_OPEN}\n{transcript}\n{CONTEXT_CLOSE}\n\n{user_prompt}")
}

/// Strip injected context from a user message content string.
/// Returns the actual user prompt (everything after the closing tag).
/// If no context tags are found, returns the original content unchanged. If an
/// opening tag is present but the closing tag is missing, the message is treated
/// as internal context and stripped entirely so a malformed/truncated wrapper
/// cannot leak into the visible transcript.
pub fn strip_injected_context(content: &str) -> &str {
    if let Some(close_pos) = content.find(CONTEXT_CLOSE) {
        let after = &content[close_pos + CONTEXT_CLOSE.len()..];
        after.trim_start_matches('\n').trim_start_matches('\n')
    } else if content.contains(CONTEXT_OPEN) {
        ""
    } else {
        content
    }
}

/// Returns whether content contains either Azureal session-context delimiter.
pub fn contains_injected_context(content: &str) -> bool {
    content.contains(CONTEXT_OPEN) || content.contains(CONTEXT_CLOSE)
}

/// Return display/store-safe user content. A normal empty prompt is preserved,
/// but a malformed context wrapper with no recoverable real prompt is dropped.
pub fn sanitize_user_message_content(content: &str) -> Option<String> {
    if is_hidden_codex_context(content) || is_hidden_skill_context(content) {
        return None;
    }
    let stripped = strip_injected_context(content);
    if is_internal_auto_continue_prompt(stripped) {
        return None;
    }
    if stripped.is_empty() && contains_injected_context(content) {
        None
    } else {
        Some(stripped.to_string())
    }
}

/// Codex can emit hidden developer context in its JSONL stream. Older Azureal
/// builds stored those developer messages as visible user messages, so hide
/// AGENTS instruction blocks, including rows truncated before their closing
/// environment-context tag.
pub fn is_hidden_codex_context(content: &str) -> bool {
    let trimmed = content.trim_start();
    trimmed.starts_with(AGENTS_INSTRUCTIONS_PREFIX) && trimmed.contains("<INSTRUCTIONS>")
}

/// Returns whether a user-role item is a standalone injected skill definition.
pub fn is_hidden_skill_context(content: &str) -> bool {
    let trimmed = content.trim();
    trimmed.starts_with(SKILL_CONTEXT_OPEN)
        && trimmed.contains("<name>")
        && (trimmed.ends_with(SKILL_CONTEXT_CLOSE) || trimmed.contains("<path>"))
}

/// Returns whether content is Azureal's hidden post-compaction continuation prompt.
pub fn is_internal_auto_continue_prompt(content: &str) -> bool {
    let trimmed = content.trim();
    trimmed.starts_with(AUTO_CONTINUE_OPEN) && trimmed.ends_with(AUTO_CONTINUE_CLOSE)
}

/// Appends a sanitized non-empty user message while preserving its event identifier.
fn push_stripped_user_message(out: &mut Vec<DisplayEvent>, _uuid: String, content: &str) {
    let Some(stripped) = sanitize_user_message_content(content) else {
        return;
    };
    if !stripped.trim().is_empty() {
        out.push(DisplayEvent::UserMessage {
            _uuid,
            content: stripped,
        });
    }
}

/// Strip injected context from parsed event streams before display or storage.
/// Agent JSONL files record the actual prompt sent to the backend, which may
/// include Azureal's hidden context wrapper. The UI/store should retain only
/// the user's real prompt.
pub fn strip_injected_context_from_events(events: Vec<DisplayEvent>) -> Vec<DisplayEvent> {
    let mut out = Vec::with_capacity(events.len());
    let mut pending_user_messages = Vec::new();
    let mut inside_injected_context = false;

    for event in events {
        match event {
            DisplayEvent::UserMessage { _uuid, content } => {
                if is_hidden_codex_context(&content) || is_hidden_skill_context(&content) {
                    continue;
                }

                if inside_injected_context {
                    if content.contains(CONTEXT_CLOSE) {
                        inside_injected_context = false;
                        push_stripped_user_message(&mut out, _uuid, &content);
                    }
                    continue;
                }

                if contains_injected_context(&content) {
                    // Codex may serialize one prompt as several user/developer
                    // message items. If the context close tag appears in this
                    // contiguous user-message run, all earlier buffered user
                    // items belong to hidden injected context.
                    pending_user_messages.clear();
                    if content.contains(CONTEXT_CLOSE) {
                        push_stripped_user_message(&mut out, _uuid, &content);
                    } else {
                        inside_injected_context = true;
                    }
                    continue;
                }

                pending_user_messages.push(DisplayEvent::UserMessage { _uuid, content });
            }
            other => {
                if !inside_injected_context {
                    out.append(&mut pending_user_messages);
                } else {
                    pending_user_messages.clear();
                    inside_injected_context = false;
                }
                out.push(other);
            }
        }
    }

    if !inside_injected_context {
        out.append(&mut pending_user_messages);
    }

    out
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
        DisplayEvent::UserMessage { content, .. } => sanitize_user_message_content(content)
            .filter(|content| !content.trim().is_empty())
            .map(|content| format!("## User\n{content}\n")),
        DisplayEvent::AssistantText { text, .. } => Some(format!("## Assistant\n{text}\n")),
        DisplayEvent::ToolCall {
            tool_name, input, ..
        } => {
            let param = extract_key_param(tool_name, input);
            if param.is_empty() {
                Some(format!("## Tool: {tool_name}\n"))
            } else {
                Some(format!("## Tool: {tool_name} ({param})\n"))
            }
        }
        DisplayEvent::ToolResult {
            tool_name,
            content,
            is_error,
            ..
        } => {
            let prefix = if *is_error { "Error" } else { "Result" };
            let compact = compact_result(content);
            Some(format!("[{prefix}: {tool_name}] {compact}\n"))
        }
        DisplayEvent::Plan { name, content, .. } => Some(format!("## Plan: {name}\n{content}\n")),
        DisplayEvent::Command { name } => Some(format!("## Command: {name}\n")),
        DisplayEvent::Complete {
            duration_ms,
            cost_usd,
            ..
        } => Some(format!(
            "[Session complete: {:.1}s, ${:.4}]\n",
            *duration_ms as f64 / 1000.0,
            cost_usd
        )),
        // Omit non-content events
        DisplayEvent::Init { .. }
        | DisplayEvent::Hook { .. }
        | DisplayEvent::ModelSwitch { .. }
        | DisplayEvent::Compacting
        | DisplayEvent::Compacted
        | DisplayEvent::MayBeCompacting
        | DisplayEvent::Filtered => None,
    }
}

/// Extract the most relevant parameter from a tool input for the transcript.
fn extract_key_param(tool_name: &str, input: &serde_json::Value) -> String {
    let key = match tool_name {
        "Bash" | "bash" | "Exec" | "exec" => "command",
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
    input
        .get(key)
        .or_else(|| input.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// Compact a tool result to a reasonable length for context injection.
/// Keeps short results intact and preserves both head and tail for long results.
fn compact_result(content: &str) -> String {
    let content = content
        .split("<system-reminder>")
        .next()
        .unwrap_or(content)
        .trim_end();
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= 6 {
        content.to_string()
    } else {
        format!(
            "{}\n(+{} more lines)\n{}",
            lines[..3].join("\n"),
            lines.len() - 6,
            lines[lines.len() - 3..].join("\n")
        )
    }
}

/// Build the prompt for a background compaction agent. The agent receives the
/// older portion of the conversation (everything before the last 3 user
/// exchanges) and returns a summary. Recent exchanges are preserved verbatim.
pub fn build_compaction_prompt(payload: &ContextPayload) -> String {
    let transcript = build_transcript(payload);
    format!(
        "You are summarizing older conversation history for future context injection. \
The transcript below contains messages that precede the most recent exchanges \
(which are preserved verbatim elsewhere). Your summary will replace this older \
content to keep the context window compact.\n\n\
<transcript>\n{transcript}\n</transcript>\n\n\
Produce a concise summary (2000-4000 characters) that preserves:\n\
1. Key decisions made and their rationale\n\
2. Files created, modified, or deleted (with paths)\n\
3. Important technical context (architecture, patterns, constraints)\n\
4. Current state of work at the end of this transcript\n\
5. Unresolved issues or agreed next steps\n\n\
Write in third person, past tense. Focus on information needed to understand \
context that leads into the recent exchanges. Output ONLY the summary text, \
no preamble."
    )
}

/// Regression coverage for context construction and hidden-message filtering.
#[cfg(test)]
#[path = "context_injection_tests.rs"]
mod tests;
