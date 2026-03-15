//! Display event rendering for TUI
//!
//! Thin orchestrator that dispatches to specialized renderers.

use ratatui::{
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::collections::HashSet;

use crate::app::state::backend_for_model;
use crate::backend::Backend;
use crate::events::DisplayEvent;
use crate::syntax::SyntaxHighlighter;
use super::colorize::ORANGE;
use super::util::{truncate, AZURE};
use super::markdown::parse_markdown_spans;
use super::render_markdown::render_assistant_text;
use super::render_tools::{extract_tool_param, render_tool_result, render_edit_diff, render_write_preview, tool_display_name};
use super::render_wrap::wrap_text;

/// Clickable path entry: (line_idx, start_col, end_col, file_path, old_string, new_string)
/// (line_idx, start_col, end_col, file_path, old_string, new_string, wrap_line_count)
pub type ClickablePath = (usize, usize, usize, String, String, String, usize);

/// Clickable table entry: (cache_line_start, cache_line_end, raw_markdown_text)
/// Identifies rendered table regions so mouse clicks can open a full-width popup.
pub type ClickableTable = (usize, usize, String);

/// Render DisplayEvents into Lines for the session pane with iMessage-style layout
/// Returns (lines, animation_indices, bubble_positions, clickable_paths, clickable_tables) where:
/// - animation_indices are (line_idx, span_idx, tool_use_id) for ALL tool indicators
/// - bubble_positions are (line_idx, is_user) pairs marking where message bubbles start
/// - clickable_paths are file path link regions for mouse click handling
/// - clickable_tables are table regions for click-to-expand popup
pub fn render_display_events(
    events: &[DisplayEvent],
    width: u16,
    pending_tools: &HashSet<String>,
    failed_tools: &HashSet<String>,
    syntax_highlighter: &mut SyntaxHighlighter,
    pending_user_message: Option<&str>,
) -> (Vec<Line<'static>>, Vec<(usize, usize, String)>, Vec<(usize, bool)>, Vec<ClickablePath>, Vec<ClickableTable>) {
    render_display_events_with_state(events, width, pending_tools, failed_tools, syntax_highlighter, pending_user_message, Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Default::default())
}

/// Render only new events, appending to existing cache data.
/// `events` contains ONLY the new events (from start_idx onwards in the original array).
/// `pre_scan` contains pre-computed state flags from events before start_idx,
/// so we don't need the old events at all (eliminates the mega-clone).
pub fn render_display_events_incremental(
    events: &[DisplayEvent],
    width: u16,
    pending_tools: &HashSet<String>,
    failed_tools: &HashSet<String>,
    syntax_highlighter: &mut SyntaxHighlighter,
    pending_user_message: Option<&str>,
    pre_scan: super::render_thread::PreScanState,
) -> (Vec<Line<'static>>, Vec<(usize, usize, String)>, Vec<(usize, bool)>, Vec<ClickablePath>, Vec<ClickableTable>) {
    // Render only new events into fresh accumulators. Indices are relative to 0 —
    // the main thread offsets them by existing_line_count when extending its cache.
    render_display_events_with_state(events, width, pending_tools, failed_tools, syntax_highlighter, pending_user_message, Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), pre_scan)
}

/// Core renderer: iterates events from `start_idx`, appending to provided vectors.
/// Pre-scan state from earlier events is passed in (not re-scanned), so callers
/// can send only new events + pre-computed flags (eliminates mega-clone).
fn render_display_events_with_state(
    events: &[DisplayEvent],
    width: u16,
    pending_tools: &HashSet<String>,
    failed_tools: &HashSet<String>,
    syntax_highlighter: &mut SyntaxHighlighter,
    pending_user_message: Option<&str>,
    mut lines: Vec<Line<'static>>,
    mut animation_indices: Vec<(usize, usize, String)>,
    mut bubble_positions: Vec<(usize, bool)>,
    mut clickable_paths: Vec<ClickablePath>,
    mut clickable_tables: Vec<ClickableTable>,
    pre_scan: super::render_thread::PreScanState,
) -> (Vec<Line<'static>>, Vec<(usize, usize, String)>, Vec<(usize, bool)>, Vec<ClickablePath>, Vec<ClickableTable>) {
    let w = width as usize;
    let bubble_width = (w * 2 / 3).max(40);

    // Pre-computed state flags: for full renders these are all default (false/None),
    // for incremental renders they come from pre_scan_events() on the main thread.
    let mut saw_init = pre_scan.saw_init;
    let mut saw_content = pre_scan.saw_content;
    let mut current_model = pre_scan.current_model;
    let mut last_hook = pre_scan.last_hook;
    let mut saw_exit_plan_mode = pre_scan.saw_exit_plan_mode;
    let mut saw_user_after_exit_plan = pre_scan.saw_user_after_exit_plan;
    let mut saw_ask_user_question = pre_scan.saw_ask_user_question;
    let mut saw_user_after_ask = pre_scan.saw_user_after_ask;
    let mut last_ask_input = pre_scan.last_ask_input;

    for event in events.iter() {
        match event {
            DisplayEvent::Init { model, cwd, .. } => {
                current_model = Some(model.clone());
                if saw_init || saw_content { continue; }
                saw_init = true;
                render_init(&mut lines, model, cwd);
            }
            DisplayEvent::Hook { name, output } => {
                // Dedup consecutive identical hooks — compare by reference first to avoid clone
                if let Some((ref ln, ref lo)) = last_hook {
                    if ln == name && lo == output { continue; }
                }
                last_hook = Some((name.clone(), output.clone()));
                render_hook(&mut lines, name, output, bubble_width);
            }
            DisplayEvent::Command { name } => {
                render_command(&mut lines, name);
            }
            DisplayEvent::Compacting => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled(" ✓ Context compacted ", Style::default().fg(Color::Black).bg(Color::Green)),
                ]).alignment(Alignment::Center));
            }
            DisplayEvent::Compacted => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled(" ✓ Context compacted ", Style::default().fg(Color::Black).bg(Color::Green)),
                ]).alignment(Alignment::Center));
            }
            DisplayEvent::MayBeCompacting => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled(" ⏳ Compacting context... ", Style::default().fg(Color::Black).bg(Color::Yellow)),
                ]).alignment(Alignment::Center));
            }
            DisplayEvent::Plan { name, content } => {
                saw_content = true;
                last_hook = None;
                render_plan(&mut lines, name, content, w);
            }
            DisplayEvent::UserMessage { content, .. } => {
                // Safety net: if a compaction summary slipped through parsing,
                // render the banner instead of the raw multi-page summary text
                if content.starts_with("This session is being continued from a previous conversation") {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled(" ✓ Context compacted ", Style::default().fg(Color::Black).bg(Color::Green)),
                    ]).alignment(Alignment::Center));
                    continue;
                }
                saw_content = true;
                last_hook = None;
                if saw_exit_plan_mode { saw_user_after_exit_plan = true; }
                if saw_ask_user_question { saw_user_after_ask = true; }
                // Track bubble position (line index after the empty lines)
                bubble_positions.push((lines.len() + 2, true));
                render_user_message(&mut lines, content, bubble_width, w);
            }
            DisplayEvent::AssistantText { text, .. } => {
                saw_content = true;
                last_hook = None;
                // Track bubble position (line index after the empty lines)
                bubble_positions.push((lines.len() + 2, false));
                lines.push(Line::from(""));
                lines.push(Line::from(""));

                let (_, assistant_color) = assistant_identity(current_model.as_deref().filter(|m| !m.is_empty()));
                lines.push(render_assistant_header_line(current_model.as_deref(), bubble_width));

                let base_offset = lines.len();
                let (text_lines, table_regions) = render_assistant_text(text, bubble_width, syntax_highlighter);
                lines.extend(text_lines);
                // Offset table regions to absolute cache line positions
                for (start, end, raw) in table_regions {
                    clickable_tables.push((start + base_offset, end + base_offset, raw));
                }

                lines.push(Line::from(vec![
                    Span::styled(format!("└{}", "─".repeat(bubble_width - 1)), Style::default().fg(assistant_color)),
                ]));
            }
            DisplayEvent::ToolCall { tool_name, file_path, input, tool_use_id, .. } => {
                saw_content = true;
                last_hook = None;
                if tool_name == "ExitPlanMode" {
                    saw_exit_plan_mode = true;
                    saw_user_after_exit_plan = false;
                }
                if tool_name == "AskUserQuestion" {
                    saw_ask_user_question = true;
                    saw_user_after_ask = false;
                    last_ask_input = Some(input.clone());
                }
                // TodoWrite rendered as sticky widget, skip inline display
                if tool_name == "TodoWrite" { continue; }
                render_tool_call(&mut lines, &mut animation_indices, &mut clickable_paths, tool_name, file_path, input, tool_use_id, pending_tools, failed_tools, bubble_width, syntax_highlighter);
            }
            DisplayEvent::ToolResult { tool_use_id, tool_name, file_path, content, is_error, .. } => {
                saw_content = true;
                last_hook = None;
                // TodoWrite result is noise ("Todos have been modified successfully"), skip it
                if tool_name == "TodoWrite" { continue; }
                let is_failed = *is_error || failed_tools.contains(tool_use_id);
                let tool_max = bubble_width + 10;
                lines.extend(render_tool_result(tool_name, file_path.as_deref(), content, is_failed, tool_max));
                // Show approval prompt immediately after ExitPlanMode result
                if tool_name == "ExitPlanMode" && saw_exit_plan_mode && !saw_user_after_exit_plan {
                    render_plan_approval(&mut lines, w);
                }
                // Show AskUserQuestion options box after the tool result
                if tool_name == "AskUserQuestion" && saw_ask_user_question && !saw_user_after_ask {
                    if let Some(ref input) = last_ask_input {
                        render_ask_user_question(&mut lines, input, w);
                    }
                }
            }
            DisplayEvent::Complete { duration_ms, cost_usd, success, .. } => {
                render_complete(&mut lines, *duration_ms, *cost_usd, *success);
            }
            DisplayEvent::Filtered => {}
        }
    }

    // Render pending user message (sent but not yet in session file)
    if let Some(msg) = pending_user_message {
        bubble_positions.push((lines.len() + 2, true));
        render_user_message(&mut lines, msg, bubble_width, w);
    }

    (lines, animation_indices, bubble_positions, clickable_paths, clickable_tables)
}

