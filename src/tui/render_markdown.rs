//! Markdown rendering for assistant messages
//!
//! Renders markdown text with code blocks, tables, headers, lists, and quotes.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use textwrap::Options;

use super::colorize::ORANGE;
use super::markdown::{is_table_separator, parse_markdown_segments};
use super::render_wrap::wrap_text;
use super::util::AZURE;
use crate::syntax::SyntaxHighlighter;

pub type AssistantPathRegion = (usize, usize, usize, String);

fn starts_verification_paragraph(trimmed: &str) -> bool {
    if trimmed.starts_with("Verification:") {
        return true;
    }

    let stripped = trimmed.trim_start_matches(|c| c == '*' || c == '_');
    stripped != trimmed && stripped.starts_with("Verification:")
}

/// Render assistant markdown text into lines (with syntax-highlighted code blocks)
/// Returns (rendered_lines, table_regions) where table_regions are
/// (output_line_start, output_line_end, raw_markdown) for click-to-expand.
pub fn render_assistant_text(
    text: &str,
    bubble_width: usize,
    highlighter: &mut SyntaxHighlighter,
) -> (Vec<Line<'static>>, Vec<(usize, usize, String)>) {
    let (lines, table_regions, _) =
        render_assistant_text_with_paths(text, bubble_width, highlighter);
    (lines, table_regions)
}

pub fn render_assistant_text_with_paths(
    text: &str,
    bubble_width: usize,
    highlighter: &mut SyntaxHighlighter,
) -> (
    Vec<Line<'static>>,
    Vec<(usize, usize, String)>,
    Vec<AssistantPathRegion>,
) {
    let mut lines = Vec::new();
    let mut table_regions: Vec<(usize, usize, String)> = Vec::new();
    let mut clickable_paths: Vec<AssistantPathRegion> = Vec::new();
    let mut in_code_block = false;
    let mut in_verification_paragraph = false;
    let mut code_block_lang = String::new();
    let mut code_block_lines: Vec<&str> = Vec::new();
    let text_lines: Vec<&str> = text.lines().collect();

    // Pre-scan for tables and calculate column widths, clamped to fit bubble
    let table_info = scan_tables(&text_lines, bubble_width);

    let get_table_info = |idx: usize| -> Option<&(usize, usize, Vec<usize>)> {
        table_info.iter().find(|(s, e, _)| idx >= *s && idx < *e)
    };

    // Track which table regions we've already recorded (by source start line)
    let mut recorded_tables: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for (i, line) in text_lines.iter().enumerate() {
        let trimmed = line.trim();

        // Code block delimiters
        if trimmed.starts_with("```") {
            in_verification_paragraph = false;
            if !in_code_block {
                // Opening fence
                in_code_block = true;
                code_block_lang = trimmed.trim_start_matches('`').trim().to_string();
                code_block_lines.clear();
                let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
                if !code_block_lang.is_empty() {
                    spans.push(Span::styled("┌─ ", Style::default().fg(Color::DarkGray)));
                    spans.push(Span::styled(
                        code_block_lang.clone(),
                        Style::default().fg(AZURE),
                    ));
                    spans.push(Span::styled(" ─", Style::default().fg(Color::DarkGray)));
                } else {
                    spans.push(Span::styled(
                        "┌──────",
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                lines.push(Line::from(spans));
            } else {
                // Closing fence — highlight collected code and emit
                emit_code_block(
                    &mut lines,
                    &code_block_lines,
                    &code_block_lang,
                    bubble_width,
                    highlighter,
                );
                in_code_block = false;
                lines.push(Line::from(vec![
                    Span::styled("│ ", Style::default().fg(ORANGE)),
                    Span::styled("└──────", Style::default().fg(Color::DarkGray)),
                ]));
            }
            continue;
        }

        // Code block content — collect for batch highlighting
        if in_code_block {
            code_block_lines.push(line);
            continue;
        }

        // Table rows — record region on first row of each table
        if let Some((table_start, table_end, col_widths)) = get_table_info(i) {
            in_verification_paragraph = false;
            if !recorded_tables.contains(table_start) {
                recorded_tables.insert(*table_start);
                // Record output line index before rendering this table's first row
                let output_start = lines.len();
                // Render all rows of this table to get the output line range
                for j in *table_start..*table_end {
                    let row_trimmed = text_lines[j].trim();
                    render_table_row(
                        &mut lines,
                        row_trimmed,
                        j,
                        *table_start,
                        *table_end,
                        col_widths,
                        &text_lines,
                    );
                }
                let output_end = lines.len();
                // Collect raw markdown for the table
                let raw = text_lines[*table_start..*table_end].join("\n");
                table_regions.push((output_start, output_end, raw));
            }
            // Already rendered above (or by a previous iteration for this table)
            continue;
        }

        // Headers
        if trimmed.starts_with('#') {
            in_verification_paragraph = false;
            render_header(&mut lines, &mut clickable_paths, trimmed, bubble_width);
            continue;
        }

        // Bullet lists
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• ") {
            in_verification_paragraph = false;
            render_bullet(&mut lines, &mut clickable_paths, trimmed, bubble_width);
            continue;
        }

        // Numbered lists
        if trimmed
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
            && trimmed.contains(". ")
        {
            in_verification_paragraph = false;
            render_numbered(&mut lines, &mut clickable_paths, trimmed, bubble_width);
            continue;
        }

        // Blockquotes
        if trimmed.starts_with("> ") {
            in_verification_paragraph = false;
            render_quote(&mut lines, &mut clickable_paths, trimmed, bubble_width);
            continue;
        }

        let starts_verification = starts_verification_paragraph(trimmed);
        let base_style = if starts_verification || in_verification_paragraph {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::ITALIC)
        } else {
            Style::default().fg(Color::White)
        };

        // Regular paragraph text
        render_wrapped_markdown(
            &mut lines,
            &mut clickable_paths,
            line,
            bubble_width.saturating_sub(2),
            vec![Span::styled("│ ", Style::default().fg(ORANGE))],
            vec![Span::styled("│ ", Style::default().fg(ORANGE))],
            base_style,
        );

        if trimmed.is_empty() {
            in_verification_paragraph = false;
        } else if starts_verification {
            in_verification_paragraph = true;
        }
    }

    // Handle unclosed code block — emit any remaining collected lines
    if in_code_block {
        emit_code_block(
            &mut lines,
            &code_block_lines,
            &code_block_lang,
            bubble_width,
            highlighter,
        );
    }

    (lines, table_regions, clickable_paths)
}

/// Re-render a table at wider width for the popup overlay.
/// Takes raw markdown (pipe-delimited rows) and renders at the given width.
pub fn render_table_for_popup(raw_markdown: &str, width: usize) -> Vec<Line<'static>> {
    let text_lines: Vec<&str> = raw_markdown.lines().collect();
    let table_info = scan_tables(&text_lines, width);
    let mut lines = Vec::new();

    if let Some((start, end, col_widths)) = table_info.first() {
        for i in *start..*end {
            let trimmed = text_lines[i].trim();
            render_table_row(
                &mut lines,
                trimmed,
                i,
                *start,
                *end,
                col_widths,
                &text_lines,
            );
        }
    }

    lines
}

