//! Claude session file parsing
//!
//! Parses Claude's JSONL session files into DisplayEvents for the TUI.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use chrono::{DateTime, Duration, Utc};

use crate::events::DisplayEvent;

/// Result of parsing a Claude session file
pub struct ParsedSession {
    pub events: Vec<DisplayEvent>,
    pub pending_tools: HashSet<String>,
    pub failed_tools: HashSet<String>,
    /// Number of lines in the JSONL file
    pub total_lines: usize,
    /// Number of lines that failed to parse
    pub parse_errors: usize,
    /// Diagnostics about assistant event parsing
    pub assistant_total: usize,
    pub assistant_no_message: usize,
    pub assistant_no_content_arr: usize,
    pub assistant_text_blocks: usize,
    /// True if ExitPlanMode was called and no user message followed
    pub awaiting_plan_approval: bool,
    /// Byte offset after the last successfully parsed line (for incremental parsing)
    pub end_offset: u64,
    /// Latest token usage from most recent assistant event: (context_tokens, output_tokens)
    /// context_tokens = input_tokens + cache_read + cache_creation (effective context size)
    pub session_tokens: Option<(u64, u64)>,
    /// Model context window size detected from assistant events' message.model field
    pub context_window: Option<u64>,
}

/// Persistent parser state that survives between incremental parses.
/// Tracks tool_call IDs → names so tool_results from new lines can resolve them,
/// plus parentUuid dedup maps for user-message rewrites.
pub struct IncrementalParserState {
    pub tool_calls: HashMap<String, (String, Option<String>)>,
    pub user_msg_by_parent: HashMap<String, (usize, DateTime<Utc>)>,
    pub session_slug: Option<String>,
}

impl IncrementalParserState {
    /// Rebuild parser context from existing display_events (cheap — just scans tool_call IDs)
    pub fn from_events(events: &[DisplayEvent], slug: Option<String>) -> Self {
        let mut tool_calls = HashMap::new();
        let user_msg_by_parent = HashMap::new();

        for event in events {
            if let DisplayEvent::ToolCall { tool_use_id, tool_name, file_path, .. } = event {
                tool_calls.insert(tool_use_id.clone(), (tool_name.clone(), file_path.clone()));
            }
        }

        Self { tool_calls, user_msg_by_parent, session_slug: slug }
    }
}

/// Parse a Claude session JSONL file into display events (full parse from byte 0)
pub fn parse_session_file(session_file: &Path) -> ParsedSession {
    parse_session_file_from(session_file, 0, None)
}

