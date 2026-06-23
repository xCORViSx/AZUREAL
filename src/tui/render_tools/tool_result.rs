//! Tool result rendering
//!
//! Renders summarized tool output (results, write previews) as styled TUI lines.

use std::borrow::Cow;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::tool_params::truncate_line;
use crate::tui::util::AZURE;

/// Returns whether a tool result should be summarized like terminal output.
fn is_bash_like_tool(tool_name: &str) -> bool {
    matches!(tool_name, "Bash" | "bash" | "exec_command" | "write_stdin")
}

/// Normalizes wrapped command-runner output into the user-visible payload.
fn normalize_bash_like_output(content: &str) -> String {
    if !content.starts_with("Chunk ID:") {
        return content.to_string();
    }

    if let Some((_, tail)) = content.split_once("\nOutput:\n") {
        let actual = tail.trim_end_matches('\n');
        if !actual.trim().is_empty() {
            return actual.to_string();
        }
    }

    if let Some(code) = content
        .lines()
        .find_map(|line| line.strip_prefix("Process exited with code "))
    {
        let code = code.trim();
        return if code == "0" {
            String::new()
        } else {
            format!("Exit code: {code}")
        };
    }

    if let Some(session_id) = content
        .lines()
        .find_map(|line| line.strip_prefix("Process running with session ID "))
    {
        return format!("Process running with session ID {}", session_id.trim());
    }

    content.to_string()
}

/// Removes system-reminder blocks while preserving real output after closed blocks.
///
/// Claude can append hidden reminder markup into tool streams. Closed blocks are
/// removed in place, while an unmatched opening tag discards the rest of the
/// content because the remaining text is no longer safely distinguishable from
/// hidden reminder text.
fn strip_system_reminder_blocks(content: &str) -> Cow<'_, str> {
    /// Opening marker for hidden system reminder blocks.
    const OPEN: &str = "<system-reminder>";
    /// Closing marker for hidden system reminder blocks.
    const CLOSE: &str = "</system-reminder>";

    if !content.contains(OPEN) {
        return Cow::Borrowed(content);
    }

    let mut stripped = String::with_capacity(content.len());
    let mut remaining = content;
    while let Some(start) = remaining.find(OPEN) {
        stripped.push_str(&remaining[..start]);
        let after_open = &remaining[start + OPEN.len()..];
        let Some(end) = after_open.find(CLOSE) else {
            return Cow::Owned(stripped);
        };
        remaining = &after_open[end + CLOSE.len()..];
    }
    stripped.push_str(remaining);

    Cow::Owned(stripped)
}