/// Emit syntax-highlighted code block lines with gutter prefixes and wrapping
fn emit_code_block(
    lines: &mut Vec<Line<'static>>,
    code_lines: &[&str],
    lang: &str,
    bubble_width: usize,
    highlighter: &mut SyntaxHighlighter,
) {
    let code_max = bubble_width.saturating_sub(4);
    if code_max == 0 {
        return;
    }

    let content = code_lines.join("\n");
    let highlighted = highlighter.highlight_code_block(&content, lang);

    for (idx, highlighted_spans) in highlighted.iter().enumerate() {
        let raw_line = code_lines.get(idx).unwrap_or(&"");

        // Check if line needs wrapping
        let char_count: usize = raw_line.chars().count();
        if char_count <= code_max {
            // Single line — use syntax-highlighted spans
            let mut spans = vec![
                Span::styled("│ ", Style::default().fg(ORANGE)),
                Span::styled("│ ", Style::default().fg(Color::DarkGray)),
            ];
            spans.extend(highlighted_spans.iter().cloned());
            lines.push(Line::from(spans));
        } else {
            // Line needs wrapping — fall back to wrap_text with the dominant color
            // (wrapping mid-span is complex; for long lines, use the first span's color)
            let fallback_color = highlighted_spans
                .first()
                .and_then(|s| s.style.fg)
                .unwrap_or(Color::Yellow);
            for wrapped in wrap_text(raw_line, code_max) {
                lines.push(Line::from(vec![
                    Span::styled("│ ", Style::default().fg(ORANGE)),
                    Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(wrapped, Style::default().fg(fallback_color)),
                ]));
            }
        }
    }
}

/// Pre-scan text to identify table ranges and calculate column widths.
/// Column widths are clamped so the total table fits within bubble_width.
/// Layout budget: 2 (orange gutter "│ ") + 1 (left border) + sum(col+2) + (ncols-1) separators + 1 (right border)
fn scan_tables(text_lines: &[&str], bubble_width: usize) -> Vec<(usize, usize, Vec<usize>)> {
    let mut table_info = Vec::new();
    let mut table_start: Option<usize> = None;
    let mut current_widths: Vec<usize> = Vec::new();

    for (idx, tl) in text_lines.iter().enumerate() {
        let t = tl.trim();
        let pipe_count = t.matches('|').count();
        let is_table_row = pipe_count >= 2;
        let is_sep = is_table_separator(t);

        if is_table_row || is_sep {
            if table_start.is_none() {
                table_start = Some(idx);
                current_widths.clear();
            }
            if !is_sep {
                let cells: Vec<&str> = t.split('|').filter(|s| !s.is_empty()).collect();
                for (col, cell) in cells.iter().enumerate() {
                    let w = cell.trim().chars().count();
                    if col >= current_widths.len() {
                        current_widths.push(w);
                    } else if w > current_widths[col] {
                        current_widths[col] = w;
                    }
                }
            }
        } else if let Some(start) = table_start {
            clamp_col_widths(&mut current_widths, bubble_width);
            table_info.push((start, idx, current_widths.clone()));
            table_start = None;
        }
    }
    if let Some(start) = table_start {
        clamp_col_widths(&mut current_widths, bubble_width);
        table_info.push((start, text_lines.len(), current_widths.clone()));
    }

    table_info
}

