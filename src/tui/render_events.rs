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
use super::render_markdown::render_assistant_text;
use super::render_tools::{extract_tool_param, render_tool_result, render_edit_diff, render_write_preview, tool_display_name};
use super::render_wrap::wrap_text;

/// Render DisplayEvents into Lines for the output panel with iMessage-style layout
/// Returns (lines, animation_indices) where animation_indices are (line_idx, span_idx) pairs
/// for pending tool indicators that need animation color patching
pub fn render_display_events(
    events: &[DisplayEvent],
    width: u16,
    pending_tools: &HashSet<String>,
    failed_tools: &HashSet<String>,
    syntax_highlighter: &SyntaxHighlighter,
) -> (Vec<Line<'static>>, Vec<(usize, usize)>) {
    let mut lines = Vec::new();
    let mut animation_indices = Vec::new();
    let w = width as usize;
    let bubble_width = (w * 2 / 3).max(40);

    let mut saw_init = false;
    let mut saw_content = false;
    let mut last_hook: Option<(String, String)> = None;

    for event in events {
        match event {
            DisplayEvent::Init { model, cwd, .. } => {
                if saw_init || saw_content { continue; }
                saw_init = true;
                render_init(&mut lines, model, cwd);
            }
            DisplayEvent::Hook { name, output } => {
                let key = (name.clone(), output.clone());
                if last_hook.as_ref() == Some(&key) { continue; }
                last_hook = Some(key);
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
                saw_content = true;
                last_hook = None;
                render_user_message(&mut lines, content, bubble_width, w);
            }
            DisplayEvent::AssistantText { text, .. } => {
                saw_content = true;
                last_hook = None;
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
                render_tool_call(&mut lines, &mut animation_indices, tool_name, file_path, input, tool_use_id, pending_tools, failed_tools, bubble_width, syntax_highlighter);
            }
            DisplayEvent::ToolResult { tool_use_id, tool_name, file_path, content, .. } => {
                saw_content = true;
                last_hook = None;
                let is_failed = failed_tools.contains(tool_use_id);
                let tool_max = bubble_width + 10;
                lines.extend(render_tool_result(tool_name, file_path.as_deref(), content, is_failed, tool_max));
            }
            DisplayEvent::Complete { duration_ms, cost_usd, success, .. } => {
                render_complete(&mut lines, *duration_ms, *cost_usd, *success);
            }
            DisplayEvent::Error { message } => {
                render_error(&mut lines, message);
            }
            DisplayEvent::Filtered => {}
        }
    }

    (lines, animation_indices)
}