/// Incrementally parse only new lines appended after `start_offset`.
/// `existing` provides the already-parsed events and state to append to.
/// Falls back to full re-parse if the file shrank or offset is 0.
pub fn parse_session_file_incremental(
    session_file: &Path,
    start_offset: u64,
    existing_events: &[DisplayEvent],
    existing_pending: &HashSet<String>,
    existing_failed: &HashSet<String>,
) -> ParsedSession {
    // If offset is 0 or file doesn't exist, do a full parse
    if start_offset == 0 {
        return parse_session_file(session_file);
    }

    // Check file size — if it shrank, full re-parse (shouldn't happen with JSONL append-only)
    let file_len = std::fs::metadata(session_file).map(|m| m.len()).unwrap_or(0);
    if file_len < start_offset {
        return parse_session_file(session_file);
    }

    // Nothing new to parse
    if file_len == start_offset {
        return ParsedSession {
            events: existing_events.to_vec(),
            pending_tools: existing_pending.clone(),
            failed_tools: existing_failed.clone(),
            total_lines: 0,
            parse_errors: 0,
            assistant_total: 0,
            assistant_no_message: 0,
            assistant_no_content_arr: 0,
            assistant_text_blocks: 0,
            awaiting_plan_approval: check_plan_approval(existing_events),
            end_offset: start_offset,
            session_tokens: None,
            context_window: None,
        };
    }

    // Rebuild parser context from existing events so tool_results can resolve tool names
    // Slug is None for incremental path — plan file loading only matters on full parse
    let state = IncrementalParserState::from_events(existing_events, None);

    // Parse only new bytes
    let result = parse_session_file_from(session_file, start_offset, Some(state));

    // "user" events with parentUuid dedup can rewrite earlier events.
    // If the new parse produced any Filtered events (user-message rewrites referencing old indices),
    // we need a full re-parse to handle the cross-reference correctly.
    // This is rare (only when user edits/rewinds a message).
    let has_user_rewrite = result.events.iter().any(|e| matches!(e, DisplayEvent::Filtered));
    if has_user_rewrite {
        return parse_session_file(session_file);
    }

    // Merge: existing events + newly parsed events
    let mut merged_events = existing_events.to_vec();
    merged_events.extend(result.events);

    let mut merged_pending = existing_pending.clone();
    // Remove tools that got results in the new batch
    for id in existing_pending {
        if !result.pending_tools.contains(id) {
            // Check if a ToolResult appeared for this tool in the new events
            let got_result = merged_events.iter().any(|e| {
                matches!(e, DisplayEvent::ToolResult { tool_use_id, .. } if tool_use_id == id)
            });
            if got_result { merged_pending.remove(id); }
        }
    }
    merged_pending.extend(result.pending_tools);

    let mut merged_failed = existing_failed.clone();
    merged_failed.extend(result.failed_tools);

    ParsedSession {
        awaiting_plan_approval: check_plan_approval(&merged_events),
        events: merged_events,
        pending_tools: merged_pending,
        failed_tools: merged_failed,
        total_lines: result.total_lines,
        parse_errors: result.parse_errors,
        assistant_total: result.assistant_total,
        assistant_no_message: result.assistant_no_message,
        assistant_no_content_arr: result.assistant_no_content_arr,
        assistant_text_blocks: result.assistant_text_blocks,
        end_offset: result.end_offset,
        // Use new parse's tokens if present, otherwise keep None (no assistant events in this batch)
        session_tokens: result.session_tokens,
        context_window: result.context_window,
    }
}

/// Check if awaiting plan approval across a set of events
fn check_plan_approval(events: &[DisplayEvent]) -> bool {
    let mut saw_exit_plan = false;
    let mut saw_user_after = false;
    for event in events {
        match event {
            DisplayEvent::ToolCall { tool_name, .. } if tool_name == "ExitPlanMode" => {
                saw_exit_plan = true;
                saw_user_after = false;
            }
            DisplayEvent::UserMessage { .. } if saw_exit_plan => {
                saw_user_after = true;
            }
            _ => {}
        }
    }
    saw_exit_plan && !saw_user_after
}