/// Shrink column widths proportionally so the table fits within bubble_width.
/// Each column gets at least 3 chars (enough for "a…" + padding).
fn clamp_col_widths(widths: &mut [usize], bubble_width: usize) {
    if widths.is_empty() {
        return;
    }
    // Total table width = 2 (gutter) + 1 (border) + sum(w+2) + (n-1) separators + 1 (border)
    // = 4 + sum(w+2) + (n-1) = 4 + 2*n + sum(w) + n - 1 = 3 + 3*n + sum(w)
    let n = widths.len();
    let overhead = 3 + 3 * n;
    let total: usize = widths.iter().sum();
    let table_width = overhead + total;
    if table_width <= bubble_width {
        return;
    }

    // Available space for all column content combined
    let available = bubble_width.saturating_sub(overhead);
    if available == 0 {
        for w in widths.iter_mut() {
            *w = 1;
        }
        return;
    }

    // Proportional shrink: each col gets (original_w / total) * available, min 3
    let min_w = 3usize;
    for w in widths.iter_mut() {
        let shrunk = (*w as f64 / total as f64 * available as f64).floor() as usize;
        *w = shrunk.max(min_w).min(available);
    }

    // If rounding left us over budget, trim largest columns one char at a time
    let mut sum: usize = widths.iter().sum();
    while sum > available {
        let max_idx = widths
            .iter()
            .enumerate()
            .max_by_key(|(_, w)| **w)
            .map(|(i, _)| i)
            .unwrap();
        if widths[max_idx] <= min_w {
            break;
        }
        widths[max_idx] -= 1;
        sum -= 1;
    }
}

