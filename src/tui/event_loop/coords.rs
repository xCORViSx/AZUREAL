//! Screen-to-content coordinate mapping
//!
//! Translates screen pixel/cell coordinates into logical positions within
//! the various panes (viewer, session, input, edit). Used by mouse click,
//! drag, and input cursor placement.

use super::super::draw_input::word_wrap_break_points;
use super::super::draw_viewer::word_wrap_breaks;
use crate::app::App;

/// Map screen coordinates to (cache_line, cache_col) within a bordered pane.
/// Returns None if outside the content area (inside borders).
pub fn screen_to_cache_pos(
    screen_col: u16,
    screen_row: u16,
    pane: ratatui::layout::Rect,
    scroll: usize,
    cache_len: usize,
) -> Option<(usize, usize)> {
    // Content sits inside the 1px border on all sides
    let cx = pane.x + 1;
    let cy = pane.y + 1;
    let ch = pane.height.saturating_sub(2) as usize;
    if screen_col < cx || screen_row < cy {
        return None;
    }
    let vrow = (screen_row - cy) as usize;
    let col = (screen_col - cx) as usize;
    if vrow >= ch {
        return None;
    }
    let line = scroll + vrow;
    if line >= cache_len {
        return None;
    }
    Some((line, col))
}

/// Map screen coordinates to (source_line, source_col) in the edit buffer.
/// Walks source lines summing their visual wrap counts to find which source
/// line the clicked visual row falls on, then computes the source column
/// from the wrap segment offset + click column within content area.
pub fn screen_to_edit_pos(app: &App, screen_col: u16, screen_row: u16) -> Option<(usize, usize)> {
    let pane = app.pane_viewer;
    let cx = pane.x + 1; // inside left border
    let cy = pane.y + 1; // inside top border
    if screen_row < cy || screen_col < cx {
        return None;
    }

    let total_lines = app.viewer_edit_content.len();
    let line_num_width = total_lines.to_string().len().max(3);
    let gutter = line_num_width + 3; // "NNN │ " = line_num_width + " │ "
    let cw = app.viewer_edit_content_width.max(1);

    // Click column relative to content area (after gutter)
    let click_x = if (screen_col as usize) >= (cx as usize + gutter) {
        (screen_col as usize) - (cx as usize) - gutter
    } else {
        0
    };
    // Click visual row (absolute, accounting for scroll)
    let visual_row = app.viewer_scroll + (screen_row - cy) as usize;

    // Walk source lines, summing visual line counts, to find which source
    // line the clicked visual row falls on
    let mut running = 0usize;
    for (i, line_str) in app.viewer_edit_content.iter().enumerate() {
        let len = line_str.chars().count();
        let breaks = word_wrap_breaks(line_str, cw);
        let wraps = breaks.len();
        if visual_row < running + wraps {
            // Found it — wrap_seg tells us which visual row within this source line
            let wrap_seg = visual_row - running;
            // Convert click_x to a char offset within the source line using break positions
            let row_start = breaks[wrap_seg];
            let row_end = if wrap_seg + 1 < breaks.len() {
                breaks[wrap_seg + 1]
            } else {
                len
            };
            let src_col = (row_start + click_x).min(row_end);
            return Some((i, src_col));
        }
        running += wraps;
    }
    // Click is past last line — place at end of last line
    if !app.viewer_edit_content.is_empty() {
        let last = total_lines - 1;
        let last_len = app.viewer_edit_content[last].chars().count();
        return Some((last, last_len));
    }
    None
}

/// Map screen coordinates to a char index in the input text.
/// Uses word-wrap break points to find the exact char at (row, col).
pub fn screen_to_input_char(app: &App, click_col: u16, click_row: u16) -> usize {
    let inner_x = app.input_area.x + 1;
    let inner_y = app.input_area.y + 1;
    let inner_width = (app.input_area.width.saturating_sub(2)) as usize;
    if inner_width == 0 {
        return 0;
    }
    let target_row = (click_row.saturating_sub(inner_y)) as usize;
    let target_col = (click_col.saturating_sub(inner_x)) as usize;
    let visible_rows = app.input_area.height.saturating_sub(2) as usize;
    let cursor_row_current = compute_cursor_row_fast(&app.input, app.input_cursor, inner_width);
    let scroll_offset = if visible_rows > 0 && cursor_row_current >= visible_rows {
        cursor_row_current - visible_rows + 1
    } else {
        0
    };
    let actual_row = target_row + scroll_offset;
    row_col_to_char_index(&app.input, actual_row, target_col, inner_width)
}

