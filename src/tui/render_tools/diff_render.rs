//! Diff rendering for edit and write tool previews
//!
//! Renders apply-patch and unified-diff formats as styled TUI lines with
//! syntax highlighting, line numbers, and added/removed line coloring.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::diff_parse::{
    extract_edit_preview_strings, parse_apply_patch_lines, parse_unified_diff_lines,
    ApplyPatchLineKind,
};
use crate::syntax::SyntaxHighlighter;
use crate::tui::render_wrap::wrap_spans;

fn render_apply_patch_preview(
    lines: &mut Vec<Line<'static>>,
    patch: &str,
    tool_color: Color,
    max_width: usize,
) {
    let dim_red_bg = Color::Rgb(60, 25, 25);
    let dim_green_bg = Color::Rgb(25, 50, 25);
    let removed_style = Style::default()
        .fg(Color::Rgb(170, 170, 170))
        .bg(dim_red_bg);
    let added_style = Style::default().fg(Color::White).bg(dim_green_bg);
    let content_max = max_width.saturating_sub(4);

    for patch_line in parse_apply_patch_lines(patch) {
        // Skip header/hunk lines — the tree node already shows the file path
        if matches!(
            patch_line.kind,
            ApplyPatchLineKind::Header | ApplyPatchLineKind::Hunk
        ) {
            continue;
        }

        let style = match patch_line.kind {
            ApplyPatchLineKind::Header | ApplyPatchLineKind::Hunk => unreachable!(),
            ApplyPatchLineKind::Meta => Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
            ApplyPatchLineKind::Context => Style::default().fg(Color::DarkGray),
            ApplyPatchLineKind::Added => added_style,
            ApplyPatchLineKind::Removed => removed_style,
        };

        for wrapped_spans in wrap_spans(vec![Span::styled(patch_line.text, style)], content_max) {
            let mut all_spans = vec![Span::styled(" ┃  ", Style::default().fg(tool_color))];
            all_spans.extend(wrapped_spans);
            lines.push(Line::from(all_spans));
        }
    }
}

fn render_unified_diff_preview(
    lines: &mut Vec<Line<'static>>,
    diff: &str,
    tool_color: Color,
    max_width: usize,
) {
    let dim_red_bg = Color::Rgb(60, 25, 25);
    let dim_green_bg = Color::Rgb(25, 50, 25);
    let removed_style = Style::default()
        .fg(Color::Rgb(170, 170, 170))
        .bg(dim_red_bg);
    let added_style = Style::default().fg(Color::White).bg(dim_green_bg);
    let content_max = max_width.saturating_sub(4);

    for diff_line in parse_unified_diff_lines(diff) {
        // Skip header/hunk lines — the tree node already shows the file path
        if matches!(
            diff_line.kind,
            ApplyPatchLineKind::Header | ApplyPatchLineKind::Hunk
        ) {
            continue;
        }

        let style = match diff_line.kind {
            ApplyPatchLineKind::Header | ApplyPatchLineKind::Hunk => unreachable!(),
            ApplyPatchLineKind::Meta => Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
            ApplyPatchLineKind::Context => Style::default().fg(Color::DarkGray),
            ApplyPatchLineKind::Added => added_style,
            ApplyPatchLineKind::Removed => removed_style,
        };

        for wrapped_spans in wrap_spans(vec![Span::styled(diff_line.text, style)], content_max) {
            let mut all_spans = vec![Span::styled(" ┃  ", Style::default().fg(tool_color))];
            all_spans.extend(wrapped_spans);
            lines.push(Line::from(all_spans));
        }
    }
}

