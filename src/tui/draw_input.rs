//! Input field rendering
//!
//! Supports multi-line input via Shift+Enter. Text is pre-wrapped at word
//! boundaries (falls back to char boundaries when a word exceeds the width).
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

/// Split hint segments across top and bottom borders.
/// Returns (top_title, bottom_title_or_none).
/// `label` is the mode name (e.g. " COMMAND "), `hints` is the `|`-delimited hint string.
/// Packs as many segments onto the top border (after label) as fit, then the rest go bottom.
pub fn split_title_hints(label: &str, hints: &str, max_w: usize) -> (String, Option<String>) {
    let full = format!("{} ({}) ", label.trim_end(), hints);
    if full.chars().count() <= max_w { return (format!(" {} ", full.trim()), None); }

    let segments: Vec<&str> = hints.split(" | ").collect();
    // Budget for top: " LABEL (seg | seg | ...) " — label + parens + spaces
    let overhead = label.chars().count() + 3; // "(" + ") " after segments
    let top_budget = max_w.saturating_sub(overhead);

    let mut top_parts: Vec<&str> = Vec::new();
    let mut top_len = 0usize;
    let mut split_at = 0;
    for (i, seg) in segments.iter().enumerate() {
        let sep = if top_parts.is_empty() { 0 } else { 3 };
        if top_len + sep + seg.chars().count() > top_budget { break; }
        top_len += sep + seg.chars().count();
        top_parts.push(seg);
        split_at = i + 1;
    }

    let top = if top_parts.is_empty() {
        format!("{}", label)
    } else {
        format!("{}({}) ", label, top_parts.join(" | "))
    };

    let bottom = if split_at < segments.len() {
        let rest = segments[split_at..].join(" | ");
        // Wrap in parens to match top border format, truncate to fit
        let content = format!("({})", rest);
        let trimmed: String = content.chars().take(max_w.saturating_sub(2)).collect();
        Some(format!(" {} ", trimmed))
    } else {
        None
    };

    (top, bottom)
}

