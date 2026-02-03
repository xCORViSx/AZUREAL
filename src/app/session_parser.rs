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
}

/// Parse a Claude session JSONL file into display events
pub fn parse_session_file(session_file: &Path) -> ParsedSession {
    // Reset diagnostics
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
        },
    };

    let reader = BufReader::new(file);
    let mut timed_events: Vec<(DateTime<Utc>, DisplayEvent)> = Vec::new();
    let mut user_msg_by_parent: HashMap<String, (usize, DateTime<Utc>)> = HashMap::new();
    let mut tool_calls: HashMap<String, (String, Option<String>)> = HashMap::new();
    let mut pending_tools: HashSet<String> = HashSet::new();
    let mut failed_tools: HashSet<String> = HashSet::new();
    let mut last_user_msg: Option<(usize, DateTime<Utc>)> = None;
    let mut ups_hooks: Vec<(usize, DateTime<Utc>, DisplayEvent)> = Vec::new();
    let mut total_lines = 0;
    let mut parse_errors = 0;
    let mut session_slug: Option<String> = None;

    for line in reader.lines().map_while(Result::ok) {
        total_lines += 1;
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) else {
            parse_errors += 1;
            continue;
        };

        let timestamp = json.get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        let event_type = json.get("type").and_then(|t| t.as_str()).unwrap_or("");

        // Capture session slug for plan file lookup
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
    let events: Vec<DisplayEvent> = timed_events.into_iter()
        .filter(|(_, e)| !matches!(e, DisplayEvent::Filtered))
        .map(|(_, e)| e)
        .collect();

    // Determine if awaiting plan approval: LAST ExitPlanMode has no user message after it
    // Reset saw_user_after whenever we see a new ExitPlanMode (each plan is independent)
    let awaiting_plan_approval = {
        let mut saw_exit_plan = false;
        let mut saw_user_after = false;
        for event in &events {
            match event {
                DisplayEvent::ToolCall { tool_name, .. } if tool_name == "ExitPlanMode" => {
                    saw_exit_plan = true;
                    saw_user_after = false; // Reset: new plan, no user response yet
                }
                DisplayEvent::UserMessage { .. } if saw_exit_plan => {
                    saw_user_after = true;
                }
                _ => {}
            }
        }
        saw_exit_plan && !saw_user_after
    };

    // Get diagnostics
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

fn parse_assistant_event(
    json: &serde_json::Value,
    timestamp: DateTime<Utc>,
    events: &mut Vec<(DateTime<Utc>, DisplayEvent)>,
    tool_calls: &mut HashMap<String, (String, Option<String>)>,
    pending_tools: &mut HashSet<String>,
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
