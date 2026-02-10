//! Screen-to-content coordinate mapping
//!
//! Translates screen pixel/cell coordinates into logical positions within
//! the various panes (viewer, convo, input, edit). Used by mouse click,
//! drag, and input cursor placement.

use crate::app::App;
use super::super::draw_input::word_wrap_break_points;
use super::super::draw_viewer::word_wrap_breaks;

/// Map screen coordinates to (cache_line, cache_col) within a bordered pane.
/// Returns None if outside the content area (inside borders).
pub fn screen_to_cache_pos(
    screen_col: u16, screen_row: u16,
    pane: ratatui::layout::Rect, scroll: usize, cache_len: usize,
) -> Option<(usize, usize)> {
    // Content sits inside the 1px border on all sides
    let cx = pane.x + 1;
    let cy = pane.y + 1;
    let ch = pane.height.saturating_sub(2) as usize;
    if screen_col < cx || screen_row < cy { return None; }
    let vrow = (screen_row - cy) as usize;
    let col = (screen_col - cx) as usize;
    if vrow >= ch { return None; }
    let line = scroll + vrow;
    if line >= cache_len { return None; }
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
    if screen_row < cy || screen_col < cx { return None; }

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
            let row_end = if wrap_seg + 1 < breaks.len() { breaks[wrap_seg + 1] } else { len };
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
    if inner_width == 0 { return 0; }
    let target_row = (click_row.saturating_sub(inner_y)) as usize;
    let target_col = (click_col.saturating_sub(inner_x)) as usize;
    let visible_rows = app.input_area.height.saturating_sub(2) as usize;
    let cursor_row_current = compute_cursor_row_fast(&app.input, app.input_cursor, inner_width);
    let scroll_offset = if visible_rows > 0 && cursor_row_current >= visible_rows {
        cursor_row_current - visible_rows + 1
    } else { 0 };
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
    if inner_width == 0 { return; }
    let target_col = (click_col.saturating_sub(inner_x)) as usize;
    let target_row = (click_row.saturating_sub(inner_y)) as usize;

    // Scroll offset so we map screen row → absolute visual row
    let visible_rows = app.input_area.height.saturating_sub(2) as usize;
    let cursor_row_current = compute_cursor_row_fast(&app.input, app.input_cursor, inner_width);
    let scroll_offset = if visible_rows > 0 && cursor_row_current >= visible_rows {
        cursor_row_current - visible_rows + 1
    } else { 0 };
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
        if target >= prev && target < bp { return row; }
        row += 1;
        prev = bp;
    }
    row // cursor in final row
}

/// Map a visual (row, col) coordinate back to a char index in the input text.
/// Uses word-wrap break points so clicking and cursor math agree with rendering.
fn row_col_to_char_index(input: &str, target_row: usize, target_col: usize, inner_width: usize) -> usize {
    let chars: Vec<char> = input.chars().collect();
    if chars.is_empty() { return 0; }
    let breaks = word_wrap_break_points(&chars, inner_width);

    // Find the start and end char indices for the target row
    let mut row = 0usize;
    let mut prev = 0usize;
    let mut row_start = 0usize;
    let mut row_end = chars.len();
    let mut found = false;
    for &bp in &breaks {
        if row == target_row { row_start = prev; row_end = bp; found = true; break; }
        row += 1;
        prev = bp;
    }
    // If target_row is the last (or only) row
    if !found {
        if row == target_row { row_start = prev; row_end = chars.len(); }
        else { return chars.len(); } // clicked below content
    }

    // Skip trailing newline from row content (it's not a visible character)
    let content_end = if row_end > row_start && chars.get(row_end - 1) == Some(&'\n') {
        row_end - 1
    } else { row_end };

    // Walk chars in this row until display width reaches or passes target_col
    let mut col_accum = 0usize;
    for i in row_start..content_end {
        if col_accum >= target_col { return i; }
        col_accum += unicode_width::UnicodeWidthChar::width(chars[i]).unwrap_or(1);
    }
    content_end // click past row content → place at row end
}
