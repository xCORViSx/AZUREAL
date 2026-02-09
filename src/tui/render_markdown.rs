//! Markdown rendering for assistant messages
//!
//! Renders markdown text with code blocks, tables, headers, lists, and quotes.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::colorize::ORANGE;
use super::util::AZURE;
use super::markdown::{parse_markdown_spans, is_table_separator};
use super::render_wrap::wrap_text;

/// Render assistant markdown text into lines
pub fn render_assistant_text(text: &str, bubble_width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code_block = false;
    let text_lines: Vec<&str> = text.lines().collect();

    // Pre-scan for tables and calculate column widths, clamped to fit bubble
    let table_info = scan_tables(&text_lines, bubble_width);

    let get_table_info = |idx: usize| -> Option<&(usize, usize, Vec<usize>)> {
        table_info.iter().find(|(s, e, _)| idx >= *s && idx < *e)
    };

    for (i, line) in text_lines.iter().enumerate() {
        let trimmed = line.trim();

        // Code block delimiters
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            let lang = trimmed.trim_start_matches('`').trim();
            let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
            if in_code_block && !lang.is_empty() {
                spans.push(Span::styled("┌─ ", Style::default().fg(Color::DarkGray)));
                spans.push(Span::styled(lang.to_string(), Style::default().fg(AZURE)));
                spans.push(Span::styled(" ─", Style::default().fg(Color::DarkGray)));
            } else if !in_code_block {
                spans.push(Span::styled("└──────", Style::default().fg(Color::DarkGray)));
            } else {
                spans.push(Span::styled("┌──────", Style::default().fg(Color::DarkGray)));
            }
            lines.push(Line::from(spans));
            continue;
        }

        // Code block content
        if in_code_block {
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

        // Table rows
        if let Some((table_start, table_end, col_widths)) = get_table_info(i) {
            render_table_row(&mut lines, trimmed, i, *table_start, *table_end, col_widths, &text_lines);
            continue;
        }

        // Headers
        if trimmed.starts_with('#') {
            render_header(&mut lines, trimmed, bubble_width);
            continue;
        }

        // Bullet lists
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• ") {
            render_bullet(&mut lines, trimmed, bubble_width);
            continue;
        }

        // Numbered lists
        if trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) && trimmed.contains(". ") {
            render_numbered(&mut lines, trimmed, bubble_width);
            continue;
        }

        // Blockquotes
        if trimmed.starts_with("> ") {
            render_quote(&mut lines, trimmed, bubble_width);
            continue;
        }

        // Regular paragraph text
        let content_width = bubble_width.saturating_sub(2);
        for wrapped in wrap_text(line, content_width) {
            let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
            spans.extend(parse_markdown_spans(&wrapped, Style::default().fg(Color::White)));
            lines.push(Line::from(spans));
        }
    }

    lines
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
    if widths.is_empty() { return; }
    // Total table width = 2 (gutter) + 1 (border) + sum(w+2) + (n-1) separators + 1 (border)
    // = 4 + sum(w+2) + (n-1) = 4 + 2*n + sum(w) + n - 1 = 3 + 3*n + sum(w)
    let n = widths.len();
    let overhead = 3 + 3 * n;
    let total: usize = widths.iter().sum();
    let table_width = overhead + total;
    if table_width <= bubble_width { return; }

    // Available space for all column content combined
    let available = bubble_width.saturating_sub(overhead);
    if available == 0 {
        for w in widths.iter_mut() { *w = 1; }
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
        let max_idx = widths.iter().enumerate().max_by_key(|(_, w)| **w).map(|(i, _)| i).unwrap();
        if widths[max_idx] <= min_w { break; }
        widths[max_idx] -= 1;
        sum -= 1;
    }
}

