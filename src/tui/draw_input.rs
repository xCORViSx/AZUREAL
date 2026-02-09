//! Input field rendering
//!
//! Supports multi-line input via Shift+Enter. Text is pre-wrapped at character
//! boundaries (not word boundaries) so cursor positioning is always accurate.
//! Each `Line` given to ratatui represents exactly one visual row — no `.wrap()`
//! is used, eliminating mismatch between cursor math and text layout.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::app::{App, Focus};
use super::keybindings::{prompt_type_title, prompt_command_title};

/// Draw the Claude prompt input field with pre-wrapped text and cursor positioning
pub fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    // Border color reflects current input state:
    // magenta = STT recording/transcribing, yellow = prompt mode, red = command mode
    let (border_color, title) = if app.stt_recording {
        (Color::Magenta, format!(" REC {}", prompt_type_title().trim_start()))
    } else if app.stt_transcribing {
        (Color::Magenta, format!(" ... {}", prompt_type_title().trim_start()))
    } else if app.prompt_mode {
        (Color::Yellow, prompt_type_title())
    } else {
        (Color::Red, prompt_command_title())
    };

    let is_focused = app.focus == Focus::Input;
    let inner_width = area.width.saturating_sub(2) as usize;
    // Visible rows inside the border (total height minus top+bottom border)
    let visible_rows = area.height.saturating_sub(2) as usize;

    // Pre-wrap content at character boundaries and compute cursor position
    // in a single pass — both use identical wrapping logic so they always agree
    let (content, cursor_row, cursor_col) =
        build_wrapped_content(app, inner_width);

    // Scroll offset: keep cursor visible within the box
    let scroll_offset = if visible_rows > 0 && cursor_row >= visible_rows {
        (cursor_row - visible_rows + 1) as u16
    } else {
        0
    };

    // No .wrap() — content is already pre-wrapped, one Line per visual row
    let input = Paragraph::new(content)
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
        let adjusted_row = cursor_row as u16 - scroll_offset;
        f.set_cursor_position((
            area.x + 1 + cursor_col as u16,
            area.y + 1 + adjusted_row,
        ));
    }
}

/// Build pre-wrapped lines AND compute cursor position in one pass.
/// Returns (visual_lines, cursor_row, cursor_col).
///
/// Each output Line is exactly one visual row — wrapping happens at character
/// boundaries (col + char_width > inner_width triggers a new row), identical
/// to how run.rs computes input_height and fast_draw_input renders text.
fn build_wrapped_content(app: &App, inner_width: usize) -> (Vec<Line<'static>>, usize, usize) {
    let chars: Vec<char> = app.input.chars().collect();
    if chars.is_empty() {
        return (vec![Line::from("")], 0, 0);
    }

    let target = app.input_cursor.min(chars.len());

    // Normalized selection range (if any)
    let selection = app.input_selection.and_then(|(s, e)| {
        if s == e { None } else if s < e { Some((s, e)) } else { Some((e, s)) }
    });

    let normal_style = Style::default();
    let selection_style = Style::default().bg(Color::Blue).fg(Color::White);

    let mut lines: Vec<Line<'static>> = Vec::new();
    // Current visual row being built: segments of (start_char_idx, end_char_idx)
    let mut row_start = 0usize; // char index where current row started
    let mut col = 0usize;       // current column width in current row
    let mut cursor_row = 0usize;
    let mut cursor_col = 0usize;

    for (i, &c) in chars.iter().enumerate() {
        // Track cursor position BEFORE processing this character.
        // When cursor == i, it's positioned right before char[i].
        if i == target {
            cursor_row = lines.len();
            cursor_col = col;
        }

        if c == '\n' {
            // Flush current row, start new one
            flush_row(&chars, row_start, i, selection, normal_style, selection_style, &mut lines);
            row_start = i + 1;
            col = 0;
        } else {
            let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
            // Wrap: if this char would exceed width, start a new visual row
            if inner_width > 0 && col + w > inner_width {
                flush_row(&chars, row_start, i, selection, normal_style, selection_style, &mut lines);
                row_start = i;
                col = 0;
                // Re-check cursor — it might land at the start of this new wrapped row
                if i == target {
                    cursor_row = lines.len();
                    cursor_col = 0;
                }
            }
            col += w;
        }
    }

    // Cursor at the very end (after all chars)
    if target == chars.len() {
        cursor_row = lines.len();
        cursor_col = col;
    }

    // Flush final row
    flush_row(&chars, row_start, chars.len(), selection, normal_style, selection_style, &mut lines);

    (lines, cursor_row, cursor_col)
}

/// Emit one visual row as a Line with selection highlighting
fn flush_row(
    chars: &[char],
    start: usize,
    end: usize,
    selection: Option<(usize, usize)>,
    normal: Style,
    selected: Style,
    lines: &mut Vec<Line<'static>>,
) {
    if start >= end {
        lines.push(Line::from(""));
        return;
    }

    let mut spans: Vec<Span<'static>> = Vec::new();
    match selection {
        Some((sel_s, sel_e)) => {
            // Clamp selection to this row's char range
            let s = sel_s.max(start);
            let e = sel_e.min(end);
            if start < s {
                spans.push(Span::styled(chars[start..s].iter().collect::<String>(), normal));
            }
            if s < e {
                spans.push(Span::styled(chars[s..e].iter().collect::<String>(), selected));
            }
            if e < end {
                spans.push(Span::styled(chars[e..end].iter().collect::<String>(), normal));
            }
        }
        None => {
            spans.push(Span::styled(chars[start..end].iter().collect::<String>(), normal));
        }
    }
    lines.push(Line::from(spans));
}
