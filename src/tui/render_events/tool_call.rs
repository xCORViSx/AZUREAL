//! Tool invocation rendering — status indicators, clickable paths, and edit/write previews

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::HashSet;

use crate::syntax::SyntaxHighlighter;
use crate::tui::colorize::ORANGE;
use crate::tui::render_tools::{
    extract_edit_preview_strings, extract_tool_param, render_edit_diff, render_write_preview,
    tool_display_name,
};
use crate::tui::render_wrap::wrap_text;
use crate::tui::util::AZURE;

use super::ClickablePath;

pub(super) fn render_tool_call(
    lines: &mut Vec<Line<'static>>,
    animation_indices: &mut Vec<(usize, usize, String)>,
    clickable_paths: &mut Vec<ClickablePath>,
    tool_name: &str,
    file_path: &Option<String>,
    input: &serde_json::Value,
    tool_use_id: &str,
    pending_tools: &HashSet<String>,
    failed_tools: &HashSet<String>,
    bubble_width: usize,
    highlighter: &mut SyntaxHighlighter,
) {
    let tool_color = AZURE;
    let is_pending = pending_tools.contains(tool_use_id);
    let is_failed = failed_tools.contains(tool_use_id);

    lines.push(Line::from(vec![Span::styled(
        " ┃",
        Style::default().fg(tool_color),
    )]));

    // Avoid cloning file_path — borrow when available, allocate only for fallback
    let param_owned;
    let param_raw: &str = match file_path {
        Some(fp) => fp.as_str(),
        None => {
            param_owned = extract_tool_param(tool_name, input);
            &param_owned
        }
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
        Style::default()
            .fg(ORANGE)
            .add_modifier(Modifier::UNDERLINED)
    } else {
        Style::default().fg(ORANGE)
    };

    let wrapped_param_lines = wrap_text(param_raw, param_max);
    let wrap_line_count = wrapped_param_lines.len();
    for (i, wrapped) in wrapped_param_lines.into_iter().enumerate() {
        if i == 0 {
            // Track line index for draw-time status patching (span index 1 is the indicator).
            // ALL tool calls are tracked (not just pending) so completed/failed status
            // updates immediately without waiting for a full re-render.
            animation_indices.push((lines.len(), 1, tool_use_id.to_string()));
            // Record clickable region for file tools — wrap_line_count tells highlight
            // how many cache lines the path spans (for multi-line highlight)
            if is_file_tool && !param_raw.is_empty() {
                let start_col = prefix_len;
                let end_col = start_col + wrapped.chars().count();
                let (old_s, new_s) = if tool_name == "Edit" {
                    extract_edit_preview_strings(input)
                } else {
                    (String::new(), String::new())
                };
                clickable_paths.push((
                    lines.len(),
                    start_col,
                    end_col,
                    param_raw.to_string(),
                    old_s,
                    new_s,
                    wrap_line_count,
                ));
            }
            lines.push(Line::from(vec![
                Span::styled(" ┣━", Style::default().fg(tool_color)),
                Span::styled(indicator, Style::default().fg(indicator_color)),
                Span::styled(
                    display_name.to_string(),
                    Style::default().fg(tool_color).add_modifier(Modifier::BOLD),
                ),
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