fn assistant_identity(model: Option<&str>) -> (&'static str, Color) {
    match model.map(backend_for_model).unwrap_or(Backend::Claude) {
        Backend::Codex => ("Codex", Color::Cyan),
        Backend::Claude => ("Claude", ORANGE),
    }
}

fn render_assistant_header_line(model: Option<&str>, bubble_width: usize) -> Line<'static> {
    let model = model.filter(|m| !m.is_empty());
    let (assistant_name, assistant_color) = assistant_identity(model);
    let header_style = Style::default().fg(Color::Black).bg(assistant_color).add_modifier(Modifier::BOLD);
    let model_style = Style::default()
        .fg(Color::Black)
        .bg(assistant_color)
        .add_modifier(Modifier::BOLD | Modifier::DIM);
    let fill_style = Style::default().bg(assistant_color);

    let left = format!(" {} ▶ ", assistant_name);
    let right = model.map(|model| {
        let max_model_width = bubble_width
            .saturating_sub(left.chars().count() + 3)
            .max(1);
        format!(" {} ", truncate(model, max_model_width))
    }).unwrap_or_default();
    let gap = bubble_width.saturating_sub(left.chars().count() + right.chars().count());

    let mut spans = vec![
        Span::styled(left, header_style),
        Span::styled(" ".repeat(gap), fill_style),
    ];
    if !right.is_empty() {
        spans.push(Span::styled(right, model_style));
    }

    Line::from(spans)
}

fn render_init(lines: &mut Vec<Line<'static>>, model: &str, cwd: &str) {
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" Session Started ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
    ]).alignment(Alignment::Center));
    lines.push(Line::from(vec![
        Span::styled("Model: ", Style::default().fg(Color::DarkGray)),
        Span::styled(model.to_string(), Style::default().fg(AZURE).add_modifier(Modifier::BOLD)),
    ]).alignment(Alignment::Center));
    lines.push(Line::from(vec![
        Span::styled("Path: ", Style::default().fg(Color::DarkGray)),
        Span::styled(cwd.to_string(), Style::default().fg(Color::White)),
    ]).alignment(Alignment::Center));
    lines.push(Line::from(""));
}

fn render_hook(lines: &mut Vec<Line<'static>>, name: &str, output: &str, hook_max: usize) {
    let hook_max = hook_max + 10;
    if !output.trim().is_empty() {
        let prefix_len = 2 + name.len() + 2;
        let output_max = hook_max.saturating_sub(prefix_len);
        let first_line = output.lines().next().unwrap_or("");
        for (i, wrapped) in wrap_text(first_line, output_max).into_iter().enumerate() {
            if i == 0 {
                lines.push(Line::from(vec![
                    Span::styled("› ", Style::default().fg(Color::DarkGray)),
                    Span::styled(name.to_string(), Style::default().fg(Color::DarkGray)),
                    Span::styled(": ", Style::default().fg(Color::DarkGray)),
                    Span::styled(wrapped, Style::default().fg(Color::DarkGray)),
                ]));
            } else {
                let indent = " ".repeat(prefix_len);
                lines.push(Line::from(vec![
                    Span::styled(indent, Style::default()),
                    Span::styled(wrapped, Style::default().fg(Color::DarkGray)),
                ]));
            }
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled("› ", Style::default().fg(Color::DarkGray)),
            Span::styled(name.to_string(), Style::default().fg(Color::DarkGray)),
        ]));
    }
}

fn render_command(lines: &mut Vec<Line<'static>>, name: &str) {
    let cmd_style = Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD);
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("/ ", cmd_style),
        Span::styled(name.to_string(), cmd_style),
    ]));
}

fn render_user_message(lines: &mut Vec<Line<'static>>, content: &str, bubble_width: usize, total_width: usize) {
    lines.push(Line::from(""));
    lines.push(Line::from(""));

    let header = " You ◀ ".to_string();
    let header_pad = " ".repeat(bubble_width.saturating_sub(header.len()));
    let right_offset = total_width.saturating_sub(bubble_width);
    let offset_str = " ".repeat(right_offset);

    lines.push(Line::from(vec![
        Span::raw(offset_str.clone()),
        Span::styled(header_pad, Style::default().bg(AZURE)),
        Span::styled(header, Style::default().fg(Color::Black).bg(AZURE).add_modifier(Modifier::BOLD)),
    ]));

    let content_width = bubble_width.saturating_sub(2);
    for wrapped in wrap_text(content, content_width) {
        let pad = bubble_width.saturating_sub(wrapped.chars().count() + 2);
        lines.push(Line::from(vec![
            Span::raw(offset_str.clone()),
            Span::styled(" ".repeat(pad), Style::default()),
            Span::styled(wrapped, Style::default().fg(Color::White)),
            Span::styled(" │", Style::default().fg(AZURE)),
        ]));
    }

    lines.push(Line::from(vec![
        Span::raw(offset_str),
        Span::styled(format!("{}┘", "─".repeat(bubble_width - 1)), Style::default().fg(AZURE)),
    ]));
}