/// Core parser: reads JSONL starting at `start_offset` bytes.
/// If `prior_state` is Some, uses it for tool_call resolution (incremental mode).
fn parse_session_file_from(
    session_file: &Path,
    start_offset: u64,
    prior_state: Option<IncrementalParserState>,
) -> ParsedSession {
    PARSE_DIAGNOSTICS.with(|d| *d.borrow_mut() = ParseDiagnostics::default());

    let file = match File::open(session_file) {
        Ok(f) => f,
        Err(_) => return ParsedSession {
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
        },
    };

    // Seek to start_offset if nonzero
    use std::io::Seek;
    let mut file = file;
    if start_offset > 0 {
        if file.seek(std::io::SeekFrom::Start(start_offset)).is_err() {
            return parse_session_file(session_file);
        }
    }

    let mut reader = BufReader::new(&mut file);
    let mut timed_events: Vec<(DateTime<Utc>, DisplayEvent)> = Vec::new();

    // Use prior state if provided (incremental), otherwise start fresh
    let (mut tool_calls, mut user_msg_by_parent, mut session_slug) = match prior_state {
        Some(s) => (s.tool_calls, s.user_msg_by_parent, s.session_slug),
        None => (HashMap::new(), HashMap::new(), None),
    };

    let mut pending_tools: HashSet<String> = HashSet::new();
    let mut failed_tools: HashSet<String> = HashSet::new();
    let mut last_user_msg: Option<(usize, DateTime<Utc>)> = None;
    let mut ups_hooks: Vec<(usize, DateTime<Utc>, DisplayEvent)> = Vec::new();
    let mut total_lines = 0;
    let mut parse_errors = 0;
    let mut bytes_read: u64 = 0;
    // Tracks latest assistant event's token usage (overwritten each time, last wins)
    let mut session_tokens: Option<(u64, u64)> = None;
    // Context window detected from model string (overwritten each assistant event, last wins)
    let mut context_window: Option<u64> = None;

    // Read line-by-line tracking byte offset
    let mut line_buf = String::new();
    loop {
        line_buf.clear();
        let n = match reader.read_line(&mut line_buf) {
            Ok(n) => n,
            Err(_) => break,
        };
        if n == 0 { break; }
        bytes_read += n as u64;

        let line = line_buf.trim_end_matches('\n').trim_end_matches('\r');
        if line.is_empty() { continue; }

        total_lines += 1;
        let Ok(json) = serde_json::from_str::<serde_json::Value>(line) else {
            parse_errors += 1;
            continue;
        };

        let timestamp = json.get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        let event_type = json.get("type").and_then(|t| t.as_str()).unwrap_or("");

        if session_slug.is_none() {
            session_slug = json.get("slug").and_then(|s| s.as_str()).map(|s| s.to_string());
        }

        match event_type {
            "user" => parse_user_event(
                &json, timestamp, &mut timed_events, &mut user_msg_by_parent,
                &tool_calls, &mut pending_tools, &mut failed_tools,
                &mut last_user_msg, &mut ups_hooks,
                session_slug.as_deref(),
            ),
            "assistant" => {
                parse_assistant_event(
                    &json, timestamp, &mut timed_events, &mut tool_calls, &mut pending_tools,
                    &mut session_tokens, &mut context_window,
                );
            }
            "result" => parse_result_event(&json, timestamp, &mut timed_events, &mut context_window),
            "system" => parse_system_event(&json, timestamp, &mut timed_events),
            "progress" => parse_progress_event(&json, timestamp, &mut timed_events),
            _ => {}
        }
    }

    // Insert UPS hooks at proper positions
    for (_idx, ts, hook_event) in ups_hooks {
        timed_events.push((ts, hook_event));
    }

    // Filter out Filtered events and extract just the DisplayEvents
    let events: Vec<DisplayEvent> = timed_events.into_iter()
        .filter(|(_, e)| !matches!(e, DisplayEvent::Filtered))
        .map(|(_, e)| e)
        .collect();

    let awaiting_plan_approval = check_plan_approval(&events);

    let (ast_total, ast_no_msg, ast_no_arr, ast_text) = PARSE_DIAGNOSTICS.with(|d| {
        let d = d.borrow();
        (d.assistant_events_total, d.assistant_events_no_message, d.assistant_events_no_content_arr, d.assistant_text_blocks)
    });

    ParsedSession {
        events,
        pending_tools,
        failed_tools,
        total_lines,
        parse_errors,
        assistant_total: ast_total,
        assistant_no_message: ast_no_msg,
        assistant_no_content_arr: ast_no_arr,
        assistant_text_blocks: ast_text,
        awaiting_plan_approval,
        end_offset: start_offset + bytes_read,
        session_tokens,
        context_window,
    }
}

