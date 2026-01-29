//! Display event rendering for TUI
//!
//! Renders DisplayEvents into Lines for the output panel with iMessage-style layout.

use ratatui::{
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::collections::HashSet;
use textwrap::{wrap, Options};

use crate::events::DisplayEvent;
use crate::syntax::SyntaxHighlighter;
use super::colorize::ORANGE;
use super::markdown::{parse_markdown_spans, parse_table_row, is_table_separator};
use super::render_tools::{extract_tool_param, render_tool_result, truncate_line};

/// Wrap text to fit within max_width, returning wrapped lines
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if text.is_empty() { return vec![String::new()]; }
    let opts = Options::new(max_width).break_words(true);
    wrap(text, opts).into_iter().map(|cow| cow.into_owned()).collect()
}

/// Render DisplayEvents into Lines for the output panel with iMessage-style layout
/// User messages are right-aligned (cyan), Claude messages are left-aligned (orange)
pub fn render_display_events(
    events: &[DisplayEvent],
    width: u16,
    pending_tools: &HashSet<String>,
    failed_tools: &HashSet<String>,
    animation_tick: u64,
    syntax_highlighter: &SyntaxHighlighter,
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

                // Hooks constrained to bubble_width + 10
                let hook_max = bubble_width + 10;
                if !output.trim().is_empty() {
                    let prefix_len = 2 + name.len() + 2; // "› " + name + ": "
                    let output_max = hook_max.saturating_sub(prefix_len);
                    let first_line = output.lines().next().unwrap_or("");
                    for (i, wrapped) in wrap_text(first_line, output_max).into_iter().enumerate() {
                        if i == 0 {
                            lines.push(Line::from(vec![
                                Span::styled("› ", Style::default().fg(Color::DarkGray)),
                                Span::styled(name.clone(), Style::default().fg(Color::DarkGray)),
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

                let header = " ◀ You ".to_string();
                let header_pad = " ".repeat(bubble_width.saturating_sub(header.len()));
                lines.push(Line::from(vec![
                    Span::styled(header_pad, Style::default().bg(Color::Cyan)),
                    Span::styled(header, Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
                ]).alignment(Alignment::Right));

                // Pre-wrap content to bubble width for accurate line count
                let content_width = bubble_width.saturating_sub(4);
                for line in content.lines() {
                    for wrapped in wrap_text(line, content_width) {
                        let padded = format!("{:>width$} │", wrapped, width = bubble_width - 3);
                        lines.push(Line::from(vec![Span::styled(padded, Style::default().fg(Color::White))]).alignment(Alignment::Right));
                    }
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

                let header = " Claude ▶ ".to_string();
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
                        // Wrap code lines to fit within bubble (minus "│ │ " prefix)
                        let code_max = bubble_width.saturating_sub(4);
                        for wrapped in wrap_text(line, code_max) {
                            lines.push(Line::from(vec![
                                Span::styled("│ ", Style::default().fg(ORANGE)),
                                Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                                Span::styled(wrapped, Style::default().fg(Color::Yellow)),
                            ]));
                        }
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
                        // Wrap header to fit within bubble (minus "│ " + prefix)
                        let header_max = bubble_width.saturating_sub(4);
                        for (i, wrapped) in wrap_text(header_text, header_max).into_iter().enumerate() {
                            if i == 0 {
                                lines.push(Line::from(vec![
                                    Span::styled("│ ", Style::default().fg(ORANGE)),
                                    Span::styled(prefix, style),
                                    Span::styled(wrapped, style),
                                ]));
                            } else {
                                lines.push(Line::from(vec![
                                    Span::styled("│ ", Style::default().fg(ORANGE)),
                                    Span::styled("  ", Style::default()), // Indent continuation
                                    Span::styled(wrapped, style),
                                ]));
                            }
                        }
                        continue;
                    }

                    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• ") {
                        let bullet_content = trimmed.trim_start_matches("- ").trim_start_matches("* ").trim_start_matches("• ");
                        // Wrap bullet content (minus "│   • ")
                        let bullet_max = bubble_width.saturating_sub(6);
                        for (i, wrapped) in wrap_text(bullet_content, bullet_max).into_iter().enumerate() {
                            let mut spans = vec![
                                Span::styled("│ ", Style::default().fg(ORANGE)),
                            ];
                            if i == 0 {
                                spans.push(Span::styled("  • ", Style::default().fg(Color::Cyan)));
                            } else {
                                spans.push(Span::styled("    ", Style::default())); // Indent continuation
                            }
                            spans.extend(parse_markdown_spans(&wrapped, Style::default().fg(Color::White)));
                            lines.push(Line::from(spans));
                        }
                        continue;
                    }

                    if trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) && trimmed.contains(". ") {
                        let num_end = trimmed.find(". ").unwrap_or(0);
                        let num = &trimmed[..num_end];
                        let content = &trimmed[num_end + 2..];
                        // Wrap numbered content (minus "│   N. ")
                        let num_prefix = format!("  {}. ", num);
                        let num_max = bubble_width.saturating_sub(2 + num_prefix.len());
                        for (i, wrapped) in wrap_text(content, num_max).into_iter().enumerate() {
                            let mut spans = vec![
                                Span::styled("│ ", Style::default().fg(ORANGE)),
                            ];
                            if i == 0 {
                                spans.push(Span::styled(num_prefix.clone(), Style::default().fg(Color::Cyan)));
                            } else {
                                spans.push(Span::styled(" ".repeat(num_prefix.len()), Style::default())); // Indent continuation
                            }
                            spans.extend(parse_markdown_spans(&wrapped, Style::default().fg(Color::White)));
                            lines.push(Line::from(spans));
                        }
                        continue;
                    }

                    if trimmed.starts_with("> ") {
                        let quote_content = trimmed.trim_start_matches("> ");
                        // Wrap quote content (minus "│ ┃ ")
                        let quote_max = bubble_width.saturating_sub(4);
                        for wrapped in wrap_text(quote_content, quote_max) {
                            let mut spans = vec![
                                Span::styled("│ ", Style::default().fg(ORANGE)),
                                Span::styled("┃ ", Style::default().fg(Color::DarkGray)),
                            ];
                            spans.extend(parse_markdown_spans(&wrapped, Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)));
                            lines.push(Line::from(spans));
                        }
                        continue;
                    }

                    // Wrap to bubble_width - 2 (for "│ " prefix) to stay within bubble
                    let content_width = bubble_width.saturating_sub(2);
                    for wrapped in wrap_text(line, content_width) {
                        let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
                        spans.extend(parse_markdown_spans(&wrapped, Style::default().fg(Color::White)));
                        lines.push(Line::from(spans));
                    }
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

                let param_raw = if let Some(path) = file_path {
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

                // Constrain tool command line to bubble + 10, wrap if needed
                let tool_line_max = bubble_width + 10;
                let prefix_len = 3 + 2 + tool_name.len() + 2; // " ┣━" + indicator + name + "  "
                let param_max = tool_line_max.saturating_sub(prefix_len);

                for (i, wrapped) in wrap_text(&param_raw, param_max).into_iter().enumerate() {
                    if i == 0 {
                        lines.push(Line::from(vec![
                            Span::styled(" ┣━", Style::default().fg(tool_color)),
                            Span::styled(indicator, Style::default().fg(indicator_color)),
                            Span::styled(tool_name.clone(), Style::default().fg(tool_color).add_modifier(Modifier::BOLD)),
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
                    render_edit_diff(&mut lines, input, file_path, tool_color, tool_max, syntax_highlighter);
                }
                if tool_name == "Write" {
                    render_write_preview(&mut lines, input, tool_color, tool_max);
                }
            }
            DisplayEvent::ToolResult { tool_use_id, tool_name, file_path, content, .. } => {
                saw_content = true;
                last_hook = None;
                let is_failed = failed_tools.contains(tool_use_id);
                // Tool results can extend up to 10 units past bubble
                let tool_max = bubble_width + 10;
                let result_lines = render_tool_result(tool_name, file_path.as_deref(), content, is_failed, tool_max);
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
fn render_edit_diff(lines: &mut Vec<Line<'static>>, input: &serde_json::Value, file_path: &Option<String>, tool_color: Color, max_width: usize, _highlighter: &SyntaxHighlighter) {
    let old_str = input.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
    let new_str = input.get("new_string").and_then(|v| v.as_str()).unwrap_or("");

    if old_str.is_empty() && new_str.is_empty() { return; }

    // Dimmer red/green backgrounds
    let dim_red_bg = Color::Rgb(60, 25, 25);
    let dim_green_bg = Color::Rgb(25, 50, 25);
    // Dim white for removed text
    let dim_white = Color::Rgb(170, 170, 170);

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
    // Content width after " ┃   XX +/- " prefix
    let content_max = max_width.saturating_sub(4 + num_width + 3 + 1);

    for i in 0..max_len {
        let old_line = old_lines.get(i).copied();
        let new_line = new_lines.get(i).copied();

        match (old_line, new_line) {
            (Some(old), Some(new)) if old == new => {
                // Unchanged line - dim gray, wrap if needed
                for (j, wrapped) in wrap_text(old, content_max).into_iter().enumerate() {
                    let line_num = if j == 0 { format!(" {:>width$}   ", start_line + i, width = num_width) } else { " ".repeat(num_width + 4) };
                    lines.push(Line::from(vec![
                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                        Span::styled(line_num, Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("{} ", wrapped), Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }
            (Some(old), Some(new_text)) => {
                // Changed line - old (red bg, dim white) and new (green bg)
                for (j, wrapped) in wrap_text(old, content_max).into_iter().enumerate() {
                    let line_num = if j == 0 { format!(" {:>width$} - ", start_line + i, width = num_width) } else { " ".repeat(num_width + 4) };
                    lines.push(Line::from(vec![
                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                        Span::styled(line_num, Style::default().fg(Color::Red)),
                        Span::styled(format!("{} ", wrapped), Style::default().fg(dim_white).bg(dim_red_bg)),
                    ]));
                }

                for (j, wrapped) in wrap_text(new_text, content_max).into_iter().enumerate() {
                    let line_num = if j == 0 { format!(" {:>width$} + ", start_line + i, width = num_width) } else { " ".repeat(num_width + 4) };
                    lines.push(Line::from(vec![
                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                        Span::styled(line_num, Style::default().fg(Color::Green)),
                        Span::styled(format!("{} ", wrapped), Style::default().fg(Color::White).bg(dim_green_bg)),
                    ]));
                }
            }
            (Some(old), None) => {
                // Deleted line - red bg with dim white text, wrap if needed
                for (j, wrapped) in wrap_text(old, content_max).into_iter().enumerate() {
                    let line_num = if j == 0 { format!(" {:>width$} - ", start_line + i, width = num_width) } else { " ".repeat(num_width + 4) };
                    lines.push(Line::from(vec![
                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                        Span::styled(line_num, Style::default().fg(Color::Red)),
                        Span::styled(format!("{} ", wrapped), Style::default().fg(dim_white).bg(dim_red_bg)),
                    ]));
                }
            }
            (None, Some(new_text)) => {
                // Added line - green bg, wrap if needed
                for (j, wrapped) in wrap_text(new_text, content_max).into_iter().enumerate() {
                    let line_num = if j == 0 { format!(" {:>width$} + ", start_line + i, width = num_width) } else { " ".repeat(num_width + 4) };
                    lines.push(Line::from(vec![
                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                        Span::styled(line_num, Style::default().fg(Color::Green)),
                        Span::styled(format!("{} ", wrapped), Style::default().fg(Color::White).bg(dim_green_bg)),
                    ]));
                }
            }
            (None, None) => {}
        }
    }
}

/// Render Write tool preview showing line count and purpose
fn render_write_preview(lines: &mut Vec<Line<'static>>, input: &serde_json::Value, tool_color: Color, max_width: usize) {
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

        // " ┃  └─ ✓ XX lines  " prefix is ~20 chars
        let purpose_max = max_width.saturating_sub(20 + format!("{}", line_count).len());
        lines.push(Line::from(vec![
            Span::styled(" ┃  └─ ", Style::default().fg(tool_color)),
            Span::styled("✓ ", Style::default().fg(Color::Green)),
            Span::styled(format!("{} lines", line_count), Style::default().fg(Color::White)),
            if !purpose_line.is_empty() {
                Span::styled(format!("  {}", truncate_line(purpose_line, purpose_max)), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC))
            } else {
                Span::raw("")
            },
        ]));
    }
}
