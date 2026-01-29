//! Tool rendering utilities for TUI
//!
//! Handles extraction of tool parameters and rendering tool results.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::syntax::SyntaxHighlighter;
use super::render_wrap::wrap_spans;

/// Map internal tool names to user-friendly display names
pub fn tool_display_name(tool_name: &str) -> &str {
    match tool_name {
        "Grep" | "grep" => "Search",
        "Glob" | "glob" => "Find",
        _ => tool_name,
    }
}

/// Extract the most relevant parameter from a tool's input for display
pub fn extract_tool_param(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "Read" | "read" => {
            input.get("file_path")
                .or_else(|| input.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "Write" | "write" => {
            input.get("file_path")
                .or_else(|| input.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "Edit" | "edit" => {
            input.get("file_path")
                .or_else(|| input.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "Bash" | "bash" => {
            // Full command - no truncation
            input.get("command").and_then(|v| v.as_str()).unwrap_or("").to_string()
        }
        "Glob" | "glob" => {
            input.get("pattern").and_then(|v| v.as_str()).unwrap_or("").to_string()
        }
        "Grep" | "grep" => {
            input.get("pattern").and_then(|v| v.as_str()).unwrap_or("").to_string()
        }
        "WebFetch" | "webfetch" => {
            input.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string()
        }
        "WebSearch" | "websearch" => {
            input.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string()
        }
        "Task" | "task" => {
            let agent_type = input.get("subagent_type").and_then(|v| v.as_str()).unwrap_or("agent");
            let desc = input.get("description").and_then(|v| v.as_str()).unwrap_or("");
            format!("[{}] {}", agent_type, desc)
        }
        "LSP" | "lsp" => {
            let op = input.get("operation").and_then(|v| v.as_str()).unwrap_or("");
            let file = input.get("filePath").and_then(|v| v.as_str()).unwrap_or("");
            format!("{} {}", op, file)
        }
        "EnterPlanMode" => "🔍 Planning...".to_string(),
        "ExitPlanMode" => "📋 Plan complete".to_string(),
        _ => {
            // Full parameter - no truncation
            input.get("file_path")
                .or_else(|| input.get("path"))
                .or_else(|| input.get("command"))
                .or_else(|| input.get("query"))
                .or_else(|| input.get("pattern"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
    }
}

/// Truncate a line to max length with ellipsis indicator
pub fn truncate_line(s: &str, max_len: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max_len {
        trimmed.to_string()
    } else if max_len > 1 {
        format!("{}…", trimmed.chars().take(max_len - 1).collect::<String>())
    } else {
        "…".to_string()
    }
}

/// Render tool result output based on tool type
/// Shows summarized output constrained to max_width
pub fn render_tool_result(tool_name: &str, _file_path: Option<&str>, content: &str, is_failed: bool, max_width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let tool_color = if is_failed { Color::Red } else { Color::Cyan };
    let result_style = Style::default().fg(if is_failed { Color::Red } else { Color::Gray });

    // Filter out system-reminder blocks
    let content = if let Some(start) = content.find("<system-reminder>") {
        &content[..start]
    } else {
        content
    }.trim_end();

    let content_lines: Vec<&str> = content.lines().collect();
    let line_count = content_lines.len();
    // Account for " ┃  └─ " prefix (7 chars)
    let text_max = max_width.saturating_sub(8);

    if line_count == 0 {
        let msg = match tool_name {
            "Read" => "(empty file)",
            "Bash" => "✓",
            _ => "✓",
        };
        lines.push(Line::from(vec![
            Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
            Span::styled(msg, if tool_name == "Bash" { Style::default().fg(Color::Green) } else { result_style }),
        ]));
        return lines;
    }

    // Tool-specific summarization
    match tool_name {
        "Read" | "read" => {
            // First + last line with line count
            let first = truncate_line(content_lines[0], text_max);
            lines.push(Line::from(vec![
                Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                Span::styled(first, result_style),
            ]));
            if line_count > 2 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                    Span::styled(format!("  ({} lines)", line_count), Style::default().fg(Color::DarkGray)),
                ]));
            }
            if line_count > 1 {
                let last = content_lines.iter().rev().find(|l| !l.trim().is_empty()).unwrap_or(&"");
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(truncate_line(last, text_max), result_style),
                ]));
            } else {
                // Single line - mark as last
                lines.last_mut().map(|l| {
                    if let Some(span) = l.spans.first_mut() {
                        *span = Span::styled(" ┃  └─ ", result_style.fg(tool_color));
                    }
                });
            }
        }
        "Bash" | "bash" => {
            // Last 2 non-empty lines (results usually at end)
            let non_empty: Vec<&str> = content_lines.iter().filter(|l| !l.trim().is_empty()).copied().collect();
            let show: Vec<&str> = non_empty.iter().rev().take(2).rev().copied().collect();
            for (i, l) in show.iter().enumerate() {
                let prefix = if i == show.len() - 1 { " ┃  └─ " } else { " ┃  │ " };
                lines.push(Line::from(vec![
                    Span::styled(prefix, result_style.fg(tool_color)),
                    Span::styled(truncate_line(l, text_max), result_style),
                ]));
            }
            if lines.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled("✓", Style::default().fg(Color::Green)),
                ]));
            }
        }
        "Grep" | "grep" => {
            // First 3 matches
            let show_count = 3.min(line_count);
            for (i, l) in content_lines.iter().take(show_count).enumerate() {
                let prefix = if i == show_count - 1 && line_count <= 3 { " ┃  └─ " } else { " ┃  │ " };
                lines.push(Line::from(vec![
                    Span::styled(prefix, result_style.fg(tool_color)),
                    Span::styled(truncate_line(l, text_max), result_style),
                ]));
            }
            if line_count > 3 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(format!("  (+{} more)", line_count - 3), Style::default().fg(Color::DarkGray)),
                ]));
            }
        }
        "Glob" | "glob" => {
            // File count summary
            lines.push(Line::from(vec![
                Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                Span::styled(format!("{} files", line_count), result_style),
            ]));
        }
        "Task" | "task" => {
            // First 5 lines of subagent output
            let show_count = 5.min(line_count);
            for (i, l) in content_lines.iter().take(show_count).enumerate() {
                let prefix = if i == show_count - 1 && line_count <= 5 { " ┃  └─ " } else { " ┃  │ " };
                lines.push(Line::from(vec![
                    Span::styled(prefix, result_style.fg(tool_color)),
                    Span::styled(truncate_line(l, text_max), result_style),
                ]));
            }
            if line_count > 5 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(format!("  (+{} more lines)", line_count - 5), Style::default().fg(Color::DarkGray)),
                ]));
            }
        }
        _ => {
            // Default: first 3 lines
            let show_count = 3.min(line_count);
            for (i, l) in content_lines.iter().take(show_count).enumerate() {
                let prefix = if i == show_count - 1 && line_count <= 3 { " ┃  └─ " } else { " ┃  │ " };
                lines.push(Line::from(vec![
                    Span::styled(prefix, result_style.fg(tool_color)),
                    Span::styled(truncate_line(l, text_max), result_style),
                ]));
            }
            if line_count > 3 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(format!("  (+{} more)", line_count - 3), Style::default().fg(Color::DarkGray)),
                ]));
            }
        }
    }

    lines
}

