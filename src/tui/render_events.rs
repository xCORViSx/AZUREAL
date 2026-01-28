//! Display event rendering for TUI
//!
//! Renders DisplayEvents into Lines for the output panel with iMessage-style layout.

use ratatui::{
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::collections::HashSet;

use crate::events::DisplayEvent;
use super::colorize::ORANGE;
use super::markdown::{parse_markdown_spans, parse_table_row, is_table_separator};
use super::render_tools::{extract_tool_param, render_tool_result, truncate_line};

/// Render DisplayEvents into Lines for the output panel with iMessage-style layout
/// User messages are right-aligned (cyan), Claude messages are left-aligned (orange)
pub fn render_display_events(
    events: &[DisplayEvent],
    width: u16,
    pending_tools: &HashSet<String>,
    failed_tools: &HashSet<String>,
    animation_tick: u64,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
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

                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled(" Session Started ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(vec![
                    Span::styled("Model: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(model.clone(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(vec![
                    Span::styled("Path: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(cwd.clone(), Style::default().fg(Color::White)),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(""));
            }
            DisplayEvent::Hook { name, output } => {
                let key = (name.clone(), output.clone());
                if last_hook.as_ref() == Some(&key) { continue; }
                last_hook = Some(key);

                if !output.trim().is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("› ", Style::default().fg(Color::DarkGray)),
                        Span::styled(name.clone(), Style::default().fg(Color::DarkGray)),
                        Span::styled(": ", Style::default().fg(Color::DarkGray)),
                        Span::styled(output.lines().next().unwrap_or("").to_string(), Style::default().fg(Color::DarkGray)),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled("› ", Style::default().fg(Color::DarkGray)),
                        Span::styled(name.clone(), Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }
            DisplayEvent::Command { name } => {
                let cmd_style = Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD);
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled("━".repeat(20), Style::default().fg(Color::Magenta))]).alignment(Alignment::Center));
                lines.push(Line::from(vec![Span::styled(format!("  {}  ", name), cmd_style)]).alignment(Alignment::Center));
                lines.push(Line::from(vec![Span::styled("━".repeat(20), Style::default().fg(Color::Magenta))]).alignment(Alignment::Center));
                lines.push(Line::from(""));
            }
            DisplayEvent::Compacting => {
                let compact_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled("═".repeat(30), Style::default().fg(Color::Yellow))]).alignment(Alignment::Center));
                lines.push(Line::from(vec![Span::styled("  COMPACTING CONVERSATION  ", compact_style)]).alignment(Alignment::Center));
                lines.push(Line::from(vec![Span::styled("═".repeat(30), Style::default().fg(Color::Yellow))]).alignment(Alignment::Center));
                lines.push(Line::from(""));
            }
            DisplayEvent::Compacted => {
                let compact_style = Style::default().fg(Color::Green).add_modifier(Modifier::BOLD);
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled("═".repeat(30), Style::default().fg(Color::Green))]).alignment(Alignment::Center));
                lines.push(Line::from(vec![Span::styled("  CONVERSATION COMPACTED  ", compact_style)]).alignment(Alignment::Center));
                lines.push(Line::from(vec![Span::styled("═".repeat(30), Style::default().fg(Color::Green))]).alignment(Alignment::Center));
                lines.push(Line::from(""));
            }
            DisplayEvent::UserMessage { content, .. } => {
                saw_content = true;
                last_hook = None;
                if content.trim().is_empty() { continue; }

                lines.push(Line::from(""));
                lines.push(Line::from(""));

                let header = " You ▶ ".to_string();
                let header_pad = " ".repeat(bubble_width.saturating_sub(header.len()));
                lines.push(Line::from(vec![
                    Span::styled(header_pad, Style::default().bg(Color::Cyan)),
                    Span::styled(header, Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
                ]).alignment(Alignment::Right));

                for line in content.lines() {
                    let text = line.to_string();
                    let padded = if text.len() < bubble_width - 4 {
                        format!("{:>width$} │", text, width = bubble_width - 3)
                    } else {
                        format!("{} │", text)
                    };
                    lines.push(Line::from(vec![Span::styled(padded, Style::default().fg(Color::White))]).alignment(Alignment::Right));
                }
                lines.push(Line::from(vec![
                    Span::styled(format!("{}┘", "─".repeat(bubble_width - 1)), Style::default().fg(Color::Cyan)),
                ]).alignment(Alignment::Right));
            }
            DisplayEvent::AssistantText { text, .. } => {
                saw_content = true;
                last_hook = None;
                lines.push(Line::from(""));
                lines.push(Line::from(""));

                let header = " ◀ Claude ".to_string();
                let header_pad = " ".repeat(bubble_width.saturating_sub(header.len()));
                lines.push(Line::from(vec![
                    Span::styled(header, Style::default().fg(Color::Black).bg(ORANGE).add_modifier(Modifier::BOLD)),
                    Span::styled(header_pad, Style::default().bg(ORANGE)),
                ]));

                let mut in_code_block = false;
                let mut in_table = false;
                let text_lines: Vec<&str> = text.lines().collect();

                for (i, line) in text_lines.iter().enumerate() {
                    let trimmed = line.trim();

                    if trimmed.starts_with("```") {
                        in_code_block = !in_code_block;
                        let lang = trimmed.trim_start_matches('`').trim();
                        let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
                        if in_code_block && !lang.is_empty() {
                            spans.push(Span::styled("┌─ ", Style::default().fg(Color::DarkGray)));
                            spans.push(Span::styled(lang.to_string(), Style::default().fg(Color::Cyan)));
                            spans.push(Span::styled(" ─", Style::default().fg(Color::DarkGray)));
                        } else if !in_code_block {
                            spans.push(Span::styled("└──────", Style::default().fg(Color::DarkGray)));
                        } else {
                            spans.push(Span::styled("┌──────", Style::default().fg(Color::DarkGray)));
                        }
                        lines.push(Line::from(spans));
                        continue;
                    }

                    if in_code_block {
                        lines.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(ORANGE)),
                            Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                            Span::styled(line.to_string(), Style::default().fg(Color::Yellow)),
                        ]));
                        continue;
                    }

                    let is_table_line = trimmed.contains('|') && trimmed.starts_with('|');
                    let is_sep = is_table_separator(trimmed);

                    if is_table_line || is_sep {
                        if !in_table && is_table_line && !is_sep {
                            in_table = true;
                            let next_is_sep = text_lines.get(i + 1).map(|l| is_table_separator(l)).unwrap_or(false);
                            let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
                            if next_is_sep {
                                let cells: Vec<&str> = trimmed.split('|').filter(|s| !s.is_empty()).collect();
                                spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
                                for (j, cell) in cells.iter().enumerate() {
                                    spans.push(Span::styled(cell.trim().to_string(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
                                    if j < cells.len() - 1 {
                                        spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
                                    }
                                }
                                spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
                            } else {
                                spans.extend(parse_table_row(trimmed, false));
                            }
                            lines.push(Line::from(spans));
                        } else if is_sep {
                            let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
                            spans.extend(parse_table_row(trimmed, true));
                            lines.push(Line::from(spans));
                        } else {
                            let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
                            spans.extend(parse_table_row(trimmed, false));
                            lines.push(Line::from(spans));
                        }
                        continue;
                    } else {
                        in_table = false;
                    }

                    if trimmed.starts_with('#') {
                        let header_level = trimmed.chars().take_while(|&c| c == '#').count();
                        let header_text = trimmed.trim_start_matches('#').trim();
                        let (prefix, style) = match header_level {
                            1 => ("█ ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)),
                            2 => ("▓ ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                            3 => ("▒ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                            _ => ("░ ", Style::default().fg(Color::Green)),
                        };
                        lines.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(ORANGE)),
                            Span::styled(prefix, style),
                            Span::styled(header_text.to_string(), style),
                        ]));
                        continue;
                    }

                    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• ") {
                        let bullet_content = trimmed.trim_start_matches("- ").trim_start_matches("* ").trim_start_matches("• ");
                        let mut spans = vec![
                            Span::styled("│ ", Style::default().fg(ORANGE)),
                            Span::styled("  • ", Style::default().fg(Color::Cyan)),
                        ];
                        spans.extend(parse_markdown_spans(bullet_content, Style::default().fg(Color::White)));
                        lines.push(Line::from(spans));
                        continue;
                    }

                    if trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) && trimmed.contains(". ") {
                        let num_end = trimmed.find(". ").unwrap_or(0);
                        let num = &trimmed[..num_end];
                        let content = &trimmed[num_end + 2..];
                        let mut spans = vec![
                            Span::styled("│ ", Style::default().fg(ORANGE)),
                            Span::styled(format!("  {}. ", num), Style::default().fg(Color::Cyan)),
                        ];
                        spans.extend(parse_markdown_spans(content, Style::default().fg(Color::White)));
                        lines.push(Line::from(spans));
                        continue;
                    }

                    if trimmed.starts_with("> ") {
                        let quote_content = trimmed.trim_start_matches("> ");
                        let mut spans = vec![
                            Span::styled("│ ", Style::default().fg(ORANGE)),
                            Span::styled("┃ ", Style::default().fg(Color::DarkGray)),
                        ];
                        spans.extend(parse_markdown_spans(quote_content, Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)));
                        lines.push(Line::from(spans));
                        continue;
                    }

                    let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
                    spans.extend(parse_markdown_spans(line, Style::default().fg(Color::White)));
                    lines.push(Line::from(spans));
                }

                lines.push(Line::from(vec![
                    Span::styled(format!("└{}", "─".repeat(bubble_width - 1)), Style::default().fg(ORANGE)),
                ]));
            }
            DisplayEvent::ToolCall { tool_name, file_path, input, tool_use_id, .. } => {
                saw_content = true;
                last_hook = None;
                let tool_color = Color::Cyan;
                let is_pending = pending_tools.contains(tool_use_id);

                lines.push(Line::from(vec![Span::styled(" ┃", Style::default().fg(tool_color))]));

                let param_display = if let Some(path) = file_path {
                    path.clone()
                } else {
                    extract_tool_param(tool_name, input)
                };

                let is_failed = failed_tools.contains(tool_use_id);
                let (indicator, indicator_color) = if is_pending {
                    let pulse_colors = [Color::White, Color::Gray, Color::DarkGray, Color::Gray];
                    let pulse_idx = (animation_tick / 2) as usize % pulse_colors.len();
                    ("◐ ", pulse_colors[pulse_idx])
                } else if is_failed {
                    ("✗ ", Color::Red)
                } else {
                    ("● ", Color::Green)
                };

                lines.push(Line::from(vec![
                    Span::styled(" ┣━", Style::default().fg(tool_color)),
                    Span::styled(indicator, Style::default().fg(indicator_color)),
                    Span::styled(tool_name.clone(), Style::default().fg(tool_color).add_modifier(Modifier::BOLD)),
                    Span::styled("  ", Style::default()),
                    Span::styled(param_display, Style::default().fg(ORANGE)),
                ]));

                if tool_name == "Edit" {
                    render_edit_diff(&mut lines, input, file_path, tool_color);
                }
                if tool_name == "Write" {
                    render_write_preview(&mut lines, input, tool_color);
                }
            }
            DisplayEvent::ToolResult { tool_use_id, tool_name, file_path, content, .. } => {
                saw_content = true;
                last_hook = None;
                let is_failed = failed_tools.contains(tool_use_id);
                let result_lines = render_tool_result(tool_name, file_path.as_deref(), content, is_failed);
                lines.extend(result_lines);
            }
            DisplayEvent::Complete { duration_ms, cost_usd, success, .. } => {
                lines.push(Line::from(""));
                let (status, color) = if *success { ("Completed", Color::Green) } else { ("Failed", Color::Red) };
                lines.push(Line::from(vec![
                    Span::styled(format!(" ● {} ", status), Style::default().fg(Color::Black).bg(color).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" {:.1}s ", *duration_ms as f64 / 1000.0), Style::default().fg(Color::White)),
                    Span::styled(format!("${:.4}", cost_usd), Style::default().fg(Color::Yellow)),
                ]).alignment(Alignment::Center));
                lines.push(Line::from(""));
            }
            DisplayEvent::Error { message } => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled(" ✗ Error ", Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD)),
                ]).alignment(Alignment::Center));
                for line in message.lines() {
                    lines.push(Line::from(Span::styled(line.to_string(), Style::default().fg(Color::Red))).alignment(Alignment::Center));
                }
                lines.push(Line::from(""));
            }
            DisplayEvent::Filtered => {}
        }
    }

    lines
}

