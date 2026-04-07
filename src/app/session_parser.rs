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
    /// Model string from the last assistant event (e.g. "claude-opus-4-6")
    pub model: Option<String>,
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

        Self {
            tool_calls,
            user_msg_by_parent,
            session_slug: slug,
        }
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
    let file_len = std::fs::metadata(session_file)
        .map(|m| m.len())
        .unwrap_or(0);
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
            model: None,
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
    let has_user_rewrite = result
        .events
        .iter()
        .any(|e| matches!(e, DisplayEvent::Filtered));
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
            let got_result = merged_events.iter().any(
                |e| matches!(e, DisplayEvent::ToolResult { tool_use_id, .. } if tool_use_id == id),
            );
            if got_result {
                merged_pending.remove(id);
            }
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
        model: result.model,
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
        Err(_) => {
            return ParsedSession {
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
                model: None,
            }
        }
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
    // Model string from the last assistant event (e.g. "claude-opus-4-6")
    let mut model: Option<String> = None;
    // Track active Agent/Task tool IDs to suppress sub-agent prompt user events
    let mut active_agent_tool_ids: HashSet<String> = HashSet::new();

    // Read line-by-line tracking byte offset
    let mut line_buf = String::new();
    loop {
        line_buf.clear();
        let n = match reader.read_line(&mut line_buf) {
            Ok(n) => n,
            Err(_) => break,
        };
        if n == 0 {
            break;
        }
        bytes_read += n as u64;

        let line = line_buf.trim_end_matches('\n').trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }

        total_lines += 1;
        let Ok(json) = serde_json::from_str::<serde_json::Value>(line) else {
            parse_errors += 1;
            continue;
        };

        let timestamp = json
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        let event_type = json.get("type").and_then(|t| t.as_str()).unwrap_or("");

        if session_slug.is_none() {
            session_slug = json
                .get("slug")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string());
        }

        match event_type {
            "user" => parse_user_event(
                &json,
                timestamp,
                &mut timed_events,
                &mut user_msg_by_parent,
                &tool_calls,
                &mut pending_tools,
                &mut failed_tools,
                &mut last_user_msg,
                &mut ups_hooks,
                session_slug.as_deref(),
                &mut active_agent_tool_ids,
            ),
            "assistant" => {
                parse_assistant_event(
                    &json,
                    timestamp,
                    &mut timed_events,
                    &mut tool_calls,
                    &mut pending_tools,
                    &mut model,
                    &mut active_agent_tool_ids,
                );
            }
            "result" => parse_result_event(&json, timestamp, &mut timed_events),
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
    let events: Vec<DisplayEvent> = timed_events
        .into_iter()
        .filter(|(_, e)| !matches!(e, DisplayEvent::Filtered))
        .map(|(_, e)| e)
        .collect();

    let awaiting_plan_approval = check_plan_approval(&events);

    let (ast_total, ast_no_msg, ast_no_arr, ast_text) = PARSE_DIAGNOSTICS.with(|d| {
        let d = d.borrow();
        (
            d.assistant_events_total,
            d.assistant_events_no_message,
            d.assistant_events_no_content_arr,
            d.assistant_text_blocks,
        )
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
        model,
    }
}

/// Extract hook events from system-reminder tags in content
pub fn extract_hooks_from_content(
    content: &str,
    timestamp: DateTime<Utc>,
) -> Vec<(DateTime<Utc>, DisplayEvent)> {
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
                    hooks.push((
                        timestamp,
                        DisplayEvent::Hook {
                            name: name.clone(),
                            output: format!("[{}]", name),
                        },
                    ));
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
                    hooks.push((
                        timestamp,
                        DisplayEvent::Hook {
                            name,
                            output: format!("FAILED: {}", output),
                        },
                    ));
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
    active_agent_tool_ids: &mut HashSet<String>,
) {
    let message = json.get("message");
    let content_val = message.and_then(|m| m.get("content"));
    let is_meta = json
        .get("isMeta")
        .and_then(|m| m.as_bool())
        .unwrap_or(false);

    let content_str = if let Some(s) = content_val.and_then(|c| c.as_str()) {
        Some(s.to_string())
    } else if let Some(arr) = content_val.and_then(|c| c.as_array()) {
        Some(
            arr.iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    } else {
        None
    };

    let is_compaction_summary = content_str
        .as_ref()
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

    if is_meta {
        return;
    }

    if let Some(content) = content_val.and_then(|c| c.as_str()) {
        if content.contains("<local-command-caveat>") {
            return;
        }
        // Task notifications are injected by Claude Code as user-role messages
        // when background commands complete — filter them from display.
        if content.contains("<task-notification>") {
            return;
        }

        if content.contains("<local-command-stdout>") {
            if content.contains("Compacted") {
                events.push((timestamp, DisplayEvent::Compacted));
            }
            return;
        }

        if content.starts_with("<command-name>") {
            if let Some(end) = content.find("</command-name>") {
                let cmd = &content[14..end];
                events.push((
                    timestamp,
                    DisplayEvent::Command {
                        name: cmd.to_string(),
                    },
                ));
                return;
            }
        }

        // Sub-agent prompt suppression: when an Agent/Task tool is active,
        // string-content user events are the sub-agent's internal prompt.
        if !active_agent_tool_ids.is_empty() {
            return;
        }

        let parent_uuid = json
            .get("parentUuid")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string();
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
        events.push((
            timestamp,
            DisplayEvent::UserMessage {
                _uuid: json
                    .get("uuid")
                    .and_then(|u| u.as_str())
                    .unwrap_or("")
                    .to_string(),
                content: content.to_string(),
            },
        ));
    } else if let Some(content_arr) = content_val.and_then(|c| c.as_array()) {
        for block in content_arr {
            if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                parse_tool_result_block(
                    block,
                    timestamp,
                    events,
                    tool_calls,
                    pending_tools,
                    failed_tools,
                    last_user_msg,
                    ups_hooks,
                    session_slug,
                    active_agent_tool_ids,
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
    active_agent_tool_ids: &mut HashSet<String>,
) {
    let tool_use_id = block
        .get("tool_use_id")
        .and_then(|i| i.as_str())
        .unwrap_or("")
        .to_string();
    // Clear agent tool tracking when its result arrives
    active_agent_tool_ids.remove(&tool_use_id);
    let (tool_name, file_path) = tool_calls
        .get(&tool_use_id)
        .cloned()
        .unwrap_or(("Unknown".to_string(), None));

    let content = if let Some(s) = block.get("content").and_then(|c| c.as_str()) {
        s.to_string()
    } else if let Some(arr) = block.get("content").and_then(|c| c.as_array()) {
        arr.iter()
            .filter_map(|b| {
                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                    b.get("text").and_then(|t| t.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    };

    pending_tools.remove(&tool_use_id);

    // Use is_error from Claude Code's JSON when available (authoritative).
    // Fall back to conservative heuristic for older session files that lack the field.
    let is_error = block
        .get("is_error")
        .and_then(|v| v.as_bool())
        .unwrap_or_else(|| {
            let first = content.lines().next().unwrap_or("").to_lowercase();
            first.contains("<tool_use_error>")
                || (first.starts_with("error") && !first.starts_with("error:"))
                || first.contains("enoent")
        });

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
    let is_plan_write = tool_name == "Write"
        && file_path
            .as_ref()
            .map(|p| p.contains("/.claude/plans/") && p.ends_with(".md"))
            .unwrap_or(false);

    if !content.is_empty() {
        events.push((
            timestamp,
            DisplayEvent::ToolResult {
                tool_use_id,
                tool_name,
                file_path,
                content,
                is_error,
            },
        ));
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

fn parse_assistant_event(
    json: &serde_json::Value,
    timestamp: DateTime<Utc>,
    events: &mut Vec<(DateTime<Utc>, DisplayEvent)>,
    tool_calls: &mut HashMap<String, (String, Option<String>)>,
    pending_tools: &mut HashSet<String>,
    model_out: &mut Option<String>,
    active_agent_tool_ids: &mut HashSet<String>,
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

    if let Some(model_str) = message.get("model").and_then(|m| m.as_str()) {
        *model_out = Some(model_str.to_string());
    }

    for block in content_arr {
        let Some(block_type) = block.get("type").and_then(|t| t.as_str()) else {
            continue;
        };

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
                    events.push((
                        timestamp,
                        DisplayEvent::AssistantText {
                            _uuid: json
                                .get("uuid")
                                .and_then(|u| u.as_str())
                                .unwrap_or("")
                                .to_string(),
                            _message_id: message
                                .get("id")
                                .and_then(|i| i.as_str())
                                .unwrap_or("")
                                .to_string(),
                            text: text.to_string(),
                        },
                    ));
                }
            }
            "tool_use" => {
                let tool_name = block
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_id = block
                    .get("id")
                    .and_then(|i| i.as_str())
                    .unwrap_or("")
                    .to_string();
                let input = block
                    .get("input")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let file_path = input
                    .get("file_path")
                    .or(input.get("path"))
                    .and_then(|p| p.as_str())
                    .map(|s| s.to_string());

                tool_calls.insert(tool_id.clone(), (tool_name.clone(), file_path.clone()));
                pending_tools.insert(tool_id.clone());
                // Track Agent/Task tools so we can suppress sub-agent prompt
                // user events
                if tool_name == "Agent" || tool_name == "Task" {
                    active_agent_tool_ids.insert(tool_id.clone());
                }

                events.push((
                    timestamp,
                    DisplayEvent::ToolCall {
                        _uuid: json
                            .get("uuid")
                            .and_then(|u| u.as_str())
                            .unwrap_or("")
                            .to_string(),
                        tool_use_id: tool_id,
                        tool_name,
                        file_path,
                        input,
                    },
                ));
            }
            _ => {}
        }
    }
}

fn parse_result_event(
    json: &serde_json::Value,
    timestamp: DateTime<Utc>,
    events: &mut Vec<(DateTime<Utc>, DisplayEvent)>,
) {
    if let Some(duration) = json.get("durationMs").and_then(|d| d.as_f64()) {
        let cost = json.get("costUsd").and_then(|c| c.as_f64()).unwrap_or(0.0);
        events.push((
            timestamp,
            DisplayEvent::Complete {
                _session_id: json
                    .get("sessionId")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                duration_ms: duration as u64,
                cost_usd: cost,
                success: true,
            },
        ));
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
                    events.push((
                        timestamp,
                        DisplayEvent::Command {
                            name: cmd.to_string(),
                        },
                    ));
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
    if data.get("type").and_then(|t| t.as_str()) != Some("hook_progress") {
        return;
    }

    let hook_name = data
        .get("hookName")
        .or_else(|| data.get("hookEvent"))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let command = data.get("command").and_then(|c| c.as_str()).unwrap_or("");

    if hook_name.is_empty() {
        return;
    }

    let output = if command.starts_with("echo '") && command.ends_with('\'') {
        command[6..command.len() - 1].to_string()
    } else if command.starts_with("echo \"") && command.ends_with('"') {
        command[6..command.len() - 1].to_string()
    } else if command.contains("; echo \"$OUT\"") || command.contains("; echo '$OUT'") {
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

    // Always show hooks - use [hookName] as fallback when no output extracted
    let display_output = if output.is_empty() {
        format!("[{}]", hook_name)
    } else {
        output
    };
    events.push((
        timestamp,
        DisplayEvent::Hook {
            name: hook_name,
            output: display_output,
        },
    ));
}

/// Load plan file from ~/.claude/plans/{slug}.md
fn load_plan_file(slug: &str) -> Option<DisplayEvent> {
    let plans_dir = dirs::home_dir()?.join(".claude").join("plans");
    let plan_path = plans_dir.join(format!("{}.md", slug));

    if plan_path.exists() {
        let content = std::fs::read_to_string(&plan_path).ok()?;
        // Extract plan name from first line (# Plan: Name or just # Title)
        let name = content
            .lines()
            .next()
            .and_then(|line| {
                line.strip_prefix("# Plan: ")
                    .or_else(|| line.strip_prefix("# "))
            })
            .unwrap_or(slug)
            .to_string();

        Some(DisplayEvent::Plan { name, content })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── check_plan_approval ──

    #[test]
    fn test_plan_approval_no_events() {
        assert!(!check_plan_approval(&[]));
    }

    #[test]
    fn test_plan_approval_exit_plan_no_user() {
        let events = vec![DisplayEvent::ToolCall {
            _uuid: "u1".into(),
            tool_use_id: "t1".into(),
            tool_name: "ExitPlanMode".into(),
            file_path: None,
            input: serde_json::Value::Null,
        }];
        assert!(check_plan_approval(&events));
    }

    #[test]
    fn test_plan_approval_exit_plan_then_user() {
        let events = vec![
            DisplayEvent::ToolCall {
                _uuid: "u1".into(),
                tool_use_id: "t1".into(),
                tool_name: "ExitPlanMode".into(),
                file_path: None,
                input: serde_json::Value::Null,
            },
            DisplayEvent::UserMessage {
                _uuid: "u2".into(),
                content: "approved".into(),
            },
        ];
        assert!(!check_plan_approval(&events));
    }

    #[test]
    fn test_plan_approval_no_exit_plan() {
        let events = vec![
            DisplayEvent::UserMessage {
                _uuid: "u1".into(),
                content: "hello".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: "u2".into(),
                _message_id: "m1".into(),
                text: "response".into(),
            },
        ];
        assert!(!check_plan_approval(&events));
    }

    #[test]
    fn test_plan_approval_multiple_exit_plans() {
        // First ExitPlanMode followed by user msg (resolved),
        // then second ExitPlanMode with no user msg (still pending)
        let events = vec![
            DisplayEvent::ToolCall {
                _uuid: "u1".into(),
                tool_use_id: "t1".into(),
                tool_name: "ExitPlanMode".into(),
                file_path: None,
                input: serde_json::Value::Null,
            },
            DisplayEvent::UserMessage {
                _uuid: "u2".into(),
                content: "ok".into(),
            },
            DisplayEvent::ToolCall {
                _uuid: "u3".into(),
                tool_use_id: "t2".into(),
                tool_name: "ExitPlanMode".into(),
                file_path: None,
                input: serde_json::Value::Null,
            },
        ];
        assert!(check_plan_approval(&events));
    }

    #[test]
    fn test_plan_approval_other_tool_not_exit_plan() {
        let events = vec![DisplayEvent::ToolCall {
            _uuid: "u1".into(),
            tool_use_id: "t1".into(),
            tool_name: "Read".into(),
            file_path: Some("/test.rs".into()),
            input: serde_json::Value::Null,
        }];
        assert!(!check_plan_approval(&events));
    }

    // ── extract_hooks_from_content ──

    #[test]
    fn test_extract_hooks_success() {
        let content =
            "<system-reminder>\nMyHook hook success: All checks passed\n</system-reminder>";
        let ts = Utc::now();
        let hooks = extract_hooks_from_content(content, ts);
        assert_eq!(hooks.len(), 1);
        if let DisplayEvent::Hook { name, output } = &hooks[0].1 {
            assert_eq!(name, "MyHook");
            assert_eq!(output, "All checks passed");
        } else {
            panic!("expected Hook event");
        }
    }

    #[test]
    fn test_extract_hooks_failed() {
        let content =
            "<system-reminder>\nBuildCheck hook failed: compilation error\n</system-reminder>";
        let ts = Utc::now();
        let hooks = extract_hooks_from_content(content, ts);
        assert_eq!(hooks.len(), 1);
        if let DisplayEvent::Hook { name, output } = &hooks[0].1 {
            assert_eq!(name, "BuildCheck");
            assert!(output.starts_with("FAILED:"));
        } else {
            panic!("expected Hook event");
        }
    }

    #[test]
    fn test_extract_hooks_ellipsis_output() {
        let content = "<system-reminder>\nStartup hook success: ...\n</system-reminder>";
        let ts = Utc::now();
        let hooks = extract_hooks_from_content(content, ts);
        assert_eq!(hooks.len(), 1);
        if let DisplayEvent::Hook { name, output } = &hooks[0].1 {
            assert_eq!(name, "Startup");
            assert_eq!(output, "[Startup]");
        } else {
            panic!("expected Hook event");
        }
    }

    #[test]
    fn test_extract_hooks_empty_content() {
        let hooks = extract_hooks_from_content("no hooks here", Utc::now());
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_extract_hooks_multiple() {
        let content = "<system-reminder>\nA hook success: ok\n</system-reminder>\
                       <system-reminder>\nB hook success: done\n</system-reminder>";
        let ts = Utc::now();
        let hooks = extract_hooks_from_content(content, ts);
        assert_eq!(hooks.len(), 2);
    }

    #[test]
    fn test_extract_hooks_unclosed_tag() {
        let content = "<system-reminder>\nTest hook success: data";
        let hooks = extract_hooks_from_content(content, Utc::now());
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_extract_hooks_empty_name_skipped() {
        let content = "<system-reminder>\n hook success: data\n</system-reminder>";
        let ts = Utc::now();
        let hooks = extract_hooks_from_content(content, ts);
        // The name would be empty after trimming, so the hook should be skipped
        // Actually " " trimmed is empty
        assert!(hooks.is_empty());
    }

    // ── IncrementalParserState::from_events ──

    #[test]
    fn test_incremental_state_empty() {
        let state = IncrementalParserState::from_events(&[], None);
        assert!(state.tool_calls.is_empty());
        assert!(state.user_msg_by_parent.is_empty());
        assert!(state.session_slug.is_none());
    }

    #[test]
    fn test_incremental_state_captures_tool_calls() {
        let events = vec![
            DisplayEvent::ToolCall {
                _uuid: "u1".into(),
                tool_use_id: "tc-1".into(),
                tool_name: "Read".into(),
                file_path: Some("/file.rs".into()),
                input: serde_json::Value::Null,
            },
            DisplayEvent::AssistantText {
                _uuid: "u2".into(),
                _message_id: "m1".into(),
                text: "text".into(),
            },
            DisplayEvent::ToolCall {
                _uuid: "u3".into(),
                tool_use_id: "tc-2".into(),
                tool_name: "Write".into(),
                file_path: None,
                input: serde_json::Value::Null,
            },
        ];
        let state = IncrementalParserState::from_events(&events, Some("slug".into()));
        assert_eq!(state.tool_calls.len(), 2);
        assert_eq!(state.tool_calls.get("tc-1").unwrap().0, "Read");
        assert_eq!(
            state.tool_calls.get("tc-1").unwrap().1,
            Some("/file.rs".to_string())
        );
        assert_eq!(state.tool_calls.get("tc-2").unwrap().0, "Write");
        assert_eq!(state.tool_calls.get("tc-2").unwrap().1, None);
        assert_eq!(state.session_slug, Some("slug".to_string()));
    }

    // ── extract_hooks_from_content: more edge cases ──

    #[test]
    fn test_extract_hooks_no_space_before_hook() {
        // "SomeName hook success:" - name is "SomeName"
        let content = "<system-reminder>\nSomeName hook success: output data\n</system-reminder>";
        let hooks = extract_hooks_from_content(content, Utc::now());
        assert_eq!(hooks.len(), 1);
        if let DisplayEvent::Hook { name, output } = &hooks[0].1 {
            assert_eq!(name, "SomeName");
            assert_eq!(output, "output data");
        }
    }

    #[test]
    fn test_extract_hooks_with_newline_escape_in_name() {
        // Name with leading \\n should be trimmed
        let content = "<system-reminder>\n\\nMyHook hook success: data\n</system-reminder>";
        let hooks = extract_hooks_from_content(content, Utc::now());
        assert_eq!(hooks.len(), 1);
        if let DisplayEvent::Hook { name, .. } = &hooks[0].1 {
            assert_eq!(name, "MyHook");
        }
    }

    #[test]
    fn test_extract_hooks_failed_empty_output() {
        let content = "<system-reminder>\nTestHook hook failed: \n</system-reminder>";
        let hooks = extract_hooks_from_content(content, Utc::now());
        assert_eq!(hooks.len(), 1);
        if let DisplayEvent::Hook { name, output } = &hooks[0].1 {
            assert_eq!(name, "TestHook");
            assert!(output.starts_with("FAILED:"));
        }
    }

    #[test]
    fn test_extract_hooks_success_empty_output_skipped() {
        // Empty output after "hook success:" should be skipped
        let content = "<system-reminder>\nTestHook hook success: \n</system-reminder>";
        let hooks = extract_hooks_from_content(content, Utc::now());
        // Empty output after trimming is empty string, which is_empty() => skipped
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_extract_hooks_no_closing_tag() {
        let content = "<system-reminder>\nTestHook hook success: data";
        let hooks = extract_hooks_from_content(content, Utc::now());
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_extract_hooks_nested_tags() {
        // Only the first system-reminder/close pair should match
        let content = "<system-reminder>\nOuter hook success: data1\n</system-reminder>text<system-reminder>\nInner hook success: data2\n</system-reminder>";
        let hooks = extract_hooks_from_content(content, Utc::now());
        assert_eq!(hooks.len(), 2);
    }

    #[test]
    fn test_extract_hooks_content_between_tags() {
        // Content with no "hook success:" or "hook failed:" keyword
        let content = "<system-reminder>\nJust some random text\n</system-reminder>";
        let hooks = extract_hooks_from_content(content, Utc::now());
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_extract_hooks_multiline_content() {
        let content = "<system-reminder>\nPreToolUse:Bash hook success: line1\nline2\nline3\n</system-reminder>";
        let hooks = extract_hooks_from_content(content, Utc::now());
        assert_eq!(hooks.len(), 1);
        if let DisplayEvent::Hook { output, .. } = &hooks[0].1 {
            assert!(output.contains("line1"));
        }
    }

    #[test]
    fn test_extract_hooks_unicode_name() {
        let content = "<system-reminder>\n日本語Hook hook success: ok\n</system-reminder>";
        let hooks = extract_hooks_from_content(content, Utc::now());
        assert_eq!(hooks.len(), 1);
        if let DisplayEvent::Hook { name, .. } = &hooks[0].1 {
            assert!(name.contains("日本語"));
        }
    }

    #[test]
    fn test_extract_hooks_colon_in_name() {
        let content =
            "<system-reminder>\nPreToolUse:Bash hook success: allowed\n</system-reminder>";
        let hooks = extract_hooks_from_content(content, Utc::now());
        assert_eq!(hooks.len(), 1);
        if let DisplayEvent::Hook { name, .. } = &hooks[0].1 {
            assert_eq!(name, "PreToolUse:Bash");
        }
    }

    #[test]
    fn test_extract_hooks_only_opening_tag() {
        let content = "<system-reminder>some text but no close";
        let hooks = extract_hooks_from_content(content, Utc::now());
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_extract_hooks_empty_string() {
        let hooks = extract_hooks_from_content("", Utc::now());
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_extract_hooks_tag_without_content() {
        let content = "<system-reminder></system-reminder>";
        let hooks = extract_hooks_from_content(content, Utc::now());
        assert!(hooks.is_empty());
    }

    // ── check_plan_approval: more scenarios ──

    #[test]
    fn test_plan_approval_exit_plan_then_tool_result_no_user() {
        // ToolResult is not a UserMessage, so plan should still be awaiting
        let events = vec![
            DisplayEvent::ToolCall {
                _uuid: "u1".into(),
                tool_use_id: "t1".into(),
                tool_name: "ExitPlanMode".into(),
                file_path: None,
                input: serde_json::Value::Null,
            },
            DisplayEvent::ToolResult {
                tool_use_id: "t2".into(),
                tool_name: "Read".into(),
                file_path: Some("/test".into()),
                content: "data".into(),
                is_error: false,
            },
        ];
        assert!(check_plan_approval(&events));
    }

    #[test]
    fn test_plan_approval_exit_plan_then_assistant_text_no_user() {
        let events = vec![
            DisplayEvent::ToolCall {
                _uuid: "u1".into(),
                tool_use_id: "t1".into(),
                tool_name: "ExitPlanMode".into(),
                file_path: None,
                input: serde_json::Value::Null,
            },
            DisplayEvent::AssistantText {
                _uuid: "u2".into(),
                _message_id: "m1".into(),
                text: "Here is the plan".into(),
            },
        ];
        assert!(check_plan_approval(&events));
    }

    #[test]
    fn test_plan_approval_two_exit_plans_both_resolved() {
        let events = vec![
            DisplayEvent::ToolCall {
                _uuid: "u1".into(),
                tool_use_id: "t1".into(),
                tool_name: "ExitPlanMode".into(),
                file_path: None,
                input: serde_json::Value::Null,
            },
            DisplayEvent::UserMessage {
                _uuid: "u2".into(),
                content: "approved".into(),
            },
            DisplayEvent::ToolCall {
                _uuid: "u3".into(),
                tool_use_id: "t2".into(),
                tool_name: "ExitPlanMode".into(),
                file_path: None,
                input: serde_json::Value::Null,
            },
            DisplayEvent::UserMessage {
                _uuid: "u4".into(),
                content: "approved again".into(),
            },
        ];
        assert!(!check_plan_approval(&events));
    }

    #[test]
    fn test_plan_approval_only_user_messages() {
        let events = vec![
            DisplayEvent::UserMessage {
                _uuid: "u1".into(),
                content: "hello".into(),
            },
            DisplayEvent::UserMessage {
                _uuid: "u2".into(),
                content: "world".into(),
            },
        ];
        assert!(!check_plan_approval(&events));
    }

    #[test]
    fn test_plan_approval_other_tool_between_exit_and_user() {
        let events = vec![
            DisplayEvent::ToolCall {
                _uuid: "u1".into(),
                tool_use_id: "t1".into(),
                tool_name: "ExitPlanMode".into(),
                file_path: None,
                input: serde_json::Value::Null,
            },
            DisplayEvent::ToolCall {
                _uuid: "u2".into(),
                tool_use_id: "t2".into(),
                tool_name: "Read".into(),
                file_path: None,
                input: serde_json::Value::Null,
            },
            DisplayEvent::UserMessage {
                _uuid: "u3".into(),
                content: "approved".into(),
            },
        ];
        assert!(!check_plan_approval(&events));
    }

    #[test]
    fn test_plan_approval_hook_event_does_not_resolve() {
        let events = vec![
            DisplayEvent::ToolCall {
                _uuid: "u1".into(),
                tool_use_id: "t1".into(),
                tool_name: "ExitPlanMode".into(),
                file_path: None,
                input: serde_json::Value::Null,
            },
            DisplayEvent::Hook {
                name: "test".into(),
                output: "data".into(),
            },
        ];
        assert!(check_plan_approval(&events));
    }

    #[test]
    fn test_plan_approval_command_does_not_resolve() {
        let events = vec![
            DisplayEvent::ToolCall {
                _uuid: "u1".into(),
                tool_use_id: "t1".into(),
                tool_name: "ExitPlanMode".into(),
                file_path: None,
                input: serde_json::Value::Null,
            },
            DisplayEvent::Command {
                name: "compact".into(),
            },
        ];
        assert!(check_plan_approval(&events));
    }

    #[test]
    fn test_plan_approval_compacting_does_not_resolve() {
        let events = vec![
            DisplayEvent::ToolCall {
                _uuid: "u1".into(),
                tool_use_id: "t1".into(),
                tool_name: "ExitPlanMode".into(),
                file_path: None,
                input: serde_json::Value::Null,
            },
            DisplayEvent::Compacting,
        ];
        assert!(check_plan_approval(&events));
    }

    #[test]
    fn test_plan_approval_init_does_not_resolve() {
        let events = vec![
            DisplayEvent::ToolCall {
                _uuid: "u1".into(),
                tool_use_id: "t1".into(),
                tool_name: "ExitPlanMode".into(),
                file_path: None,
                input: serde_json::Value::Null,
            },
            DisplayEvent::Init {
                _session_id: "s".into(),
                cwd: "/".into(),
                model: "m".into(),
            },
        ];
        assert!(check_plan_approval(&events));
    }

    #[test]
    fn test_plan_approval_complete_does_not_resolve() {
        let events = vec![
            DisplayEvent::ToolCall {
                _uuid: "u1".into(),
                tool_use_id: "t1".into(),
                tool_name: "ExitPlanMode".into(),
                file_path: None,
                input: serde_json::Value::Null,
            },
            DisplayEvent::Complete {
                _session_id: "s".into(),
                success: true,
                duration_ms: 1000,
                cost_usd: 0.01,
            },
        ];
        assert!(check_plan_approval(&events));
    }

    #[test]
    fn test_plan_approval_filtered_does_not_resolve() {
        let events = vec![
            DisplayEvent::ToolCall {
                _uuid: "u1".into(),
                tool_use_id: "t1".into(),
                tool_name: "ExitPlanMode".into(),
                file_path: None,
                input: serde_json::Value::Null,
            },
            DisplayEvent::Filtered,
        ];
        assert!(check_plan_approval(&events));
    }

    // ── IncrementalParserState: more edge cases ──

    #[test]
    fn test_incremental_state_ignores_non_tool_call_events() {
        let events = vec![
            DisplayEvent::UserMessage {
                _uuid: "u1".into(),
                content: "hello".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: "u2".into(),
                _message_id: "m1".into(),
                text: "text".into(),
            },
            DisplayEvent::Hook {
                name: "hook".into(),
                output: "out".into(),
            },
            DisplayEvent::Command {
                name: "compact".into(),
            },
            DisplayEvent::Compacting,
            DisplayEvent::Compacted,
            DisplayEvent::MayBeCompacting,
            DisplayEvent::Filtered,
        ];
        let state = IncrementalParserState::from_events(&events, None);
        assert!(state.tool_calls.is_empty());
    }

    #[test]
    fn test_incremental_state_with_slug() {
        let state = IncrementalParserState::from_events(&[], Some("my-slug".into()));
        assert_eq!(state.session_slug, Some("my-slug".to_string()));
    }

    #[test]
    fn test_incremental_state_none_slug() {
        let state = IncrementalParserState::from_events(&[], None);
        assert!(state.session_slug.is_none());
    }

    #[test]
    fn test_incremental_state_overwrites_duplicate_tool_ids() {
        let events = vec![
            DisplayEvent::ToolCall {
                _uuid: "u1".into(),
                tool_use_id: "same-id".into(),
                tool_name: "Read".into(),
                file_path: Some("/a.rs".into()),
                input: serde_json::Value::Null,
            },
            DisplayEvent::ToolCall {
                _uuid: "u2".into(),
                tool_use_id: "same-id".into(),
                tool_name: "Write".into(),
                file_path: Some("/b.rs".into()),
                input: serde_json::Value::Null,
            },
        ];
        let state = IncrementalParserState::from_events(&events, None);
        assert_eq!(state.tool_calls.len(), 1);
        // Last one wins since HashMap::insert overwrites
        assert_eq!(state.tool_calls.get("same-id").unwrap().0, "Write");
        assert_eq!(
            state.tool_calls.get("same-id").unwrap().1,
            Some("/b.rs".to_string())
        );
    }

    #[test]
    fn test_incremental_state_many_tool_calls() {
        let events: Vec<DisplayEvent> = (0..100)
            .map(|i| DisplayEvent::ToolCall {
                _uuid: format!("u{}", i),
                tool_use_id: format!("tc-{}", i),
                tool_name: "Bash".into(),
                file_path: None,
                input: serde_json::Value::Null,
            })
            .collect();
        let state = IncrementalParserState::from_events(&events, None);
        assert_eq!(state.tool_calls.len(), 100);
    }

    #[test]
    fn test_incremental_state_tool_result_not_captured() {
        // ToolResult events should NOT be captured in tool_calls map
        let events = vec![DisplayEvent::ToolResult {
            tool_use_id: "tc-1".into(),
            tool_name: "Read".into(),
            file_path: Some("/file.rs".into()),
            content: "data".into(),
            is_error: false,
        }];
        let state = IncrementalParserState::from_events(&events, None);
        assert!(state.tool_calls.is_empty());
    }

    #[test]
    fn test_incremental_state_complete_not_captured() {
        let events = vec![DisplayEvent::Complete {
            _session_id: "s1".into(),
            success: true,
            duration_ms: 5000,
            cost_usd: 0.05,
        }];
        let state = IncrementalParserState::from_events(&events, None);
        assert!(state.tool_calls.is_empty());
    }

    #[test]
    fn test_incremental_state_init_not_captured() {
        let events = vec![DisplayEvent::Init {
            _session_id: "s1".into(),
            cwd: "/home".into(),
            model: "claude".into(),
        }];
        let state = IncrementalParserState::from_events(&events, None);
        assert!(state.tool_calls.is_empty());
    }

    #[test]
    fn test_incremental_state_plan_not_captured() {
        let events = vec![DisplayEvent::Plan {
            name: "plan".into(),
            content: "content".into(),
        }];
        let state = IncrementalParserState::from_events(&events, None);
        assert!(state.tool_calls.is_empty());
    }

    #[test]
    fn test_incremental_state_user_msg_by_parent_always_empty() {
        // from_events never populates user_msg_by_parent
        let events = vec![DisplayEvent::UserMessage {
            _uuid: "u1".into(),
            content: "test".into(),
        }];
        let state = IncrementalParserState::from_events(&events, None);
        assert!(state.user_msg_by_parent.is_empty());
    }

    #[test]
    fn test_incremental_state_mixed_events() {
        let events = vec![
            DisplayEvent::Init {
                _session_id: "s".into(),
                cwd: "/".into(),
                model: "m".into(),
            },
            DisplayEvent::UserMessage {
                _uuid: "u1".into(),
                content: "hi".into(),
            },
            DisplayEvent::ToolCall {
                _uuid: "u2".into(),
                tool_use_id: "tc-1".into(),
                tool_name: "Read".into(),
                file_path: Some("/main.rs".into()),
                input: serde_json::Value::Null,
            },
            DisplayEvent::ToolResult {
                tool_use_id: "tc-1".into(),
                tool_name: "Read".into(),
                file_path: Some("/main.rs".into()),
                content: "fn main() {}".into(),
                is_error: false,
            },
            DisplayEvent::AssistantText {
                _uuid: "u3".into(),
                _message_id: "m1".into(),
                text: "done".into(),
            },
            DisplayEvent::ToolCall {
                _uuid: "u4".into(),
                tool_use_id: "tc-2".into(),
                tool_name: "Write".into(),
                file_path: Some("/out.rs".into()),
                input: serde_json::Value::Null,
            },
        ];
        let state = IncrementalParserState::from_events(&events, Some("test-slug".into()));
        assert_eq!(state.tool_calls.len(), 2);
        assert_eq!(state.tool_calls.get("tc-1").unwrap().0, "Read");
        assert_eq!(state.tool_calls.get("tc-2").unwrap().0, "Write");
        assert_eq!(state.session_slug, Some("test-slug".to_string()));
        assert!(state.user_msg_by_parent.is_empty());
    }

    // ── ParsedSession defaults (parse_session_file on nonexistent file) ──

    #[test]
    fn test_parse_session_file_nonexistent() {
        let result = parse_session_file(std::path::Path::new("/nonexistent/path/session.jsonl"));
        assert!(result.events.is_empty());
        assert!(result.pending_tools.is_empty());
        assert!(result.failed_tools.is_empty());
        assert_eq!(result.total_lines, 0);
        assert_eq!(result.parse_errors, 0);
        assert!(!result.awaiting_plan_approval);
        assert_eq!(result.end_offset, 0);
        assert!(result.model.is_none());
    }

    // ── Helper for temp file creation ──

    use std::sync::atomic::{AtomicU32, Ordering};
    static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn test_file(name: &str) -> std::path::PathBuf {
        let n = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("azureal_test_{}_{}.jsonl", name, n))
    }

    // ── parse_session_file with temp files ──

    #[test]
    fn test_parse_session_file_empty_file() {
        let file_path = test_file("empty");
        std::fs::write(&file_path, "").unwrap();
        let result = parse_session_file(&file_path);
        assert!(result.events.is_empty());
        assert_eq!(result.total_lines, 0);
        assert_eq!(result.parse_errors, 0);
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_session_file_invalid_json_lines() {
        let file_path = test_file("invalid_json");
        std::fs::write(&file_path, "not json\nalso not json\n").unwrap();
        let result = parse_session_file(&file_path);
        assert!(result.events.is_empty());
        assert_eq!(result.total_lines, 2);
        assert_eq!(result.parse_errors, 2);
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_session_file_mixed_valid_invalid() {
        let file_path = test_file("mixed");
        let content = "not json\n{\"type\":\"system\",\"subtype\":\"local_command\",\"content\":\"<command-name>compact</command-name>\"}\n";
        std::fs::write(&file_path, content).unwrap();
        let result = parse_session_file(&file_path);
        assert_eq!(result.total_lines, 2);
        assert_eq!(result.parse_errors, 1);
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_session_file_system_command_event() {
        let file_path = test_file("sys_cmd");
        let line = r#"{"type":"system","subtype":"local_command","content":"<command-name>compact</command-name>","timestamp":"2026-01-01T00:00:00Z"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::Command { name } => assert_eq!(name, "compact"),
            other => panic!("expected Command, got {:?}", other),
        }
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_session_file_user_message() {
        let file_path = test_file("user_msg");
        let line = r#"{"type":"user","message":{"content":"Hello world"},"timestamp":"2026-01-01T00:00:00Z","uuid":"u1"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::UserMessage { content, .. } => assert_eq!(content, "Hello world"),
            other => panic!("expected UserMessage, got {:?}", other),
        }
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_session_file_assistant_text() {
        let file_path = test_file("asst_text");
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Response text"}],"model":"claude-opus-4-6","usage":{"input_tokens":100,"output_tokens":50}},"timestamp":"2026-01-01T00:00:00Z","uuid":"u1"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::AssistantText { text, .. } => assert_eq!(text, "Response text"),
            other => panic!("expected AssistantText, got {:?}", other),
        }
        assert_eq!(result.model, Some("claude-opus-4-6".to_string()));
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_session_file_result_event() {
        let file_path = test_file("result");
        let line = r#"{"type":"result","durationMs":5000,"costUsd":0.05,"sessionId":"s1","timestamp":"2026-01-01T00:00:00Z"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::Complete {
                duration_ms,
                cost_usd,
                success,
                ..
            } => {
                assert_eq!(*duration_ms, 5000);
                assert!((*cost_usd - 0.05).abs() < f64::EPSILON);
                assert!(*success);
            }
            other => panic!("expected Complete, got {:?}", other),
        }
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_session_file_compaction_summary() {
        let file_path = test_file("compaction");
        let line = r#"{"type":"user","message":{"content":"This session is being continued from a previous conversation..."},"timestamp":"2026-01-01T00:00:00Z"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert_eq!(result.events.len(), 1);
        assert!(matches!(&result.events[0], DisplayEvent::Compacting));
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_session_file_is_meta_skipped() {
        let file_path = test_file("meta");
        let line = r#"{"type":"user","message":{"content":"meta content"},"isMeta":true,"timestamp":"2026-01-01T00:00:00Z"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert!(result.events.is_empty());
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_session_file_local_command_caveat_filtered() {
        let file_path = test_file("caveat");
        let line = r#"{"type":"user","message":{"content":"<local-command-caveat>blah</local-command-caveat>"},"timestamp":"2026-01-01T00:00:00Z","uuid":"u1"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert!(result.events.is_empty());
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_session_file_task_notification_filtered() {
        let file_path = test_file("task_notif");
        let line = r#"{"type":"user","message":{"content":"<task-notification><task-id>abc123</task-id><status>completed</status><summary>Background command completed</summary></task-notification>Read the output file"},"timestamp":"2026-01-01T00:00:00Z","uuid":"u1"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert!(
            result.events.is_empty(),
            "task-notification should be filtered from display"
        );
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_session_file_local_command_stdout_compacted() {
        let file_path = test_file("stdout_compact");
        let line = r#"{"type":"user","message":{"content":"<local-command-stdout>Compacted successfully</local-command-stdout>"},"timestamp":"2026-01-01T00:00:00Z","uuid":"u1"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert_eq!(result.events.len(), 1);
        assert!(matches!(&result.events[0], DisplayEvent::Compacted));
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_session_file_command_name_tag() {
        let file_path = test_file("cmd_tag");
        let line = r#"{"type":"user","message":{"content":"<command-name>crt</command-name>"},"timestamp":"2026-01-01T00:00:00Z","uuid":"u1"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::Command { name } => assert_eq!(name, "crt"),
            other => panic!("expected Command, got {:?}", other),
        }
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_session_file_tool_call_and_result() {
        let file_path = test_file("tool_call_result");
        let call_line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"tu-1","name":"Read","input":{"file_path":"/test.rs"}}],"model":"claude"},"timestamp":"2026-01-01T00:00:00Z","uuid":"u1"}"#;
        let result_line = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tu-1","content":"fn main() {}"}]},"timestamp":"2026-01-01T00:00:01Z","uuid":"u2"}"#;
        std::fs::write(&file_path, format!("{}\n{}\n", call_line, result_line)).unwrap();
        let result = parse_session_file(&file_path);
        let tool_calls: Vec<_> = result
            .events
            .iter()
            .filter(|e| matches!(e, DisplayEvent::ToolCall { .. }))
            .collect();
        let tool_results: Vec<_> = result
            .events
            .iter()
            .filter(|e| matches!(e, DisplayEvent::ToolResult { .. }))
            .collect();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_results.len(), 1);
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_session_file_unknown_event_type_ignored() {
        let file_path = test_file("unknown_type");
        let line =
            r#"{"type":"unknown_type","data":"whatever","timestamp":"2026-01-01T00:00:00Z"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert!(result.events.is_empty());
        assert_eq!(result.total_lines, 1);
        assert_eq!(result.parse_errors, 0);
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_session_file_end_offset() {
        let file_path = test_file("end_offset");
        let line = r#"{"type":"system","subtype":"local_command","content":"<command-name>test</command-name>"}"#;
        let content = format!("{}\n", line);
        let expected_len = content.len() as u64;
        std::fs::write(&file_path, &content).unwrap();
        let result = parse_session_file(&file_path);
        assert_eq!(result.end_offset, expected_len);
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_session_file_blank_lines_ignored() {
        let file_path = test_file("blank_lines");
        let content = "\n\n\n{\"type\":\"system\",\"subtype\":\"local_command\",\"content\":\"<command-name>test</command-name>\"}\n\n";
        std::fs::write(&file_path, content).unwrap();
        let result = parse_session_file(&file_path);
        assert_eq!(result.total_lines, 1);
        assert_eq!(result.parse_errors, 0);
        assert_eq!(result.events.len(), 1);
        let _ = std::fs::remove_file(&file_path);
    }

    // ── ParseDiagnostics ──

    #[test]
    fn test_parse_diagnostics_default() {
        let d = ParseDiagnostics::default();
        assert_eq!(d.assistant_events_total, 0);
        assert_eq!(d.assistant_events_no_message, 0);
        assert_eq!(d.assistant_events_no_content_arr, 0);
        assert_eq!(d.assistant_text_blocks, 0);
    }

    #[test]
    fn test_parse_diagnostics_counts_assistant() {
        let file_path = test_file("diag_counts");
        let line1 = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"a"}],"model":"claude"},"timestamp":"2026-01-01T00:00:00Z"}"#;
        let line2 = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"b"}],"model":"claude"},"timestamp":"2026-01-01T00:00:01Z"}"#;
        std::fs::write(&file_path, format!("{}\n{}\n", line1, line2)).unwrap();
        let result = parse_session_file(&file_path);
        assert_eq!(result.assistant_total, 2);
        assert_eq!(result.assistant_text_blocks, 2);
        assert_eq!(result.assistant_no_message, 0);
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_diagnostics_assistant_no_message() {
        let file_path = test_file("diag_no_msg");
        let line = r#"{"type":"assistant","timestamp":"2026-01-01T00:00:00Z"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert_eq!(result.assistant_total, 1);
        assert_eq!(result.assistant_no_message, 1);
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_diagnostics_assistant_no_content_array() {
        let file_path = test_file("diag_no_arr");
        let line = r#"{"type":"assistant","message":{"content":"not an array","model":"claude"},"timestamp":"2026-01-01T00:00:00Z"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert_eq!(result.assistant_total, 1);
        assert_eq!(result.assistant_no_content_arr, 1);
        let _ = std::fs::remove_file(&file_path);
    }

    // ── Incremental parse ──

    #[test]
    fn test_incremental_parse_no_new_data() {
        let file_path = test_file("incr_no_new");
        let line = r#"{"type":"system","subtype":"local_command","content":"<command-name>test</command-name>"}"#;
        let content = format!("{}\n", line);
        std::fs::write(&file_path, &content).unwrap();

        let initial = parse_session_file(&file_path);
        let offset = initial.end_offset;

        let result = parse_session_file_incremental(
            &file_path,
            offset,
            &initial.events,
            &initial.pending_tools,
            &initial.failed_tools,
        );
        assert_eq!(result.events.len(), initial.events.len());
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_incremental_parse_zero_offset_does_full_parse() {
        let file_path = test_file("incr_zero");
        let line = r#"{"type":"system","subtype":"local_command","content":"<command-name>test</command-name>"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();

        let result =
            parse_session_file_incremental(&file_path, 0, &[], &HashSet::new(), &HashSet::new());
        assert_eq!(result.events.len(), 1);
        let _ = std::fs::remove_file(&file_path);
    }

    // ── progress event parsing ──

    #[test]
    fn test_parse_progress_echo_single_quote() {
        let file_path = test_file("prog_sq");
        let line = r#"{"type":"progress","data":{"type":"hook_progress","hookEvent":"PreToolUse","hookName":"PreToolUse:Bash","command":"echo 'Check CLAUDE.md'"},"timestamp":"2026-01-01T00:00:00Z"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "PreToolUse:Bash");
                assert_eq!(output, "Check CLAUDE.md");
            }
            other => panic!("expected Hook, got {:?}", other),
        }
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_progress_echo_double_quote() {
        let file_path = test_file("prog_dq");
        let line = r#"{"type":"progress","data":{"type":"hook_progress","hookEvent":"PreToolUse","hookName":"PreToolUse:Write","command":"echo \"Validating\""},"timestamp":"2026-01-01T00:00:00Z"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "PreToolUse:Write");
                assert_eq!(output, "Validating");
            }
            other => panic!("expected Hook, got {:?}", other),
        }
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_progress_empty_hook_name_ignored() {
        let file_path = test_file("prog_empty_name");
        let line = r#"{"type":"progress","data":{"type":"hook_progress","hookEvent":"","hookName":"","command":"echo 'test'"},"timestamp":"2026-01-01T00:00:00Z"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert!(result.events.is_empty());
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_progress_non_hook_progress_ignored() {
        let file_path = test_file("prog_non_hook");
        let line = r#"{"type":"progress","data":{"type":"other_type","hookName":"test","command":"echo 'x'"},"timestamp":"2026-01-01T00:00:00Z"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert!(result.events.is_empty());
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_progress_no_data_field() {
        let file_path = test_file("prog_no_data");
        let line = r#"{"type":"progress","timestamp":"2026-01-01T00:00:00Z"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert!(result.events.is_empty());
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_parse_progress_fallback_output() {
        let file_path = test_file("prog_fallback");
        let line = r#"{"type":"progress","data":{"type":"hook_progress","hookEvent":"PreToolUse","hookName":"MyHook","command":"cargo test"},"timestamp":"2026-01-01T00:00:00Z"}"#;
        std::fs::write(&file_path, format!("{}\n", line)).unwrap();
        let result = parse_session_file(&file_path);
        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            DisplayEvent::Hook { output, .. } => assert_eq!(output, "[MyHook]"),
            other => panic!("expected Hook, got {:?}", other),
        }
        let _ = std::fs::remove_file(&file_path);
    }

    // ── Sub-agent prompt suppression ──

    #[test]
    fn test_agent_tool_suppresses_subagent_prompt() {
        let file_path = test_file("agent_suppress");
        let tool_use = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"tu-agent","name":"Agent","input":{"prompt":"explore codebase","description":"explore"}}],"model":"claude"},"timestamp":"2026-01-01T00:00:00Z","uuid":"a1"}"#;
        let subagent_prompt = r#"{"type":"user","message":{"content":"explore codebase"},"timestamp":"2026-01-01T00:00:01Z","uuid":"u1"}"#;
        let tool_result = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tu-agent","content":"found 5 files"}]},"timestamp":"2026-01-01T00:00:02Z","uuid":"u2"}"#;
        std::fs::write(
            &file_path,
            format!("{}\n{}\n{}\n", tool_use, subagent_prompt, tool_result),
        )
        .unwrap();
        let result = parse_session_file(&file_path);
        // Should have: ToolCall + ToolResult (no UserMessage for sub-agent prompt)
        let user_msgs: Vec<_> = result
            .events
            .iter()
            .filter(|e| matches!(e, DisplayEvent::UserMessage { .. }))
            .collect();
        assert!(
            user_msgs.is_empty(),
            "sub-agent prompt should be suppressed, got {} UserMessage(s)",
            user_msgs.len()
        );
        let tool_calls: Vec<_> = result
            .events
            .iter()
            .filter(|e| matches!(e, DisplayEvent::ToolCall { .. }))
            .collect();
        assert_eq!(tool_calls.len(), 1);
        let tool_results: Vec<_> = result
            .events
            .iter()
            .filter(|e| matches!(e, DisplayEvent::ToolResult { .. }))
            .collect();
        assert_eq!(tool_results.len(), 1);
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_agent_suppression_clears_after_result() {
        let file_path = test_file("agent_suppress_clear");
        let tool_use = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"tu-agent","name":"Agent","input":{"prompt":"search"}}],"model":"claude"},"timestamp":"2026-01-01T00:00:00Z","uuid":"a1"}"#;
        let tool_result = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tu-agent","content":"done"}]},"timestamp":"2026-01-01T00:00:01Z","uuid":"u2"}"#;
        let real_user = r#"{"type":"user","message":{"content":"next question"},"timestamp":"2026-01-01T00:00:02Z","uuid":"u3"}"#;
        std::fs::write(
            &file_path,
            format!("{}\n{}\n{}\n", tool_use, tool_result, real_user),
        )
        .unwrap();
        let result = parse_session_file(&file_path);
        let user_msgs: Vec<_> = result
            .events
            .iter()
            .filter(|e| matches!(e, DisplayEvent::UserMessage { .. }))
            .collect();
        assert_eq!(
            user_msgs.len(),
            1,
            "real user message after agent result should not be suppressed"
        );
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_non_agent_tool_does_not_suppress() {
        let file_path = test_file("non_agent_no_suppress");
        let tool_use = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"tu-read","name":"Read","input":{"file_path":"/test.rs"}}],"model":"claude"},"timestamp":"2026-01-01T00:00:00Z","uuid":"a1"}"#;
        let user_msg = r#"{"type":"user","message":{"content":"hello"},"timestamp":"2026-01-01T00:00:01Z","uuid":"u1"}"#;
        std::fs::write(&file_path, format!("{}\n{}\n", tool_use, user_msg)).unwrap();
        let result = parse_session_file(&file_path);
        let user_msgs: Vec<_> = result
            .events
            .iter()
            .filter(|e| matches!(e, DisplayEvent::UserMessage { .. }))
            .collect();
        assert_eq!(
            user_msgs.len(),
            1,
            "user message after non-agent tool should not be suppressed"
        );
        let _ = std::fs::remove_file(&file_path);
    }
}
