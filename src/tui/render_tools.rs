//! Tool rendering utilities for TUI
//!
//! Handles extraction of tool parameters and rendering tool results.

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

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

/// Truncate a line to max length (NO ellipsis - just cut)
pub fn truncate_line(s: &str, max_len: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max_len {
        trimmed.to_string()
    } else {
        trimmed.chars().take(max_len).collect::<String>()
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
