//! Input field rendering
//!
//! Supports multi-line input via Shift+Enter. Newlines split the text into
//! separate `Line`s for ratatui. Cursor positioning accounts for both
//! newlines and word-wrapping within each line.

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

    // Build styled content with selection highlighting, split on newlines
    let content = build_input_content(app);

    let input = Paragraph::new(content)
        .wrap(Wrap { trim: false })
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
    // Walk chars accounting for newlines and word-wrapping
    if app.prompt_mode && is_focused && inner_width > 0 {
        let chars: Vec<char> = app.input.chars().collect();
        let cursor_char_idx = app.input_cursor.min(chars.len());

        let mut visual_col = 0usize;
        let mut visual_row = 0usize;
        for i in 0..cursor_char_idx {
            let c = chars[i];
            if c == '\n' {
                // Newline: move to start of next row
                visual_row += 1;
                visual_col = 0;
            } else {
                let char_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
                if visual_col + char_width > inner_width {
                    // Word-wrap: overflow to next row
                    visual_row += 1;
                    visual_col = char_width;
                } else {
                    visual_col += char_width;
                }
            }
        }

        f.set_cursor_position((
            area.x + 1 + visual_col as u16,
            area.y + 1 + visual_row as u16,
        ));
    }
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
