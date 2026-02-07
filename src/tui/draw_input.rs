//! Input field rendering
//!
//! Supports multi-line input via Shift+Enter. Newlines split the text into
//! separate `Line`s for ratatui. Cursor positioning accounts for both
//! newlines and word-wrapping within each line. When content exceeds the
//! visible area, the view scrolls to keep the cursor visible.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, Focus};
use super::keybindings::{prompt_type_title, prompt_command_title};

/// Draw the Claude prompt input field with text wrapping and optional selection highlighting
pub fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let (border_color, title) = if app.prompt_mode {
        (Color::Yellow, prompt_type_title())
    } else {
        (Color::Red, prompt_command_title())
    };

    let is_focused = app.focus == Focus::Input;
    let inner_width = area.width.saturating_sub(2) as usize;
    // Visible rows inside the border (total height minus top+bottom border)
    let visible_rows = area.height.saturating_sub(2) as usize;

    // Build styled content with selection highlighting, split on newlines
    let content = build_input_content(app);

    // Figure out which visual row the cursor sits on (accounting for wrapping)
    // so we can scroll the Paragraph to keep the cursor in view
    let cursor_row = if inner_width > 0 {
        compute_cursor_row(&app.input, app.input_cursor, inner_width)
    } else {
        0
    };

    // Scroll offset: keep cursor visible within the box
    let scroll_offset = if visible_rows > 0 && cursor_row >= visible_rows {
        (cursor_row - visible_rows + 1) as u16
    } else {
        0
    };

    let input = Paragraph::new(content)
        .wrap(Wrap { trim: false })
        .scroll((scroll_offset, 0))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
                .title(if is_focused {
                    Span::styled(title, Style::default().fg(border_color).add_modifier(Modifier::BOLD))
                } else {
                    Span::styled(title, Style::default().fg(Color::White))
                })
                .border_style(if is_focused {
                    Style::default().fg(border_color).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                }),
        );

    f.render_widget(input, area);

    // Show cursor only in prompt mode when focused
    if app.prompt_mode && is_focused && inner_width > 0 {
        let visual_col = compute_cursor_col(&app.input, app.input_cursor, inner_width);
        // Adjust row for scroll offset so cursor renders in the visible portion
        let adjusted_row = cursor_row as u16 - scroll_offset;

        f.set_cursor_position((
            area.x + 1 + visual_col as u16,
            area.y + 1 + adjusted_row,
        ));
    }
}

/// Compute the visual row the cursor is on, accounting for newlines and word-wrap
fn compute_cursor_row(input: &str, cursor_idx: usize, inner_width: usize) -> usize {
    let chars: Vec<char> = input.chars().collect();
    let target = cursor_idx.min(chars.len());
    let mut row = 0usize;
    let mut col = 0usize;
    for i in 0..target {
        if chars[i] == '\n' {
            row += 1;
            col = 0;
        } else {
            let w = unicode_width::UnicodeWidthChar::width(chars[i]).unwrap_or(1);
            if col + w > inner_width { row += 1; col = w; }
            else { col += w; }
        }
    }
    row
}

/// Compute the visual column the cursor is on within its current row
fn compute_cursor_col(input: &str, cursor_idx: usize, inner_width: usize) -> usize {
    let chars: Vec<char> = input.chars().collect();
    let target = cursor_idx.min(chars.len());
    let mut col = 0usize;
    for i in 0..target {
        if chars[i] == '\n' {
            col = 0;
        } else {
            let w = unicode_width::UnicodeWidthChar::width(chars[i]).unwrap_or(1);
            if col + w > inner_width { col = w; }
            else { col += w; }
        }
    }
    col
}

/// Build styled input content split on newlines, with selection highlighting
fn build_input_content(app: &App) -> Vec<Line<'static>> {
    let chars: Vec<char> = app.input.chars().collect();
    if chars.is_empty() {
        return vec![Line::from("")];
    }

    // Get normalized selection range (if any)
    let selection = app.input_selection.and_then(|(s, e)| {
        if s == e { None } else if s < e { Some((s, e)) } else { Some((e, s)) }
    });

    let normal_style = Style::default();
    let selection_style = Style::default().bg(Color::Blue).fg(Color::White);

    // Split input into lines at '\n', tracking char position for selection overlay
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut line_spans: Vec<Span<'static>> = Vec::new();
    let mut seg_start = 0usize;

    for (i, &c) in chars.iter().enumerate() {
        if c == '\n' {
            // Flush segment before newline
            flush_segment(&chars, seg_start, i, selection, normal_style, selection_style, &mut line_spans);
            lines.push(Line::from(line_spans));
            line_spans = Vec::new();
            seg_start = i + 1;
        }
    }
    // Flush final segment (after last newline or entire string if no newlines)
    flush_segment(&chars, seg_start, chars.len(), selection, normal_style, selection_style, &mut line_spans);
    lines.push(Line::from(line_spans));

    lines
}

/// Emit spans for chars[start..end] with selection highlighting into `out`
fn flush_segment(
    chars: &[char],
    start: usize,
    end: usize,
    selection: Option<(usize, usize)>,
    normal: Style,
    selected: Style,
    out: &mut Vec<Span<'static>>,
) {
    if start >= end { return; }

    match selection {
        Some((sel_s, sel_e)) => {
            // Clamp selection to this segment's range
            let s = sel_s.max(start);
            let e = sel_e.min(end);
            // Before selection
            if start < s {
                out.push(Span::styled(chars[start..s].iter().collect::<String>(), normal));
            }
            // Selected portion (if any overlaps this segment)
            if s < e {
                out.push(Span::styled(chars[s..e].iter().collect::<String>(), selected));
            }
            // After selection
            if e < end {
                out.push(Span::styled(chars[e..end].iter().collect::<String>(), normal));
            }
        }
        None => {
            out.push(Span::styled(chars[start..end].iter().collect::<String>(), normal));
        }
    }
}