/// Render a table row with borders
fn render_table_row(lines: &mut Vec<Line<'static>>, trimmed: &str, idx: usize, table_start: usize, table_end: usize, col_widths: &[usize], text_lines: &[&str]) {
    let is_sep = is_table_separator(trimmed);
    let cells: Vec<&str> = trimmed.split('|').filter(|s| !s.is_empty()).collect();
    let is_first_row = idx == table_start;
    let is_last_row = idx == table_end - 1;
    let is_header = is_first_row && text_lines.get(idx + 1).map(|l| is_table_separator(l)).unwrap_or(false);

    // Top border
    if is_first_row {
        let mut top = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
        top.push(Span::styled("┌", Style::default().fg(Color::DarkGray)));
        for (j, w) in col_widths.iter().enumerate() {
            top.push(Span::styled("─".repeat(*w + 2), Style::default().fg(Color::DarkGray)));
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
            spans.push(Span::styled("─".repeat(*w + 2), Style::default().fg(Color::DarkGray)));
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
                spans.push(Span::styled(display_text, Style::default().fg(AZURE).add_modifier(Modifier::BOLD)));
            } else {
                spans.push(Span::styled(display_text, Style::default().fg(Color::White)));
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
            bot.push(Span::styled("─".repeat(*w + 2), Style::default().fg(Color::DarkGray)));
            if j < col_widths.len() - 1 {
                bot.push(Span::styled("┴", Style::default().fg(Color::DarkGray)));
            }
        }
        bot.push(Span::styled("┘", Style::default().fg(Color::DarkGray)));
        lines.push(Line::from(bot));
    }
}

fn render_header(lines: &mut Vec<Line<'static>>, trimmed: &str, bubble_width: usize) {
    let header_level = trimmed.chars().take_while(|&c| c == '#').count();
    let header_text = trimmed.trim_start_matches('#').trim();
    let (prefix, style) = match header_level {
        1 => ("█ ", Style::default().fg(AZURE).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)),
        2 => ("▓ ", Style::default().fg(AZURE).add_modifier(Modifier::BOLD)),
        3 => ("▒ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        _ => ("░ ", Style::default().fg(Color::Green)),
    };
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
                Span::styled("  ", Style::default()),
                Span::styled(wrapped, style),
            ]));
        }
    }
}

fn render_bullet(lines: &mut Vec<Line<'static>>, trimmed: &str, bubble_width: usize) {
    let bullet_content = trimmed.trim_start_matches("- ").trim_start_matches("* ").trim_start_matches("• ");
    let bullet_max = bubble_width.saturating_sub(6);
    for (i, wrapped) in wrap_text(bullet_content, bullet_max).into_iter().enumerate() {
        let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
        if i == 0 {
            spans.push(Span::styled("  • ", Style::default().fg(AZURE)));
        } else {
            spans.push(Span::styled("    ", Style::default()));
        }
        spans.extend(parse_markdown_spans(&wrapped, Style::default().fg(Color::White)));
        lines.push(Line::from(spans));
    }
}

fn render_numbered(lines: &mut Vec<Line<'static>>, trimmed: &str, bubble_width: usize) {
    let num_end = trimmed.find(". ").unwrap_or(0);
    let num = &trimmed[..num_end];
    let content = &trimmed[num_end + 2..];
    let num_prefix = format!("  {}. ", num);
    let num_max = bubble_width.saturating_sub(2 + num_prefix.len());
    for (i, wrapped) in wrap_text(content, num_max).into_iter().enumerate() {
        let mut spans = vec![Span::styled("│ ", Style::default().fg(ORANGE))];
        if i == 0 {
            spans.push(Span::styled(num_prefix.clone(), Style::default().fg(AZURE)));
        } else {
            spans.push(Span::styled(" ".repeat(num_prefix.len()), Style::default()));
        }
        spans.extend(parse_markdown_spans(&wrapped, Style::default().fg(Color::White)));
        lines.push(Line::from(spans));
    }
}

fn render_quote(lines: &mut Vec<Line<'static>>, trimmed: &str, bubble_width: usize) {
    let quote_content = trimmed.trim_start_matches("> ");
    let quote_max = bubble_width.saturating_sub(4);
    for wrapped in wrap_text(quote_content, quote_max) {
        let mut spans = vec![
            Span::styled("│ ", Style::default().fg(ORANGE)),
            Span::styled("┃ ", Style::default().fg(Color::DarkGray)),
        ];
        spans.extend(parse_markdown_spans(&wrapped, Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)));
        lines.push(Line::from(spans));
    }
}
