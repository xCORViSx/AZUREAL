//! Tool rendering utilities for TUI
//!
//! Handles extraction of tool parameters and rendering tool results.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::collections::HashMap;

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
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if cmd.chars().count() > 50 {
                format!("{}...", cmd.chars().take(47).collect::<String>())
            } else {
                cmd.to_string()
            }
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
            input.get("file_path")
                .or_else(|| input.get("path"))
                .or_else(|| input.get("command"))
                .or_else(|| input.get("query"))
                .or_else(|| input.get("pattern"))
                .and_then(|v| v.as_str())
                .map(|s| if s.chars().count() > 60 { format!("{}...", s.chars().take(57).collect::<String>()) } else { s.to_string() })
                .unwrap_or_default()
        }
    }
}

/// Truncate a line to max length, adding ellipsis if needed
pub fn truncate_line(s: &str, max_len: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max_len {
        trimmed.to_string()
    } else {
        format!("{}...", trimmed.chars().take(max_len.saturating_sub(3)).collect::<String>())
    }
}

/// Render tool result output based on tool type
/// Each tool has a specific display format optimized for readability
pub fn render_tool_result(tool_name: &str, _file_path: Option<&str>, content: &str, is_failed: bool) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let tool_color = if is_failed { Color::Red } else { Color::Cyan };
    let result_style = Style::default().fg(if is_failed { Color::Red } else { Color::Gray });

    // Filter out system-reminder blocks
    let content = if let Some(start) = content.find("<system-reminder>") {
        &content[..start]
    } else {
        content
    }.trim_end();

    match tool_name {
        "Read" => {
            let content_lines: Vec<&str> = content.lines().collect();
            let line_count = content_lines.len();
            if line_count == 0 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled("(empty file)", result_style),
                ]));
            } else if line_count <= 2 {
                for l in content_lines {
                    lines.push(Line::from(vec![
                        Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                        Span::styled(truncate_line(l, 100), result_style),
                    ]));
                }
            } else {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                    Span::styled(truncate_line(content_lines[0], 100), result_style),
                ]));
                lines.push(Line::from(vec![
                    Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                    Span::styled(format!("... ({} lines)", line_count - 2), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                ]));
                let last_line = content_lines.iter().rev()
                    .find(|l| l.find('→').map(|i| !l[i+3..].trim().is_empty()).unwrap_or(!l.trim().is_empty()))
                    .unwrap_or(&content_lines[line_count - 1]);
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(truncate_line(last_line, 100), result_style),
                ]));
            }
        }
        "Bash" => {
            let content_lines: Vec<&str> = content.lines().collect();
            let line_count = content_lines.len();
            let exit_hint = if content.contains("exit code") || content.contains("Exit code") { "" } else { " → exit 0" };

            if line_count == 0 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(format!("✓{}", exit_hint), Style::default().fg(Color::Green)),
                ]));
            } else if line_count <= 2 {
                for (i, l) in content_lines.iter().enumerate() {
                    let prefix = if i == line_count - 1 { " ┃  └─ " } else { " ┃  │ " };
                    lines.push(Line::from(vec![
                        Span::styled(prefix, result_style.fg(tool_color)),
                        Span::styled(truncate_line(l, 100), result_style),
                    ]));
                }
            } else {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                    Span::styled(format!("... ({} lines)", line_count - 2), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                ]));
                for l in content_lines.iter().skip(line_count - 2) {
                    lines.push(Line::from(vec![
                        Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                        Span::styled(truncate_line(l, 100), result_style),
                    ]));
                }
            }
        }
        "Edit" => {
            lines.push(Line::from(vec![
                Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                Span::styled(truncate_line(content, 80), result_style),
            ]));
        }
        "Write" => {
            lines.push(Line::from(vec![
                Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                Span::styled(truncate_line(content.lines().next().unwrap_or("written"), 80), result_style),
            ]));
        }
        "Grep" => {
            let content_lines: Vec<&str> = content.lines().collect();
            let line_count = content_lines.len();
            let show_count = 3.min(line_count);
            for (i, l) in content_lines.iter().take(show_count).enumerate() {
                let prefix = if i == show_count - 1 && line_count <= 3 { " ┃  └─ " } else { " ┃  │ " };
                lines.push(Line::from(vec![
                    Span::styled(prefix, result_style.fg(tool_color)),
                    Span::styled(truncate_line(l, 100), result_style),
                ]));
            }
            if line_count > 3 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(format!("... ({} more matches)", line_count - 3), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                ]));
            }
        }
        "Glob" => {
            let content_lines: Vec<&str> = content.lines().collect();
            let line_count = content_lines.len();
            let mut dir_counts: HashMap<&str, usize> = HashMap::new();
            for l in &content_lines {
                let dir = l.rsplit('/').nth(1).unwrap_or(".");
                *dir_counts.entry(dir).or_insert(0) += 1;
            }
            lines.push(Line::from(vec![
                Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                Span::styled(format!("→ {} files", line_count), Style::default().fg(Color::White)),
            ]));
            let mut dirs: Vec<_> = dir_counts.into_iter().collect();
            dirs.sort_by(|a, b| b.1.cmp(&a.1));
            let dir_summary: String = dirs.iter().take(5).map(|(d, c)| format!("{}/ ({})", d, c)).collect::<Vec<_>>().join("  ");
            lines.push(Line::from(vec![
                Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                Span::styled(truncate_line(&dir_summary, 100), result_style),
            ]));
        }
        "Task" => {
            let content_lines: Vec<&str> = content.lines().collect();
            let line_count = content_lines.len();
            for (i, l) in content_lines.iter().enumerate() {
                let prefix = if i == line_count - 1 { " ┃  └─ " } else { " ┃  │ " };
                lines.push(Line::from(vec![
                    Span::styled(prefix, result_style.fg(tool_color)),
                    Span::styled(truncate_line(l, 120), result_style),
                ]));
            }
        }
        "WebFetch" => {
            let content_lines: Vec<&str> = content.lines().collect();
            if let Some(title) = content_lines.first() {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                    Span::styled(format!("\"{}\"", truncate_line(title, 60)), result_style),
                ]));
            }
            if let Some(preview) = content_lines.get(1) {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(truncate_line(preview, 80), result_style),
                ]));
            } else {
                let (symbol, style) = if is_failed {
                    ("✗ failed", result_style)
                } else {
                    ("✓ fetched", Style::default().fg(Color::Green))
                };
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(symbol, style),
                ]));
            }
        }
        "WebSearch" => {
            let content_lines: Vec<&str> = content.lines().collect();
            let line_count = content_lines.len();
            let show_count = 3.min(line_count);
            for (i, l) in content_lines.iter().take(show_count).enumerate() {
                let prefix = if i == show_count - 1 && line_count <= 3 { " ┃  └─ " } else { " ┃  │ " };
                lines.push(Line::from(vec![
                    Span::styled(prefix, result_style.fg(tool_color)),
                    Span::styled(format!("{}. ", i + 1), Style::default().fg(Color::Yellow)),
                    Span::styled(truncate_line(l, 90), result_style),
                ]));
            }
            if line_count > 3 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(format!("... ({} more results)", line_count - 3), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                ]));
            }
        }
        "LSP" => {
            let content_lines: Vec<&str> = content.lines().collect();
            for (i, l) in content_lines.iter().take(3).enumerate() {
                let prefix = if i == content_lines.len().min(3) - 1 { " ┃  └─ " } else { " ┃  │ " };
                lines.push(Line::from(vec![
                    Span::styled(prefix, result_style.fg(tool_color)),
                    Span::styled(truncate_line(l, 100), result_style),
                ]));
            }
        }
        _ => {
            let content_lines: Vec<&str> = content.lines().collect();
            let line_count = content_lines.len();
            let first_line = content_lines.first().copied().unwrap_or("✓");
            if line_count <= 1 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(truncate_line(first_line, 100), result_style),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  │ ", result_style.fg(tool_color)),
                    Span::styled(truncate_line(first_line, 100), result_style),
                ]));
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(format!("... ({} lines)", line_count - 1), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                ]));
            }
        }
    }

    lines
}
