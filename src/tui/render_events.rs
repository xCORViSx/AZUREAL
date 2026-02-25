//! Display event rendering for TUI
//!
//! Thin orchestrator that dispatches to specialized renderers.

use ratatui::{
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::collections::HashSet;

use crate::events::DisplayEvent;
use crate::syntax::SyntaxHighlighter;
use super::colorize::ORANGE;
use super::util::AZURE;
use super::markdown::parse_markdown_spans;
use super::render_markdown::render_assistant_text;
use super::render_tools::{extract_tool_param, render_tool_result, render_edit_diff, render_write_preview, tool_display_name};
use super::render_wrap::wrap_text;

/// Clickable path entry: (line_idx, start_col, end_col, file_path, old_string, new_string)
/// (line_idx, start_col, end_col, file_path, old_string, new_string, wrap_line_count)
pub type ClickablePath = (usize, usize, usize, String, String, String, usize);

/// Render DisplayEvents into Lines for the output panel with iMessage-style layout
/// Returns (lines, animation_indices, bubble_positions, clickable_paths) where:
/// - animation_indices are (line_idx, span_idx) pairs for pending tool indicators
/// - bubble_positions are (line_idx, is_user) pairs marking where message bubbles start
/// - clickable_paths are file path link regions for mouse click handling
pub fn render_display_events(
    events: &[DisplayEvent],
    width: u16,
    pending_tools: &HashSet<String>,
    failed_tools: &HashSet<String>,
    syntax_highlighter: &SyntaxHighlighter,
    pending_user_message: Option<&str>,
) -> (Vec<Line<'static>>, Vec<(usize, usize)>, Vec<(usize, bool)>, Vec<ClickablePath>) {
    render_display_events_with_state(events, width, pending_tools, failed_tools, syntax_highlighter, pending_user_message, Vec::new(), Vec::new(), Vec::new(), Vec::new(), Default::default())
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
    syntax_highlighter: &SyntaxHighlighter,
    pending_user_message: Option<&str>,
    existing_lines: Vec<Line<'static>>,
    mut existing_anim: Vec<(usize, usize)>,
    existing_bubbles: Vec<(usize, bool)>,
    existing_clickable: Vec<ClickablePath>,
    pre_scan: super::render_thread::PreScanState,
) -> (Vec<Line<'static>>, Vec<(usize, usize)>, Vec<(usize, bool)>, Vec<ClickablePath>) {
    // Pending user message bubble is stripped from existing_lines by
    // submit_render_request() BEFORE sending — no duplicate trimming needed here.
    existing_anim.retain(|&(line_idx, _)| line_idx < existing_lines.len());

    // Render new events (they ARE only the new events),
    // with pre-computed state from older events injected into the renderer.
    render_display_events_with_state(events, width, pending_tools, failed_tools, syntax_highlighter, pending_user_message, existing_lines, existing_anim, existing_bubbles, existing_clickable, pre_scan)
}

/// Core renderer: iterates events from `start_idx`, appending to provided vectors.
/// Pre-scan state from earlier events is passed in (not re-scanned), so callers
/// can send only new events + pre-computed flags (eliminates mega-clone).
fn render_display_events_with_state(
    events: &[DisplayEvent],
    width: u16,
    pending_tools: &HashSet<String>,
    failed_tools: &HashSet<String>,
    syntax_highlighter: &SyntaxHighlighter,
    pending_user_message: Option<&str>,
    mut lines: Vec<Line<'static>>,
    mut animation_indices: Vec<(usize, usize)>,
    mut bubble_positions: Vec<(usize, bool)>,
    mut clickable_paths: Vec<ClickablePath>,
    pre_scan: super::render_thread::PreScanState,
) -> (Vec<Line<'static>>, Vec<(usize, usize)>, Vec<(usize, bool)>, Vec<ClickablePath>) {
    let w = width as usize;
    let bubble_width = (w * 2 / 3).max(40);

    // Pre-computed state flags: for full renders these are all default (false/None),
    // for incremental renders they come from pre_scan_events() on the main thread.
    let mut saw_init = pre_scan.saw_init;
    let mut saw_content = pre_scan.saw_content;
    let mut last_hook = pre_scan.last_hook;
    let mut saw_exit_plan_mode = pre_scan.saw_exit_plan_mode;
    let mut saw_user_after_exit_plan = pre_scan.saw_user_after_exit_plan;
    let mut saw_ask_user_question = pre_scan.saw_ask_user_question;
    let mut saw_user_after_ask = pre_scan.saw_user_after_ask;
    let mut last_ask_input = pre_scan.last_ask_input;

    for event in events.iter() {
        match event {
            DisplayEvent::Init { model, cwd, .. } => {
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
                    Span::styled(" ⏳ Compacting context... ", Style::default().fg(Color::Black).bg(Color::Yellow)),
                ]).alignment(Alignment::Center));
            }
            DisplayEvent::Compacted => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled(" ✓ Context compacted ", Style::default().fg(Color::Black).bg(Color::Green)),
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
                        Span::styled(" ⏳ Compacting context... ", Style::default().fg(Color::Black).bg(Color::Yellow)),
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

                let header = " Claude ▶ ".to_string();
                let header_pad = " ".repeat(bubble_width.saturating_sub(header.len()));
                lines.push(Line::from(vec![
                    Span::styled(header, Style::default().fg(Color::Black).bg(ORANGE).add_modifier(Modifier::BOLD)),
                    Span::styled(header_pad, Style::default().bg(ORANGE)),
                ]));

                lines.extend(render_assistant_text(text, bubble_width));

                lines.push(Line::from(vec![
                    Span::styled(format!("└{}", "─".repeat(bubble_width - 1)), Style::default().fg(ORANGE)),
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
            DisplayEvent::ToolResult { tool_use_id, tool_name, file_path, content, .. } => {
                saw_content = true;
                last_hook = None;
                // TodoWrite result is noise ("Todos have been modified successfully"), skip it
                if tool_name == "TodoWrite" { continue; }
                let is_failed = failed_tools.contains(tool_use_id);
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

    (lines, animation_indices, bubble_positions, clickable_paths)
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
    animation_indices: &mut Vec<(usize, usize)>,
    clickable_paths: &mut Vec<ClickablePath>,
    tool_name: &str,
    file_path: &Option<String>,
    input: &serde_json::Value,
    tool_use_id: &str,
    pending_tools: &HashSet<String>,
    failed_tools: &HashSet<String>,
    bubble_width: usize,
    highlighter: &SyntaxHighlighter,
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
            // Track line index for animation patching (span index 1 is the indicator)
            if is_pending {
                animation_indices.push((lines.len(), 1));
            }
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

fn render_complete(lines: &mut Vec<Line<'static>>, duration_ms: u64, cost_usd: f64, success: bool) {
    lines.push(Line::from(""));
    let (status, color) = if success { ("Completed", Color::Green) } else { ("Failed", Color::Red) };
    lines.push(Line::from(vec![
        Span::styled(format!(" ● {} ", status), Style::default().fg(Color::Black).bg(color).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {:.1}s ", duration_ms as f64 / 1000.0), Style::default().fg(Color::White)),
        Span::styled(format!("${:.4}", cost_usd), Style::default().fg(Color::Yellow)),
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
}