/// Extract hook events from system-reminder tags in content
pub fn extract_hooks_from_content(content: &str, timestamp: DateTime<Utc>) -> Vec<(DateTime<Utc>, DisplayEvent)> {
    let mut hooks = Vec::new();
    let mut search_start = 0;

    while let Some(start) = content[search_start..].find("<system-reminder>") {
        let abs_start = search_start + start + 17;
        if let Some(end) = content[abs_start..].find("</system-reminder>") {
            let reminder_content = &content[abs_start..abs_start + end];

            if let Some(hook_pos) = reminder_content.find(" hook success:") {
                let name = reminder_content[..hook_pos]
                    .trim()
                    .trim_start_matches("\\n")
                    .trim_end_matches("\\n")
                    .to_string();
                let output = reminder_content[hook_pos + 14..]
                    .trim()
                    .trim_start_matches("\\n")
                    .trim_end_matches("\\n")
                    .to_string();

                if !output.is_empty() && output != "..." && !name.is_empty() {
                    hooks.push((timestamp, DisplayEvent::Hook { name, output }));
                } else if output == "..." && !name.is_empty() {
                    hooks.push((timestamp, DisplayEvent::Hook { name: name.clone(), output: format!("[{}]", name) }));
                }
            } else if let Some(hook_pos) = reminder_content.find(" hook failed:") {
                let name = reminder_content[..hook_pos]
                    .trim()
                    .trim_start_matches("\\n")
                    .trim_end_matches("\\n")
                    .to_string();
                let output = reminder_content[hook_pos + 13..]
                    .trim()
                    .trim_start_matches("\\n")
                    .trim_end_matches("\\n")
                    .to_string();
                if !name.is_empty() {
                    hooks.push((timestamp, DisplayEvent::Hook { name, output: format!("FAILED: {}", output) }));
                }
            }
            search_start = abs_start + end + 18;
        } else {
            break;
        }
    }
    hooks
}

fn parse_user_event(
    json: &serde_json::Value,
    timestamp: DateTime<Utc>,
    events: &mut Vec<(DateTime<Utc>, DisplayEvent)>,
    user_msg_by_parent: &mut HashMap<String, (usize, DateTime<Utc>)>,
    tool_calls: &HashMap<String, (String, Option<String>)>,
    pending_tools: &mut HashSet<String>,
    failed_tools: &mut HashSet<String>,
    last_user_msg: &mut Option<(usize, DateTime<Utc>)>,
    ups_hooks: &mut Vec<(usize, DateTime<Utc>, DisplayEvent)>,
    session_slug: Option<&str>,
) {
    let message = json.get("message");
    let content_val = message.and_then(|m| m.get("content"));
    let is_meta = json.get("isMeta").and_then(|m| m.as_bool()).unwrap_or(false);

    let content_str = if let Some(s) = content_val.and_then(|c| c.as_str()) {
        Some(s.to_string())
    } else if let Some(arr) = content_val.and_then(|c| c.as_array()) {
        Some(arr.iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"))
    } else {
        None
    };

    let is_compaction_summary = content_str.as_ref()
        .map(|c| c.starts_with("This session is being continued from a previous conversation"))
        .unwrap_or(false);

    if is_compaction_summary {
        events.push((timestamp, DisplayEvent::Compacting));
        return;
    }

    if let Some(ref content) = content_str {
        for hook in extract_hooks_from_content(content, timestamp) {
            events.push(hook);
        }
    }

    if is_meta { return; }

    if let Some(content) = content_val.and_then(|c| c.as_str()) {
        if content.contains("<local-command-caveat>") { return; }

        if content.contains("<local-command-stdout>") {
            if content.contains("Compacted") {
                events.push((timestamp, DisplayEvent::Compacted));
            }
            return;
        }

        if content.starts_with("<command-name>") {
            if let Some(end) = content.find("</command-name>") {
                let cmd = &content[14..end];
                events.push((timestamp, DisplayEvent::Command { name: cmd.to_string() }));
                return;
            }
        }

        let parent_uuid = json.get("parentUuid").and_then(|p| p.as_str()).unwrap_or("").to_string();
        let event_idx = events.len();

        if !parent_uuid.is_empty() {
            if let Some((old_idx, old_ts)) = user_msg_by_parent.get(&parent_uuid) {
                if timestamp > *old_ts {
                    events[*old_idx] = (DateTime::<Utc>::MIN_UTC, DisplayEvent::Filtered);
                    user_msg_by_parent.insert(parent_uuid, (event_idx, timestamp));
                } else {
                    return;
                }
            } else {
                user_msg_by_parent.insert(parent_uuid, (event_idx, timestamp));
            }
        }

        *last_user_msg = Some((events.len(), timestamp));
        events.push((timestamp, DisplayEvent::UserMessage {
            uuid: json.get("uuid").and_then(|u| u.as_str()).unwrap_or("").to_string(),
            content: content.to_string(),
        }));
    } else if let Some(content_arr) = content_val.and_then(|c| c.as_array()) {
        for block in content_arr {
            if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                parse_tool_result_block(
                    block, timestamp, events, tool_calls, pending_tools, failed_tools,
                    last_user_msg, ups_hooks, session_slug,
                );
            }
        }
    }
}