/// Render tool result output based on tool type.
/// Shows summarized output constrained to max_width.
pub fn render_tool_result(
    tool_name: &str,
    _file_path: Option<&str>,
    content: &str,
    is_failed: bool,
    max_width: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let tool_color = if is_failed { Color::Red } else { AZURE };
    let result_style = Style::default().fg(if is_failed { Color::Red } else { Color::Gray });
    let content = if is_bash_like_tool(tool_name) {
        normalize_bash_like_output(content)
    } else {
        content.to_string()
    };

    let content = strip_system_reminder_blocks(&content);
    let content = content.trim_end();

    let content_lines: Vec<&str> = content.lines().collect();
    let line_count = content_lines.len();
    // Account for " ┃  └─ " prefix (7 chars)
    let text_max = max_width.saturating_sub(8);

    if line_count == 0 {
        let msg = match tool_name {
            "Read" => "(empty file)",
            "Bash" | "exec_command" | "write_stdin" => "✓",
            _ => "✓",
        };
        lines.push(Line::from(vec![
            Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
            Span::styled(
                msg,
                if is_bash_like_tool(tool_name) {
                    Style::default().fg(Color::Green)
                } else {
                    result_style
                },
            ),
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
                    Span::styled(
                        format!("  ({} lines)", line_count),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
            if line_count > 1 {
                let last = content_lines
                    .iter()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or(&"");
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(truncate_line(last, text_max), result_style),
                ]));
            } else {
                // Single line - mark as last
                if let Some(l) = lines.last_mut() {
                    if let Some(span) = l.spans.first_mut() {
                        *span = Span::styled(" ┃  └─ ", result_style.fg(tool_color));
                    }
                }
            }
        }
        "Bash" | "bash" | "exec_command" | "write_stdin" => {
            // Last 2 non-empty lines (results usually at end)
            let non_empty: Vec<&str> = content_lines
                .iter()
                .filter(|l| !l.trim().is_empty())
                .copied()
                .collect();
            let show: Vec<&str> = non_empty.iter().rev().take(2).rev().copied().collect();
            for (i, l) in show.iter().enumerate() {
                let prefix = if i == show.len() - 1 {
                    " ┃  └─ "
                } else {
                    " ┃  │ "
                };
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
                let prefix = if i == show_count - 1 && line_count <= 3 {
                    " ┃  └─ "
                } else {
                    " ┃  │ "
                };
                lines.push(Line::from(vec![
                    Span::styled(prefix, result_style.fg(tool_color)),
                    Span::styled(truncate_line(l, text_max), result_style),
                ]));
            }
            if line_count > 3 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(
                        format!("  (+{} more)", line_count - 3),
                        Style::default().fg(Color::DarkGray),
                    ),
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
        "Agent" | "agent" | "Task" | "task" => {
            // First 5 lines of subagent output
            let show_count = 5.min(line_count);
            for (i, l) in content_lines.iter().take(show_count).enumerate() {
                let prefix = if i == show_count - 1 && line_count <= 5 {
                    " ┃  └─ "
                } else {
                    " ┃  │ "
                };
                lines.push(Line::from(vec![
                    Span::styled(prefix, result_style.fg(tool_color)),
                    Span::styled(truncate_line(l, text_max), result_style),
                ]));
            }
            if line_count > 5 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(
                        format!("  (+{} more lines)", line_count - 5),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }
        _ => {
            // Default: first 3 lines
            let show_count = 3.min(line_count);
            for (i, l) in content_lines.iter().take(show_count).enumerate() {
                let prefix = if i == show_count - 1 && line_count <= 3 {
                    " ┃  └─ "
                } else {
                    " ┃  │ "
                };
                lines.push(Line::from(vec![
                    Span::styled(prefix, result_style.fg(tool_color)),
                    Span::styled(truncate_line(l, text_max), result_style),
                ]));
            }
            if line_count > 3 {
                lines.push(Line::from(vec![
                    Span::styled(" ┃  └─ ", result_style.fg(tool_color)),
                    Span::styled(
                        format!("  (+{} more)", line_count - 3),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }
    }

    lines
}

/// Render Write tool preview showing line count and purpose
pub fn render_write_preview(
    lines: &mut Vec<Line<'static>>,
    input: &serde_json::Value,
    tool_color: Color,
    max_width: usize,
) {
    if let Some(content) = input.get("content").and_then(|v| v.as_str()) {
        let content_lines: Vec<&str> = content.lines().collect();
        let line_count = content_lines.len();

        let purpose_line = content_lines
            .iter()
            .find(|l| {
                let trimmed = l.trim();
                trimmed.starts_with("//")
                    || trimmed.starts_with("#")
                    || trimmed.starts_with("/*")
                    || trimmed.starts_with("\"\"\"")
                    || trimmed.starts_with("///")
                    || trimmed.starts_with("//!")
            })
            .or(content_lines.first())
            .copied()
            .unwrap_or("");

        let purpose_max = max_width.saturating_sub(20 + format!("{}", line_count).len());
        lines.push(Line::from(vec![
            Span::styled(" ┃  └─ ", Style::default().fg(tool_color)),
            Span::styled("✓ ", Style::default().fg(Color::Green)),
            Span::styled(
                format!("{} lines", line_count),
                Style::default().fg(Color::White),
            ),
            if !purpose_line.is_empty() {
                Span::styled(
                    format!("  {}", truncate_line(purpose_line, purpose_max)),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )
            } else {
                Span::raw("")
            },
        ]));
    }
}

#[cfg(test)]
/// Regression coverage for tool-result summarization and write previews.
mod tests {
    use super::*;
    use serde_json::json;

    /// Collects a rendered line's spans into plain text for assertions.
    fn spans_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — empty content
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies render result read empty content.
    #[test]
    fn render_result_read_empty_content() {
        let lines = render_tool_result("Read", None, "", false, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("empty file"));
    }

    /// Verifies render result bash empty content.
    #[test]
    fn render_result_bash_empty_content() {
        let lines = render_tool_result("Bash", None, "", false, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("\u{2713}"));
    }

    /// Verifies render result unknown empty content.
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

    /// Verifies render result read single line.
    #[test]
    fn render_result_read_single_line() {
        let lines = render_tool_result("Read", None, "one line", false, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("one line"));
    }

    /// Verifies render result read two lines.
    #[test]
    fn render_result_read_two_lines() {
        let lines = render_tool_result("Read", None, "first\nlast", false, 80);
        assert_eq!(lines.len(), 2);
        assert!(spans_text(&lines[0]).contains("first"));
        assert!(spans_text(&lines[1]).contains("last"));
    }

    /// Verifies render result read many lines.
    #[test]
    fn render_result_read_many_lines() {
        let content = (1..=10)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = render_tool_result("Read", None, &content, false, 80);
        assert_eq!(lines.len(), 3);
        assert!(spans_text(&lines[0]).contains("line 1"));
        assert!(spans_text(&lines[1]).contains("10 lines"));
        assert!(spans_text(&lines[2]).contains("line 10"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — Bash tool
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies render result bash single line.
    #[test]
    fn render_result_bash_single_line() {
        let lines = render_tool_result("Bash", None, "output", false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("output"));
    }

    /// Verifies render result bash multiple lines.
    #[test]
    fn render_result_bash_multiple_lines() {
        let lines = render_tool_result("Bash", None, "line1\nline2\nline3", false, 80);
        assert_eq!(lines.len(), 2);
        assert!(spans_text(&lines[0]).contains("line2"));
        assert!(spans_text(&lines[1]).contains("line3"));
    }

    /// Verifies render result bash skips empty lines.
    #[test]
    fn render_result_bash_skips_empty_lines() {
        let lines = render_tool_result("Bash", None, "result\n\n\n", false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("result"));
    }

    /// Verifies render result bash all empty lines.
    #[test]
    fn render_result_bash_all_empty_lines() {
        let lines = render_tool_result("Bash", None, "\n\n\n", false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("\u{2713}"));
    }

    /// Verifies render result exec command strips exec wrapper.
    #[test]
    fn render_result_exec_command_strips_exec_wrapper() {
        let content = "Chunk ID: 6bf9d8\nWall time: 0.0000 seconds\nProcess exited with code 0\nOriginal token count: 7\nOutput:\n/Users/macbookpro/AZUREAL\n";
        let lines = render_tool_result("exec_command", None, content, false, 80);
        assert_eq!(lines.len(), 1);
        assert_eq!(spans_text(&lines[0]), " ┃  └─ /Users/macbookpro/AZUREAL");
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — Grep tool
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies render result grep few matches.
    #[test]
    fn render_result_grep_few_matches() {
        let lines = render_tool_result("Grep", None, "match1\nmatch2", false, 80);
        assert_eq!(lines.len(), 2);
    }

    /// Verifies render result grep exactly three.
    #[test]
    fn render_result_grep_exactly_three() {
        let lines = render_tool_result("Grep", None, "a\nb\nc", false, 80);
        assert_eq!(lines.len(), 3);
    }

    /// Verifies render result grep more than three.
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

    /// Verifies render result glob file count.
    #[test]
    fn render_result_glob_file_count() {
        let content = "file1.rs\nfile2.rs\nfile3.rs";
        let lines = render_tool_result("Glob", None, content, false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("3 files"));
    }

    /// Verifies render result glob single file.
    #[test]
    fn render_result_glob_single_file() {
        let lines = render_tool_result("Glob", None, "one.rs", false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("1 files"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — Task tool
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies render result task few lines.
    #[test]
    fn render_result_task_few_lines() {
        let lines = render_tool_result("Task", None, "line1\nline2", false, 80);
        assert_eq!(lines.len(), 2);
    }

    /// Verifies render result task many lines.
    #[test]
    fn render_result_task_many_lines() {
        let content = (1..=10)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = render_tool_result("Task", None, &content, false, 80);
        assert_eq!(lines.len(), 6);
        let last_text = spans_text(lines.last().unwrap());
        assert!(last_text.contains("+5 more lines"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — Unknown/default tool
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies render result default few lines.
    #[test]
    fn render_result_default_few_lines() {
        let lines = render_tool_result("SomeTool", None, "a\nb", false, 80);
        assert_eq!(lines.len(), 2);
    }

    /// Verifies render result default more than three.
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

    /// Verifies render result failed uses red.
    #[test]
    fn render_result_failed_uses_red() {
        let lines = render_tool_result("Bash", None, "error occurred", true, 80);
        assert!(!lines.is_empty());
        let has_red = lines[0]
            .spans
            .iter()
            .any(|s| s.style.fg == Some(Color::Red));
        assert!(has_red);
    }

    /// Verifies render result success uses azure.
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

    /// Verifies render result strips system reminder.
    #[test]
    fn render_result_strips_system_reminder() {
        let content = "real output\n<system-reminder>hidden stuff</system-reminder>";
        let lines = render_tool_result("Bash", None, content, false, 80);
        for line in &lines {
            let text = spans_text(line);
            assert!(!text.contains("hidden"));
        }
    }

    /// Verifies render result preserves output after closed system reminder.
    #[test]
    fn render_result_preserves_output_after_closed_system_reminder() {
        let content = "before\n<system-reminder>hidden stuff</system-reminder>\nafter reminder";
        let lines = render_tool_result("Bash", None, content, false, 80);
        let rendered = lines.iter().map(spans_text).collect::<Vec<_>>().join("\n");
        assert!(rendered.contains("before"));
        assert!(rendered.contains("after reminder"));
        assert!(!rendered.contains("hidden"));
    }

    /// Verifies render result truncates unmatched system reminder tails.
    #[test]
    fn render_result_truncates_unmatched_system_reminder_tail() {
        let content = "before\n<system-reminder>hidden stuff\nafter marker";
        let lines = render_tool_result("Bash", None, content, false, 80);
        let rendered = lines.iter().map(spans_text).collect::<Vec<_>>().join("\n");
        assert!(rendered.contains("before"));
        assert!(!rendered.contains("hidden"));
        assert!(!rendered.contains("after marker"));
    }

    /// Verifies render result system reminder at start.
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

    /// Verifies render result narrow width truncates.
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

    /// Verifies write preview with content.
    #[test]
    fn write_preview_with_content() {
        let input = json!({"content": "// module doc\nfn main() {}\nreturn;\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("3 lines"));
    }

    /// Verifies write preview shows purpose comment.
    #[test]
    fn write_preview_shows_purpose_comment() {
        let input = json!({"content": "// This is the purpose\nfn foo() {}\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("// This is the purpose"));
    }

    /// Verifies write preview hash comment.
    #[test]
    fn write_preview_hash_comment() {
        let input = json!({"content": "# Python module\nimport os\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("# Python module"));
    }

    /// Verifies write preview no comment shows first line.
    #[test]
    fn write_preview_no_comment_shows_first_line() {
        let input = json!({"content": "fn main() {}\nlet x = 1;\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("fn main()"));
    }

    /// Verifies write preview no content field.
    #[test]
    fn write_preview_no_content_field() {
        let input = json!({"file_path": "/foo.rs"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        assert!(lines.is_empty());
    }

    /// Verifies write preview empty content.
    #[test]
    fn write_preview_empty_content() {
        let input = json!({"content": ""});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("0 lines"));
    }

    /// Verifies write preview checkmark.
    #[test]
    fn write_preview_checkmark() {
        let input = json!({"content": "hello\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("\u{2713}"));
    }

    /// Verifies write preview triple slash comment.
    #[test]
    fn write_preview_triple_slash_comment() {
        let input = json!({"content": "some code\n/// Doc comment\nmore code\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("/// Doc comment"));
    }

    /// Verifies write preview inner doc comment.
    #[test]
    fn write_preview_inner_doc_comment() {
        let input = json!({"content": "some code\n//! Inner doc\nmore code\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("//! Inner doc"));
    }

    /// Verifies write preview block comment.
    #[test]
    fn write_preview_block_comment() {
        let input = json!({"content": "/* Block comment */\ncode\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("/* Block comment */"));
    }
}