/// Render a table row with borders
fn render_table_row(
    lines: &mut Vec<Line<'static>>,
    trimmed: &str,
    idx: usize,
    table_start: usize,
    table_end: usize,
    col_widths: &[usize],
    text_lines: &[&str],
) {
    let is_sep = is_table_separator(trimmed);
    let cells: Vec<&str> = trimmed.split('|').filter(|s| !s.is_empty()).collect();
    let is_first_row = idx == table_start;
    let is_last_row = idx == table_end - 1;
    let is_header = is_first_row
        && text_lines
            .get(idx + 1)
            .map(|l| is_table_separator(l))
            .unwrap_or(false);

    // Top border
    if is_first_row {
        let mut top = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
        top.push(Span::styled("┌", Style::default().fg(Color::DarkGray)));
        for (j, w) in col_widths.iter().enumerate() {
            top.push(Span::styled(
                "─".repeat(*w + 2),
                Style::default().fg(Color::DarkGray),
            ));
            if j < col_widths.len() - 1 {
                top.push(Span::styled("┬", Style::default().fg(Color::DarkGray)));
            }
        }
        top.push(Span::styled("┐", Style::default().fg(Color::DarkGray)));
        lines.push(Line::from(top));
    }

    if is_sep {
        let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
        spans.push(Span::styled("├", Style::default().fg(Color::DarkGray)));
        for (j, w) in col_widths.iter().enumerate() {
            spans.push(Span::styled(
                "─".repeat(*w + 2),
                Style::default().fg(Color::DarkGray),
            ));
            if j < col_widths.len() - 1 {
                spans.push(Span::styled("┼", Style::default().fg(Color::DarkGray)));
            }
        }
        spans.push(Span::styled("┤", Style::default().fg(Color::DarkGray)));
        lines.push(Line::from(spans));
    } else {
        let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
        spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
        for (j, cell) in cells.iter().enumerate() {
            let w = col_widths.get(j).copied().unwrap_or(cell.trim().len());
            // Truncate cell text with ellipsis if it exceeds column width
            let trimmed_cell = cell.trim();
            let cell_chars: usize = trimmed_cell.chars().count();
            let display_text = if cell_chars > w && w >= 2 {
                let truncated: String = trimmed_cell.chars().take(w - 1).collect();
                format!(" {}… ", truncated)
            } else {
                format!(" {:width$} ", trimmed_cell, width = w)
            };
            if is_header {
                spans.push(Span::styled(
                    display_text,
                    Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::styled(
                    display_text,
                    Style::default().fg(Color::White),
                ));
            }
            spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
        }
        lines.push(Line::from(spans));
    }

    // Bottom border
    if is_last_row {
        let mut bot = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
        bot.push(Span::styled("└", Style::default().fg(Color::DarkGray)));
        for (j, w) in col_widths.iter().enumerate() {
            bot.push(Span::styled(
                "─".repeat(*w + 2),
                Style::default().fg(Color::DarkGray),
            ));
            if j < col_widths.len() - 1 {
                bot.push(Span::styled("┴", Style::default().fg(Color::DarkGray)));
            }
        }
        bot.push(Span::styled("┘", Style::default().fg(Color::DarkGray)));
        lines.push(Line::from(bot));
    }
}

fn render_wrapped_markdown(
    lines: &mut Vec<Line<'static>>,
    clickable_paths: &mut Vec<AssistantPathRegion>,
    text: &str,
    content_width: usize,
    first_prefix: Vec<Span<'static>>,
    rest_prefix: Vec<Span<'static>>,
    base_style: Style,
) {
    let (wrapped_lines, wrapped_links) =
        wrap_markdown_segments(parse_markdown_segments(text, base_style), content_width);
    let first_prefix_width = prefix_width(&first_prefix);
    let rest_prefix_width = prefix_width(&rest_prefix);

    for (idx, spans) in wrapped_lines.into_iter().enumerate() {
        let line_idx = lines.len();
        let (prefix, prefix_width) = if idx == 0 {
            (first_prefix.clone(), first_prefix_width)
        } else {
            (rest_prefix.clone(), rest_prefix_width)
        };
        let mut line_spans = prefix;
        line_spans.extend(spans);
        lines.push(Line::from(line_spans));

        for (link_start, link_end, target) in wrapped_links
            .iter()
            .filter(|(line, _, _, _)| *line == idx)
            .map(|(_, start, end, target)| (*start, *end, target.clone()))
        {
            clickable_paths.push((
                line_idx,
                prefix_width + link_start,
                prefix_width + link_end,
                target,
            ));
        }
    }
}

fn wrap_markdown_segments(
    segments: Vec<super::markdown::MarkdownSegment>,
    max_width: usize,
) -> (Vec<Vec<Span<'static>>>, Vec<(usize, usize, usize, String)>) {
    if segments.is_empty() {
        return (vec![vec![Span::styled("", Style::default())]], Vec::new());
    }

    let mut full_text = String::new();
    let mut ranges: Vec<(usize, usize, Style, Option<String>, String)> = Vec::new();

    for segment in segments {
        let start = full_text.chars().count();
        full_text.push_str(&segment.text);
        let end = full_text.chars().count();
        ranges.push((start, end, segment.style, segment.file_target, segment.text));
    }

    if full_text.is_empty() {
        return (vec![vec![Span::styled("", Style::default())]], Vec::new());
    }

    let wrapped_text: Vec<String> = if max_width == 0 {
        vec![full_text.clone()]
    } else {
        textwrap::wrap(&full_text, Options::new(max_width).break_words(true))
            .into_iter()
            .map(|cow| cow.into_owned())
            .collect()
    };

    let full_chars: Vec<char> = full_text.chars().collect();
    let mut wrapped_lines = Vec::with_capacity(wrapped_text.len());
    let mut clickable_paths = Vec::new();
    let mut char_offset = 0usize;

    for (line_idx, wrapped) in wrapped_text.into_iter().enumerate() {
        let line_len = wrapped.chars().count();
        let line_start = char_offset;
        let line_end = line_start + line_len;
        let mut line_spans = Vec::new();

        for (seg_start, seg_end, style, target, text) in &ranges {
            if *seg_end <= line_start || *seg_start >= line_end {
                continue;
            }

            let overlap_start = (*seg_start).max(line_start);
            let overlap_end = (*seg_end).min(line_end);
            let local_start = overlap_start.saturating_sub(*seg_start);
            let local_len = overlap_end.saturating_sub(overlap_start);
            let chunk: String = text.chars().skip(local_start).take(local_len).collect();
            if chunk.is_empty() {
                continue;
            }

            if let Some(target) = target {
                clickable_paths.push((
                    line_idx,
                    overlap_start - line_start,
                    overlap_end - line_start,
                    target.clone(),
                ));
            }

            line_spans.push(Span::styled(chunk, *style));
        }

        if line_spans.is_empty() {
            line_spans.push(Span::styled("", Style::default()));
        }
        wrapped_lines.push(line_spans);
        char_offset = line_end;
        if full_chars.get(char_offset) == Some(&' ') {
            char_offset += 1;
        }
    }

    if wrapped_lines.is_empty() {
        wrapped_lines.push(vec![Span::styled("", Style::default())]);
    }

    (wrapped_lines, clickable_paths)
}

fn prefix_width(spans: &[Span<'static>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
}

fn render_header(
    lines: &mut Vec<Line<'static>>,
    clickable_paths: &mut Vec<AssistantPathRegion>,
    trimmed: &str,
    bubble_width: usize,
) {
    let header_level = trimmed.chars().take_while(|&c| c == '#').count();
    let header_text = trimmed.trim_start_matches('#').trim();
    let (prefix, style) = match header_level {
        1 => (
            "█ ",
            Style::default()
                .fg(AZURE)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ),
        2 => (
            "▓ ",
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
        ),
        3 => (
            "▒ ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        _ => ("░ ", Style::default().fg(Color::Green)),
    };
    render_wrapped_markdown(
        lines,
        clickable_paths,
        header_text,
        bubble_width.saturating_sub(4),
        vec![
            Span::styled("│ ", Style::default().fg(ORANGE)),
            Span::styled(prefix, style),
        ],
        vec![
            Span::styled("│ ", Style::default().fg(ORANGE)),
            Span::styled("  ", Style::default()),
        ],
        style,
    );
}

fn render_bullet(
    lines: &mut Vec<Line<'static>>,
    clickable_paths: &mut Vec<AssistantPathRegion>,
    trimmed: &str,
    bubble_width: usize,
) {
    let bullet_content = trimmed
        .trim_start_matches("- ")
        .trim_start_matches("* ")
        .trim_start_matches("• ");
    render_wrapped_markdown(
        lines,
        clickable_paths,
        bullet_content,
        bubble_width.saturating_sub(6),
        vec![
            Span::styled("│ ", Style::default().fg(ORANGE)),
            Span::styled("  • ", Style::default().fg(AZURE)),
        ],
        vec![
            Span::styled("│ ", Style::default().fg(ORANGE)),
            Span::styled("    ", Style::default()),
        ],
        Style::default().fg(Color::White),
    );
}

fn render_numbered(
    lines: &mut Vec<Line<'static>>,
    clickable_paths: &mut Vec<AssistantPathRegion>,
    trimmed: &str,
    bubble_width: usize,
) {
    let num_end = trimmed.find(". ").unwrap_or(0);
    let num = &trimmed[..num_end];
    let content = &trimmed[num_end + 2..];
    let num_prefix = format!("  {}. ", num);
    render_wrapped_markdown(
        lines,
        clickable_paths,
        content,
        bubble_width.saturating_sub(2 + num_prefix.len()),
        vec![
            Span::styled("│ ", Style::default().fg(ORANGE)),
            Span::styled(num_prefix.clone(), Style::default().fg(AZURE)),
        ],
        vec![
            Span::styled("│ ", Style::default().fg(ORANGE)),
            Span::styled(" ".repeat(num_prefix.len()), Style::default()),
        ],
        Style::default().fg(Color::White),
    );
}

fn render_quote(
    lines: &mut Vec<Line<'static>>,
    clickable_paths: &mut Vec<AssistantPathRegion>,
    trimmed: &str,
    bubble_width: usize,
) {
    let quote_content = trimmed.trim_start_matches("> ");
    render_wrapped_markdown(
        lines,
        clickable_paths,
        quote_content,
        bubble_width.saturating_sub(4),
        vec![
            Span::styled("│ ", Style::default().fg(ORANGE)),
            Span::styled("┃ ", Style::default().fg(Color::DarkGray)),
        ],
        vec![
            Span::styled("│ ", Style::default().fg(ORANGE)),
            Span::styled("┃ ", Style::default().fg(Color::DarkGray)),
        ],
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    );
}

/// Render markdown for the viewer pane (no session gutter prefix, full-width).
/// Reuses the assistant text renderer then strips the orange `│ ` prefix from
/// each line, giving a clean markdown reading experience.
pub fn render_markdown_for_viewer(
    text: &str,
    width: usize,
    highlighter: &mut SyntaxHighlighter,
) -> Vec<Line<'static>> {
    // +2 compensates for the gutter we strip — keeps content wrapping at `width`
    let (mut lines, _) = render_assistant_text(text, width + 2, highlighter);
    for line in &mut lines {
        if !line.spans.is_empty() && line.spans[0].content.as_ref() == "│ " {
            line.spans.remove(0);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hl() -> SyntaxHighlighter {
        SyntaxHighlighter::new()
    }

    // ═══════════════════════════════════════════════════════════════════
    // clamp_col_widths
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn clamp_empty_widths() {
        let mut w: Vec<usize> = vec![];
        clamp_col_widths(&mut w, 80);
        assert!(w.is_empty());
    }

    #[test]
    fn clamp_single_col_fits() {
        let mut w = vec![10];
        clamp_col_widths(&mut w, 80);
        assert_eq!(w, vec![10]);
    }

    #[test]
    fn clamp_two_cols_fit() {
        let mut w = vec![5, 5];
        // overhead = 3 + 3*2 = 9, total content = 10, table = 19 — fits in 80
        clamp_col_widths(&mut w, 80);
        assert_eq!(w, vec![5, 5]);
    }

    #[test]
    fn clamp_two_cols_too_wide() {
        let mut w = vec![50, 50];
        // overhead = 3 + 3*2 = 9, total content = 100, table = 109 > 20
        clamp_col_widths(&mut w, 20);
        let total: usize = w.iter().sum();
        // Available = 20 - 9 = 11, so total should be <= 11
        assert!(total <= 11);
    }

    #[test]
    fn clamp_zero_available() {
        let mut w = vec![10, 10];
        // overhead = 9, bubble_width = 5 means available = 0
        clamp_col_widths(&mut w, 5);
        assert_eq!(w, vec![1, 1]);
    }

    #[test]
    fn clamp_preserves_minimum_width() {
        let mut w = vec![100, 100, 100];
        clamp_col_widths(&mut w, 20);
        for col in &w {
            assert!(*col >= 1, "column width must be at least 1");
        }
    }

    #[test]
    fn clamp_proportional_shrink() {
        let mut w = vec![20, 10];
        // overhead = 3 + 3*2 = 9, total = 30, table = 39 > 25
        clamp_col_widths(&mut w, 25);
        // Bigger column should still be bigger after shrink
        assert!(w[0] >= w[1]);
    }

    #[test]
    fn clamp_single_col_oversized() {
        let mut w = vec![200];
        // overhead = 3 + 3*1 = 6, available = 20-6 = 14
        clamp_col_widths(&mut w, 20);
        assert!(w[0] <= 14);
    }

    #[test]
    fn clamp_exact_fit_no_change() {
        let mut w = vec![5, 5];
        // overhead = 9, total = 10, table = 19
        let original = w.clone();
        clamp_col_widths(&mut w, 19);
        assert_eq!(w, original);
    }

    #[test]
    fn clamp_many_columns() {
        let mut w = vec![10; 10];
        // overhead = 3 + 3*10 = 33, content = 100, table = 133 > 60
        // available = 27. Each col proportional: floor(10/100*27)=2, min 3.
        // 10*3 = 30 > 27 but while loop stops at min_w=3. Total capped at 30.
        clamp_col_widths(&mut w, 60);
        let total: usize = w.iter().sum();
        // Each column should be at minimum width (3)
        for col in &w {
            assert!(*col >= 3, "column should be at least min_w=3, got {}", col);
        }
        assert_eq!(total, 30);
    }

    // ═══════════════════════════════════════════════════════════════════
    // scan_tables
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn scan_tables_no_tables() {
        let lines = vec!["hello", "world"];
        let result = scan_tables(&lines, 80);
        assert!(result.is_empty());
    }

    #[test]
    fn scan_tables_simple_table() {
        let lines = vec!["| A | B |", "|---|---|", "| 1 | 2 |"];
        let result = scan_tables(&lines, 80);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, 0); // start
        assert_eq!(result[0].1, 3); // end
    }

    #[test]
    fn scan_tables_col_widths_track_max() {
        let lines = vec![
            "| Short | LongerColumn |",
            "|-------|--------------|",
            "| X | Y |",
        ];
        let result = scan_tables(&lines, 200);
        assert_eq!(result.len(), 1);
        // "LongerColumn" = 12 chars, which is the max in column 2
        assert!(result[0].2[1] >= 12);
    }

    #[test]
    fn scan_tables_multiple_tables() {
        let lines = vec![
            "| A | B |",
            "|---|---|",
            "| 1 | 2 |",
            "not a table",
            "| C | D |",
            "|---|---|",
            "| 3 | 4 |",
        ];
        let result = scan_tables(&lines, 80);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn scan_tables_table_at_end() {
        let lines = vec!["hello", "| A | B |", "|---|---|"];
        let result = scan_tables(&lines, 80);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, 1);
        assert_eq!(result[0].1, 3);
    }

    #[test]
    fn scan_tables_separator_only() {
        let lines = vec!["|---|---|"];
        let result = scan_tables(&lines, 80);
        assert_eq!(result.len(), 1);
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_assistant_text — structure tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_empty_text() {
        let (lines, _table_regions) = render_assistant_text("", 80, &mut hl());
        // Empty text produces no output or just an empty line
        assert!(lines.len() <= 1);
    }

    #[test]
    fn render_plain_paragraph() {
        let (lines, _table_regions) = render_assistant_text("hello world", 80, &mut hl());
        assert!(!lines.is_empty());
        // First span should be the orange gutter
        let first_span = &lines[0].spans[0];
        assert_eq!(first_span.content.as_ref(), "│ ");
    }

    #[test]
    fn render_assistant_text_exposes_clickable_file_links() {
        let text = "Open [render_tools.rs](/Users/test/render_tools.rs#L42)";
        let (lines, _tables, paths) = render_assistant_text_with_paths(text, 80, &mut hl());
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].0, 0);
        assert_eq!(paths[0].3, "/Users/test/render_tools.rs#L42");
        let link_text: String = lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(link_text.contains("render_tools.rs"));
    }

    #[test]
    fn render_assistant_text_wraps_file_links_into_multiple_hitboxes() {
        let text =
            "[very_long_file_name.rs](/Users/test/very_long_file_name.rs#L142) trailing text";
        let (_lines, _tables, paths) = render_assistant_text_with_paths(text, 18, &mut hl());
        assert!(
            paths.len() >= 2,
            "wrapped link should produce multiple hitboxes"
        );
        assert!(paths.iter().all(|(_, start, end, _)| end > start));
        assert!(paths
            .iter()
            .all(|(_, _, _, path)| path == "/Users/test/very_long_file_name.rs#L142"));
    }

    #[test]
    fn render_verification_paragraph_in_italics() {
        let (lines, _table_regions) =
            render_assistant_text("Verification: elapsed time recorded.", 80, &mut hl());
        assert!(lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .any(|span| span.content.contains("Verification:")
                && span.style.add_modifier.contains(Modifier::ITALIC)));
    }

    #[test]
    fn render_multiline_verification_paragraph_in_italics() {
        let text = "Verification: first line.\nSecond verification line.";
        let (lines, _table_regions) = render_assistant_text(text, 80, &mut hl());
        assert!(lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .filter(|span| {
                span.content.contains("Verification:")
                    || span.content.contains("Second verification line.")
            })
            .all(|span| span.style.add_modifier.contains(Modifier::ITALIC)));
    }

    #[test]
    fn render_emphasized_verification_prefix_keeps_following_line_italic() {
        let text = "*Verification:* first line.\nSecond verification line.";
        let (lines, _table_regions) = render_assistant_text(text, 80, &mut hl());
        assert!(lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .filter(|span| span.content.contains("Second verification line."))
            .all(|span| span.style.add_modifier.contains(Modifier::ITALIC)));
    }

    #[test]
    fn render_verification_paragraph_keeps_inline_code_italic() {
        let text = "Verification: `cargo check` passed.";
        let (lines, _table_regions) = render_assistant_text(text, 80, &mut hl());
        let code_span = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content.contains("cargo check"))
            .expect("inline code span");
        assert!(code_span.style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn render_verification_paragraph_keeps_file_link_italic() {
        let text = "Verification: see [app.rs](/Users/test/app.rs#L42).";
        let (lines, _table_regions, _paths) = render_assistant_text_with_paths(text, 80, &mut hl());
        let link_span = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content.contains("app.rs"))
            .expect("file link span");
        assert!(link_span.style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn render_verification_paragraph_stops_after_blank_line() {
        let text = "Verification: first line.\nSecond verification line.\n\nNormal paragraph.";
        let (lines, _table_regions) = render_assistant_text(text, 80, &mut hl());
        let normal_span = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content.contains("Normal paragraph."))
            .expect("normal paragraph span");
        assert!(!normal_span.style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn render_code_block() {
        let text = "```rust\nlet x = 1;\n```";
        let (lines, _table_regions) = render_assistant_text(text, 80, &mut hl());
        // Should have: open delimiter + code line + close delimiter = 3 lines
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn render_code_block_no_language() {
        let text = "```\ncode\n```";
        let (lines, _table_regions) = render_assistant_text(text, 80, &mut hl());
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn render_header_h1() {
        let (lines, _table_regions) = render_assistant_text("# Title", 80, &mut hl());
        assert!(!lines.is_empty());
    }

    #[test]
    fn render_header_h2() {
        let (lines, _table_regions) = render_assistant_text("## Subtitle", 80, &mut hl());
        assert!(!lines.is_empty());
    }

    #[test]
    fn render_header_h3() {
        let (lines, _table_regions) = render_assistant_text("### Section", 80, &mut hl());
        assert!(!lines.is_empty());
    }

    #[test]
    fn render_header_h4() {
        let (lines, _table_regions) = render_assistant_text("#### Deep", 80, &mut hl());
        assert!(!lines.is_empty());
    }

    #[test]
    fn render_bullet_dash() {
        let (lines, _table_regions) = render_assistant_text("- item one", 80, &mut hl());
        assert!(!lines.is_empty());
    }

    #[test]
    fn render_bullet_asterisk() {
        let (lines, _table_regions) = render_assistant_text("* item two", 80, &mut hl());
        assert!(!lines.is_empty());
    }

    #[test]
    fn render_bullet_unicode() {
        let (lines, _table_regions) = render_assistant_text("• item three", 80, &mut hl());
        assert!(!lines.is_empty());
    }

    #[test]
    fn render_numbered_list() {
        let (lines, _table_regions) = render_assistant_text("1. first\n2. second", 80, &mut hl());
        assert!(lines.len() >= 2);
    }

    #[test]
    fn render_blockquote() {
        let (lines, _table_regions) = render_assistant_text("> quoted text", 80, &mut hl());
        assert!(!lines.is_empty());
    }

    #[test]
    fn render_mixed_content() {
        let text = "# Title\n\nParagraph\n\n- bullet\n\n```\ncode\n```\n\n> quote";
        let (lines, _table_regions) = render_assistant_text(text, 80, &mut hl());
        assert!(lines.len() >= 5);
    }

    #[test]
    fn render_wraps_long_lines() {
        let long = "a ".repeat(100);
        let (lines, _table_regions) = render_assistant_text(&long, 40, &mut hl());
        assert!(lines.len() > 1);
    }

    #[test]
    fn render_table_basic() {
        let text = "| Col1 | Col2 |\n|------|------|\n| a | b |";
        let (lines, _table_regions) = render_assistant_text(text, 80, &mut hl());
        // Table rendering adds borders: top, header, separator, data, bottom
        assert!(lines.len() >= 4);
    }

    #[test]
    fn render_narrow_width() {
        let (lines, _table_regions) = render_assistant_text("hello world", 10, &mut hl());
        assert!(!lines.is_empty());
    }

    #[test]
    fn render_code_block_wraps() {
        let text = format!("```\n{}\n```", "x".repeat(200));
        let (lines, _table_regions) = render_assistant_text(&text, 40, &mut hl());
        // Code should wrap within the block
        assert!(lines.len() > 3);
    }

    #[test]
    fn render_multiple_paragraphs() {
        let text = "para one\n\npara two\n\npara three";
        let (lines, _table_regions) = render_assistant_text(text, 80, &mut hl());
        assert!(lines.len() >= 3);
    }

    #[test]
    fn render_unicode_content() {
        let (lines, _table_regions) = render_assistant_text("日本語テスト", 80, &mut hl());
        assert!(!lines.is_empty());
    }

    #[test]
    fn render_emoji_content() {
        let (lines, _table_regions) = render_assistant_text("🎉 celebration 🎊", 80, &mut hl());
        assert!(!lines.is_empty());
    }

    #[test]
    fn render_header_wraps_long() {
        let title = format!("# {}", "W".repeat(200));
        let (lines, _table_regions) = render_assistant_text(&title, 40, &mut hl());
        assert!(lines.len() > 1);
    }

    #[test]
    fn render_numbered_double_digit() {
        let (lines, _table_regions) = render_assistant_text("12. twelfth item", 80, &mut hl());
        assert!(!lines.is_empty());
    }

    #[test]
    fn render_quote_wraps() {
        let long_quote = format!("> {}", "word ".repeat(50));
        let (lines, _table_regions) = render_assistant_text(&long_quote, 40, &mut hl());
        assert!(lines.len() > 1);
    }

    #[test]
    fn render_bullet_wraps() {
        let long_bullet = format!("- {}", "text ".repeat(50));
        let (lines, _table_regions) = render_assistant_text(&long_bullet, 40, &mut hl());
        assert!(lines.len() > 1);
    }

    #[test]
    fn render_width_1_no_panic() {
        // Extremely narrow — should not panic
        let (lines, _table_regions) = render_assistant_text("hello", 1, &mut hl());
        let _ = lines;
    }

    #[test]
    fn render_width_0_no_panic() {
        let (lines, _table_regions) = render_assistant_text("hello", 0, &mut hl());
        let _ = lines;
    }

    #[test]
    fn render_unclosed_code_block() {
        let text = "```\ncode without closing";
        let (lines, _table_regions) = render_assistant_text(text, 80, &mut hl());
        assert!(!lines.is_empty());
    }

    #[test]
    fn render_multiple_code_blocks() {
        let text = "```\nfirst\n```\n\n```python\nsecond\n```";
        let (lines, _table_regions) = render_assistant_text(text, 80, &mut hl());
        assert!(lines.len() >= 6);
    }

    #[test]
    fn render_table_clamped_columns() {
        let text =
            "| Very Long Column Name Here | Another Very Long Column |\n|---|---|\n| x | y |";
        let (lines, _table_regions) = render_assistant_text(text, 30, &mut hl());
        assert!(!lines.is_empty());
    }

    #[test]
    fn clamp_one_col_exact_fit() {
        let mut w = vec![10];
        // overhead = 3 + 3*1 = 6, total = 10, table = 16 <= 16
        clamp_col_widths(&mut w, 16);
        assert_eq!(w, vec![10]);
    }

    #[test]
    fn scan_tables_header_only_one_line() {
        let lines = vec!["| A | B |"];
        let result = scan_tables(&lines, 80);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn render_single_newline() {
        let (lines, _table_regions) = render_assistant_text("\n", 80, &mut hl());
        // Should produce at least one line (empty paragraph)
        assert!(!lines.is_empty());
    }

    #[test]
    fn render_only_whitespace() {
        let (lines, _table_regions) = render_assistant_text("   ", 80, &mut hl());
        assert!(!lines.is_empty());
    }

    // ═══════════════════════════════════════════════════════════════════
    // Code block syntax highlighting
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn code_block_rust_has_colored_spans() {
        let text = "```rust\nfn main() {\n    let x = 42;\n}\n```";
        let (lines, _table_regions) = render_assistant_text(text, 80, &mut hl());
        // Line 0 = opening fence, lines 1-3 = code, line 4 = closing fence
        assert_eq!(lines.len(), 5);
        // Code lines should have >2 spans (gutter + code gutter + highlighted spans)
        let code_line = &lines[1]; // "fn main() {"
        assert!(
            code_line.spans.len() > 2,
            "highlighted code should have multiple spans, got {}",
            code_line.spans.len()
        );
        // Should have magenta for `fn` keyword
        let has_magenta = code_line
            .spans
            .iter()
            .any(|s| s.style.fg == Some(Color::Magenta));
        assert!(has_magenta, "Rust `fn` keyword should be Magenta");
    }

    #[test]
    fn code_block_python_has_colored_spans() {
        let text = "```python\ndef hello():\n    print('hi')\n```";
        let (lines, _table_regions) = render_assistant_text(text, 80, &mut hl());
        assert_eq!(lines.len(), 4);
        // "def" keyword should be magenta
        let code_line = &lines[1];
        let has_magenta = code_line
            .spans
            .iter()
            .any(|s| s.style.fg == Some(Color::Magenta));
        assert!(has_magenta, "Python `def` keyword should be Magenta");
    }

    #[test]
    fn code_block_no_lang_still_renders() {
        let text = "```\nplain code\n```";
        let (lines, _table_regions) = render_assistant_text(text, 80, &mut hl());
        assert_eq!(lines.len(), 3);
        // Should have spans (gutter + code gutter + text)
        assert!(lines[1].spans.len() >= 3);
    }

    #[test]
    fn code_block_unknown_lang_falls_back() {
        let text = "```unknownlang\nsome code\n```";
        let (lines, _table_regions) = render_assistant_text(text, 80, &mut hl());
        assert_eq!(lines.len(), 3);
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_markdown_for_viewer
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn viewer_no_gutter_prefix() {
        let lines = render_markdown_for_viewer("hello world", 80, &mut hl());
        assert!(!lines.is_empty());
        // No line should start with the orange "│ " gutter
        for line in &lines {
            if let Some(first) = line.spans.first() {
                assert_ne!(
                    first.content.as_ref(),
                    "│ ",
                    "viewer lines must not have session gutter"
                );
            }
        }
    }

    #[test]
    fn viewer_header_rendered() {
        let lines = render_markdown_for_viewer("# Title", 80, &mut hl());
        assert!(!lines.is_empty());
        // Should contain the block prefix for h1
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("█"), "h1 should have block prefix");
        assert!(text.contains("Title"));
    }

    #[test]
    fn viewer_bullet_rendered() {
        let lines = render_markdown_for_viewer("- item one", 80, &mut hl());
        assert!(!lines.is_empty());
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("•"), "bullet should have dot prefix");
    }

    #[test]
    fn viewer_code_block_highlighted() {
        let text = "```rust\nfn main() {}\n```";
        let lines = render_markdown_for_viewer(text, 80, &mut hl());
        assert!(lines.len() >= 3);
        // Code line should have syntax highlighting (magenta for `fn`)
        let code_line = &lines[1];
        let has_magenta = code_line
            .spans
            .iter()
            .any(|s| s.style.fg == Some(Color::Magenta));
        assert!(has_magenta, "code block should have syntax highlighting");
    }

    #[test]
    fn viewer_empty_text() {
        let lines = render_markdown_for_viewer("", 80, &mut hl());
        assert!(lines.len() <= 1);
    }

    #[test]
    fn viewer_width_respected() {
        let long = "word ".repeat(50);
        let lines = render_markdown_for_viewer(&long, 40, &mut hl());
        assert!(lines.len() > 1, "long text should wrap");
    }

    #[test]
    fn viewer_blockquote_rendered() {
        let lines = render_markdown_for_viewer("> quoted text", 80, &mut hl());
        assert!(!lines.is_empty());
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("┃"), "blockquote should have pipe prefix");
    }

    #[test]
    fn viewer_numbered_list() {
        let lines = render_markdown_for_viewer("1. first\n2. second", 80, &mut hl());
        assert!(lines.len() >= 2);
    }

    #[test]
    fn viewer_mixed_content() {
        let md = "# Title\n\nParagraph text.\n\n- bullet\n\n```\ncode\n```\n\n> quote";
        let lines = render_markdown_for_viewer(md, 80, &mut hl());
        assert!(lines.len() >= 5);
        // No ORANGE gutter on any line (gray code gutters are fine)
        let orange_gutter = Style::default().fg(ORANGE);
        for line in &lines {
            if let Some(first) = line.spans.first() {
                if first.content.as_ref() == "│ " {
                    assert_ne!(
                        first.style, orange_gutter,
                        "viewer lines must not have orange session gutter"
                    );
                }
            }
        }
    }

    #[test]
    fn viewer_table_no_gutter() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let lines = render_markdown_for_viewer(md, 80, &mut hl());
        assert!(!lines.is_empty());
        for line in &lines {
            if let Some(first) = line.spans.first() {
                assert_ne!(first.content.as_ref(), "│ ");
            }
        }
    }
}