fn parse_tool_result_block(
    block: &serde_json::Value,
    timestamp: DateTime<Utc>,
    events: &mut Vec<(DateTime<Utc>, DisplayEvent)>,
    tool_calls: &HashMap<String, (String, Option<String>)>,
    pending_tools: &mut HashSet<String>,
    failed_tools: &mut HashSet<String>,
    last_user_msg: &Option<(usize, DateTime<Utc>)>,
    ups_hooks: &mut Vec<(usize, DateTime<Utc>, DisplayEvent)>,
    session_slug: Option<&str>,
) {
    let tool_use_id = block.get("tool_use_id").and_then(|i| i.as_str()).unwrap_or("").to_string();
    let (tool_name, file_path) = tool_calls.get(&tool_use_id).cloned().unwrap_or(("Unknown".to_string(), None));

    let content = if let Some(s) = block.get("content").and_then(|c| c.as_str()) {
        s.to_string()
    } else if let Some(arr) = block.get("content").and_then(|c| c.as_array()) {
        arr.iter()
            .filter_map(|b| {
                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                    b.get("text").and_then(|t| t.as_str())
                } else { None }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    };

    pending_tools.remove(&tool_use_id);

    // Check for error conditions
    let is_error = match tool_name.as_str() {
        "Read" | "Write" | "Edit" | "Glob" | "Grep" => {
            let first = content.lines().next().unwrap_or("").to_lowercase();
            first.starts_with("error") || first.contains("enoent")
                || first.contains("file does not exist")
                || first.contains("does not exist")
                || first.contains("<tool_use_error>")
        }
        "Bash" => content.lines().any(|line| {
            let l = line.to_lowercase();
            l.contains(": no such file") || l.contains(": permission denied")
                || l.contains(": command not found")
                || ((l.contains("exit code") || l.contains("exit status"))
                    && !l.ends_with("0") && !l.ends_with("0\n"))
        }),
        "WebFetch" => {
            let first = content.lines().next().unwrap_or("").to_lowercase();
            first.contains("status code 4") || first.contains("status code 5")
                || first.contains("failed") || first.starts_with("error")
        }
        _ => {
            let first = content.lines().next().unwrap_or("").to_lowercase();
            first.starts_with("error")
        }
    };

    if is_error {
        failed_tools.insert(tool_use_id.clone());
    }

    // Extract hooks from content
    let extracted = extract_hooks_from_content(&content, timestamp);
    for hook in extracted {
        if let (_, DisplayEvent::Hook { ref name, .. }) = &hook {
            if name == "UserPromptSubmit" {
                if let Some((idx, user_ts)) = last_user_msg {
                    let hook_ts = *user_ts + Duration::milliseconds(1);
                    ups_hooks.push((*idx, hook_ts, hook.1.clone()));
                }
                continue;
            }
        }
        events.push(hook);
    }

    // Check if this is a Write to a plan file before moving values
    let is_plan_write = tool_name == "Write" && file_path.as_ref()
        .map(|p| p.contains("/.claude/plans/") && p.ends_with(".md"))
        .unwrap_or(false);

    if !content.is_empty() {
        events.push((timestamp, DisplayEvent::ToolResult {
            tool_use_id,
            tool_name,
            file_path,
            content,
        }));
    }

    // Insert plan content after successful Write to plan file (show every plan for full history)
    if is_plan_write && !is_error {
        if let Some(slug) = session_slug {
            if let Some(plan_event) = load_plan_file(slug) {
                events.push((timestamp, plan_event));
            }
        }
    }
}

/// Track assistant parsing issues for diagnostics
#[derive(Default)]
pub struct ParseDiagnostics {
    pub assistant_events_total: usize,
    pub assistant_events_no_message: usize,
    pub assistant_events_no_content_arr: usize,
    pub assistant_text_blocks: usize,
}

thread_local! {
    pub static PARSE_DIAGNOSTICS: std::cell::RefCell<ParseDiagnostics> = std::cell::RefCell::new(ParseDiagnostics::default());
}

/// Map a Claude model string to its context window size in tokens.
/// Model strings from message.model look like "claude-opus-4-6", "claude-sonnet-4-5-20250929", etc.
/// Falls back to 200k for unknown models — safe default since all Claude models have at least 200k.
pub fn context_window_for_model(model: &str) -> u64 {
    // Claude 3.5 and earlier: 200k across the board
    // Claude 4.x family: default 200k, but Opus 4.6 and Sonnet 4.5 support 1M (beta)
    // We use 200k as default since 1M beta requires special access and we auto-detect
    // via actual token counts if they exceed 200k (see draw_output.rs)
    if model.contains("opus-4-6") { return 200_000; }
    if model.contains("sonnet-4-5") { return 200_000; }
    if model.contains("haiku-4-5") { return 200_000; }
    if model.contains("sonnet-4-") { return 200_000; }
    if model.contains("opus-4-") { return 200_000; }
    // Claude 3.x family
    if model.contains("claude-3") { return 200_000; }
    // Unknown model — safe default
    200_000
}

fn parse_assistant_event(
    json: &serde_json::Value,
    timestamp: DateTime<Utc>,
    events: &mut Vec<(DateTime<Utc>, DisplayEvent)>,
    tool_calls: &mut HashMap<String, (String, Option<String>)>,
    pending_tools: &mut HashSet<String>,
    session_tokens: &mut Option<(u64, u64)>,
    context_window: &mut Option<u64>,
) {
    PARSE_DIAGNOSTICS.with(|d| d.borrow_mut().assistant_events_total += 1);

    let Some(message) = json.get("message") else {
        PARSE_DIAGNOSTICS.with(|d| d.borrow_mut().assistant_events_no_message += 1);
        return;
    };
    let Some(content_arr) = message.get("content").and_then(|c| c.as_array()) else {
        PARSE_DIAGNOSTICS.with(|d| d.borrow_mut().assistant_events_no_content_arr += 1);
        return;
    };

    // Extract token usage — each assistant event overwrites previous (last = most recent context)
    if let Some(usage) = message.get("usage") {
        let input = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let output = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let cache_read = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let cache_create = usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        *session_tokens = Some((input + cache_read + cache_create, output));
    }

    // Extract model string → context window size (each assistant event has message.model)
    if let Some(model) = message.get("model").and_then(|m| m.as_str()) {
        *context_window = Some(context_window_for_model(model));
    }

    for block in content_arr {
        let Some(block_type) = block.get("type").and_then(|t| t.as_str()) else { continue };

        match block_type {
            "thinking" => {
                if let Some(thinking) = block.get("thinking").and_then(|t| t.as_str()) {
                    for hook in extract_hooks_from_content(thinking, timestamp) {
                        events.push(hook);
                    }
                }
            }
            "text" => {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    PARSE_DIAGNOSTICS.with(|d| d.borrow_mut().assistant_text_blocks += 1);
                    events.push((timestamp, DisplayEvent::AssistantText {
                        uuid: json.get("uuid").and_then(|u| u.as_str()).unwrap_or("").to_string(),
                        message_id: message.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string(),
                        text: text.to_string(),
                    }));
                }
            }
            "tool_use" => {
                let tool_name = block.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                let tool_id = block.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                let input = block.get("input").cloned().unwrap_or(serde_json::Value::Null);
                let file_path = input.get("file_path").or(input.get("path"))
                    .and_then(|p| p.as_str()).map(|s| s.to_string());

                tool_calls.insert(tool_id.clone(), (tool_name.clone(), file_path.clone()));
                pending_tools.insert(tool_id.clone());

                events.push((timestamp, DisplayEvent::ToolCall {
                    uuid: json.get("uuid").and_then(|u| u.as_str()).unwrap_or("").to_string(),
                    tool_use_id: tool_id,
                    tool_name,
                    file_path,
                    input,
                }));
            }
            _ => {}
        }
    }
}

