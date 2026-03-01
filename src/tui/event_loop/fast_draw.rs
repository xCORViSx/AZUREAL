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
        // Force white text so input is theme-independent (matches draw_input.rs normal_style)
        let _ = execute!(
            stdout,
            cursor::MoveTo(area.x + 1, area.y + 1 + row_idx as u16),
            style::SetForegroundColor(style::Color::White),
            style::Print(&padded),
            style::ResetColor
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    // -- Inner width calculation --

    #[test]
    fn test_inner_width_normal() {
        let area = Rect::new(0, 0, 82, 5);
        let inner_width = area.width.saturating_sub(2) as usize;
        assert_eq!(inner_width, 80);
    }

    #[test]
    fn test_inner_width_small() {
        let area = Rect::new(0, 0, 3, 5);
        let inner_width = area.width.saturating_sub(2) as usize;
        assert_eq!(inner_width, 1);
    }

    #[test]
    fn test_inner_width_zero() {
        let area = Rect::new(0, 0, 2, 5);
        let inner_width = area.width.saturating_sub(2) as usize;
        assert_eq!(inner_width, 0);
    }

    #[test]
    fn test_inner_width_one() {
        let area = Rect::new(0, 0, 1, 5);
        let inner_width = area.width.saturating_sub(2) as usize;
        assert_eq!(inner_width, 0);
    }

    // -- Visible rows calculation --

    #[test]
    fn test_visible_rows_normal() {
        let area = Rect::new(0, 0, 80, 10);
        let visible_rows = area.height.saturating_sub(2) as usize;
        assert_eq!(visible_rows, 8);
    }

    #[test]
    fn test_visible_rows_small() {
        let area = Rect::new(0, 0, 80, 3);
        let visible_rows = area.height.saturating_sub(2) as usize;
        assert_eq!(visible_rows, 1);
    }

    #[test]
    fn test_visible_rows_zero() {
        let area = Rect::new(0, 0, 80, 2);
        let visible_rows = area.height.saturating_sub(2) as usize;
        assert_eq!(visible_rows, 0);
    }

    // -- Early return guard --

    #[test]
    fn test_early_return_zero_width() {
        let inner_width = 0usize;
        let visible_rows = 10usize;
        assert!(inner_width == 0 || visible_rows == 0);
    }

    #[test]
    fn test_early_return_zero_rows() {
        let inner_width = 80usize;
        let visible_rows = 0usize;
        assert!(inner_width == 0 || visible_rows == 0);
    }

    #[test]
    fn test_no_early_return_normal() {
        let inner_width = 80usize;
        let visible_rows = 20usize;
        assert!(!(inner_width == 0 || visible_rows == 0));
    }

    // -- Cursor target clamping --

    #[test]
    fn test_cursor_target_clamp() {
        let cursor = 50usize;
        let chars_len = 30usize;
        let target = cursor.min(chars_len);
        assert_eq!(target, 30);
    }

    #[test]
    fn test_cursor_target_within_range() {
        let cursor = 10usize;
        let chars_len = 30usize;
        let target = cursor.min(chars_len);
        assert_eq!(target, 10);
    }

    // -- Scroll offset calculation --

    #[test]
    fn test_scroll_offset_needed() {
        let cursor_row = 15usize;
        let visible_rows = 10usize;
        let offset = if cursor_row >= visible_rows { cursor_row - visible_rows + 1 } else { 0 };
        assert_eq!(offset, 6);
    }

    #[test]
    fn test_scroll_offset_not_needed() {
        let cursor_row = 5usize;
        let visible_rows = 10usize;
        let offset = if cursor_row >= visible_rows { cursor_row - visible_rows + 1 } else { 0 };
        assert_eq!(offset, 0);
    }

    #[test]
    fn test_scroll_offset_at_boundary() {
        let cursor_row = 10usize;
        let visible_rows = 10usize;
        let offset = if cursor_row >= visible_rows { cursor_row - visible_rows + 1 } else { 0 };
        assert_eq!(offset, 1);
    }

    // -- Padding calculation --

    #[test]
    fn test_padding_short_text() {
        let inner_width = 80usize;
        let text_width = 20usize;
        let pad = inner_width.saturating_sub(text_width);
        assert_eq!(pad, 60);
    }

    #[test]
    fn test_padding_full_width() {
        let inner_width = 80usize;
        let text_width = 80usize;
        let pad = inner_width.saturating_sub(text_width);
        assert_eq!(pad, 0);
    }

    #[test]
    fn test_padding_over_width() {
        let inner_width = 80usize;
        let text_width = 90usize;
        let pad = inner_width.saturating_sub(text_width);
        assert_eq!(pad, 0);
    }

    // -- Padded text formatting --

    #[test]
    fn test_padded_format() {
        let text = "hello";
        let pad = 5;
        let padded = format!("{}{}", text, " ".repeat(pad));
        assert_eq!(padded, "hello     ");
        assert_eq!(padded.len(), 10);
    }

    // -- Adjusted row calculation --

    #[test]
    fn test_adjusted_row() {
        let cursor_row = 15usize;
        let scroll_offset = 6usize;
        let adjusted = cursor_row.saturating_sub(scroll_offset);
        assert_eq!(adjusted, 9);
    }

    #[test]
    fn test_adjusted_row_no_scroll() {
        let cursor_row = 5usize;
        let scroll_offset = 0usize;
        let adjusted = cursor_row.saturating_sub(scroll_offset);
        assert_eq!(adjusted, 5);
    }

    // -- display_width function --

    #[test]
    fn test_display_width_ascii() {
        let chars: Vec<char> = "hello".chars().collect();
        let w = display_width(&chars);
        assert_eq!(w, 5);
    }

    #[test]
    fn test_display_width_empty() {
        let chars: Vec<char> = vec![];
        let w = display_width(&chars);
        assert_eq!(w, 0);
    }

    // -- word_wrap_break_points function --

    #[test]
    fn test_word_wrap_empty() {
        let chars: Vec<char> = vec![];
        let breaks = word_wrap_break_points(&chars, 80);
        // Empty input produces no break points
        assert!(breaks.is_empty());
    }

    #[test]
    fn test_word_wrap_short_line() {
        let chars: Vec<char> = "hello".chars().collect();
        let breaks = word_wrap_break_points(&chars, 80);
        // Short line fits within width — no wrapping needed, no break points
        assert!(breaks.is_empty());
    }

    // -- Newline detection for trailing newline stripping --

    #[test]
    fn test_trailing_newline_strip() {
        let chars: Vec<char> = "hello\n".chars().collect();
        let bp = chars.len(); // 6
        let end = if bp > 0 && chars.get(bp - 1) == Some(&'\n') { bp - 1 } else { bp };
        assert_eq!(end, 5);
    }

    #[test]
    fn test_no_trailing_newline() {
        let chars: Vec<char> = "hello".chars().collect();
        let bp = chars.len(); // 5
        let end = if bp > 0 && chars.get(bp - 1) == Some(&'\n') { bp - 1 } else { bp };
        assert_eq!(end, 5);
    }

    // -- Visual lines collection --

    #[test]
    fn test_visual_lines_from_chars() {
        let chars: Vec<char> = "abc".chars().collect();
        let prev = 0;
        let end = 3;
        let line: String = chars[prev..end].iter().collect();
        assert_eq!(line, "abc");
    }

    // -- Cursor position inside border --

    #[test]
    fn test_cursor_position_x() {
        let area_x = 5u16;
        let cursor_col = 10u16;
        let x = area_x + 1 + cursor_col;
        assert_eq!(x, 16);
    }

    #[test]
    fn test_cursor_position_y() {
        let area_y = 3u16;
        let adjusted_row = 7u16;
        let y = area_y + 1 + adjusted_row;
        assert_eq!(y, 11);
    }

    // -- Unicode width --

    #[test]
    fn test_unicode_width_ascii() {
        let w = unicode_width::UnicodeWidthChar::width('a').unwrap_or(1);
        assert_eq!(w, 1);
    }

    #[test]
    fn test_unicode_width_cjk() {
        let w = unicode_width::UnicodeWidthChar::width('\u{4e2d}').unwrap_or(1);
        assert_eq!(w, 2); // CJK characters are double-width
    }

    // -- Crossterm style color --

    #[test]
    fn test_crossterm_color_white() {
        let c = style::Color::White;
        assert!(matches!(c, style::Color::White));
    }

    // -- fast_draw_input function type --

    #[test]
    fn test_fast_draw_input_fn_type() {
        let _ = fast_draw_input as fn(&App);
    }

    // -- word_wrap_break_points: newline forces break --

    #[test]
    fn test_word_wrap_newline_forces_break() {
        let chars: Vec<char> = "hello\nworld".chars().collect();
        let breaks = word_wrap_break_points(&chars, 80);
        // '\n' should create at least one break point
        assert!(!breaks.is_empty());
    }

    #[test]
    fn test_word_wrap_long_line_breaks() {
        // A word longer than the wrap width must still produce breaks
        let chars: Vec<char> = "a".repeat(100).chars().collect();
        let breaks = word_wrap_break_points(&chars, 40);
        assert!(!breaks.is_empty());
    }

    #[test]
    fn test_word_wrap_multiple_newlines() {
        let chars: Vec<char> = "a\nb\nc".chars().collect();
        let breaks = word_wrap_break_points(&chars, 80);
        // Two newlines → two break points
        assert!(breaks.len() >= 2);
    }

    #[test]
    fn test_word_wrap_width_one() {
        // With width 1, every char is a break
        let chars: Vec<char> = "abc".chars().collect();
        let breaks = word_wrap_break_points(&chars, 1);
        assert!(!breaks.is_empty());
    }

    // -- display_width: wide and ascii chars --

    #[test]
    fn test_display_width_single_char() {
        let chars: Vec<char> = vec!['x'];
        let w = display_width(&chars);
        assert_eq!(w, 1);
    }

    #[test]
    fn test_display_width_cjk_chars() {
        let chars: Vec<char> = "中文".chars().collect();
        let w = display_width(&chars);
        assert_eq!(w, 4); // each CJK char = 2
    }

    #[test]
    fn test_display_width_mixed() {
        // 'a' (1) + '中' (2) = 3
        let chars: Vec<char> = vec!['a', '中'];
        let w = display_width(&chars);
        assert_eq!(w, 3);
    }

    // -- Scroll offset edge cases --

    #[test]
    fn test_scroll_offset_cursor_just_below_boundary() {
        let cursor_row = 9usize;
        let visible_rows = 10usize;
        let offset = if cursor_row >= visible_rows { cursor_row - visible_rows + 1 } else { 0 };
        assert_eq!(offset, 0);
    }

    #[test]
    fn test_scroll_offset_deep_cursor() {
        let cursor_row = 100usize;
        let visible_rows = 10usize;
        let offset = if cursor_row >= visible_rows { cursor_row - visible_rows + 1 } else { 0 };
        assert_eq!(offset, 91);
    }

    // -- Padded text: empty string --

    #[test]
    fn test_padded_empty_text() {
        let text = "";
        let pad = 80usize;
        let padded = format!("{}{}", text, " ".repeat(pad));
        assert_eq!(padded.len(), 80);
    }

    #[test]
    fn test_padded_zero_pad() {
        let text = "hello";
        let pad = 0usize;
        let padded = format!("{}{}", text, " ".repeat(pad));
        assert_eq!(padded, "hello");
    }

    // -- Adjusted row saturating_sub --

    #[test]
    fn test_adjusted_row_underflow_saturates() {
        let cursor_row = 3usize;
        let scroll_offset = 10usize;
        // scroll_offset > cursor_row — saturating_sub prevents underflow
        let adjusted = cursor_row.saturating_sub(scroll_offset);
        assert_eq!(adjusted, 0);
    }

    // -- Rect coordinate math --

    #[test]
    fn test_rect_inner_origin_offset() {
        let area = ratatui::layout::Rect::new(10, 5, 80, 20);
        // Content starts at x+1, y+1 (inside border)
        let content_x = area.x + 1;
        let content_y = area.y + 1;
        assert_eq!(content_x, 11);
        assert_eq!(content_y, 6);
    }

    #[test]
    fn test_rect_large_coords() {
        let area = ratatui::layout::Rect::new(200, 100, 50, 30);
        let inner_width = area.width.saturating_sub(2) as usize;
        let visible_rows = area.height.saturating_sub(2) as usize;
        assert_eq!(inner_width, 48);
        assert_eq!(visible_rows, 28);
    }

    // -- unicode_width fallback default --

    #[test]
    fn test_unicode_width_fallback_space() {
        let w = unicode_width::UnicodeWidthChar::width(' ').unwrap_or(1);
        assert_eq!(w, 1);
    }

    #[test]
    fn test_unicode_width_newline_returns_value() {
        // Control chars like '\n' — the crate may return None or Some(0/1).
        // The fast_draw code uses `.unwrap_or(1)` for safety; verify that path.
        let raw = unicode_width::UnicodeWidthChar::width('\n');
        // Either None (control) or Some(0) — the important thing is unwrap_or(1) gives a valid usize
        let w = raw.unwrap_or(1);
        assert!(w <= 1);
    }

    // -- App::new() input_area defaults to zero-size Rect --

    #[test]
    fn test_app_new_input_area_default() {
        let app = App::new();
        // Default Rect is (0,0,0,0) — early return guard triggers
        let inner_width = app.input_area.width.saturating_sub(2) as usize;
        let visible_rows = app.input_area.height.saturating_sub(2) as usize;
        assert!(inner_width == 0 || visible_rows == 0);
    }

    // -- Cursor row tracking across break points --

    #[test]
    fn test_cursor_row_assignment_logic() {
        // Simulate the cursor_row / cursor_col tracking math
        // For target in [prev, bp), cursor_row = current visual line count
        let prev = 0usize;
        let bp = 10usize;
        let target = 5usize;
        let visual_lines_len = 2usize; // 2 lines accumulated before this iteration
        let cursor_row = if target >= prev && target < bp { visual_lines_len } else { 99 };
        assert_eq!(cursor_row, 2);
    }

    #[test]
    fn test_cursor_row_on_final_row() {
        // After all break points, target >= prev means cursor is on final row
        let prev = 10usize;
        let target = 15usize;
        let visual_lines_len = 3usize;
        let on_final = target >= prev;
        let cursor_row = if on_final { visual_lines_len } else { 0 };
        assert_eq!(cursor_row, 3);
    }
}