/// Position the input cursor at the clicked screen coordinates.
/// Uses word-wrap break points (identical to draw_input.rs) to map
/// the clicked (col, row) → char index in the input buffer.
pub fn click_to_input_cursor(app: &mut App, click_col: u16, click_row: u16) {
    let inner_x = app.input_area.x + 1;
    let inner_y = app.input_area.y + 1;
    let inner_width = (app.input_area.width.saturating_sub(2)) as usize;
    if inner_width == 0 {
        return;
    }
    let target_col = (click_col.saturating_sub(inner_x)) as usize;
    let target_row = (click_row.saturating_sub(inner_y)) as usize;

    // Scroll offset so we map screen row → absolute visual row
    let visible_rows = app.input_area.height.saturating_sub(2) as usize;
    let cursor_row_current = compute_cursor_row_fast(&app.input, app.input_cursor, inner_width);
    let scroll_offset = if visible_rows > 0 && cursor_row_current >= visible_rows {
        cursor_row_current - visible_rows + 1
    } else {
        0
    };
    let actual_row = target_row + scroll_offset;

    app.input_cursor = row_col_to_char_index(&app.input, actual_row, target_col, inner_width);
}

/// Compute visual row for cursor using word-wrap break points (matches draw_input.rs)
pub fn compute_cursor_row_fast(input: &str, cursor_idx: usize, inner_width: usize) -> usize {
    let chars: Vec<char> = input.chars().collect();
    let target = cursor_idx.min(chars.len());
    let breaks = word_wrap_break_points(&chars, inner_width);
    // Each break point starts a new row; cursor is on row N if target falls in
    // the range [breaks[N-1]..breaks[N]) (with breaks[-1] = 0)
    let mut row = 0usize;
    let mut prev = 0usize;
    for &bp in &breaks {
        if target >= prev && target < bp {
            return row;
        }
        row += 1;
        prev = bp;
    }
    row // cursor in final row
}