fn parse_result_event(
    json: &serde_json::Value,
    timestamp: DateTime<Utc>,
    events: &mut Vec<(DateTime<Utc>, DisplayEvent)>,
    context_window: &mut Option<u64>,
) {
    if let Some(duration) = json.get("durationMs").and_then(|d| d.as_f64()) {
        let cost = json.get("costUsd").and_then(|c| c.as_f64()).unwrap_or(0.0);
        events.push((timestamp, DisplayEvent::Complete {
            session_id: json.get("sessionId").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            duration_ms: duration as u64,
            cost_usd: cost,
            success: true,
        }));
    }
    // modelUsage contains the authoritative contextWindow from the API — overrides
    // the heuristic from context_window_for_model(). Session JSONL uses camelCase.
    if let Some(obj) = json.get("modelUsage").and_then(|v| v.as_object()) {
        for (_model, usage) in obj {
            if let Some(cw) = usage.get("contextWindow").and_then(|v| v.as_u64()) {
                *context_window = Some(cw);
            }
        }
    }
}

fn parse_system_event(
    json: &serde_json::Value,
    timestamp: DateTime<Utc>,
    events: &mut Vec<(DateTime<Utc>, DisplayEvent)>,
) {
    let subtype = json.get("subtype").and_then(|s| s.as_str()).unwrap_or("");
    if subtype == "local_command" {
        if let Some(content) = json.get("content").and_then(|c| c.as_str()) {
            if content.starts_with("<command-name>") {
                if let Some(end) = content.find("</command-name>") {
                    let cmd = &content[14..end];
                    events.push((timestamp, DisplayEvent::Command { name: cmd.to_string() }));
                }
            }
        }
    }
}