/// Render Edit tool diff inline.
/// Reads file to find actual line numbers (runs on background render thread,
/// not the draw path). Removed lines show grey text on dim red bg (no syntax
/// highlighting). Added lines get syntax highlighting on dim green bg.
pub fn render_edit_diff(
    lines: &mut Vec<Line<'static>>,
    input: &serde_json::Value,
    file_path: &Option<String>,
    tool_color: Color,
    max_width: usize,
    highlighter: &mut SyntaxHighlighter,
) {
    if let Some(patch) = input.get("patch").and_then(|v| v.as_str()) {
        render_apply_patch_preview(lines, patch, tool_color, max_width);
        return;
    }
    if let Some(diff) = input.get("unified_diff").and_then(|v| v.as_str()) {
        render_unified_diff_preview(lines, diff, tool_color, max_width);
        return;
    }

    let (old_owned, new_owned) = extract_edit_preview_strings(input);
    let old_str = old_owned.as_str();
    let new_str = new_owned.as_str();

    if old_str.is_empty() && new_str.is_empty() {
        return;
    }

    let dim_red_bg = Color::Rgb(60, 25, 25);
    let dim_green_bg = Color::Rgb(25, 50, 25);

    let filename = file_path
        .as_ref()
        .and_then(|p| std::path::Path::new(p).file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file.txt".to_string());

    // Find actual line number by reading the file and locating the edit position.
    // Two cases: (1) edit already applied → new_string is in the file,
    // (2) live preview during streaming → old_string is still in the file.
    // Try new_string first (post-edit), fall back to old_string (mid-edit).
    // Keep file content for full-context tree-sitter highlighting.
    let (start_line, file_content) = file_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|content| {
            let needle = if !new_str.is_empty() && content.contains(new_str) {
                Some(new_str)
            } else if !old_str.is_empty() && content.contains(old_str) {
                Some(old_str)
            } else {
                None
            };
            let line = needle
                .and_then(|s| {
                    content
                        .find(s)
                        .map(|byte_pos| content[..byte_pos].lines().count() + 1)
                })
                .unwrap_or(1);
            (line, content)
        })
        .map_or((1, None), |(l, c)| (l, Some(c)));

    let new_line_count = new_str.lines().count();

    // Syntax highlight new (added) lines using full-file context when available.
    // Tree-sitter needs complete file context to build a proper AST — a bare snippet
    // starting mid-function only parses the first few tokens, then falls to white.
    let new_highlighted = match file_content {
        Some(ref full) if !new_str.is_empty() && full.contains(new_str) => {
            // Edit already applied: highlight full file, extract the edited region
            let all = highlighter.highlight_file(full, &filename);
            let start = start_line.saturating_sub(1);
            all.into_iter()
                .skip(start)
                .take(new_line_count)
                .map(|spans| {
                    spans
                        .into_iter()
                        .map(|s| Span::styled(s.content, s.style.bg(dim_green_bg)))
                        .collect()
                })
                .collect()
        }
        _ => {
            // Fallback: snippet-only highlighting (mid-edit or file unreadable)
            highlighter.highlight_with_bg(new_str, &filename, Some(dim_green_bg))
        }
    };
    // Removed lines: dark grey text on dim red bg (darker than comment grey
    // in syntax-highlighted green lines, which is typically ~128 grey)
    let removed_style = Style::default()
        .fg(Color::Rgb(100, 100, 100))
        .bg(dim_red_bg);

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
                let dimmed = vec![Span::styled(
                    old.to_string(),
                    Style::default().fg(Color::DarkGray),
                )];
                for (j, wrapped_spans) in wrap_spans(dimmed, content_max).into_iter().enumerate() {
                    let line_num = if j == 0 {
                        format!(" {:>width$}   ", start_line + i, width = num_width)
                    } else {
                        " ".repeat(num_width + 4)
                    };
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
                for (j, wrapped_spans) in wrap_spans(old_spans, content_max).into_iter().enumerate()
                {
                    let line_num = if j == 0 {
                        format!(" {:>width$} - ", start_line + i, width = num_width)
                    } else {
                        " ".repeat(num_width + 4)
                    };
                    let mut all_spans = vec![
                        Span::styled(" ┃  ", Style::default().fg(tool_color)),
                        Span::styled(line_num, Style::default().fg(Color::Red)),
                    ];
                    all_spans.extend(wrapped_spans);
                    lines.push(Line::from(all_spans));
                }
                // Added line — syntax highlighted, dim green bg
                let new_spans = new_highlighted.get(i).cloned().unwrap_or_default();
                for (j, wrapped_spans) in wrap_spans(new_spans, content_max).into_iter().enumerate()
                {
                    let line_num = if j == 0 {
                        format!(" {:>width$} + ", start_line + i, width = num_width)
                    } else {
                        " ".repeat(num_width + 4)
                    };
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
                for (j, wrapped_spans) in wrap_spans(old_spans, content_max).into_iter().enumerate()
                {
                    let line_num = if j == 0 {
                        format!(" {:>width$} - ", start_line + i, width = num_width)
                    } else {
                        " ".repeat(num_width + 4)
                    };
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
                for (j, wrapped_spans) in wrap_spans(new_spans, content_max).into_iter().enumerate()
                {
                    let line_num = if j == 0 {
                        format!(" {:>width$} + ", start_line + i, width = num_width)
                    } else {
                        " ".repeat(num_width + 4)
                    };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::util::AZURE;
    use serde_json::json;

    fn spans_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn render_edit_diff_from_patch_shows_diff_lines() {
        let input = json!({
            "patch": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old_value();\n+new_value();\n unchanged();\n*** End Patch"
        });
        let mut lines = Vec::new();
        let mut highlighter = SyntaxHighlighter::new();
        render_edit_diff(
            &mut lines,
            &input,
            &Some("src/main.rs".to_string()),
            AZURE,
            80,
            &mut highlighter,
        );
        let rendered = lines.iter().map(spans_text).collect::<Vec<_>>().join("\n");
        assert!(!rendered.contains("Update File:"));
        assert!(!rendered.contains("@@"));
        assert!(rendered.contains("-old_value();"));
        assert!(rendered.contains("+new_value();"));
        assert!(rendered.contains(" unchanged();"));
    }

    #[test]
    fn render_edit_diff_from_unified_diff_shows_diff_lines() {
        let input = json!({
            "unified_diff": "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,3 @@\n-old_value();\n+new_value();\n unchanged();\n"
        });
        let mut lines = Vec::new();
        let mut highlighter = SyntaxHighlighter::new();
        render_edit_diff(
            &mut lines,
            &input,
            &Some("src/main.rs".to_string()),
            AZURE,
            80,
            &mut highlighter,
        );
        let rendered = lines.iter().map(spans_text).collect::<Vec<_>>().join("\n");
        assert!(!rendered.contains("diff --git"));
        assert!(!rendered.contains("@@ -1,3 +1,3 @@"));
        assert!(rendered.contains("-old_value();"));
        assert!(rendered.contains("+new_value();"));
        assert!(rendered.contains(" unchanged();"));
    }

    #[test]
    fn render_edit_diff_from_patch_skips_header_and_hunk() {
        let input = json!({
            "patch": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-old\n+new\n*** End Patch"
        });
        let mut lines = Vec::new();
        let mut highlighter = SyntaxHighlighter::new();
        render_edit_diff(
            &mut lines,
            &input,
            &Some("src/lib.rs".to_string()),
            AZURE,
            80,
            &mut highlighter,
        );
        // Header ("Update File:") and hunk ("@@") lines should be skipped;
        // the first rendered content line is the removed line "-old"
        let rendered: String = lines.iter().flat_map(|l| l.spans.iter()).map(|s| s.content.as_ref()).collect();
        assert!(!rendered.contains("Update File:"));
        assert!(!rendered.contains("@@"));
        assert!(rendered.contains("-old"));
        assert!(rendered.contains("+new"));
    }
}
