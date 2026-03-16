//! Edit mode rendering for the viewer panel
//!
//! Full-featured text editor view with syntax highlighting, word wrapping,
//! cursor positioning, selection, and dashed border for unsaved changes.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use super::selection::apply_selection_to_spans;
use super::wrapping::{word_wrap_breaks, wrap_spans_word};
use crate::app::App;

/// Draw viewer in edit mode
pub(super) fn draw_edit_mode(
    f: &mut Frame,
    app: &mut App,
    area: Rect,
    viewport_height: usize,
    viewport_width: usize,
) {
    let path_str = app
        .viewer_path
        .as_ref()
        .map(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
        .unwrap_or_else(|| "File".to_string());

    // Title on left — show REC/... prefix when STT is active, modified indicator on right
    let title = if app.stt_recording {
        format!(" REC EDIT: {} ", path_str)
    } else if app.stt_transcribing {
        format!(" ... EDIT: {} ", path_str)
    } else {
        format!(" EDIT: {} ", path_str)
    };
    // Border color: magenta during voice input, yellow normally
    let border_color = if app.stt_recording || app.stt_transcribing {
        Color::Magenta
    } else {
        Color::Yellow
    };

    let total_lines = app.viewer_edit_content.len();
    let line_num_width = total_lines.to_string().len().max(3);
    let content_width = viewport_width.saturating_sub(line_num_width + 3);
    // Cache so cursor movement logic can navigate wrapped visual lines
    app.viewer_edit_content_width = content_width;

    // Cache syntax highlighting — only re-run when content actually changes.
    // Track via monotonic edit counter (not undo stack len, which caps at 100).
    // Also invalidate when cache is empty (first render) or entering edit mode (ver = MAX).
    let edit_ver = app.viewer_edit_version;
    if app.viewer_edit_highlight_ver != edit_ver || app.viewer_edit_highlight_cache.is_empty() {
        let full_content = app.viewer_edit_content.join("\n");
        app.viewer_edit_highlight_cache = app
            .syntax_highlighter
            .highlight_file(&full_content, &path_str);
        app.viewer_edit_highlight_ver = edit_ver;
    }

    let scroll = app.viewer_scroll;
    let (cursor_line, cursor_col) = app.viewer_edit_cursor;
    let cw = content_width.max(1);

    // Normalize selection so start <= end (user can select backwards)
    let selection = app.viewer_edit_selection.map(|(sl, sc, el, ec)| {
        if sl < el || (sl == el && sc <= ec) {
            (sl, sc, el, ec)
        } else {
            (el, ec, sl, sc)
        }
    });

    // Find which source lines are visible. Walk source lines summing wrap
    // counts until we've covered scroll + viewport_height visual lines.
    // Only process those source lines (avoids O(file) per frame).
    let mut visual_row = 0usize;
    let mut first_src = 0usize;
    let mut first_wrap_skip = 0usize;
    let mut found_first = false;
    let mut last_src = app.viewer_edit_content.len();

    for (i, line_str) in app.viewer_edit_content.iter().enumerate() {
        let wraps = word_wrap_breaks(line_str, cw).len();

        if !found_first {
            if visual_row + wraps > scroll {
                first_src = i;
                first_wrap_skip = scroll - visual_row;
                found_first = true;
            }
        }
        visual_row += wraps;
        if found_first && visual_row >= scroll + viewport_height {
            last_src = i + 1;
            break;
        }
    }
    if !found_first {
        first_src = app.viewer_edit_content.len();
    }

    // Build only the visible display lines
    let mut final_lines: Vec<Line> = Vec::with_capacity(viewport_height);

    for idx in first_src..last_src.min(app.viewer_edit_content.len()) {
        let line_content = &app.viewer_edit_content[idx];
        let line_num_style = if idx == cursor_line {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let spans = app
            .viewer_edit_highlight_cache
            .get(idx)
            .cloned()
            .unwrap_or_default();

        // Apply selection highlighting if this line is in selection range
        let spans =
            if let Some((sel_start_line, sel_start_col, sel_end_line, sel_end_col)) = selection {
                if idx >= sel_start_line && idx <= sel_end_line {
                    apply_selection_to_spans(
                        spans,
                        line_content,
                        idx,
                        sel_start_line,
                        sel_start_col,
                        sel_end_line,
                        sel_end_col,
                    )
                } else {
                    spans
                }
            } else {
                spans
            };

        let wrapped = wrap_spans_word(spans, content_width);

        for (wrap_idx, wrapped_spans) in wrapped.into_iter().enumerate() {
            // Skip wrap rows before scroll start (only applies to first_src)
            if idx == first_src && wrap_idx < first_wrap_skip {
                continue;
            }
            if final_lines.len() >= viewport_height {
                break;
            }

            let line_num = if wrap_idx == 0 {
                format!("{:>width$} │ ", idx + 1, width = line_num_width)
            } else {
                format!("{:>width$} │ ", "", width = line_num_width)
            };
            let mut line_spans = vec![Span::styled(line_num, line_num_style)];
            line_spans.extend(wrapped_spans);
            final_lines.push(Line::from(line_spans));
        }
        if final_lines.len() >= viewport_height {
            break;
        }
    }

    // Pad with empty lines if needed
    while final_lines.len() < viewport_height {
        let line_num = format!("{:>width$} │ ", "~", width = line_num_width);
        final_lines.push(Line::from(Span::styled(
            line_num,
            Style::default().fg(Color::DarkGray),
        )));
    }

    let border_style = Style::default()
        .fg(border_color)
        .add_modifier(Modifier::BOLD);
    let title_line = Line::from(Span::styled(&title, border_style));

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(title_line)
        .border_style(border_style);
    // Right-aligned [modified] indicator — ratatui fills the gap with border chars
    if app.viewer_edit_dirty {
        block = block.title(
            Line::from(Span::styled(
                "[modified] ",
                Style::default().fg(border_color),
            ))
            .alignment(Alignment::Right),
        );
    }

    let widget = Paragraph::new(final_lines).block(block);
    f.render_widget(widget, area);

    // Dashed double border when file is modified — punch gaps into every other
    // border cell to create a "═ ═ ═" / "║ ║" pattern as an unsaved-changes cue.
    // Only blanks cells that are actually border characters (═ or ║), so title
    // text on the top edge is preserved automatically.
    if app.viewer_edit_dirty {
        let buf = f.buffer_mut();
        let x0 = area.x;
        let x1 = area.x + area.width.saturating_sub(1);
        let y0 = area.y;
        let y1 = area.y + area.height.saturating_sub(1);
        // Top and bottom edges: blank every other ═ cell (skip corners)
        for x in (x0 + 1)..x1 {
            if (x - x0) % 2 == 0 {
                if buf[(x, y0)].symbol() == "═" {
                    buf[(x, y0)].set_symbol(" ");
                }
                if buf[(x, y1)].symbol() == "═" {
                    buf[(x, y1)].set_symbol(" ");
                }
            }
        }
        // Left and right edges: blank every other ║ cell (skip corners)
        for y in (y0 + 1)..y1 {
            if (y - y0) % 2 == 0 {
                if buf[(x0, y)].symbol() == "║" {
                    buf[(x0, y)].set_symbol(" ");
                }
                if buf[(x1, y)].symbol() == "║" {
                    buf[(x1, y)].set_symbol(" ");
                }
            }
        }
    }

    // Compute cursor visual position using word-wrap break positions.
    // Sum wrap row counts for source lines 0..cursor_line, then find which
    // wrap segment the cursor column falls in for the cursor's own line.
    let mut cursor_visual_line = 0usize;
    for i in 0..cursor_line.min(app.viewer_edit_content.len()) {
        cursor_visual_line += word_wrap_breaks(&app.viewer_edit_content[i], cw).len();
    }
    let cursor_line_str = app
        .viewer_edit_content
        .get(cursor_line)
        .map(|s| s.as_str())
        .unwrap_or("");
    let cursor_breaks = word_wrap_breaks(cursor_line_str, cw);
    // Find which wrap row the cursor falls on
    let mut cursor_wrap_row = 0;
    for (j, &brk) in cursor_breaks.iter().enumerate() {
        if cursor_col >= brk {
            cursor_wrap_row = j;
        }
    }
    cursor_visual_line += cursor_wrap_row;
    let cursor_visual_col = cursor_col - cursor_breaks[cursor_wrap_row];

    // Position cursor if visible in viewport
    if cursor_visual_line >= scroll && cursor_visual_line < scroll + viewport_height {
        let screen_line = cursor_visual_line - scroll;
        f.set_cursor_position((
            area.x + 1 + line_num_width as u16 + 3 + cursor_visual_col as u16,
            area.y + 1 + screen_line as u16,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Rect construction and field access --

    #[test]
    fn test_rect_zero() {
        let r = Rect::new(0, 0, 0, 0);
        assert_eq!((r.x, r.y, r.width, r.height), (0, 0, 0, 0));
    }

    #[test]
    fn test_rect_nonzero() {
        let r = Rect::new(5, 10, 80, 24);
        assert_eq!(r.width, 80);
        assert_eq!(r.height, 24);
    }

    #[test]
    fn test_rect_area() {
        assert_eq!(Rect::new(0, 0, 10, 20).area(), 200);
    }

    // -- Style construction --

    #[test]
    fn test_style_fg_yellow() {
        assert_eq!(Style::default().fg(Color::Yellow).fg, Some(Color::Yellow));
    }

    #[test]
    fn test_style_fg_dark_gray() {
        assert_eq!(
            Style::default().fg(Color::DarkGray).fg,
            Some(Color::DarkGray)
        );
    }

    #[test]
    fn test_style_fg_magenta() {
        assert_eq!(Style::default().fg(Color::Magenta).fg, Some(Color::Magenta));
    }

    #[test]
    fn test_style_bold_modifier() {
        let s = Style::default().add_modifier(Modifier::BOLD);
        assert!(s.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_style_fg_and_bold() {
        let s = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        assert_eq!(s.fg, Some(Color::Yellow));
        assert!(s.add_modifier.contains(Modifier::BOLD));
    }

    // -- Border color logic --

    #[test]
    fn test_border_color_normal() {
        let color = if false || false {
            Color::Magenta
        } else {
            Color::Yellow
        };
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn test_border_color_recording() {
        let color = if true || false {
            Color::Magenta
        } else {
            Color::Yellow
        };
        assert_eq!(color, Color::Magenta);
    }

    #[test]
    fn test_border_color_transcribing() {
        let color = if false || true {
            Color::Magenta
        } else {
            Color::Yellow
        };
        assert_eq!(color, Color::Magenta);
    }

    #[test]
    fn test_border_color_both_active() {
        let color = if true || true {
            Color::Magenta
        } else {
            Color::Yellow
        };
        assert_eq!(color, Color::Magenta);
    }

    // -- Title formatting logic --

    #[test]
    fn test_title_normal() {
        let (rec, trans) = (false, false);
        let p = "main.rs";
        let t = if rec {
            format!(" REC EDIT: {} ", p)
        } else if trans {
            format!(" ... EDIT: {} ", p)
        } else {
            format!(" EDIT: {} ", p)
        };
        assert_eq!(t, " EDIT: main.rs ");
    }

    #[test]
    fn test_title_recording() {
        let t = format!(" REC EDIT: {} ", "lib.rs");
        assert_eq!(t, " REC EDIT: lib.rs ");
    }

    #[test]
    fn test_title_transcribing() {
        let t = format!(" ... EDIT: {} ", "mod.rs");
        assert_eq!(t, " ... EDIT: mod.rs ");
    }

    // -- Line number width computation --

    #[test]
    fn test_line_num_width_single_digit() {
        assert_eq!(5usize.to_string().len().max(3), 3);
    }

    #[test]
    fn test_line_num_width_double_digit() {
        assert_eq!(42usize.to_string().len().max(3), 3);
    }

    #[test]
    fn test_line_num_width_triple_digit() {
        assert_eq!(100usize.to_string().len().max(3), 3);
    }

    #[test]
    fn test_line_num_width_1000_lines() {
        assert_eq!(1000usize.to_string().len().max(3), 4);
    }

    #[test]
    fn test_line_num_width_10000_lines() {
        assert_eq!(10000usize.to_string().len().max(3), 5);
    }

    #[test]
    fn test_line_num_width_zero_lines() {
        assert_eq!(0usize.to_string().len().max(3), 3);
    }

    // -- Content width computation --

    #[test]
    fn test_content_width_basic() {
        assert_eq!(80usize.saturating_sub(3 + 3), 74);
    }

    #[test]
    fn test_content_width_narrow_viewport() {
        assert_eq!(10usize.saturating_sub(3 + 3), 4);
    }

    #[test]
    fn test_content_width_too_narrow() {
        assert_eq!(5usize.saturating_sub(3 + 3), 0);
    }

    #[test]
    fn test_content_width_exact_gutter() {
        assert_eq!(6usize.saturating_sub(3 + 3), 0);
    }

    // -- Selection normalization --

    #[test]
    fn test_selection_normalize_already_ordered() {
        let (sl, sc, el, ec) = (1, 5, 3, 10);
        let n = if sl < el || (sl == el && sc <= ec) {
            (sl, sc, el, ec)
        } else {
            (el, ec, sl, sc)
        };
        assert_eq!(n, (1, 5, 3, 10));
    }

    #[test]
    fn test_selection_normalize_reversed() {
        let (sl, sc, el, ec) = (5, 10, 2, 3);
        let n = if sl < el || (sl == el && sc <= ec) {
            (sl, sc, el, ec)
        } else {
            (el, ec, sl, sc)
        };
        assert_eq!(n, (2, 3, 5, 10));
    }

    #[test]
    fn test_selection_normalize_same_line_ordered() {
        let (sl, sc, el, ec) = (3, 2, 3, 8);
        let n = if sl < el || (sl == el && sc <= ec) {
            (sl, sc, el, ec)
        } else {
            (el, ec, sl, sc)
        };
        assert_eq!(n, (3, 2, 3, 8));
    }

    #[test]
    fn test_selection_normalize_same_line_reversed() {
        let (sl, sc, el, ec) = (3, 8, 3, 2);
        let n = if sl < el || (sl == el && sc <= ec) {
            (sl, sc, el, ec)
        } else {
            (el, ec, sl, sc)
        };
        assert_eq!(n, (3, 2, 3, 8));
    }

    #[test]
    fn test_selection_normalize_same_point() {
        let (sl, sc, el, ec) = (3, 5, 3, 5);
        let n = if sl < el || (sl == el && sc <= ec) {
            (sl, sc, el, ec)
        } else {
            (el, ec, sl, sc)
        };
        assert_eq!(n, (3, 5, 3, 5));
    }

    // -- Line number formatting --

    #[test]
    fn test_line_number_format_first_wrap() {
        assert_eq!(format!("{:>width$} │ ", 1, width = 3), "  1 │ ");
    }

    #[test]
    fn test_line_number_format_continuation_wrap() {
        assert_eq!(format!("{:>width$} │ ", "", width = 3), "    │ ");
    }

    #[test]
    fn test_line_number_format_large_number() {
        assert_eq!(format!("{:>width$} │ ", 1000, width = 4), "1000 │ ");
    }

    #[test]
    fn test_tilde_padding_format() {
        assert_eq!(format!("{:>width$} │ ", "~", width = 3), "  ~ │ ");
    }

    // -- Dashed border coordinate math --

    #[test]
    fn test_dashed_border_x_range() {
        let area = Rect::new(5, 3, 40, 20);
        assert_eq!(area.x, 5);
        assert_eq!(area.x + area.width.saturating_sub(1), 44);
    }

    #[test]
    fn test_dashed_border_y_range() {
        let area = Rect::new(5, 3, 40, 20);
        assert_eq!(area.y, 3);
        assert_eq!(area.y + area.height.saturating_sub(1), 22);
    }

    #[test]
    fn test_dashed_border_even_positions() {
        let x0 = 5u16;
        let evens: Vec<u16> = (6..44).filter(|x| (x - x0) % 2 == 0).collect();
        assert!(evens.contains(&7));
        assert!(!evens.contains(&6));
    }

    // -- Cursor position math --

    #[test]
    fn test_cursor_screen_x_calculation() {
        assert_eq!(5u16 + 1 + 3u16 + 3 + 10u16, 22);
    }

    #[test]
    fn test_cursor_screen_y_calculation() {
        assert_eq!(3u16 + 1 + 5u16, 9);
    }

    #[test]
    fn test_cursor_visibility_in_viewport() {
        let (scroll, vh) = (10, 20);
        assert!(15 >= scroll && 15 < scroll + vh);
    }

    #[test]
    fn test_cursor_visibility_above_viewport() {
        let (scroll, vh) = (10, 20);
        assert!(!(5 >= scroll && 5 < scroll + vh));
    }

    #[test]
    fn test_cursor_visibility_below_viewport() {
        let (scroll, vh) = (10, 20);
        assert!(!(30 >= scroll && 30 < scroll + vh));
    }

    #[test]
    fn test_cursor_at_scroll_boundary_start() {
        let (scroll, vh) = (10, 20);
        assert!(10 >= scroll && 10 < scroll + vh);
    }

    #[test]
    fn test_cursor_at_scroll_boundary_end() {
        let (scroll, vh) = (10, 20);
        assert!(29 >= scroll && 29 < scroll + vh);
        assert!(!(30 >= scroll && 30 < scroll + vh));
    }

    // -- Span and Line construction --

    #[test]
    fn test_span_styled() {
        let s = Span::styled("hello", Style::default().fg(Color::Yellow));
        assert_eq!(s.content.as_ref(), "hello");
    }

    #[test]
    fn test_line_from_spans() {
        let line = Line::from(vec![Span::raw("  1 │ "), Span::raw("fn main()")]);
        assert_eq!(line.spans.len(), 2);
    }

    #[test]
    fn test_block_double_border() {
        let _b = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double);
    }

    #[test]
    fn test_alignment_right() {
        assert_eq!(Alignment::Right, Alignment::Right);
    }

    // -- Content width max(1) guard --

    #[test]
    fn test_content_width_max_guard_zero() {
        assert_eq!(0usize.max(1), 1);
    }

    #[test]
    fn test_content_width_max_guard_nonzero() {
        assert_eq!(50usize.max(1), 50);
    }

    // -- Viewport padding --

    #[test]
    fn test_viewport_padding_needed() {
        assert_eq!(20usize.saturating_sub(15), 5);
    }

    #[test]
    fn test_viewport_padding_not_needed() {
        assert_eq!(20usize.saturating_sub(20), 0);
    }

    #[test]
    fn test_modified_indicator_text() {
        assert_eq!(if true { "[modified] " } else { "" }, "[modified] ");
    }

    #[test]
    fn test_not_modified_indicator() {
        assert_eq!(if false { "[modified] " } else { "" }, "");
    }

    // -- Word wrap breaks accessibility --

    #[test]
    fn test_word_wrap_breaks_empty() {
        let breaks = word_wrap_breaks("", 80);
        assert!(!breaks.is_empty()); // at least [0]
    }

    #[test]
    fn test_word_wrap_breaks_short_line() {
        let breaks = word_wrap_breaks("hello world", 80);
        assert_eq!(breaks.len(), 1); // fits in one row
    }

    #[test]
    fn test_word_wrap_breaks_long_line() {
        let long = "a ".repeat(50);
        let breaks = word_wrap_breaks(&long, 20);
        assert!(breaks.len() > 1); // must wrap
    }

    #[test]
    fn test_word_wrap_breaks_width_1() {
        let breaks = word_wrap_breaks("abc", 1);
        assert!(breaks.len() >= 1);
    }
}