/// Draw the Claude prompt input field with pre-wrapped text and cursor positioning.
/// When hints overflow the top border, remaining hints go on the bottom border.
pub fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    // Border color reflects current input state:
    // magenta = STT recording/transcribing, yellow = prompt mode, red = command mode
    let (border_color, label, _full_title, hints) = if app.stt_recording {
        let (l, ft, h) = prompt_type_title();
        (Color::Magenta, format!(" REC{}", l.trim_end()), ft, h)
    } else if app.stt_transcribing {
        let (l, ft, h) = prompt_type_title();
        (Color::Magenta, format!(" ...{}", l.trim_end()), ft, h)
    } else if app.prompt_mode {
        let (l, ft, h) = prompt_type_title();
        (Color::Yellow, l, ft, h)
    } else {
        let (l, ft, h) = prompt_command_title();
        (Color::Red, l, ft, h)
    };

    let is_focused = app.focus == Focus::Input;
    let inner_width = area.width.saturating_sub(2) as usize;
    let visible_rows = area.height.saturating_sub(2) as usize;

    // Split hints across top and bottom borders
    let (top_title, bottom_title) = split_title_hints(&label, &hints, inner_width);

    // Pre-wrap content at character boundaries and compute cursor position
    let (content, cursor_row, cursor_col) =
        build_wrapped_content(app, inner_width);

    // Scroll offset: keep cursor visible within the box
    let scroll_offset = if visible_rows > 0 && cursor_row >= visible_rows {
        (cursor_row - visible_rows + 1) as u16
    } else {
        0
    };

    let title_style = if is_focused {
        Style::default().fg(border_color).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
        .title(Span::styled(top_title, title_style))
        .border_style(title_style);

    // Overflow hints on bottom border — same style as top title (color + bold match)
    if let Some(ref bot) = bottom_title {
        block = block.title_bottom(Span::styled(bot.as_str(), title_style));
    }

    let input = Paragraph::new(content)
        .scroll((scroll_offset, 0))
        .block(block);

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
/// Wraps at word boundaries when possible (last space before width limit).
/// Falls back to char-boundary break when a single word exceeds the width.
/// Uses `word_wrap_break_points()` to pre-compute break indices so cursor
/// math and rendering agree perfectly.
fn build_wrapped_content(app: &App, inner_width: usize) -> (Vec<Line<'static>>, usize, usize) {
    let chars: Vec<char> = app.input.chars().collect();
    if chars.is_empty() {
        return (vec![Line::from("")], 0, 0);
    }

    let target = app.input_cursor.min(chars.len());
    let breaks = word_wrap_break_points(&chars, inner_width);

    let selection = app.input_selection.and_then(|(s, e)| {
        if s == e { None } else if s < e { Some((s, e)) } else { Some((e, s)) }
    });

    let normal_style = Style::default();
    let selection_style = Style::default().bg(Color::Blue).fg(Color::White);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut cursor_row = 0usize;
    let mut cursor_col = 0usize;

    // Walk rows defined by break points
    let mut prev = 0usize;
    for &bp in &breaks {
        // Cursor falls in this row if target is in [prev, bp)
        if target >= prev && target < bp {
            cursor_row = lines.len();
            cursor_col = display_width(&chars[prev..target]);
        }
        flush_row(&chars, prev, bp, selection, normal_style, selection_style, &mut lines);
        prev = bp;
        // Skip newline char (it's not displayed, next row starts after it)
        if prev > 0 && prev <= chars.len() && prev > 0 && chars.get(prev - 1) == Some(&'\n') {
            // newline already consumed by break point logic
        }
    }
    // Final row
    if target >= prev {
        cursor_row = lines.len();
        cursor_col = display_width(&chars[prev..target.min(chars.len())]);
    }
    flush_row(&chars, prev, chars.len(), selection, normal_style, selection_style, &mut lines);

    (lines, cursor_row, cursor_col)
}

/// Compute display width of a char slice (sum of unicode widths)
pub(crate) fn display_width(chars: &[char]) -> usize {
    chars.iter().map(|c| unicode_width::UnicodeWidthChar::width(*c).unwrap_or(1)).sum()
}

/// Compute word-wrap break points for a char array at the given width.
/// Returns a sorted Vec of char indices where line breaks occur.
/// A break at index `i` means chars[prev..i] is one visual row.
/// Newlines produce a break at `i+1` (the char after '\n').
/// Word-wrap prefers breaking at the char after the last space before
/// the width limit. Falls back to hard char break if no space exists.
pub(crate) fn word_wrap_break_points(chars: &[char], width: usize) -> Vec<usize> {
    if width == 0 || chars.is_empty() { return vec![]; }
    let mut breaks = Vec::new();
    let mut _row_start = 0usize;
    let mut col = 0usize;
    // Track last space position (char index) and column width at that point
    let mut last_space: Option<usize> = None;

    for (i, &c) in chars.iter().enumerate() {
        if c == '\n' {
            // Newline: break here. Next row starts at i+1.
            breaks.push(i + 1);
            _row_start =i + 1;
            col = 0;
            last_space = None;
            continue;
        }

        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if col + w > width {
            // Need to wrap. Prefer breaking at last space.
            if let Some(sp) = last_space {
                // Break after the space: row is [row_start..sp+1], new row starts at sp+1
                breaks.push(sp + 1);
                _row_start =sp + 1;
                // Recompute col from sp+1 to current char (inclusive)
                col = display_width(&chars[sp + 1..i]) + w;
                last_space = None;
                // Check for spaces in the carried-over portion
                for j in (sp + 1)..i {
                    if chars[j] == ' ' { last_space = Some(j); }
                }
            } else {
                // No space on this row — hard break at current char
                breaks.push(i);
                _row_start =i;
                col = w;
                last_space = None;
            }
        } else {
            col += w;
        }

        if c == ' ' { last_space = Some(i); }
    }
    breaks
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