fn render_init(lines: &mut Vec<Line<'static>>, model: &str, cwd: &str) {
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" Session Started ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
    ]).alignment(Alignment::Center));
    lines.push(Line::from(vec![
        Span::styled("Model: ", Style::default().fg(Color::DarkGray)),
        Span::styled(model.to_string(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
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
        Span::styled(header_pad, Style::default().bg(Color::Cyan)),
        Span::styled(header, Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
    ]));

    let content_width = bubble_width.saturating_sub(2);
    for wrapped in wrap_text(content, content_width) {
        let pad = bubble_width.saturating_sub(wrapped.chars().count() + 2);
        lines.push(Line::from(vec![
            Span::raw(offset_str.clone()),
            Span::styled(" ".repeat(pad), Style::default()),
            Span::styled(wrapped, Style::default().fg(Color::White)),
            Span::styled(" │", Style::default().fg(Color::Cyan)),
        ]));
    }

    lines.push(Line::from(vec![
        Span::raw(offset_str),
        Span::styled(format!("{}┘", "─".repeat(bubble_width - 1)), Style::default().fg(Color::Cyan)),
    ]));
}

fn render_tool_call(
    lines: &mut Vec<Line<'static>>,
    animation_indices: &mut Vec<(usize, usize)>,
    tool_name: &str,
    file_path: &Option<String>,
    input: &serde_json::Value,
    tool_use_id: &str,
    pending_tools: &HashSet<String>,
    failed_tools: &HashSet<String>,
    bubble_width: usize,
    highlighter: &SyntaxHighlighter,
) {
    let tool_color = Color::Cyan;
    let is_pending = pending_tools.contains(tool_use_id);
    let is_failed = failed_tools.contains(tool_use_id);

    lines.push(Line::from(vec![Span::styled(" ┃", Style::default().fg(tool_color))]));

    let param_raw = file_path.clone().unwrap_or_else(|| extract_tool_param(tool_name, input));

    // Use placeholder color for pending - will be patched during viewport rendering
    let (indicator, indicator_color) = if is_pending {
        ("◐ ", Color::White)
    } else if is_failed {
        ("✗ ", Color::Red)
    } else {
        ("● ", Color::Green)
    };

    let display_name = tool_display_name(tool_name);
    let tool_line_max = bubble_width + 10;
    let prefix_len = 3 + 2 + display_name.len() + 2;
    let param_max = tool_line_max.saturating_sub(prefix_len);

    for (i, wrapped) in wrap_text(&param_raw, param_max).into_iter().enumerate() {
        if i == 0 {
            // Track line index for animation patching (span index 1 is the indicator)
            if is_pending {
                animation_indices.push((lines.len(), 1));
            }
            lines.push(Line::from(vec![
                Span::styled(" ┣━", Style::default().fg(tool_color)),
                Span::styled(indicator, Style::default().fg(indicator_color)),
                Span::styled(display_name.to_string(), Style::default().fg(tool_color).add_modifier(Modifier::BOLD)),
                Span::styled("  ", Style::default()),
                Span::styled(wrapped, Style::default().fg(ORANGE)),
            ]));
        } else {
            let indent = " ".repeat(prefix_len);
            lines.push(Line::from(vec![
                Span::styled(indent, Style::default()),
                Span::styled(wrapped, Style::default().fg(ORANGE)),
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

fn render_error(lines: &mut Vec<Line<'static>>, message: &str) {
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" ✗ Error ", Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD)),
    ]).alignment(Alignment::Center));
    for line in message.lines() {
        lines.push(Line::from(Span::styled(line.to_string(), Style::default().fg(Color::Red))).alignment(Alignment::Center));
    }
    lines.push(Line::from(""));
}

/// Render a plan block with prominent full-width styling
fn render_plan(lines: &mut Vec<Line<'static>>, name: &str, content: &str, width: usize) {
    let plan_color = Color::Magenta;
    let header_bg = Color::Magenta;
    let border_char = "═";

    // Spacing before plan
    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Top border
    lines.push(Line::from(vec![
        Span::styled(format!("╔{}╗", border_char.repeat(width.saturating_sub(2))), Style::default().fg(plan_color).add_modifier(Modifier::BOLD)),
    ]));

    // Header with plan icon and name
    let header = format!(" 📋 PLAN MODE: {} ", name);
    let header_pad = width.saturating_sub(header.chars().count() + 2);
    lines.push(Line::from(vec![
        Span::styled("║", Style::default().fg(plan_color).add_modifier(Modifier::BOLD)),
        Span::styled(header, Style::default().fg(Color::White).bg(header_bg).add_modifier(Modifier::BOLD)),
        Span::styled(" ".repeat(header_pad), Style::default().bg(header_bg)),
        Span::styled("║", Style::default().fg(plan_color).add_modifier(Modifier::BOLD)),
    ]));

    // Separator under header
    lines.push(Line::from(vec![
        Span::styled(format!("╠{}╣", "─".repeat(width.saturating_sub(2))), Style::default().fg(plan_color)),
    ]));

    // Plan content with left border
    let content_width = width.saturating_sub(4);
    for line in content.lines() {
        // Simple wrapping for long lines
        let wrapped = wrap_text(line, content_width);
        for wrapped_line in wrapped {
            let pad = content_width.saturating_sub(wrapped_line.chars().count());
            lines.push(Line::from(vec![
                Span::styled("║ ", Style::default().fg(plan_color)),
                Span::styled(wrapped_line, Style::default().fg(Color::White)),
                Span::styled(format!("{} ║", " ".repeat(pad)), Style::default().fg(plan_color)),
            ]));
        }
    }

    // Bottom border
    lines.push(Line::from(vec![
        Span::styled(format!("╚{}╝", border_char.repeat(width.saturating_sub(2))), Style::default().fg(plan_color).add_modifier(Modifier::BOLD)),
    ]));

    lines.push(Line::from(""));
}