fn render_tool_call(
    lines: &mut Vec<Line<'static>>,
    animation_indices: &mut Vec<(usize, usize, String)>,
    clickable_paths: &mut Vec<ClickablePath>,
    tool_name: &str,
    file_path: &Option<String>,
    input: &serde_json::Value,
    tool_use_id: &str,
    pending_tools: &HashSet<String>,
    failed_tools: &HashSet<String>,
    bubble_width: usize,
    highlighter: &mut SyntaxHighlighter,
) {
    let tool_color = AZURE;
    let is_pending = pending_tools.contains(tool_use_id);
    let is_failed = failed_tools.contains(tool_use_id);

    lines.push(Line::from(vec![Span::styled(" ┃", Style::default().fg(tool_color))]));

    // Avoid cloning file_path — borrow when available, allocate only for fallback
    let param_owned;
    let param_raw: &str = match file_path {
        Some(fp) => fp.as_str(),
        None => { param_owned = extract_tool_param(tool_name, input); &param_owned }
    };

    // Use placeholder color for pending - will be patched during viewport rendering
    // Note: ◐ can misalign in some fonts, using ○ for pending instead
    let (indicator, indicator_color) = if is_pending {
        ("○ ", Color::White)
    } else if is_failed {
        ("✗ ", Color::Red)
    } else {
        ("● ", Color::Green)
    };

    let display_name = tool_display_name(tool_name);
    let tool_line_max = bubble_width + 10;
    let prefix_len = 3 + 2 + display_name.len() + 2;
    let param_max = tool_line_max.saturating_sub(prefix_len);

    // Edit/Read/Write tools get underlined file paths that are clickable
    let is_file_tool = matches!(tool_name, "Edit" | "Read" | "Write");
    let path_style = if is_file_tool {
        Style::default().fg(ORANGE).add_modifier(Modifier::UNDERLINED)
    } else {
        Style::default().fg(ORANGE)
    };

    let wrapped_param_lines = wrap_text(param_raw, param_max);
    let wrap_line_count = wrapped_param_lines.len();
    for (i, wrapped) in wrapped_param_lines.into_iter().enumerate() {
        if i == 0 {
            // Track line index for draw-time status patching (span index 1 is the indicator).
            // ALL tool calls are tracked (not just pending) so completed/failed status
            // updates immediately without waiting for a full re-render.
            animation_indices.push((lines.len(), 1, tool_use_id.to_string()));
            // Record clickable region for file tools — wrap_line_count tells highlight
            // how many cache lines the path spans (for multi-line highlight)
            if is_file_tool && !param_raw.is_empty() {
                let start_col = prefix_len;
                let end_col = start_col + wrapped.chars().count();
                let (old_s, new_s) = if tool_name == "Edit" {
                    (
                        input.get("old_string").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        input.get("new_string").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    )
                } else { (String::new(), String::new()) };
                clickable_paths.push((lines.len(), start_col, end_col, param_raw.to_string(), old_s, new_s, wrap_line_count));
            }
            lines.push(Line::from(vec![
                Span::styled(" ┣━", Style::default().fg(tool_color)),
                Span::styled(indicator, Style::default().fg(indicator_color)),
                Span::styled(display_name.to_string(), Style::default().fg(tool_color).add_modifier(Modifier::BOLD)),
                Span::styled("  ", Style::default()),
                Span::styled(wrapped, path_style),
            ]));
        } else {
            let indent = " ".repeat(prefix_len);
            lines.push(Line::from(vec![
                Span::styled(indent, Style::default()),
                Span::styled(wrapped, path_style),
            ]));
        }
    }

    let tool_max = bubble_width + 10;
    if tool_name == "Edit" {
        render_edit_diff(lines, input, file_path, tool_color, tool_max, highlighter);
    }
    if tool_name == "Write" {
        render_write_preview(lines, input, tool_color, tool_max);
    }
}