/// Render Edit tool diff inline with the tool call
fn render_edit_diff(lines: &mut Vec<Line<'static>>, input: &serde_json::Value, file_path: &Option<String>, tool_color: Color) {
    let old_str = input.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
    let new_str = input.get("new_string").and_then(|v| v.as_str()).unwrap_or("");

    if old_str.is_empty() && new_str.is_empty() { return; }

    let old_lines: Vec<&str> = old_str.lines().collect();
    let new_lines: Vec<&str> = new_str.lines().collect();

    let start_line = file_path.as_ref().and_then(|path| {
        std::fs::read_to_string(path).ok().and_then(|content| {
            content.find(new_str).map(|pos| content[..pos].chars().filter(|&c| c == '\n').count() + 1)
        })
    }).unwrap_or(1);

    let max_line = start_line + old_lines.len().max(new_lines.len());
    let num_width = max_line.to_string().len().max(2);
    let max_len = old_lines.len().max(new_lines.len());

    for i in 0..max_len {
        let old_line = old_lines.get(i).copied();
        let new_line = new_lines.get(i).copied();

        match (old_line, new_line) {
            (Some(old), Some(new)) if old == new => {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  ", Style::default().fg(tool_color)),
                    Span::styled(format!(" {:>width$}   {} ", start_line + i, old, width = num_width), Style::default().fg(Color::DarkGray)),
                ]));
            }
            (Some(old), Some(new)) => {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  ", Style::default().fg(tool_color)),
                    Span::styled(format!(" {:>width$} - {} ", start_line + i, old, width = num_width), Style::default().fg(Color::White).bg(Color::Red)),
                ]));
                lines.push(Line::from(vec![
                    Span::styled(" ┃  ", Style::default().fg(tool_color)),
                    Span::styled(format!(" {:>width$} + {} ", start_line + i, new, width = num_width), Style::default().fg(Color::Black).bg(Color::Green)),
                ]));
            }
            (Some(old), None) => {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  ", Style::default().fg(tool_color)),
                    Span::styled(format!(" {:>width$} - {} ", start_line + i, old, width = num_width), Style::default().fg(Color::White).bg(Color::Red)),
                ]));
            }
            (None, Some(new)) => {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  ", Style::default().fg(tool_color)),
                    Span::styled(format!(" {:>width$} + {} ", start_line + i, new, width = num_width), Style::default().fg(Color::Black).bg(Color::Green)),
                ]));
            }
            (None, None) => {}
        }
    }
}

/// Render Write tool preview showing line count and purpose
fn render_write_preview(lines: &mut Vec<Line<'static>>, input: &serde_json::Value, tool_color: Color) {
    if let Some(content) = input.get("content").and_then(|v| v.as_str()) {
        let content_lines: Vec<&str> = content.lines().collect();
        let line_count = content_lines.len();

        let purpose_line = content_lines.iter()
            .find(|l| {
                let trimmed = l.trim();
                trimmed.starts_with("//") || trimmed.starts_with("#") ||
                trimmed.starts_with("/*") || trimmed.starts_with("\"\"\"") ||
                trimmed.starts_with("///") || trimmed.starts_with("//!")
            })
            .or(content_lines.first()).copied()
            .unwrap_or("");

        lines.push(Line::from(vec![
            Span::styled(" ┃  └─ ", Style::default().fg(tool_color)),
            Span::styled("✓ ", Style::default().fg(Color::Green)),
            Span::styled(format!("{} lines", line_count), Style::default().fg(Color::White)),
            if !purpose_line.is_empty() {
                Span::styled(format!("  {}", truncate_line(purpose_line, 70)), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC))
            } else {
                Span::raw("")
            },
        ]));
    }
}