/// Map a visual (row, col) coordinate back to a char index in the input text.
/// Uses word-wrap break points so clicking and cursor math agree with rendering.
fn row_col_to_char_index(
    input: &str,
    target_row: usize,
    target_col: usize,
    inner_width: usize,
) -> usize {
    let chars: Vec<char> = input.chars().collect();
    if chars.is_empty() {
        return 0;
    }
    let breaks = word_wrap_break_points(&chars, inner_width);

    // Find the start and end char indices for the target row
    let mut row = 0usize;
    let mut prev = 0usize;
    let mut row_start = 0usize;
    let mut row_end = chars.len();
    let mut found = false;
    for &bp in &breaks {
        if row == target_row {
            row_start = prev;
            row_end = bp;
            found = true;
            break;
        }
        row += 1;
        prev = bp;
    }
    // If target_row is the last (or only) row
    if !found {
        if row == target_row {
            row_start = prev;
            row_end = chars.len();
        } else {
            return chars.len();
        } // clicked below content
    }

    // Skip trailing newline from row content (it's not a visible character)
    let content_end = if row_end > row_start && chars.get(row_end - 1) == Some(&'\n') {
        row_end - 1
    } else {
        row_end
    };

    // Walk chars in this row until display width reaches or passes target_col
    let mut col_accum = 0usize;
    for i in row_start..content_end {
        if col_accum >= target_col {
            return i;
        }
        col_accum += unicode_width::UnicodeWidthChar::width(chars[i]).unwrap_or(1);
    }
    content_end // click past row content → place at row end
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    // =====================================================================
    // screen_to_cache_pos — basic cases
    // =====================================================================

    #[test]
    fn cache_pos_top_left_of_content() {
        // Pane at (0,0) 10x10, content starts at (1,1)
        let pane = Rect::new(0, 0, 10, 10);
        let result = screen_to_cache_pos(1, 1, pane, 0, 100);
        assert_eq!(result, Some((0, 0)));
    }

    #[test]
    fn cache_pos_with_scroll_offset() {
        let pane = Rect::new(0, 0, 10, 10);
        // scroll=5 means first visible line is cache line 5
        let result = screen_to_cache_pos(1, 1, pane, 5, 100);
        assert_eq!(result, Some((5, 0)));
    }

    #[test]
    fn cache_pos_column_offset() {
        let pane = Rect::new(0, 0, 20, 10);
        // screen_col=5 means content column 4 (5 - (0+1))
        let result = screen_to_cache_pos(5, 1, pane, 0, 100);
        assert_eq!(result, Some((0, 4)));
    }

    #[test]
    fn cache_pos_row_and_col_offset() {
        let pane = Rect::new(0, 0, 20, 20);
        let result = screen_to_cache_pos(6, 4, pane, 0, 100);
        // col = 6 - 1 = 5, row = 4 - 1 = 3, line = 0 + 3 = 3
        assert_eq!(result, Some((3, 5)));
    }

    #[test]
    fn cache_pos_pane_offset() {
        // Pane at (10, 5), content starts at (11, 6)
        let pane = Rect::new(10, 5, 20, 10);
        let result = screen_to_cache_pos(11, 6, pane, 0, 100);
        assert_eq!(result, Some((0, 0)));
    }

    #[test]
    fn cache_pos_pane_offset_with_column() {
        let pane = Rect::new(10, 5, 20, 10);
        let result = screen_to_cache_pos(15, 8, pane, 0, 100);
        // col = 15 - 11 = 4, vrow = 8 - 6 = 2
        assert_eq!(result, Some((2, 4)));
    }

    // =====================================================================
    // screen_to_cache_pos — boundary conditions (None returns)
    // =====================================================================

    #[test]
    fn cache_pos_on_left_border() {
        let pane = Rect::new(0, 0, 10, 10);
        // screen_col=0 is the left border, inside pane but < cx (0+1=1)
        let result = screen_to_cache_pos(0, 1, pane, 0, 100);
        assert_eq!(result, None);
    }

    #[test]
    fn cache_pos_on_top_border() {
        let pane = Rect::new(0, 0, 10, 10);
        let result = screen_to_cache_pos(1, 0, pane, 0, 100);
        assert_eq!(result, None);
    }

    #[test]
    fn cache_pos_both_borders() {
        let pane = Rect::new(5, 5, 10, 10);
        let result = screen_to_cache_pos(5, 5, pane, 0, 100);
        assert_eq!(result, None);
    }

    #[test]
    fn cache_pos_below_content_area() {
        // Pane height=10, content height=8, last valid vrow=7
        let pane = Rect::new(0, 0, 10, 10);
        // vrow = 9 - 1 = 8, ch = 10 - 2 = 8, 8 >= 8 → None
        let result = screen_to_cache_pos(1, 9, pane, 0, 100);
        assert_eq!(result, None);
    }

    #[test]
    fn cache_pos_past_cache_len() {
        let pane = Rect::new(0, 0, 10, 10);
        // cache_len=3, scroll=0, vrow=3 → line=3 >= 3 → None
        let result = screen_to_cache_pos(1, 4, pane, 0, 3);
        assert_eq!(result, None);
    }

    #[test]
    fn cache_pos_scroll_plus_row_exceeds_cache() {
        let pane = Rect::new(0, 0, 10, 10);
        // scroll=8, vrow=0 → line=8, cache_len=5 → None
        let result = screen_to_cache_pos(1, 1, pane, 8, 5);
        assert_eq!(result, None);
    }

    #[test]
    fn cache_pos_zero_cache_len() {
        let pane = Rect::new(0, 0, 10, 10);
        let result = screen_to_cache_pos(1, 1, pane, 0, 0);
        assert_eq!(result, None);
    }

    // =====================================================================
    // screen_to_cache_pos — edge cases with pane dimensions
    // =====================================================================

    #[test]
    fn cache_pos_minimum_pane_size() {
        // Pane 3x3: borders eat 2 in each direction, content area is 1x1
        let pane = Rect::new(0, 0, 3, 3);
        let result = screen_to_cache_pos(1, 1, pane, 0, 1);
        assert_eq!(result, Some((0, 0)));
    }

    #[test]
    fn cache_pos_pane_too_small_for_content() {
        // Pane 2x2: height-2=0, so ch=0, any vrow >= 0 → None
        let pane = Rect::new(0, 0, 2, 2);
        let result = screen_to_cache_pos(1, 1, pane, 0, 100);
        assert_eq!(result, None);
    }

    #[test]
    fn cache_pos_pane_height_1() {
        let pane = Rect::new(0, 0, 10, 1);
        // ch = 1 - 2 = saturating_sub → 0
        let result = screen_to_cache_pos(1, 1, pane, 0, 100);
        assert_eq!(result, None);
    }

    #[test]
    fn cache_pos_large_pane() {
        let pane = Rect::new(0, 0, 200, 100);
        let result = screen_to_cache_pos(100, 50, pane, 0, 1000);
        // col = 100 - 1 = 99, vrow = 50 - 1 = 49
        assert_eq!(result, Some((49, 99)));
    }

    #[test]
    fn cache_pos_last_valid_row() {
        let pane = Rect::new(0, 0, 10, 10);
        // ch = 10 - 2 = 8, last valid vrow = 7
        let result = screen_to_cache_pos(1, 8, pane, 0, 100);
        // vrow = 8 - 1 = 7, 7 < 8 → valid
        assert_eq!(result, Some((7, 0)));
    }

    #[test]
    fn cache_pos_last_cache_line() {
        let pane = Rect::new(0, 0, 10, 10);
        let result = screen_to_cache_pos(1, 1, pane, 9, 10);
        // line = 9 + 0 = 9, cache_len = 10 → valid
        assert_eq!(result, Some((9, 0)));
    }

    #[test]
    fn cache_pos_exactly_at_cache_len() {
        let pane = Rect::new(0, 0, 10, 10);
        let result = screen_to_cache_pos(1, 1, pane, 10, 10);
        // line = 10 + 0 = 10, cache_len = 10 → 10 >= 10 → None
        assert_eq!(result, None);
    }

    #[test]
    fn cache_pos_left_of_pane() {
        let pane = Rect::new(5, 5, 10, 10);
        // screen_col 3 < pane.x + 1 = 6
        let result = screen_to_cache_pos(3, 6, pane, 0, 100);
        assert_eq!(result, None);
    }

    #[test]
    fn cache_pos_above_pane() {
        let pane = Rect::new(5, 5, 10, 10);
        // screen_row 4 < pane.y + 1 = 6
        let result = screen_to_cache_pos(6, 4, pane, 0, 100);
        assert_eq!(result, None);
    }

    // =====================================================================
    // compute_cursor_row_fast — basic cases
    // =====================================================================

    #[test]
    fn cursor_row_empty_input() {
        assert_eq!(compute_cursor_row_fast("", 0, 80), 0);
    }

    #[test]
    fn cursor_row_single_char() {
        assert_eq!(compute_cursor_row_fast("a", 0, 80), 0);
    }

    #[test]
    fn cursor_row_cursor_at_end_single_line() {
        assert_eq!(compute_cursor_row_fast("hello", 5, 80), 0);
    }

    #[test]
    fn cursor_row_single_newline() {
        // "a\nb" with width 80: row 0 = "a\n", row 1 = "b"
        // cursor at 0 → row 0; cursor at 2 → row 1
        assert_eq!(compute_cursor_row_fast("a\nb", 0, 80), 0);
        assert_eq!(compute_cursor_row_fast("a\nb", 2, 80), 1);
    }

    #[test]
    fn cursor_row_multiple_newlines() {
        assert_eq!(compute_cursor_row_fast("a\nb\nc", 0, 80), 0);
        assert_eq!(compute_cursor_row_fast("a\nb\nc", 2, 80), 1);
        assert_eq!(compute_cursor_row_fast("a\nb\nc", 4, 80), 2);
    }

    #[test]
    fn cursor_row_cursor_at_newline() {
        // "\n" is at index 1 in "a\nb"
        // break point is at index 2 (char after \n)
        // cursor=1: target=1, first bp is 2 → 1 >= 0 && 1 < 2 → row 0
        assert_eq!(compute_cursor_row_fast("a\nb", 1, 80), 0);
    }

    #[test]
    fn cursor_row_just_after_newline() {
        // cursor=2 in "a\nb": target=2, bp=2 → 2 >= 0 && 2 < 2 = false → next row
        assert_eq!(compute_cursor_row_fast("a\nb", 2, 80), 1);
    }

    // =====================================================================
    // compute_cursor_row_fast — word wrapping
    // =====================================================================

    #[test]
    fn cursor_row_wraps_at_width() {
        // 10 chars with width 5: should wrap at position 5
        let input = "abcdefghij";
        // cursor at 0 → row 0
        assert_eq!(compute_cursor_row_fast(input, 0, 5), 0);
        // cursor at 4 → row 0 (still in first 5-char segment)
        assert_eq!(compute_cursor_row_fast(input, 4, 5), 0);
        // cursor at 5 → row 1 (starts second segment)
        assert_eq!(compute_cursor_row_fast(input, 5, 5), 1);
    }

    #[test]
    fn cursor_row_end_of_wrapped_text() {
        let input = "abcdefghij"; // 10 chars, width 5
        assert_eq!(compute_cursor_row_fast(input, 10, 5), 1);
    }

    #[test]
    fn cursor_row_three_wraps() {
        let input = "abcdefghijklmno"; // 15 chars, width 5
        assert_eq!(compute_cursor_row_fast(input, 0, 5), 0);
        assert_eq!(compute_cursor_row_fast(input, 5, 5), 1);
        assert_eq!(compute_cursor_row_fast(input, 10, 5), 2);
    }

    #[test]
    fn cursor_row_width_1() {
        // Each char on its own row
        assert_eq!(compute_cursor_row_fast("abc", 0, 1), 0);
        assert_eq!(compute_cursor_row_fast("abc", 1, 1), 1);
        assert_eq!(compute_cursor_row_fast("abc", 2, 1), 2);
    }

    #[test]
    fn cursor_row_exactly_fits_width() {
        // "hello" is 5 chars, width 5 → fits in one row
        assert_eq!(compute_cursor_row_fast("hello", 0, 5), 0);
        assert_eq!(compute_cursor_row_fast("hello", 5, 5), 0);
    }

    // =====================================================================
    // compute_cursor_row_fast — cursor clamping
    // =====================================================================

    #[test]
    fn cursor_row_cursor_beyond_len() {
        // cursor_idx > chars.len() → clamped to chars.len()
        assert_eq!(
            compute_cursor_row_fast("hello", 100, 80),
            compute_cursor_row_fast("hello", 5, 80)
        );
    }

    #[test]
    fn cursor_row_cursor_way_beyond() {
        assert_eq!(compute_cursor_row_fast("a", 999, 80), 0);
    }

    // =====================================================================
    // compute_cursor_row_fast — unicode
    // =====================================================================

    #[test]
    fn cursor_row_unicode_chars() {
        // Each CJK char is 2 display columns wide
        // Width 4: fits 2 CJK chars per row
        let input = "\u{4F60}\u{597D}\u{4E16}\u{754C}"; // 4 chars
                                                        // Row 0: chars 0-1 (4 columns), Row 1: chars 2-3 (4 columns)
        assert_eq!(compute_cursor_row_fast(input, 0, 4), 0);
        assert_eq!(compute_cursor_row_fast(input, 2, 4), 1);
    }

    #[test]
    fn cursor_row_mixed_ascii_unicode() {
        // "a\u{4F60}b" = 3 chars. 'a' is 1 col, '\u{4F60}' is 2 cols, 'b' is 1 col
        // Width 3: 'a'(1) + '\u{4F60}'(2) = 3, then 'b'(1)
        // Actually word_wrap_break_points checks col + w > width
        // After 'a': col=1. Next is '\u{4F60}' w=2, col+w=3 which is NOT > 3, so col=3.
        // Next is 'b' w=1, col+w=4 > 3, so break at index 2
        let input = "a\u{4F60}b";
        assert_eq!(compute_cursor_row_fast(input, 0, 3), 0);
        assert_eq!(compute_cursor_row_fast(input, 1, 3), 0);
        assert_eq!(compute_cursor_row_fast(input, 2, 3), 1);
    }

    // =====================================================================
    // compute_cursor_row_fast — newlines + wrapping combined
    // =====================================================================

    #[test]
    fn cursor_row_newline_then_wrap() {
        // "ab\ncdefgh" with width 3
        // Row 0: "ab\n" (break at 3), Row 1: "cde" (break at 6), Row 2: "fgh"
        let input = "ab\ncdefgh";
        assert_eq!(compute_cursor_row_fast(input, 0, 3), 0); // 'a'
        assert_eq!(compute_cursor_row_fast(input, 3, 3), 1); // 'c'
        assert_eq!(compute_cursor_row_fast(input, 6, 3), 2); // 'f'
    }

    // =====================================================================
    // row_col_to_char_index — basic cases
    // =====================================================================

    #[test]
    fn row_col_empty_input() {
        assert_eq!(row_col_to_char_index("", 0, 0, 80), 0);
    }

    #[test]
    fn row_col_single_char_origin() {
        assert_eq!(row_col_to_char_index("a", 0, 0, 80), 0);
    }

    #[test]
    fn row_col_single_char_col_1() {
        // col 1 past the single char → content_end = 1
        assert_eq!(row_col_to_char_index("a", 0, 1, 80), 1);
    }

    #[test]
    fn row_col_simple_text_middle() {
        assert_eq!(row_col_to_char_index("hello", 0, 2, 80), 2);
    }

    #[test]
    fn row_col_simple_text_end() {
        assert_eq!(row_col_to_char_index("hello", 0, 5, 80), 5);
    }

    #[test]
    fn row_col_past_content_end() {
        assert_eq!(row_col_to_char_index("hello", 0, 100, 80), 5);
    }

    // =====================================================================
    // row_col_to_char_index — multiline with newlines
    // =====================================================================

    #[test]
    fn row_col_second_line() {
        // "ab\ncd" → row 0: "ab\n" (indices 0..3), row 1: "cd" (indices 3..5)
        // target_row=1, target_col=0 → char index 3
        assert_eq!(row_col_to_char_index("ab\ncd", 1, 0, 80), 3);
    }

    #[test]
    fn row_col_second_line_col_1() {
        assert_eq!(row_col_to_char_index("ab\ncd", 1, 1, 80), 4);
    }

    #[test]
    fn row_col_first_line_of_multiline() {
        assert_eq!(row_col_to_char_index("ab\ncd", 0, 0, 80), 0);
    }

    #[test]
    fn row_col_first_line_col_1() {
        assert_eq!(row_col_to_char_index("ab\ncd", 0, 1, 80), 1);
    }

    #[test]
    fn row_col_newline_skipped_in_content() {
        // "ab\ncd" row 0: indices 0..3, content_end = 2 (skip \n)
        // col=2 should return content_end = 2
        assert_eq!(row_col_to_char_index("ab\ncd", 0, 2, 80), 2);
    }

    // =====================================================================
    // row_col_to_char_index — wrapping
    // =====================================================================

    #[test]
    fn row_col_wrapped_second_row() {
        // "abcdefgh" with width 4: row 0 = 0..4, row 1 = 4..8
        assert_eq!(row_col_to_char_index("abcdefgh", 1, 0, 4), 4);
    }

    #[test]
    fn row_col_wrapped_second_row_col_2() {
        assert_eq!(row_col_to_char_index("abcdefgh", 1, 2, 4), 6);
    }

    #[test]
    fn row_col_wrapped_first_row_last_col() {
        // Width 4, row 0 = 0..4, col 3 → index 3
        assert_eq!(row_col_to_char_index("abcdefgh", 0, 3, 4), 3);
    }

    // =====================================================================
    // row_col_to_char_index — target row beyond content
    // =====================================================================

    #[test]
    fn row_col_row_beyond_content() {
        // "abc" with width 80 → only 1 row (row 0)
        // target_row=5 → return chars.len() = 3
        assert_eq!(row_col_to_char_index("abc", 5, 0, 80), 3);
    }

    #[test]
    fn row_col_row_1_single_line() {
        assert_eq!(row_col_to_char_index("abc", 1, 0, 80), 3);
    }

    // =====================================================================
    // row_col_to_char_index — width 1
    // =====================================================================

    #[test]
    fn row_col_width_1_each_char_own_row() {
        // "abc" width 1: row 0=[0..1], row 1=[1..2], row 2=[2..3]
        assert_eq!(row_col_to_char_index("abc", 0, 0, 1), 0);
        assert_eq!(row_col_to_char_index("abc", 1, 0, 1), 1);
        assert_eq!(row_col_to_char_index("abc", 2, 0, 1), 2);
    }

    // =====================================================================
    // row_col_to_char_index — unicode
    // =====================================================================

    #[test]
    fn row_col_unicode_wide_chars() {
        // CJK chars are 2 columns wide. Width 4 fits 2 CJK chars.
        let input = "\u{4F60}\u{597D}\u{4E16}\u{754C}"; // 4 chars, 8 display cols
                                                        // Row 0: chars 0..2 (4 cols), Row 1: chars 2..4 (4 cols)
                                                        // target_row=0, target_col=0 → char 0
        assert_eq!(row_col_to_char_index(input, 0, 0, 4), 0);
        // target_col=2 → second char (first char takes cols 0-1)
        assert_eq!(row_col_to_char_index(input, 0, 2, 4), 1);
    }

    #[test]
    fn row_col_unicode_second_row() {
        let input = "\u{4F60}\u{597D}\u{4E16}\u{754C}";
        assert_eq!(row_col_to_char_index(input, 1, 0, 4), 2);
    }

    // =====================================================================
    // row_col_to_char_index — edge cases
    // =====================================================================

    #[test]
    fn row_col_zero_target_col_and_row() {
        assert_eq!(row_col_to_char_index("test", 0, 0, 80), 0);
    }

    #[test]
    fn row_col_spaces_in_text() {
        let input = "hello world";
        // Width 5: "hello" then " worl" (break at space) then "d"
        // Actually word_wrap_break_points prefers breaking at spaces
        assert_eq!(row_col_to_char_index(input, 0, 0, 6), 0);
    }

    #[test]
    fn row_col_only_newlines() {
        // "\n\n\n" = 3 chars, each newline creates a break
        let input = "\n\n\n";
        assert_eq!(row_col_to_char_index(input, 0, 0, 80), 0);
        assert_eq!(row_col_to_char_index(input, 1, 0, 80), 1);
        assert_eq!(row_col_to_char_index(input, 2, 0, 80), 2);
    }

    #[test]
    fn row_col_single_newline() {
        // "\n" → break at 1, row 0 = 0..1, content_end = 0 (skip \n)
        assert_eq!(row_col_to_char_index("\n", 0, 0, 80), 0);
    }

    #[test]
    fn row_col_trailing_newline() {
        // "abc\n" → break at 4, row 0 = 0..4, content_end = 3 (skip \n)
        assert_eq!(row_col_to_char_index("abc\n", 0, 0, 80), 0);
        assert_eq!(row_col_to_char_index("abc\n", 0, 3, 80), 3);
    }

    #[test]
    fn row_col_large_width() {
        // Width much larger than content — everything on row 0
        assert_eq!(row_col_to_char_index("hi", 0, 0, 10000), 0);
        assert_eq!(row_col_to_char_index("hi", 0, 1, 10000), 1);
    }
}