fn render_complete(lines: &mut Vec<Line<'static>>, duration_ms: u64, _cost_usd: f64, success: bool) {
    lines.push(Line::from(""));
    let (status, color) = if success { ("Completed", Color::Green) } else { ("Failed", Color::Red) };
    lines.push(Line::from(vec![
        Span::styled(format!(" ● {} ", status), Style::default().fg(Color::Black).bg(color).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {:.1}s ", duration_ms as f64 / 1000.0), Style::default().fg(Color::White)),
    ]).alignment(Alignment::Center));
    lines.push(Line::from(""));
}

/// Render plan approval prompt when awaiting user response to ExitPlanMode
fn render_plan_approval(lines: &mut Vec<Line<'static>>, width: usize) {
    let color = Color::Yellow;
    let box_width = 50.min(width.saturating_sub(4));

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Top border
    lines.push(Line::from(vec![
        Span::styled(format!("┌{}┐", "─".repeat(box_width.saturating_sub(2))), Style::default().fg(color)),
    ]).alignment(Alignment::Center));

    // Header
    let header = " ⏳ Awaiting Plan Approval ";
    let header_pad = box_width.saturating_sub(header.chars().count() + 2);
    lines.push(Line::from(vec![
        Span::styled("│", Style::default().fg(color)),
        Span::styled(header, Style::default().fg(Color::Black).bg(color).add_modifier(Modifier::BOLD)),
        Span::styled(" ".repeat(header_pad), Style::default().bg(color)),
        Span::styled("│", Style::default().fg(color)),
    ]).alignment(Alignment::Center));

    // Separator
    lines.push(Line::from(vec![
        Span::styled(format!("├{}┤", "─".repeat(box_width.saturating_sub(2))), Style::default().fg(color)),
    ]).alignment(Alignment::Center));

    // Options
    let options = [
        "1. Yes, clear context and bypass permissions",
        "2. Yes, and manually approve edits",
        "3. Yes, and bypass permissions",
        "4. Yes, manually approve edits",
        "5. Type to tell Claude what to change",
    ];

    for opt in &options {
        let pad = box_width.saturating_sub(opt.chars().count() + 4);
        lines.push(Line::from(vec![
            Span::styled("│ ", Style::default().fg(color)),
            Span::styled(opt.to_string(), Style::default().fg(Color::White)),
            Span::styled(format!("{} │", " ".repeat(pad)), Style::default().fg(color)),
        ]).alignment(Alignment::Center));
    }

    // Bottom border
    lines.push(Line::from(vec![
        Span::styled(format!("└{}┘", "─".repeat(box_width.saturating_sub(2))), Style::default().fg(color)),
    ]).alignment(Alignment::Center));
}

/// Render AskUserQuestion tool call as a numbered options box.
/// Input structure: { "questions": [{ "question": "...", "header": "...",
///   "options": [{ "label": "...", "description": "..." }], "multiSelect": bool }] }
fn render_ask_user_question(lines: &mut Vec<Line<'static>>, input: &serde_json::Value, width: usize) {
    let color = Color::Magenta;
    let Some(questions) = input.get("questions").and_then(|v| v.as_array()) else { return };

    for q in questions {
        let question = q.get("question").and_then(|v| v.as_str()).unwrap_or("?");
        let options = q.get("options").and_then(|v| v.as_array());
        let multi = q.get("multiSelect").and_then(|v| v.as_bool()).unwrap_or(false);

        // Box width: fit content or cap at panel width
        let box_width = 60.min(width.saturating_sub(4));

        lines.push(Line::from(""));
        lines.push(Line::from(""));

        // Top border
        lines.push(Line::from(vec![
            Span::styled(format!("┌{}┐", "─".repeat(box_width.saturating_sub(2))), Style::default().fg(color)),
        ]).alignment(Alignment::Center));

        // Header with question text (wrap if needed)
        let header_icon = if multi { "☑ " } else { "❓ " };
        let header_max = box_width.saturating_sub(4 + header_icon.len());
        for (i, chunk) in wrap_text(question, header_max).into_iter().enumerate() {
            let prefix = if i == 0 { header_icon } else { "   " };
            let text = format!("{}{}", prefix, chunk);
            let pad = box_width.saturating_sub(text.chars().count() + 2);
            lines.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(color)),
                Span::styled(text, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{} │", " ".repeat(pad)), Style::default().fg(color)),
            ]).alignment(Alignment::Center));
        }

        // Separator
        lines.push(Line::from(vec![
            Span::styled(format!("├{}┤", "─".repeat(box_width.saturating_sub(2))), Style::default().fg(color)),
        ]).alignment(Alignment::Center));

        // Numbered options
        if let Some(opts) = options {
            for (idx, opt) in opts.iter().enumerate() {
                let label = opt.get("label").and_then(|v| v.as_str()).unwrap_or("?");
                let desc = opt.get("description").and_then(|v| v.as_str());
                // Option label line
                let opt_text = format!("{}. {}", idx + 1, label);
                let pad = box_width.saturating_sub(opt_text.chars().count() + 4);
                lines.push(Line::from(vec![
                    Span::styled("│ ", Style::default().fg(color)),
                    Span::styled(opt_text, Style::default().fg(AZURE)),
                    Span::styled(format!("{} │", " ".repeat(pad)), Style::default().fg(color)),
                ]).alignment(Alignment::Center));
                // Option description (dimmer, indented)
                if let Some(d) = desc {
                    let indent = "   ";
                    let desc_max = box_width.saturating_sub(4 + indent.len());
                    for chunk in wrap_text(d, desc_max) {
                        let text = format!("{}{}", indent, chunk);
                        let pad = box_width.saturating_sub(text.chars().count() + 4);
                        lines.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(color)),
                            Span::styled(text, Style::default().fg(Color::DarkGray)),
                            Span::styled(format!("{} │", " ".repeat(pad)), Style::default().fg(color)),
                        ]).alignment(Alignment::Center));
                    }
                }
            }
        }

        // "Other" note
        let other_text = format!("{}. Other (type your answer)", options.map(|o| o.len() + 1).unwrap_or(1));
        let pad = box_width.saturating_sub(other_text.chars().count() + 4);
        lines.push(Line::from(vec![
            Span::styled("│ ", Style::default().fg(color)),
            Span::styled(other_text, Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{} │", " ".repeat(pad)), Style::default().fg(color)),
        ]).alignment(Alignment::Center));

        // Bottom border
        lines.push(Line::from(vec![
            Span::styled(format!("└{}┘", "─".repeat(box_width.saturating_sub(2))), Style::default().fg(color)),
        ]).alignment(Alignment::Center));
    }
}

