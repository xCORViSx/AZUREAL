//! Tool rendering utilities for TUI
//!
//! Handles extraction of tool parameters and rendering tool results.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::syntax::SyntaxHighlighter;
use super::render_wrap::wrap_spans;
use super::util::AZURE;

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
    let tool_color = if is_failed { Color::Red } else { AZURE };
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

/// Render Edit tool diff inline.
/// Reads file to find actual line numbers (runs on background render thread,
/// not the draw path). Removed lines show grey text on dim red bg (no syntax
/// highlighting). Added lines get syntax highlighting on dim green bg.
pub fn render_edit_diff(lines: &mut Vec<Line<'static>>, input: &serde_json::Value, file_path: &Option<String>, tool_color: Color, max_width: usize, highlighter: &mut SyntaxHighlighter) {
    let old_str = input.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
    let new_str = input.get("new_string").and_then(|v| v.as_str()).unwrap_or("");

    if old_str.is_empty() && new_str.is_empty() { return; }

    let dim_red_bg = Color::Rgb(60, 25, 25);
    let dim_green_bg = Color::Rgb(25, 50, 25);

    let filename = file_path.as_ref()
        .and_then(|p| std::path::Path::new(p).file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file.txt".to_string());

    // Find actual line number by reading the file and locating the edit position.
    // Two cases: (1) edit already applied → new_string is in the file,
    // (2) live preview during streaming → old_string is still in the file.
    // Try new_string first (post-edit), fall back to old_string (mid-edit).
    let start_line = file_path.as_ref().and_then(|p| {
        std::fs::read_to_string(p).ok().and_then(|content| {
            let needle = if !new_str.is_empty() && content.contains(new_str) {
                Some(new_str)
            } else if !old_str.is_empty() && content.contains(old_str) {
                Some(old_str)
            } else {
                None
            };
            needle.and_then(|s| content.find(s).map(|byte_pos| {
                content[..byte_pos].lines().count() + 1
            }))
        })
    }).unwrap_or(1);

    // Syntax highlight only new (added) lines — removed lines use plain grey
    let new_highlighted = highlighter.highlight_with_bg(new_str, &filename, Some(dim_green_bg));
    // Removed lines: dark grey text on dim red bg (darker than comment grey
    // in syntax-highlighted green lines, which is typically ~128 grey)
    let removed_style = Style::default().fg(Color::Rgb(100, 100, 100)).bg(dim_red_bg);

    let old_lines: Vec<&str> = old_str.lines().collect();
    let new_lines: Vec<&str> = new_str.lines().collect();

    let max_line = start_line + old_lines.len().max(new_lines.len());
    let num_width = max_line.to_string().len().max(2);
    let max_len = old_lines.len().max(new_lines.len());
    let content_max = max_width.saturating_sub(4 + num_width + 3 + 1);

    for i in 0..max_len {
        let old_line = old_lines.get(i).copied();
        let new_line = new_lines.get(i).copied();

        match (old_line, new_line) {
            // Unchanged context — dim grey, no background
            (Some(old), Some(new)) if old == new => {
                let dimmed = vec![Span::styled(old.to_string(), Style::default().fg(Color::DarkGray))];
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
            // Changed: show removed then added
            (Some(old_text), Some(_)) => {
                // Removed line — grey text, dim red bg, NO syntax highlighting
                let old_spans = vec![Span::styled(old_text.to_string(), removed_style)];
                for (j, wrapped_spans) in wrap_spans(old_spans, content_max).into_iter().enumerate() {
                    let line_num = if j == 0 { format!(" {:>width$} - ", start_line + i, width = num_width) } else { " ".repeat(num_width + 4) };
                    let mut all_spans = vec![
                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                        Span::styled(line_num, Style::default().fg(Color::Red)),
                    ];
                    all_spans.extend(wrapped_spans);
                    lines.push(Line::from(all_spans));
                }
                // Added line — syntax highlighted, dim green bg
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
            // Removed only — grey text, dim red bg
            (Some(old_text), None) => {
                let old_spans = vec![Span::styled(old_text.to_string(), removed_style)];
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
            // Added only — syntax highlighted, dim green bg
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ─── Helpers ──────────────────────────────────────────────────────

    fn spans_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    // ═══════════════════════════════════════════════════════════════════
    // tool_display_name
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn display_name_grep_uppercase() {
        assert_eq!(tool_display_name("Grep"), "Search");
    }

    #[test]
    fn display_name_grep_lowercase() {
        assert_eq!(tool_display_name("grep"), "Search");
    }

    #[test]
    fn display_name_glob_uppercase() {
        assert_eq!(tool_display_name("Glob"), "Find");
    }

    #[test]
    fn display_name_glob_lowercase() {
        assert_eq!(tool_display_name("glob"), "Find");
    }

    #[test]
    fn display_name_read_passthrough() {
        assert_eq!(tool_display_name("Read"), "Read");
    }

    #[test]
    fn display_name_write_passthrough() {
        assert_eq!(tool_display_name("Write"), "Write");
    }

    #[test]
    fn display_name_bash_passthrough() {
        assert_eq!(tool_display_name("Bash"), "Bash");
    }

    #[test]
    fn display_name_edit_passthrough() {
        assert_eq!(tool_display_name("Edit"), "Edit");
    }

    #[test]
    fn display_name_task_passthrough() {
        assert_eq!(tool_display_name("Task"), "Task");
    }

    #[test]
    fn display_name_unknown_passthrough() {
        assert_eq!(tool_display_name("CustomTool"), "CustomTool");
    }

    #[test]
    fn display_name_empty_string() {
        assert_eq!(tool_display_name(""), "");
    }

    #[test]
    fn display_name_webfetch_passthrough() {
        assert_eq!(tool_display_name("WebFetch"), "WebFetch");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Read
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_read_file_path() {
        let input = json!({"file_path": "/src/main.rs"});
        assert_eq!(extract_tool_param("Read", &input), "/src/main.rs");
    }

    #[test]
    fn extract_read_path_fallback() {
        let input = json!({"path": "/src/lib.rs"});
        assert_eq!(extract_tool_param("Read", &input), "/src/lib.rs");
    }

    #[test]
    fn extract_read_lowercase() {
        let input = json!({"file_path": "/foo.rs"});
        assert_eq!(extract_tool_param("read", &input), "/foo.rs");
    }

    #[test]
    fn extract_read_empty_input() {
        let input = json!({});
        assert_eq!(extract_tool_param("Read", &input), "");
    }

    #[test]
    fn extract_read_null_value() {
        let input = json!({"file_path": null});
        assert_eq!(extract_tool_param("Read", &input), "");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Write
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_write_file_path() {
        let input = json!({"file_path": "/out.txt"});
        assert_eq!(extract_tool_param("Write", &input), "/out.txt");
    }

    #[test]
    fn extract_write_path_fallback() {
        let input = json!({"path": "/out.txt"});
        assert_eq!(extract_tool_param("write", &input), "/out.txt");
    }

    #[test]
    fn extract_write_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("Write", &input), "");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Edit
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_edit_file_path() {
        let input = json!({"file_path": "/src/config.rs"});
        assert_eq!(extract_tool_param("Edit", &input), "/src/config.rs");
    }

    #[test]
    fn extract_edit_path_fallback() {
        let input = json!({"path": "/src/config.rs"});
        assert_eq!(extract_tool_param("edit", &input), "/src/config.rs");
    }

    #[test]
    fn extract_edit_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("Edit", &input), "");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Bash
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_bash_command() {
        let input = json!({"command": "cargo build"});
        assert_eq!(extract_tool_param("Bash", &input), "cargo build");
    }

    #[test]
    fn extract_bash_lowercase() {
        let input = json!({"command": "ls -la"});
        assert_eq!(extract_tool_param("bash", &input), "ls -la");
    }

    #[test]
    fn extract_bash_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("Bash", &input), "");
    }

    #[test]
    fn extract_bash_long_command() {
        let cmd = "a".repeat(500);
        let input = json!({"command": cmd});
        assert_eq!(extract_tool_param("Bash", &input), cmd);
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Glob
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_glob_pattern() {
        let input = json!({"pattern": "**/*.rs"});
        assert_eq!(extract_tool_param("Glob", &input), "**/*.rs");
    }

    #[test]
    fn extract_glob_lowercase() {
        let input = json!({"pattern": "*.txt"});
        assert_eq!(extract_tool_param("glob", &input), "*.txt");
    }

    #[test]
    fn extract_glob_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("Glob", &input), "");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Grep
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_grep_pattern() {
        let input = json!({"pattern": "TODO"});
        assert_eq!(extract_tool_param("Grep", &input), "TODO");
    }

    #[test]
    fn extract_grep_lowercase() {
        let input = json!({"pattern": "fn main"});
        assert_eq!(extract_tool_param("grep", &input), "fn main");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — WebFetch
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_webfetch_url() {
        let input = json!({"url": "https://example.com"});
        assert_eq!(extract_tool_param("WebFetch", &input), "https://example.com");
    }

    #[test]
    fn extract_webfetch_lowercase() {
        let input = json!({"url": "https://foo.bar"});
        assert_eq!(extract_tool_param("webfetch", &input), "https://foo.bar");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — WebSearch
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_websearch_query() {
        let input = json!({"query": "rust async"});
        assert_eq!(extract_tool_param("WebSearch", &input), "rust async");
    }

    #[test]
    fn extract_websearch_lowercase() {
        let input = json!({"query": "test query"});
        assert_eq!(extract_tool_param("websearch", &input), "test query");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Task
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_task_with_type_and_desc() {
        let input = json!({"subagent_type": "code", "description": "refactor module"});
        assert_eq!(extract_tool_param("Task", &input), "[code] refactor module");
    }

    #[test]
    fn extract_task_default_agent_type() {
        let input = json!({"description": "do something"});
        assert_eq!(extract_tool_param("Task", &input), "[agent] do something");
    }

    #[test]
    fn extract_task_no_fields() {
        let input = json!({});
        assert_eq!(extract_tool_param("task", &input), "[agent] ");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — LSP
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_lsp_operation_and_file() {
        let input = json!({"operation": "hover", "filePath": "/src/main.rs"});
        assert_eq!(extract_tool_param("LSP", &input), "hover /src/main.rs");
    }

    #[test]
    fn extract_lsp_lowercase() {
        let input = json!({"operation": "goto", "filePath": "/lib.rs"});
        assert_eq!(extract_tool_param("lsp", &input), "goto /lib.rs");
    }

    #[test]
    fn extract_lsp_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("LSP", &input), " ");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Plan modes
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_enter_plan_mode() {
        let input = json!({});
        assert!(extract_tool_param("EnterPlanMode", &input).contains("Planning"));
    }

    #[test]
    fn extract_exit_plan_mode() {
        let input = json!({});
        assert!(extract_tool_param("ExitPlanMode", &input).contains("Plan complete"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Unknown tools (fallback)
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_unknown_tool_file_path() {
        let input = json!({"file_path": "/x.rs"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "/x.rs");
    }

    #[test]
    fn extract_unknown_tool_path() {
        let input = json!({"path": "/y.rs"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "/y.rs");
    }

    #[test]
    fn extract_unknown_tool_command() {
        let input = json!({"command": "echo hi"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "echo hi");
    }

    #[test]
    fn extract_unknown_tool_query() {
        let input = json!({"query": "something"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "something");
    }

    #[test]
    fn extract_unknown_tool_pattern() {
        let input = json!({"pattern": "*.md"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "*.md");
    }

    #[test]
    fn extract_unknown_tool_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("UnknownTool", &input), "");
    }

    #[test]
    fn extract_unknown_tool_priority_file_path_first() {
        let input = json!({"file_path": "/first", "command": "second"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "/first");
    }

    // ═══════════════════════════════════════════════════════════════════
    // truncate_line
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn truncate_fits_exactly() {
        assert_eq!(truncate_line("hello", 5), "hello");
    }

    #[test]
    fn truncate_shorter_than_max() {
        assert_eq!(truncate_line("hi", 10), "hi");
    }

    #[test]
    fn truncate_over_max() {
        assert_eq!(truncate_line("hello world", 5), "hell\u{2026}");
    }

    #[test]
    fn truncate_max_one() {
        assert_eq!(truncate_line("hello", 1), "\u{2026}");
    }

    #[test]
    fn truncate_empty_string() {
        assert_eq!(truncate_line("", 10), "");
    }

    #[test]
    fn truncate_trims_whitespace() {
        assert_eq!(truncate_line("  hello  ", 10), "hello");
    }

    #[test]
    fn truncate_trims_then_truncates() {
        assert_eq!(truncate_line("  hello world  ", 5), "hell\u{2026}");
    }

    #[test]
    fn truncate_unicode_chars() {
        assert_eq!(truncate_line("\u{65e5}\u{672c}\u{8a9e}", 3), "\u{65e5}\u{672c}\u{8a9e}");
    }

    #[test]
    fn truncate_unicode_over_max() {
        assert_eq!(truncate_line("\u{65e5}\u{672c}\u{8a9e}\u{30c6}\u{30b9}\u{30c8}", 4), "\u{65e5}\u{672c}\u{8a9e}\u{2026}");
    }

    #[test]
    fn truncate_max_zero() {
        assert_eq!(truncate_line("hello", 0), "\u{2026}");
    }

    #[test]
    fn truncate_single_char_fits() {
        assert_eq!(truncate_line("a", 1), "a");
    }

    #[test]
    fn truncate_two_chars_max_one() {
        assert_eq!(truncate_line("ab", 1), "\u{2026}");
    }

    #[test]
    fn truncate_preserves_special_chars() {
        assert_eq!(truncate_line("@#$%^", 5), "@#$%^");
    }

    #[test]
    fn truncate_emoji() {
        let s = "\u{1f389}\u{1f38a}\u{1f388}\u{1f381}";
        assert_eq!(truncate_line(s, 2), "\u{1f389}\u{2026}");
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — empty content
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_read_empty_content() {
        let lines = render_tool_result("Read", None, "", false, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("empty file"));
    }

    #[test]
    fn render_result_bash_empty_content() {
        let lines = render_tool_result("Bash", None, "", false, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("\u{2713}"));
    }

    #[test]
    fn render_result_unknown_empty_content() {
        let lines = render_tool_result("Unknown", None, "", false, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("\u{2713}"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — Read tool
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_read_single_line() {
        let lines = render_tool_result("Read", None, "one line", false, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("one line"));
    }

    #[test]
    fn render_result_read_two_lines() {
        let lines = render_tool_result("Read", None, "first\nlast", false, 80);
        assert_eq!(lines.len(), 2);
        assert!(spans_text(&lines[0]).contains("first"));
        assert!(spans_text(&lines[1]).contains("last"));
    }

    #[test]
    fn render_result_read_many_lines() {
        let content = (1..=10).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        let lines = render_tool_result("Read", None, &content, false, 80);
        assert_eq!(lines.len(), 3);
        assert!(spans_text(&lines[0]).contains("line 1"));
        assert!(spans_text(&lines[1]).contains("10 lines"));
        assert!(spans_text(&lines[2]).contains("line 10"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — Bash tool
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_bash_single_line() {
        let lines = render_tool_result("Bash", None, "output", false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("output"));
    }

    #[test]
    fn render_result_bash_multiple_lines() {
        let lines = render_tool_result("Bash", None, "line1\nline2\nline3", false, 80);
        assert_eq!(lines.len(), 2);
        assert!(spans_text(&lines[0]).contains("line2"));
        assert!(spans_text(&lines[1]).contains("line3"));
    }

    #[test]
    fn render_result_bash_skips_empty_lines() {
        let lines = render_tool_result("Bash", None, "result\n\n\n", false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("result"));
    }

    #[test]
    fn render_result_bash_all_empty_lines() {
        let lines = render_tool_result("Bash", None, "\n\n\n", false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("\u{2713}"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — Grep tool
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_grep_few_matches() {
        let lines = render_tool_result("Grep", None, "match1\nmatch2", false, 80);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn render_result_grep_exactly_three() {
        let lines = render_tool_result("Grep", None, "a\nb\nc", false, 80);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn render_result_grep_more_than_three() {
        let content = "a\nb\nc\nd\ne";
        let lines = render_tool_result("Grep", None, content, false, 80);
        assert_eq!(lines.len(), 4);
        let last_text = spans_text(lines.last().unwrap());
        assert!(last_text.contains("+2 more"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — Glob tool
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_glob_file_count() {
        let content = "file1.rs\nfile2.rs\nfile3.rs";
        let lines = render_tool_result("Glob", None, content, false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("3 files"));
    }

    #[test]
    fn render_result_glob_single_file() {
        let lines = render_tool_result("Glob", None, "one.rs", false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("1 files"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — Task tool
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_task_few_lines() {
        let lines = render_tool_result("Task", None, "line1\nline2", false, 80);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn render_result_task_many_lines() {
        let content = (1..=10).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        let lines = render_tool_result("Task", None, &content, false, 80);
        assert_eq!(lines.len(), 6);
        let last_text = spans_text(lines.last().unwrap());
        assert!(last_text.contains("+5 more lines"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — Unknown/default tool
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_default_few_lines() {
        let lines = render_tool_result("SomeTool", None, "a\nb", false, 80);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn render_result_default_more_than_three() {
        let content = "a\nb\nc\nd\ne";
        let lines = render_tool_result("SomeTool", None, content, false, 80);
        assert_eq!(lines.len(), 4);
        let last_text = spans_text(lines.last().unwrap());
        assert!(last_text.contains("+2 more"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — failed state
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_failed_uses_red() {
        let lines = render_tool_result("Bash", None, "error occurred", true, 80);
        assert!(!lines.is_empty());
        let has_red = lines[0].spans.iter().any(|s| s.style.fg == Some(Color::Red));
        assert!(has_red);
    }

    #[test]
    fn render_result_success_uses_azure() {
        let lines = render_tool_result("Bash", None, "ok", false, 80);
        assert!(!lines.is_empty());
        let has_azure = lines[0].spans.iter().any(|s| s.style.fg == Some(AZURE));
        assert!(has_azure);
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — system-reminder stripping
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_strips_system_reminder() {
        let content = "real output\n<system-reminder>hidden stuff</system-reminder>";
        let lines = render_tool_result("Bash", None, content, false, 80);
        for line in &lines {
            let text = spans_text(line);
            assert!(!text.contains("hidden"));
        }
    }

    #[test]
    fn render_result_system_reminder_at_start() {
        let content = "<system-reminder>all hidden</system-reminder>";
        let lines = render_tool_result("Bash", None, content, false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("\u{2713}"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — max_width truncation
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_narrow_width_truncates() {
        let long_line = "a".repeat(200);
        let lines = render_tool_result("Bash", None, &long_line, false, 30);
        assert!(!lines.is_empty());
        let text = spans_text(&lines[0]);
        assert!(text.len() < 200);
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_write_preview
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn write_preview_with_content() {
        let input = json!({"content": "// module doc\nfn main() {}\nreturn;\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("3 lines"));
    }

    #[test]
    fn write_preview_shows_purpose_comment() {
        let input = json!({"content": "// This is the purpose\nfn foo() {}\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("// This is the purpose"));
    }

    #[test]
    fn write_preview_hash_comment() {
        let input = json!({"content": "# Python module\nimport os\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("# Python module"));
    }

    #[test]
    fn write_preview_no_comment_shows_first_line() {
        let input = json!({"content": "fn main() {}\nlet x = 1;\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("fn main()"));
    }

    #[test]
    fn write_preview_no_content_field() {
        let input = json!({"file_path": "/foo.rs"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        assert!(lines.is_empty());
    }

    #[test]
    fn write_preview_empty_content() {
        let input = json!({"content": ""});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("0 lines"));
    }

    #[test]
    fn write_preview_checkmark() {
        let input = json!({"content": "hello\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("\u{2713}"));
    }

    #[test]
    fn write_preview_triple_slash_comment() {
        let input = json!({"content": "some code\n/// Doc comment\nmore code\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("/// Doc comment"));
    }

    #[test]
    fn write_preview_inner_doc_comment() {
        let input = json!({"content": "some code\n//! Inner doc\nmore code\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("//! Inner doc"));
    }

    #[test]
    fn write_preview_block_comment() {
        let input = json!({"content": "/* Block comment */\ncode\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("/* Block comment */"));
    }
}
