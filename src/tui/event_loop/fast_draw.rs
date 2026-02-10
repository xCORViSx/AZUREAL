//! Fast-path input rendering
//!
//! Writes the input box content directly via crossterm (~0.1ms) instead of
//! going through terminal.draw() (~18ms). Used during rapid typing so
//! keystrokes get instant visual feedback while the full UI catches up later.

use std::io::{self, Write};
use crossterm::{cursor, execute, style};

use crate::app::App;
use super::super::draw_input::{word_wrap_break_points, display_width};

/// Fast-path: render ONLY the input box content via direct crossterm writes.
/// Costs ~0.1ms vs ~18ms for terminal.draw(). Used during rapid typing so
/// keystrokes get instant visual feedback while the full UI catches up later.
/// Writes the input text into the cached input_area rect, positions the cursor,
/// and flushes. Ratatui's internal buffer becomes stale but the next full draw
/// will reconcile everything.
pub fn fast_draw_input(app: &App) {
    let area = app.input_area;
    let inner_width = area.width.saturating_sub(2) as usize;
    let visible_rows = area.height.saturating_sub(2) as usize;
    if inner_width == 0 || visible_rows == 0 { return; }

    // Compute word-wrap break points (identical logic as draw_input.rs)
    let chars: Vec<char> = app.input.chars().collect();
    let breaks = word_wrap_break_points(&chars, inner_width);
    let target = app.input_cursor.min(chars.len());

    // Walk rows from break points to find cursor row + col + build visual lines
    let mut visual_lines: Vec<String> = Vec::new();
    let mut cursor_row = 0usize;
    let mut cursor_col = 0usize;
    let mut prev = 0usize;
    for &bp in &breaks {
        if target >= prev && target < bp {
            cursor_row = visual_lines.len();
            cursor_col = display_width(&chars[prev..target]);
        }
        // Collect row text (exclude trailing newline if any)
        let end = if bp > 0 && chars.get(bp - 1) == Some(&'\n') { bp - 1 } else { bp };
        visual_lines.push(chars[prev..end].iter().collect());
        prev = bp;
    }
    // Final row
    if target >= prev {
        cursor_row = visual_lines.len();
        cursor_col = display_width(&chars[prev..target.min(chars.len())]);
    }
    visual_lines.push(chars[prev..].iter().collect());

    // Scroll offset: keep cursor visible
    let scroll_offset = if cursor_row >= visible_rows {
        cursor_row - visible_rows + 1
    } else { 0 };

    let mut stdout = io::stdout();

    // Write each visible row inside the border (x+1, y+1 = inside border)
    for row_idx in 0..visible_rows {
        let line_idx = scroll_offset + row_idx;
        let text = visual_lines.get(line_idx).map(|s| s.as_str()).unwrap_or("");
        // Pad to inner_width (display columns) to overwrite stale content
        let text_width: usize = text.chars()
            .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1))
            .sum();
        let pad = inner_width.saturating_sub(text_width);
        let padded = format!("{}{}", text, " ".repeat(pad));
        let _ = execute!(
            stdout,
            cursor::MoveTo(area.x + 1, area.y + 1 + row_idx as u16),
            style::Print(&padded)
        );
    }

    // Position cursor
    let adjusted_row = cursor_row.saturating_sub(scroll_offset);
    let _ = execute!(
        stdout,
        cursor::MoveTo(
            area.x + 1 + cursor_col as u16,
            area.y + 1 + adjusted_row as u16,
        ),
        cursor::Show
    );
    let _ = stdout.flush();
}