/// Render a plan block with prominent full-width styling and markdown highlighting
fn render_plan(lines: &mut Vec<Line<'static>>, name: &str, content: &str, width: usize) {
    let plan_color = Color::Green;
    let header_bg = Color::Green;
    let border = "═";
    let content_width = width.saturating_sub(4);

    // Spacing before plan
    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Top border
    lines.push(Line::from(vec![
        Span::styled(format!("╔{}╗", border.repeat(width.saturating_sub(2))), Style::default().fg(plan_color).add_modifier(Modifier::BOLD)),
    ]));

    // Header with plan icon and name
    let header = format!(" 📋 PLAN MODE: {} ", name);
    let header_pad = width.saturating_sub(header.chars().count() + 2);
    lines.push(Line::from(vec![
        Span::styled("║", Style::default().fg(plan_color).add_modifier(Modifier::BOLD)),
        Span::styled(header, Style::default().fg(Color::Black).bg(header_bg).add_modifier(Modifier::BOLD)),
        Span::styled(" ".repeat(header_pad), Style::default().bg(header_bg)),
        Span::styled("║", Style::default().fg(plan_color).add_modifier(Modifier::BOLD)),
    ]));

    // Separator under header
    lines.push(Line::from(vec![
        Span::styled(format!("╠{}╣", "─".repeat(width.saturating_sub(2))), Style::default().fg(plan_color)),
    ]));

    // Render markdown content with box borders
    let text_lines: Vec<&str> = content.lines().collect();
    let mut in_code_block = false;

    // Helper to push a line with box borders and padding
    let push_boxed = |lines: &mut Vec<Line<'static>>, mut spans: Vec<Span<'static>>, char_count: usize| {
        let pad = content_width.saturating_sub(char_count);
        spans.insert(0, Span::styled("║ ", Style::default().fg(plan_color)));
        spans.push(Span::styled(format!("{} ║", " ".repeat(pad)), Style::default().fg(plan_color)));
        lines.push(Line::from(spans));
    };

    for line in &text_lines {
        let trimmed = line.trim();

        // Code block delimiters
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            let lang = trimmed.trim_start_matches('`').trim();
            let (marker, char_len) = if in_code_block && !lang.is_empty() {
                (vec![
                    Span::styled("┌─ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(lang.to_string(), Style::default().fg(AZURE)),
                    Span::styled(" ─", Style::default().fg(Color::DarkGray)),
                ], 5 + lang.chars().count())
            } else if !in_code_block {
                (vec![Span::styled("└──────", Style::default().fg(Color::DarkGray))], 7)
            } else {
                (vec![Span::styled("┌──────", Style::default().fg(Color::DarkGray))], 7)
            };
            push_boxed(lines, marker, char_len);
            continue;
        }

        // Code block content
        if in_code_block {
            for wrapped in wrap_text(line, content_width.saturating_sub(2)) {
                let spans = vec![
                    Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(wrapped.clone(), Style::default().fg(Color::Yellow)),
                ];
                push_boxed(lines, spans, 2 + wrapped.chars().count());
            }
            continue;
        }

        // Headers
        if trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|&c| c == '#').count();
            let text = trimmed.trim_start_matches('#').trim();
            let (prefix, style) = match level {
                1 => ("█ ", Style::default().fg(AZURE).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)),
                2 => ("▓ ", Style::default().fg(AZURE).add_modifier(Modifier::BOLD)),
                3 => ("▒ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                _ => ("░ ", Style::default().fg(Color::Green)),
            };
            for (i, wrapped) in wrap_text(text, content_width.saturating_sub(2)).into_iter().enumerate() {
                let spans = if i == 0 {
                    vec![Span::styled(prefix, style), Span::styled(wrapped.clone(), style)]
                } else {
                    vec![Span::styled("  ", Style::default()), Span::styled(wrapped.clone(), style)]
                };
                push_boxed(lines, spans, 2 + wrapped.chars().count());
            }
            continue;
        }

        // Bullet lists
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• ") {
            let bullet_content = trimmed.trim_start_matches("- ").trim_start_matches("* ").trim_start_matches("• ");
            for (i, wrapped) in wrap_text(bullet_content, content_width.saturating_sub(4)).into_iter().enumerate() {
                let mut spans = if i == 0 {
                    vec![Span::styled("  • ", Style::default().fg(AZURE))]
                } else {
                    vec![Span::styled("    ", Style::default())]
                };
                spans.extend(parse_markdown_spans(&wrapped, Style::default().fg(Color::White)));
                push_boxed(lines, spans, 4 + wrapped.chars().count());
            }
            continue;
        }

        // Numbered lists
        if trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            if let Some(dot_pos) = trimmed.find(". ") {
                let num = &trimmed[..dot_pos];
                let content_text = &trimmed[dot_pos + 2..];
                let prefix = format!("  {}. ", num);
                let prefix_len = prefix.chars().count();
                for (i, wrapped) in wrap_text(content_text, content_width.saturating_sub(prefix_len)).into_iter().enumerate() {
                    let mut spans = if i == 0 {
                        vec![Span::styled(prefix.clone(), Style::default().fg(AZURE))]
                    } else {
                        vec![Span::styled(" ".repeat(prefix_len), Style::default())]
                    };
                    spans.extend(parse_markdown_spans(&wrapped, Style::default().fg(Color::White)));
                    push_boxed(lines, spans, prefix_len + wrapped.chars().count());
                }
                continue;
            }
        }

        // Blockquotes
        if trimmed.starts_with("> ") {
            let quote_content = trimmed.trim_start_matches("> ");
            for wrapped in wrap_text(quote_content, content_width.saturating_sub(2)) {
                let mut spans = vec![Span::styled("┃ ", Style::default().fg(Color::DarkGray))];
                spans.extend(parse_markdown_spans(&wrapped, Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)));
                push_boxed(lines, spans, 2 + wrapped.chars().count());
            }
            continue;
        }

        // Regular paragraph text with inline markdown
        if trimmed.is_empty() {
            push_boxed(lines, vec![], 0);
        } else {
            for wrapped in wrap_text(line, content_width) {
                let spans = parse_markdown_spans(&wrapped, Style::default().fg(Color::White));
                let char_count = wrapped.chars().count();
                push_boxed(lines, spans, char_count);
            }
        }
    }

    // Bottom border
    lines.push(Line::from(vec![
        Span::styled(format!("╚{}╝", border.repeat(width.saturating_sub(2))), Style::default().fg(plan_color).add_modifier(Modifier::BOLD)),
    ]));

    lines.push(Line::from(""));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Verifies render_ask_user_question produces visible lines with
    /// box borders, question text, numbered options, and an "Other" entry.
    /// This test exists because the rendering is the user-facing presentation
    /// of AskUserQuestion — if box drawing or numbering is wrong, the user
    /// can't correctly select options.
    #[test]
    fn test_render_ask_user_question_basic_structure() {
        let input = json!({
            "questions": [{
                "question": "Which approach?",
                "header": "Approach",
                "options": [
                    {"label": "Option A", "description": "First choice"},
                    {"label": "Option B", "description": "Second choice"}
                ],
                "multiSelect": false
            }]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);

        // Flatten all span content into strings for assertion
        let text: Vec<String> = lines.iter().map(|l| {
            l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()
        }).collect();

        // Box borders present
        assert!(text.iter().any(|l| l.contains('┌') && l.contains('┐')), "Missing top border");
        assert!(text.iter().any(|l| l.contains('└') && l.contains('┘')), "Missing bottom border");
        assert!(text.iter().any(|l| l.contains('├') && l.contains('┤')), "Missing separator");

        // Question text visible
        assert!(text.iter().any(|l| l.contains("Which approach?")), "Missing question text");

        // Numbered options
        assert!(text.iter().any(|l| l.contains("1. Option A")), "Missing option 1");
        assert!(text.iter().any(|l| l.contains("2. Option B")), "Missing option 2");

        // Descriptions visible
        assert!(text.iter().any(|l| l.contains("First choice")), "Missing option 1 description");
        assert!(text.iter().any(|l| l.contains("Second choice")), "Missing option 2 description");

        // Other option present
        assert!(text.iter().any(|l| l.contains("3. Other")), "Missing Other option");
    }

    /// Verifies multi-select annotation appears in the header.
    #[test]
    fn test_render_ask_user_question_multi_select_icon() {
        let input = json!({
            "questions": [{
                "question": "Select features",
                "options": [{"label": "A", "description": ""}],
                "multiSelect": true
            }]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);
        let text: Vec<String> = lines.iter().map(|l| {
            l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()
        }).collect();
        // Multi-select uses ☑ icon instead of ❓
        assert!(text.iter().any(|l| l.contains('☑')), "Multi-select should show checkbox icon");
    }

    /// Verifies empty questions array produces no output (no panic).
    #[test]
    fn test_render_ask_user_question_empty() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &json!({}), 80);
        assert!(lines.is_empty(), "Empty input should produce no lines");
        render_ask_user_question(&mut lines, &json!({"questions": []}), 80);
        assert!(lines.is_empty(), "Empty questions array should produce no lines");
    }

    /// Verifies narrow width doesn't panic or produce garbled output.
    /// This tests the wrapping logic with constrained box width.
    #[test]
    fn test_render_ask_user_question_narrow_width() {
        let input = json!({
            "questions": [{
                "question": "A very long question that should wrap within the narrow box width to test text wrapping behavior",
                "options": [{"label": "Short", "description": "Also a description"}],
                "multiSelect": false
            }]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        // Minimum usable width (box_width = 60.min(width-4) = 16)
        render_ask_user_question(&mut lines, &input, 20);
        assert!(!lines.is_empty(), "Should produce output even at narrow width");
    }

    // ── Helper to flatten Line spans into a single string ───────────────

    fn lines_to_text(lines: &[Line<'static>]) -> Vec<String> {
        lines.iter().map(|l| {
            l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()
        }).collect()
    }

    // ── render_ask_user_question extended tests ─────────────────────────

    /// Multiple questions each get their own box.
    #[test]
    fn test_render_ask_multi_questions_separate_boxes() {
        let input = json!({
            "questions": [
                {"question": "First?", "options": [{"label": "A", "description": ""}], "multiSelect": false},
                {"question": "Second?", "options": [{"label": "B", "description": ""}], "multiSelect": false}
            ]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);
        let text = lines_to_text(&lines);
        // Should have two top borders (one per question)
        let top_borders = text.iter().filter(|l| l.contains('┌') && l.contains('┐')).count();
        assert_eq!(top_borders, 2, "Each question should have its own top border");
    }

    /// Question with no description shows only label.
    #[test]
    fn test_render_ask_no_description() {
        let input = json!({
            "questions": [{
                "question": "Pick?",
                "options": [{"label": "Yes"}],
                "multiSelect": false
            }]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("1. Yes")));
    }

    /// Option with empty description is handled.
    #[test]
    fn test_render_ask_empty_description() {
        let input = json!({
            "questions": [{
                "question": "Q?",
                "options": [{"label": "Opt", "description": ""}],
                "multiSelect": false
            }]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("1. Opt")));
    }

    /// Questions with null options field produces no numbered options.
    #[test]
    fn test_render_ask_null_options() {
        let input = json!({
            "questions": [{"question": "Free form?", "options": null, "multiSelect": false}]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Free form?")));
        // "Other" should still be present with number 1
        assert!(text.iter().any(|l| l.contains("1. Other")));
    }

    /// "Other" option number is correct with 5 options.
    #[test]
    fn test_render_ask_other_number_five_options() {
        let options: Vec<serde_json::Value> = (1..=5)
            .map(|i| json!({"label": format!("Opt{}", i), "description": ""}))
            .collect();
        let input = json!({
            "questions": [{"question": "Q?", "options": options, "multiSelect": false}]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("6. Other")));
    }

    /// Very wide width doesn't cause issues.
    #[test]
    fn test_render_ask_wide_width() {
        let input = json!({
            "questions": [{"question": "Q?", "options": [{"label": "A", "description": "desc"}], "multiSelect": false}]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 500);
        assert!(!lines.is_empty());
    }

    /// Width of 4 (minimum before box_width = 0).
    #[test]
    fn test_render_ask_minimum_width() {
        let input = json!({
            "questions": [{"question": "Q?", "options": [], "multiSelect": false}]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 4);
        assert!(!lines.is_empty());
    }

    /// Width of 0 should not panic.
    #[test]
    fn test_render_ask_zero_width() {
        let input = json!({
            "questions": [{"question": "Q?", "options": [], "multiSelect": false}]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 0);
        assert!(!lines.is_empty());
    }

    /// Unicode in option labels.
    #[test]
    fn test_render_ask_unicode_labels() {
        let input = json!({
            "questions": [{
                "question": "言語?",
                "options": [{"label": "日本語", "description": "Japanese"}, {"label": "中文", "description": "Chinese"}],
                "multiSelect": false
            }]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("1. 日本語")));
        assert!(text.iter().any(|l| l.contains("2. 中文")));
    }

    /// Long description wraps within box.
    #[test]
    fn test_render_ask_long_description_wraps() {
        let long_desc = "This is a very long description that should definitely wrap across multiple lines within the constrained box width boundary.";
        let input = json!({
            "questions": [{
                "question": "Q?",
                "options": [{"label": "A", "description": long_desc}],
                "multiSelect": false
            }]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 60);
        let text = lines_to_text(&lines);
        // The description should be split across multiple lines
        let desc_lines: Vec<&String> = text.iter().filter(|l| l.contains("description") || l.contains("definitely") || l.contains("boundary")).collect();
        assert!(!desc_lines.is_empty(), "Long description should be present");
    }

    /// Missing label falls back to "?".
    #[test]
    fn test_render_ask_missing_label() {
        let input = json!({
            "questions": [{
                "question": "Q?",
                "options": [{"description": "no label here"}],
                "multiSelect": false
            }]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("1. ?")));
    }

    // ── render_init tests ───────────────────────────────────────────────

    /// render_init produces expected structure with model and cwd.
    #[test]
    fn test_render_init_basic() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_init(&mut lines, "claude-opus-4-20250514", "/home/user/project");
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Session Started")));
        assert!(text.iter().any(|l| l.contains("claude-opus-4-20250514")));
        assert!(text.iter().any(|l| l.contains("/home/user/project")));
    }

    /// render_init with empty model string.
    #[test]
    fn test_render_init_empty_model() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_init(&mut lines, "", "/path");
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Model:")));
    }

    /// render_init with empty cwd.
    #[test]
    fn test_render_init_empty_cwd() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_init(&mut lines, "model", "");
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Path:")));
    }

    /// render_init produces exactly 5 lines (empty, header, model, path, empty).
    #[test]
    fn test_render_init_line_count() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_init(&mut lines, "model", "/cwd");
        assert_eq!(lines.len(), 5);
    }

    /// render_init with unicode model name.
    #[test]
    fn test_render_init_unicode_model() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_init(&mut lines, "テスト-model", "/path");
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("テスト-model")));
    }

    // ── render_hook tests ───────────────────────────────────────────────

    /// Hook with output renders name and output.
    #[test]
    fn test_render_hook_with_output() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_hook(&mut lines, "pre-commit", "All checks passed", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("pre-commit") && l.contains("All checks passed")));
    }

    /// Hook with empty output renders only name.
    #[test]
    fn test_render_hook_empty_output() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_hook(&mut lines, "post-checkout", "", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("post-checkout")));
    }

    /// Hook with whitespace-only output renders only name.
    #[test]
    fn test_render_hook_whitespace_output() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_hook(&mut lines, "hook-name", "   \t  ", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("hook-name")));
        // Whitespace-only is trimmed, so output is treated as empty
    }

    /// Hook with multiline output only shows first line.
    #[test]
    fn test_render_hook_multiline_output() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_hook(&mut lines, "lint", "Line 1\nLine 2\nLine 3", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Line 1")));
    }

    /// Hook with narrow width wraps output.
    #[test]
    fn test_render_hook_narrow_wraps() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_hook(&mut lines, "h", "A long output string that should wrap", 20);
        assert!(lines.len() >= 1);
    }

    /// Hook name contains special chars.
    #[test]
    fn test_render_hook_special_name() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_hook(&mut lines, "pre-push/main", "ok", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("pre-push/main")));
    }

    // ── render_command tests ────────────────────────────────────────────

    /// Command renders with / prefix.
    #[test]
    fn test_render_command_basic() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_command(&mut lines, "compact");
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("/ ") && l.contains("compact")));
    }

    /// Command produces exactly 2 lines (empty + command).
    #[test]
    fn test_render_command_line_count() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_command(&mut lines, "test");
        assert_eq!(lines.len(), 2);
    }

    /// Command with empty name.
    #[test]
    fn test_render_command_empty_name() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_command(&mut lines, "");
        assert_eq!(lines.len(), 2);
    }

    // ── render_user_message tests ───────────────────────────────────────

    /// User message renders with "You" header.
    #[test]
    fn test_render_user_message_basic() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_user_message(&mut lines, "Hello Claude", 40, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("You")));
        assert!(text.iter().any(|l| l.contains("Hello Claude")));
    }

    /// User message renders bottom border.
    #[test]
    fn test_render_user_message_bottom_border() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_user_message(&mut lines, "test", 40, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains('┘')));
    }

    /// User message with empty content.
    #[test]
    fn test_render_user_message_empty() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_user_message(&mut lines, "", 40, 80);
        assert!(!lines.is_empty());
    }

    /// User message wraps long text.
    #[test]
    fn test_render_user_message_wraps() {
        let long = "A ".repeat(100);
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_user_message(&mut lines, &long, 40, 80);
        // Should have more lines due to wrapping
        assert!(lines.len() > 5);
    }

    /// User message with unicode.
    #[test]
    fn test_render_user_message_unicode() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_user_message(&mut lines, "こんにちは世界", 40, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("こんにちは世界")));
    }

    /// User message with newlines.
    #[test]
    fn test_render_user_message_newlines() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_user_message(&mut lines, "Line1\nLine2\nLine3", 40, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Line1")));
    }

    /// User message at minimum bubble width.
    #[test]
    fn test_render_user_message_min_bubble() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_user_message(&mut lines, "Hi", 5, 10);
        assert!(!lines.is_empty());
    }

    // ── render_complete tests ───────────────────────────────────────────

    /// Successful completion renders green Completed.
    #[test]
    fn test_render_complete_success() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_complete(&mut lines, 5000, 0.0123, true);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Completed")));
        assert!(text.iter().any(|l| l.contains("5.0s")));
    }

    /// Failed completion renders red Failed.
    #[test]
    fn test_render_complete_failure() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_complete(&mut lines, 1000, 0.05, false);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Failed")));
    }

    /// Zero duration.
    #[test]
    fn test_render_complete_zero_duration() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_complete(&mut lines, 0, 0.0, true);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("0.0s")));
    }

    /// Large duration in milliseconds.
    #[test]
    fn test_render_complete_large_duration() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_complete(&mut lines, 120000, 1.5, true);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("120.0s")));
    }

    /// Zero cost (cost not rendered, just ensure no panic).
    #[test]
    fn test_render_complete_zero_cost() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_complete(&mut lines, 100, 0.0, true);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Completed")));
    }

    /// Produces exactly 3 lines (empty, content, empty).
    #[test]
    fn test_render_complete_line_count() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_complete(&mut lines, 1000, 0.01, true);
        assert_eq!(lines.len(), 3);
    }

    // ── render_plan_approval tests ──────────────────────────────────────

    /// Plan approval renders all 5 options.
    #[test]
    fn test_render_plan_approval_all_options() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan_approval(&mut lines, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("1. Yes, clear context")));
        assert!(text.iter().any(|l| l.contains("2. Yes, and manually")));
        assert!(text.iter().any(|l| l.contains("3. Yes, and bypass")));
        assert!(text.iter().any(|l| l.contains("4. Yes, manually")));
        assert!(text.iter().any(|l| l.contains("5. Type to tell")));
    }

    /// Plan approval header present.
    #[test]
    fn test_render_plan_approval_header() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan_approval(&mut lines, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Awaiting Plan Approval")));
    }

    /// Plan approval has box borders.
    #[test]
    fn test_render_plan_approval_borders() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan_approval(&mut lines, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains('┌')));
        assert!(text.iter().any(|l| l.contains('└')));
    }

    /// Plan approval at narrow width doesn't panic.
    #[test]
    fn test_render_plan_approval_narrow() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan_approval(&mut lines, 10);
        assert!(!lines.is_empty());
    }

    /// Plan approval at zero width doesn't panic.
    #[test]
    fn test_render_plan_approval_zero_width() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan_approval(&mut lines, 0);
        assert!(!lines.is_empty());
    }

    // ── render_plan tests ───────────────────────────────────────────────

    /// Plan renders with double-line box borders.
    #[test]
    fn test_render_plan_borders() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "My Plan", "Step 1\nStep 2", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains('╔')));
        assert!(text.iter().any(|l| l.contains('╚')));
    }

    /// Plan renders name in header.
    #[test]
    fn test_render_plan_name_in_header() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "Refactor", "content", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("PLAN MODE: Refactor")));
    }

    /// Plan with empty content.
    #[test]
    fn test_render_plan_empty_content() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "Empty", "", 80);
        assert!(!lines.is_empty());
    }

    /// Plan with markdown headers.
    #[test]
    fn test_render_plan_markdown_headers() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "P", "# Title\n## Subtitle\n### Section", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Title")));
        assert!(text.iter().any(|l| l.contains("Subtitle")));
        assert!(text.iter().any(|l| l.contains("Section")));
    }

    /// Plan with bullet list.
    #[test]
    fn test_render_plan_bullets() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "P", "- Item one\n- Item two", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Item one")));
        assert!(text.iter().any(|l| l.contains("Item two")));
    }

    /// Plan with numbered list.
    #[test]
    fn test_render_plan_numbered_list() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "P", "1. First\n2. Second", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("First")));
        assert!(text.iter().any(|l| l.contains("Second")));
    }

    /// Plan with code block.
    #[test]
    fn test_render_plan_code_block() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "P", "```rust\nfn main() {}\n```", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("rust")));
        assert!(text.iter().any(|l| l.contains("fn main()")));
    }

    /// Plan with blockquote.
    #[test]
    fn test_render_plan_blockquote() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "P", "> A quoted line", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("A quoted line")));
    }

    /// Plan at very narrow width.
    #[test]
    fn test_render_plan_narrow() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "P", "Some content here", 10);
        assert!(!lines.is_empty());
    }

    // ── render_display_events integration tests ─────────────────────────

    /// Empty events produces empty output.
    #[test]
    fn test_render_events_empty() {
        let mut highlighter = SyntaxHighlighter::new();
        let (lines, anim, bubbles, clicks, _tables) = render_display_events(
            &[], 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        assert!(lines.is_empty());
        assert!(anim.is_empty());
        assert!(bubbles.is_empty());
        assert!(clicks.is_empty());
    }

    /// Single Init event renders session started.
    #[test]
    fn test_render_events_init() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::Init {
            _session_id: "s1".into(),
            cwd: "/project".into(),
            model: "claude-opus-4-20250514".into(),
        }];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Session Started")));
    }

    /// Duplicate Init events are deduplicated.
    #[test]
    fn test_render_events_dedup_init() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![
            DisplayEvent::Init { _session_id: "s1".into(), cwd: "/a".into(), model: "m".into() },
            DisplayEvent::Init { _session_id: "s2".into(), cwd: "/b".into(), model: "m2".into() },
        ];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        let text = lines_to_text(&lines);
        // Only one "Session Started" should appear
        let count = text.iter().filter(|l| l.contains("Session Started")).count();
        assert_eq!(count, 1);
    }

    /// UserMessage renders with bubble position tracked.
    #[test]
    fn test_render_events_user_message_bubble() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::UserMessage {
            _uuid: "u1".into(),
            content: "Hello".into(),
        }];
        let (lines, _, bubbles, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        assert!(!lines.is_empty());
        assert_eq!(bubbles.len(), 1);
        assert!(bubbles[0].1, "User message bubble should be marked as user");
    }

    /// AssistantText renders with bubble position tracked.
    #[test]
    fn test_render_events_assistant_bubble() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::AssistantText {
            _uuid: "a1".into(),
            _message_id: "m1".into(),
            text: "I'll help you.".into(),
        }];
        let (lines, _, bubbles, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        assert!(!lines.is_empty());
        assert_eq!(bubbles.len(), 1);
        assert!(!bubbles[0].1, "Assistant bubble should NOT be marked as user");
    }

    #[test]
    fn test_render_events_codex_assistant_header_uses_codex_label() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![
            DisplayEvent::Init {
                _session_id: "s1".into(),
                cwd: "/project".into(),
                model: "gpt-5.4".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: "a1".into(),
                _message_id: "m1".into(),
                text: "I can help with that.".into(),
            },
        ];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|line| line.contains("Codex")));
    }

    #[test]
    fn test_render_events_codex_assistant_header_right_aligns_model() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![
            DisplayEvent::Init {
                _session_id: "s1".into(),
                cwd: "/project".into(),
                model: "gpt-5.4".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: "a1".into(),
                _message_id: "m1".into(),
                text: "I can help with that.".into(),
            },
        ];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        let header = lines_to_text(&lines)
            .into_iter()
            .find(|line| line.contains("Codex"))
            .expect("assistant header line");
        assert!(header.starts_with(" Codex ▶ "));
        assert!(header.ends_with(" gpt-5.4 "));
        assert_eq!(header.chars().count(), (80usize * 2 / 3).max(40));
    }

    #[test]
    fn test_render_events_codex_assistant_header_uses_cyan() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![
            DisplayEvent::Init {
                _session_id: "s1".into(),
                cwd: "/project".into(),
                model: "gpt-5.4".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: "a1".into(),
                _message_id: "m1".into(),
                text: "I can help with that.".into(),
            },
        ];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        let codex_header = lines.iter().find_map(|line| {
            line.spans
                .iter()
                .find(|span| span.content.contains("Codex"))
                .map(|span| span.style.bg)
        });
        assert_eq!(codex_header, Some(Some(Color::Cyan)));
    }

    #[test]
    fn test_render_assistant_header_line_truncates_model_to_fit() {
        let line = render_assistant_header_line(Some("claude-opus-4-6-extra-long-model"), 16);
        let text = line.spans.iter().map(|span| span.content.as_ref()).collect::<String>();
        assert_eq!(text.chars().count(), 16);
        assert!(text.starts_with(" Claude ▶ "));
        assert!(text.ends_with(" cl… "));
    }

    #[test]
    fn test_render_assistant_header_line_dims_model_span() {
        let line = render_assistant_header_line(Some("gpt-5.4"), 24);
        let model_span = line
            .spans
            .iter()
            .find(|span| span.content.contains("gpt-5.4"))
            .expect("expected model span");
        assert!(model_span.style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn test_render_events_incremental_preserves_codex_identity() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::AssistantText {
            _uuid: "a1".into(),
            _message_id: "m1".into(),
            text: "Incremental render".into(),
        }];
        let pre_scan = super::super::render_thread::PreScanState {
            saw_init: true,
            saw_content: false,
            current_model: Some("gpt-5.4".into()),
            last_hook: None,
            saw_exit_plan_mode: false,
            saw_user_after_exit_plan: false,
            saw_ask_user_question: false,
            saw_user_after_ask: false,
            last_ask_input: None,
        };
        let (lines, _, _, _, _) = render_display_events_incremental(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None, pre_scan,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|line| line.contains("Codex")));
    }

    /// Hook event renders hook name.
    #[test]
    fn test_render_events_hook() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::Hook {
            name: "pre-tool-use".into(),
            output: "approved".into(),
        }];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("pre-tool-use")));
    }

    /// Duplicate consecutive hooks are deduplicated.
    #[test]
    fn test_render_events_dedup_hooks() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![
            DisplayEvent::Hook { name: "hook".into(), output: "out".into() },
            DisplayEvent::Hook { name: "hook".into(), output: "out".into() },
        ];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        let text = lines_to_text(&lines);
        let hook_count = text.iter().filter(|l| l.contains("hook")).count();
        assert_eq!(hook_count, 1, "Duplicate hooks should be deduplicated");
    }

    /// Command event renders with slash.
    #[test]
    fn test_render_events_command() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::Command { name: "compact".into() }];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("compact")));
    }

    /// Compacting event renders banner.
    #[test]
    fn test_render_events_compacting() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::Compacting];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Context compacted")));
    }

    /// Compacted event renders banner.
    #[test]
    fn test_render_events_compacted() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::Compacted];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Context compacted")));
    }

    /// MayBeCompacting event renders warning banner.
    #[test]
    fn test_render_events_may_be_compacting() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::MayBeCompacting];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("compacting")));
    }

    /// Complete event renders duration and cost.
    #[test]
    fn test_render_events_complete() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::Complete {
            _session_id: "s1".into(),
            success: true,
            duration_ms: 3500,
            cost_usd: 0.02,
        }];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Completed")));
        assert!(text.iter().any(|l| l.contains("3.5s")));
    }

    /// Filtered event produces no output.
    #[test]
    fn test_render_events_filtered() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::Filtered];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        assert!(lines.is_empty());
    }

    /// Pending user message renders at the end.
    #[test]
    fn test_render_events_pending_user_message() {
        let mut highlighter = SyntaxHighlighter::new();
        let (lines, _, bubbles, _, _) = render_display_events(
            &[], 80, &HashSet::new(), &HashSet::new(), &mut highlighter, Some("Waiting..."),
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Waiting...")));
        assert_eq!(bubbles.len(), 1);
        assert!(bubbles[0].1); // user bubble
    }

    /// TodoWrite tool calls are skipped in rendering.
    #[test]
    fn test_render_events_todowrite_skipped() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::ToolCall {
            _uuid: "u1".into(),
            tool_use_id: "t1".into(),
            tool_name: "TodoWrite".into(),
            file_path: None,
            input: json!({"todos": []}),
        }];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        assert!(lines.is_empty(), "TodoWrite tool calls should be skipped");
    }

    /// TodoWrite results are also skipped.
    #[test]
    fn test_render_events_todowrite_result_skipped() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::ToolResult {
            tool_use_id: "t1".into(),
            tool_name: "TodoWrite".into(),
            file_path: None,
            content: "Todos have been modified successfully".into(),
            is_error: false,
        }];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        assert!(lines.is_empty(), "TodoWrite results should be skipped");
    }

    /// ToolCall with pending status gets animation index.
    #[test]
    fn test_render_events_tool_pending_animation() {
        let mut highlighter = SyntaxHighlighter::new();
        let mut pending = HashSet::new();
        pending.insert("tool1".to_string());
        let events = vec![DisplayEvent::ToolCall {
            _uuid: "u1".into(),
            tool_use_id: "tool1".into(),
            tool_name: "Read".into(),
            file_path: Some("/path/file.rs".into()),
            input: json!({"file_path": "/path/file.rs"}),
        }];
        let (_, anim, _, _, _) = render_display_events(
            &events, 80, &pending, &HashSet::new(), &mut highlighter, None,
        );
        assert!(!anim.is_empty(), "Pending tool should have animation index");
    }

    /// Plan event renders plan content.
    #[test]
    fn test_render_events_plan() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::Plan {
            name: "Implementation".into(),
            content: "Step 1: Do thing".into(),
        }];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("PLAN MODE")));
        assert!(text.iter().any(|l| l.contains("Step 1")));
    }

    /// Compaction summary in user message renders as banner instead.
    #[test]
    fn test_render_events_compaction_summary_replaced() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::UserMessage {
            _uuid: "u1".into(),
            content: "This session is being continued from a previous conversation. Here is a summary...".into(),
        }];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Context compacted")));
        // The raw compaction text should NOT appear
        assert!(!text.iter().any(|l| l.contains("Here is a summary")));
    }

    /// Multiple event types in sequence.
    #[test]
    fn test_render_events_mixed_sequence() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![
            DisplayEvent::Init { _session_id: "s".into(), cwd: "/p".into(), model: "m".into() },
            DisplayEvent::UserMessage { _uuid: "u".into(), content: "Do something".into() },
            DisplayEvent::AssistantText { _uuid: "a".into(), _message_id: "m".into(), text: "Sure!".into() },
            DisplayEvent::Complete { _session_id: "s".into(), success: true, duration_ms: 1000, cost_usd: 0.01 },
        ];
        let (lines, _, bubbles, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        assert!(!lines.is_empty());
        assert_eq!(bubbles.len(), 2); // user + assistant
    }

    /// Init after content is suppressed.
    #[test]
    fn test_render_events_init_after_content_suppressed() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![
            DisplayEvent::UserMessage { _uuid: "u".into(), content: "Hi".into() },
            DisplayEvent::Init { _session_id: "s".into(), cwd: "/p".into(), model: "m".into() },
        ];
        let (lines, _, _, _, _) = render_display_events(
            &events, 80, &HashSet::new(), &HashSet::new(), &mut highlighter, None,
        );
        let text = lines_to_text(&lines);
        // Init should be suppressed since content was already seen
        assert!(!text.iter().any(|l| l.contains("Session Started")));
    }
}