/// Render Edit tool diff inline with syntax highlighting
pub fn render_edit_diff(lines: &mut Vec<Line<'static>>, input: &serde_json::Value, file_path: &Option<String>, tool_color: Color, max_width: usize, highlighter: &SyntaxHighlighter) {
    let old_str = input.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
    let new_str = input.get("new_string").and_then(|v| v.as_str()).unwrap_or("");

    if old_str.is_empty() && new_str.is_empty() { return; }

    let dim_red_bg = Color::Rgb(60, 25, 25);
    let dim_green_bg = Color::Rgb(25, 50, 25);

    let filename = file_path.as_ref()
        .and_then(|p| std::path::Path::new(p).file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file.txt".to_string());

    let old_highlighted = highlighter.highlight_with_bg(old_str, &filename, Some(dim_red_bg));
    let new_highlighted = highlighter.highlight_with_bg(new_str, &filename, Some(dim_green_bg));
    let unchanged_highlighted = highlighter.highlight_file(old_str, &filename);

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
    let content_max = max_width.saturating_sub(4 + num_width + 3 + 1);

    for i in 0..max_len {
        let old_line = old_lines.get(i).copied();
        let new_line = new_lines.get(i).copied();

        match (old_line, new_line) {
            (Some(old), Some(new)) if old == new => {
                let spans = unchanged_highlighted.get(i).cloned().unwrap_or_default();
                let dimmed: Vec<Span<'static>> = spans.into_iter().map(|s| {
                    Span::styled(s.content, Style::default().fg(Color::DarkGray))
                }).collect();
                for (j, wrapped_spans) in wrap_spans(dimmed, content_max).into_iter().enumerate() {
                    let line_num = if j == 0 { format!(" {:>width$}   ", start_line + i, width = num_width) } else { " ".repeat(num_width + 4) };
                    let mut all_spans = vec![
                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                        Span::styled(line_num, Style::default().fg(Color::DarkGray)),
                    ];
                    all_spans.extend(wrapped_spans);
                    lines.push(Line::from(all_spans));
                }
            }
            (Some(_), Some(_)) => {
                let old_spans = old_highlighted.get(i).cloned().unwrap_or_default();
                for (j, wrapped_spans) in wrap_spans(old_spans, content_max).into_iter().enumerate() {
                    let line_num = if j == 0 { format!(" {:>width$} - ", start_line + i, width = num_width) } else { " ".repeat(num_width + 4) };
                    let mut all_spans = vec![
                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                        Span::styled(line_num, Style::default().fg(Color::Red)),
                    ];
                    all_spans.extend(wrapped_spans);
                    lines.push(Line::from(all_spans));
                }

                let new_spans = new_highlighted.get(i).cloned().unwrap_or_default();
                for (j, wrapped_spans) in wrap_spans(new_spans, content_max).into_iter().enumerate() {
                    let line_num = if j == 0 { format!(" {:>width$} + ", start_line + i, width = num_width) } else { " ".repeat(num_width + 4) };
                    let mut all_spans = vec![
                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                        Span::styled(line_num, Style::default().fg(Color::Green)),
                    ];
                    all_spans.extend(wrapped_spans);
                    lines.push(Line::from(all_spans));
                }
            }
            (Some(_), None) => {
                let old_spans = old_highlighted.get(i).cloned().unwrap_or_default();
                for (j, wrapped_spans) in wrap_spans(old_spans, content_max).into_iter().enumerate() {
                    let line_num = if j == 0 { format!(" {:>width$} - ", start_line + i, width = num_width) } else { " ".repeat(num_width + 4) };
                    let mut all_spans = vec![
                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                        Span::styled(line_num, Style::default().fg(Color::Red)),
                    ];
                    all_spans.extend(wrapped_spans);
                    lines.push(Line::from(all_spans));
                }
            }
            (None, Some(_)) => {
                let new_spans = new_highlighted.get(i).cloned().unwrap_or_default();
                for (j, wrapped_spans) in wrap_spans(new_spans, content_max).into_iter().enumerate() {
                    let line_num = if j == 0 { format!(" {:>width$} + ", start_line + i, width = num_width) } else { " ".repeat(num_width + 4) };
                    let mut all_spans = vec![
                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                        Span::styled(line_num, Style::default().fg(Color::Green)),
                    ];
                    all_spans.extend(wrapped_spans);
                    lines.push(Line::from(all_spans));
                }
            }
            (None, None) => {}
        }
    }
}

/// Render Write tool preview showing line count and purpose
pub fn render_write_preview(lines: &mut Vec<Line<'static>>, input: &serde_json::Value, tool_color: Color, max_width: usize) {
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