fn parse_progress_event(
    json: &serde_json::Value,
    timestamp: DateTime<Utc>,
    events: &mut Vec<(DateTime<Utc>, DisplayEvent)>,
) {
    let Some(data) = json.get("data") else { return };
    if data.get("type").and_then(|t| t.as_str()) != Some("hook_progress") { return; }

    let hook_name = data.get("hookName")
        .or_else(|| data.get("hookEvent"))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let command = data.get("command").and_then(|c| c.as_str()).unwrap_or("");

    if hook_name.is_empty() { return; }

    let output = if command.starts_with("echo '") && command.ends_with('\'') {
        command[6..command.len()-1].to_string()
    } else if command.starts_with("echo \"") && command.ends_with('"') {
        command[6..command.len()-1].to_string()
    } else if command.contains("; echo \"$OUT\"") || command.contains("; echo '$OUT'") {
        if let Some(start) = command.find("OUT='") {
            let rest = &command[start + 5..];
            if let Some(end) = rest.find('\'') {
                rest[..end].to_string()
            } else { String::new() }
        } else if let Some(start) = command.find("OUT=\"") {
            let rest = &command[start + 5..];
            if let Some(end) = rest.find('"') {
                rest[..end].to_string()
            } else { String::new() }
        } else { String::new() }
    } else { String::new() };

    // Always show hooks - use [hookName] as fallback when no output extracted
    let display_output = if output.is_empty() {
        format!("[{}]", hook_name)
    } else {
        output
    };
    events.push((timestamp, DisplayEvent::Hook { name: hook_name, output: display_output }));
}

/// Load plan file from ~/.claude/plans/{slug}.md
fn load_plan_file(slug: &str) -> Option<DisplayEvent> {
    let plans_dir = dirs::home_dir()?.join(".claude").join("plans");
    let plan_path = plans_dir.join(format!("{}.md", slug));

    if plan_path.exists() {
        let content = std::fs::read_to_string(&plan_path).ok()?;
        // Extract plan name from first line (# Plan: Name or just # Title)
        let name = content.lines()
            .next()
            .and_then(|line| line.strip_prefix("# Plan: ").or_else(|| line.strip_prefix("# ")))
            .unwrap_or(slug)
            .to_string();

        Some(DisplayEvent::Plan { name, content })
    } else {
        None
    }
}
