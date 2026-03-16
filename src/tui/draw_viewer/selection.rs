//! Selection highlighting for the viewer panel
//!
//! Applies visual selection (mouse drag) highlighting to styled spans,
//! splitting spans at selection boundaries and patching background color.

use ratatui::{
    style::{Color, Style},
    text::Span,
};

/// Apply selection highlighting to a line based on visual line indices.
/// `gutter` skips that many leading chars (line number column) from highlighting.
pub(crate) fn apply_selection_to_line(
    spans: Vec<Span<'static>>,
    line_content: &str,
    visual_line_idx: usize,
    sel_start_line: usize,
    sel_start_col: usize,
    sel_end_line: usize,
    sel_end_col: usize,
    gutter: usize,
) -> Vec<Span<'static>> {
    let line_len = line_content.chars().count();
    let sel_start = if visual_line_idx == sel_start_line {
        sel_start_col.max(gutter)
    } else {
        gutter
    };
    let sel_end = if visual_line_idx == sel_end_line {
        sel_end_col.max(gutter)
    } else {
        line_len
    };

    if sel_start >= sel_end || sel_end == 0 {
        return spans;
    }

    let selection_style = Style::default().bg(Color::Rgb(60, 60, 100));
    let mut result: Vec<Span<'static>> = Vec::new();
    let mut char_pos = 0;

    for span in spans {
        let span_len = span.content.chars().count();
        let span_end = char_pos + span_len;

        if span_end <= sel_start || char_pos >= sel_end {
            result.push(span);
        } else {
            let chars: Vec<char> = span.content.chars().collect();
            if char_pos < sel_start {
                let before: String = chars[..(sel_start - char_pos)].iter().collect();
                result.push(Span::styled(before, span.style));
            }
            let sel_in_span_start = sel_start.saturating_sub(char_pos);
            let sel_in_span_end = (sel_end - char_pos).min(span_len);
            if sel_in_span_start < sel_in_span_end {
                let selected: String = chars[sel_in_span_start..sel_in_span_end].iter().collect();
                result.push(Span::styled(selected, span.style.patch(selection_style)));
            }
            if span_end > sel_end {
                let after: String = chars[(sel_end - char_pos)..].iter().collect();
                result.push(Span::styled(after, span.style));
            }
        }
        char_pos = span_end;
    }
    result
}

/// Apply selection highlighting to spans for a given line (edit mode variant — no gutter skip)
pub(super) fn apply_selection_to_spans(
    spans: Vec<Span<'static>>,
    line_content: &str,
    line_idx: usize,
    sel_start_line: usize,
    sel_start_col: usize,
    sel_end_line: usize,
    sel_end_col: usize,
) -> Vec<Span<'static>> {
    let line_len = line_content.chars().count();
    let sel_start = if line_idx == sel_start_line {
        sel_start_col
    } else {
        0
    };
    let sel_end = if line_idx == sel_end_line {
        sel_end_col
    } else {
        line_len
    };

    if sel_start >= sel_end {
        return spans;
    }

    let selection_style = Style::default().bg(Color::Rgb(60, 60, 100));

    let mut result: Vec<Span<'static>> = Vec::new();
    let mut char_pos = 0;

    for span in spans {
        let span_len = span.content.chars().count();
        let span_end = char_pos + span_len;

        if span_end <= sel_start || char_pos >= sel_end {
            result.push(span);
        } else {
            let chars: Vec<char> = span.content.chars().collect();

            if char_pos < sel_start {
                let before: String = chars[..(sel_start - char_pos)].iter().collect();
                result.push(Span::styled(before, span.style));
            }

            let sel_in_span_start = sel_start.saturating_sub(char_pos);
            let sel_in_span_end = (sel_end - char_pos).min(span_len);
            if sel_in_span_start < sel_in_span_end {
                let selected: String = chars[sel_in_span_start..sel_in_span_end].iter().collect();
                result.push(Span::styled(selected, span.style.patch(selection_style)));
            }

            if span_end > sel_end {
                let after: String = chars[(sel_end - char_pos)..].iter().collect();
                result.push(Span::styled(after, span.style));
            }
        }

        char_pos = span_end;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The selection highlight style used in production code
    fn sel_style() -> Style {
        Style::default().bg(Color::Rgb(60, 60, 100))
    }

    /// Helper: collect all text from a Vec<Span>
    fn text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    // ---------------------------------------------------------------
    //  apply_selection_to_spans  (no gutter)
    // ---------------------------------------------------------------

    #[test]
    fn spans_no_selection_same_line_start_ge_end() {
        let spans = vec![Span::raw("hello")];
        let result = apply_selection_to_spans(spans.clone(), "hello", 0, 0, 3, 0, 2);
        // sel_start(3) >= sel_end(2) => early return
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "hello");
    }

    #[test]
    fn spans_no_selection_when_start_col_beyond_end_col() {
        // To truly bypass selection, sel_start must >= sel_end.
        // This happens when line_idx == sel_start_line and sel_start_col >= sel_end_col
        let result = apply_selection_to_spans(
            vec![Span::raw("hello")],
            "hello",
            0, // line_idx == sel_start_line == sel_end_line
            0,
            10, // sel_start_col = 10 (beyond end)
            0,
            5, // sel_end_col = 5
        );
        // sel_start = 10, sel_end = 5 → 10 >= 5 → early return
        assert_eq!(text(&result), "hello");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn spans_full_line_selected() {
        let spans = vec![Span::raw("hello")];
        let result = apply_selection_to_spans(spans, "hello", 0, 0, 0, 0, 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "hello");
        assert_eq!(result[0].style, Style::default().patch(sel_style()));
    }

    #[test]
    fn spans_partial_start_selected() {
        let spans = vec![Span::raw("hello")];
        let result = apply_selection_to_spans(spans, "hello", 0, 0, 2, 0, 5);
        // sel_start=2, sel_end=5 → "he" unselected, "llo" selected
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "he");
        assert_eq!(result[0].style, Style::default());
        assert_eq!(result[1].content, "llo");
        assert_eq!(result[1].style, Style::default().patch(sel_style()));
    }

    #[test]
    fn spans_partial_end_selected() {
        let spans = vec![Span::raw("hello")];
        let result = apply_selection_to_spans(spans, "hello", 0, 0, 0, 0, 3);
        // sel_start=0, sel_end=3 → "hel" selected, "lo" unselected
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "hel");
        assert_eq!(result[0].style, Style::default().patch(sel_style()));
        assert_eq!(result[1].content, "lo");
        assert_eq!(result[1].style, Style::default());
    }

    #[test]
    fn spans_middle_selected() {
        let spans = vec![Span::raw("hello")];
        let result = apply_selection_to_spans(spans, "hello", 0, 0, 1, 0, 4);
        // "h" before, "ell" selected, "o" after
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].content, "h");
        assert_eq!(result[1].content, "ell");
        assert_eq!(result[1].style, Style::default().patch(sel_style()));
        assert_eq!(result[2].content, "o");
    }

    #[test]
    fn spans_selection_preserves_text() {
        let spans = vec![Span::raw("hello world")];
        let result = apply_selection_to_spans(spans, "hello world", 0, 0, 2, 0, 8);
        let full = text(&result);
        assert_eq!(full, "hello world");
    }

    #[test]
    fn spans_multi_span_selection_across_boundary() {
        let s1 = Style::default().fg(Color::Red);
        let s2 = Style::default().fg(Color::Blue);
        let spans = vec![
            Span::styled("abc".to_string(), s1),
            Span::styled("def".to_string(), s2),
        ];
        // Select chars 2..4 → "c" from first span, "d" from second span
        let result = apply_selection_to_spans(spans, "abcdef", 0, 0, 2, 0, 4);
        assert_eq!(text(&result), "abcdef");
        // "ab" unselected Red, "c" selected Red, "d" selected Blue, "ef" unselected Blue
        assert_eq!(result.len(), 4);
        assert_eq!(result[0].content, "ab");
        assert_eq!(result[0].style, s1);
        assert_eq!(result[1].content, "c");
        assert_eq!(result[1].style, s1.patch(sel_style()));
        assert_eq!(result[2].content, "d");
        assert_eq!(result[2].style, s2.patch(sel_style()));
        assert_eq!(result[3].content, "ef");
        assert_eq!(result[3].style, s2);
    }

    #[test]
    fn spans_selection_entire_multi_span() {
        let s1 = Style::default().fg(Color::Red);
        let s2 = Style::default().fg(Color::Blue);
        let spans = vec![
            Span::styled("ab".to_string(), s1),
            Span::styled("cd".to_string(), s2),
        ];
        let result = apply_selection_to_spans(spans, "abcd", 0, 0, 0, 0, 4);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].style, s1.patch(sel_style()));
        assert_eq!(result[1].style, s2.patch(sel_style()));
    }

    #[test]
    fn spans_empty_span_vec() {
        let result = apply_selection_to_spans(vec![], "", 0, 0, 0, 0, 5);
        assert!(result.is_empty());
    }

    #[test]
    fn spans_sel_start_equals_sel_end() {
        let spans = vec![Span::raw("hello")];
        let result = apply_selection_to_spans(spans.clone(), "hello", 0, 0, 3, 0, 3);
        // sel_start == sel_end → no selection
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "hello");
    }

    #[test]
    fn spans_line_between_start_and_end_lines() {
        // Line in the middle of a multi-line selection gets fully selected
        let spans = vec![Span::raw("middle line")];
        let result = apply_selection_to_spans(spans, "middle line", 5, 2, 0, 8, 5);
        // line_idx=5 != sel_start_line=2 → sel_start=0
        // line_idx=5 != sel_end_line=8 → sel_end=line_len=11
        // Full line selected
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].style, Style::default().patch(sel_style()));
    }

    #[test]
    fn spans_start_line_partial() {
        let spans = vec![Span::raw("start line")];
        let result = apply_selection_to_spans(spans, "start line", 2, 2, 6, 5, 10);
        // line_idx=2 == sel_start_line → sel_start=6
        // line_idx=2 != sel_end_line=5 → sel_end=line_len=10
        // Chars 6..10 selected
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "start ");
        assert_eq!(result[1].content, "line");
        assert_eq!(result[1].style, Style::default().patch(sel_style()));
    }

    #[test]
    fn spans_end_line_partial() {
        let spans = vec![Span::raw("end line")];
        let result = apply_selection_to_spans(spans, "end line", 5, 2, 0, 5, 3);
        // line_idx=5 != sel_start_line=2 → sel_start=0
        // line_idx=5 == sel_end_line=5 → sel_end=3
        // Chars 0..3 selected
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "end");
        assert_eq!(result[0].style, Style::default().patch(sel_style()));
        assert_eq!(result[1].content, " line");
    }

    #[test]
    fn spans_single_char_selected() {
        let spans = vec![Span::raw("hello")];
        let result = apply_selection_to_spans(spans, "hello", 0, 0, 2, 0, 3);
        // Just char at index 2 ("l") selected
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].content, "he");
        assert_eq!(result[1].content, "l");
        assert_eq!(result[1].style, Style::default().patch(sel_style()));
        assert_eq!(result[2].content, "lo");
    }

    #[test]
    fn spans_selection_beyond_span_end() {
        let spans = vec![Span::raw("hi")];
        // sel_end=10 but span is only 2 chars
        let result = apply_selection_to_spans(spans, "hi", 0, 0, 0, 0, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "hi");
        assert_eq!(result[0].style, Style::default().patch(sel_style()));
    }

    #[test]
    fn spans_selection_before_span() {
        // Span entirely before the selection range
        let s1 = Style::default().fg(Color::Red);
        let spans = vec![Span::styled("ab".to_string(), s1), Span::raw("cdef")];
        let result = apply_selection_to_spans(spans, "abcdef", 0, 0, 4, 0, 6);
        // "ab" (0..2) entirely before sel_start(4), "cdef" (2..6) partially selected
        assert_eq!(result[0].content, "ab");
        assert_eq!(result[0].style, s1); // unchanged
    }

    #[test]
    fn spans_selection_after_span() {
        let s1 = Style::default().fg(Color::Red);
        let spans = vec![Span::raw("abcd"), Span::styled("ef".to_string(), s1)];
        let result = apply_selection_to_spans(spans, "abcdef", 0, 0, 0, 0, 2);
        // "abcd" (0..4) partially selected (0..2), "ef" (4..6) entirely after sel_end(2)
        assert_eq!(result.last().unwrap().content, "ef");
        assert_eq!(result.last().unwrap().style, s1); // unchanged
    }

    #[test]
    fn spans_three_spans_middle_selected() {
        let s1 = Style::default().fg(Color::Red);
        let s2 = Style::default().fg(Color::Blue);
        let s3 = Style::default().fg(Color::Green);
        let spans = vec![
            Span::styled("ab".to_string(), s1),
            Span::styled("cd".to_string(), s2),
            Span::styled("ef".to_string(), s3),
        ];
        let result = apply_selection_to_spans(spans, "abcdef", 0, 0, 2, 0, 4);
        // Only "cd" should be selected
        assert_eq!(text(&result), "abcdef");
        // Find the "cd" span
        let cd = result.iter().find(|s| s.content == "cd").unwrap();
        assert_eq!(cd.style, s2.patch(sel_style()));
        // "ab" and "ef" unchanged
        let ab = result.iter().find(|s| s.content == "ab").unwrap();
        assert_eq!(ab.style, s1);
        let ef = result.iter().find(|s| s.content == "ef").unwrap();
        assert_eq!(ef.style, s3);
    }

    #[test]
    fn spans_preserves_existing_bg_color() {
        let style = Style::default().fg(Color::Red).bg(Color::DarkGray);
        let spans = vec![Span::styled("hello".to_string(), style)];
        let result = apply_selection_to_spans(spans, "hello", 0, 0, 0, 0, 5);
        // Selection patches on top, so bg becomes the selection color
        assert_eq!(result[0].style, style.patch(sel_style()));
        assert_eq!(result[0].style.bg, Some(Color::Rgb(60, 60, 100)));
    }

    #[test]
    fn spans_unicode_content() {
        // Each emoji is 1 char
        let spans = vec![Span::raw("ab")];
        let result = apply_selection_to_spans(spans, "ab", 0, 0, 0, 0, 1);
        assert_eq!(result[0].content, "a");
        assert_eq!(result[0].style, Style::default().patch(sel_style()));
        assert_eq!(result[1].content, "b");
    }

    // ---------------------------------------------------------------
    //  apply_selection_to_line  (with gutter)
    // ---------------------------------------------------------------

    #[test]
    fn line_no_selection_when_start_ge_end() {
        let spans = vec![Span::raw("hello")];
        let result = apply_selection_to_line(spans.clone(), "hello", 0, 0, 4, 0, 3, 0);
        // sel_start(4) >= sel_end(3) → early return
        assert_eq!(result[0].content, "hello");
    }

    #[test]
    fn line_full_line_selected_no_gutter() {
        let spans = vec![Span::raw("hello")];
        let result = apply_selection_to_line(spans, "hello", 0, 0, 0, 0, 5, 0);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "hello");
        assert_eq!(result[0].style, Style::default().patch(sel_style()));
    }

    #[test]
    fn line_gutter_skips_leading_chars() {
        // With gutter=3, selection start is clamped to at least 3
        let spans = vec![Span::raw("123hello")];
        let result = apply_selection_to_line(spans, "123hello", 0, 0, 0, 0, 8, 3);
        // sel_start = max(0, 3) = 3, sel_end = 8
        // "123" unselected, "hello" selected
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "123");
        assert_eq!(result[0].style, Style::default());
        assert_eq!(result[1].content, "hello");
        assert_eq!(result[1].style, Style::default().patch(sel_style()));
    }

    #[test]
    fn line_gutter_clamps_sel_start_col() {
        let spans = vec![Span::raw("  5 | code here")];
        let content = "  5 | code here";
        let gutter = 6;
        // sel on line 0, col 2..15 → clamped to 6..15
        let result = apply_selection_to_line(spans, content, 0, 0, 2, 0, 15, gutter);
        assert_eq!(text(&result), content);
        // First 6 chars unselected
        assert_eq!(result[0].content, "  5 | ");
        assert_eq!(result[0].style, Style::default());
    }

    #[test]
    fn line_sel_end_zero_returns_unchanged() {
        let spans = vec![Span::raw("hello")];
        let result = apply_selection_to_line(spans.clone(), "hello", 0, 0, 0, 0, 0, 0);
        assert_eq!(result[0].content, "hello");
    }

    #[test]
    fn line_middle_line_fully_selected() {
        let spans = vec![Span::raw("mid line")];
        let result = apply_selection_to_line(spans, "mid line", 5, 2, 0, 8, 10, 0);
        // line 5 between start(2) and end(8):
        // sel_start = gutter(0), sel_end = line_len(8)
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].style, Style::default().patch(sel_style()));
    }

    #[test]
    fn line_start_line_with_gutter() {
        let spans = vec![Span::raw("  1 | hello world")];
        let content = "  1 | hello world";
        let gutter = 6;
        let result = apply_selection_to_line(
            spans, content, 3, // visual_line_idx
            3, 8, // sel_start_line, sel_start_col
            5, 20, // sel_end_line, sel_end_col
            gutter,
        );
        // line_idx=3 == sel_start_line → sel_start = max(8, 6) = 8
        // line_idx=3 != sel_end_line → sel_end = line_len(17)
        assert_eq!(text(&result), content);
    }

    #[test]
    fn line_end_line_with_gutter() {
        let spans = vec![Span::raw("  2 | abcdef")];
        let content = "  2 | abcdef";
        let gutter = 6;
        let result = apply_selection_to_line(
            spans, content, 5, // visual_line_idx
            3, 0, // sel_start_line, col
            5, 9, // sel_end_line, col
            gutter,
        );
        // line_idx=5 != sel_start_line → sel_start = gutter(6)
        // line_idx=5 == sel_end_line → sel_end = max(9, 6) = 9
        // Selection: chars 6..9
        assert_eq!(text(&result), content);
    }

    #[test]
    fn line_partial_selection_with_styled_spans() {
        let s1 = Style::default().fg(Color::DarkGray);
        let s2 = Style::default().fg(Color::White);
        let spans = vec![
            Span::styled("  1 | ".to_string(), s1),
            Span::styled("hello world".to_string(), s2),
        ];
        let content = "  1 | hello world";
        let gutter = 6;
        let result = apply_selection_to_line(spans, content, 0, 0, 6, 0, 11, gutter);
        // sel_start=6, sel_end=11 → selects "hello" (chars 6..11)
        assert_eq!(text(&result), content);
        // Gutter span unchanged
        assert_eq!(result[0].content, "  1 | ");
        assert_eq!(result[0].style, s1);
    }

    #[test]
    fn line_preserves_all_text_content() {
        let content = "  42 | fn main() { println!(\"hello\"); }";
        let spans = vec![Span::raw(content)];
        let result = apply_selection_to_line(spans, content, 0, 0, 5, 0, 20, 7);
        assert_eq!(text(&result), content);
    }

    #[test]
    fn line_gutter_larger_than_line() {
        let spans = vec![Span::raw("hi")];
        let result = apply_selection_to_line(spans.clone(), "hi", 0, 0, 0, 0, 2, 10);
        // sel_start = max(0, 10) = 10, sel_end = 2
        // 10 >= 2 → no selection
        assert_eq!(result[0].content, "hi");
    }

    #[test]
    fn line_selection_col_beyond_line_len() {
        let spans = vec![Span::raw("abc")];
        let result = apply_selection_to_line(spans, "abc", 0, 0, 0, 0, 100, 0);
        // sel_end = 100 but line_len = 3, doesn't matter — sel_end_col is used as-is
        // but the span is only 3 chars so sel_in_span_end = min(100-0, 3) = 3
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].style, Style::default().patch(sel_style()));
    }

    #[test]
    fn line_empty_content_no_crash() {
        let spans = vec![Span::raw("")];
        let result = apply_selection_to_line(spans.clone(), "", 0, 0, 0, 0, 0, 0);
        // sel_end = 0 → early return
        assert!(result.len() <= 1);
    }

    #[test]
    fn line_multi_span_gutter_preserved() {
        let gutter_style = Style::default().fg(Color::DarkGray);
        let code_style = Style::default().fg(Color::Yellow);
        let spans = vec![
            Span::styled("  1 | ".to_string(), gutter_style),
            Span::styled("let x = 42;".to_string(), code_style),
        ];
        let content = "  1 | let x = 42;";
        let result = apply_selection_to_line(spans, content, 0, 0, 0, 0, 17, 6);
        // Gutter (0..6) should not be selected; code (6..17) should be
        assert_eq!(result[0].content, "  1 | ");
        assert_eq!(result[0].style, gutter_style);
    }

    #[test]
    fn line_start_col_at_gutter_boundary() {
        let spans = vec![Span::raw("123abc")];
        let result = apply_selection_to_line(spans, "123abc", 0, 0, 3, 0, 6, 3);
        // sel_start = max(3, 3) = 3, sel_end = 6
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "123");
        assert_eq!(result[1].content, "abc");
        assert_eq!(result[1].style, Style::default().patch(sel_style()));
    }

    #[test]
    fn line_sel_end_col_at_gutter() {
        let spans = vec![Span::raw("123abc")];
        let result = apply_selection_to_line(spans, "123abc", 0, 0, 0, 0, 3, 3);
        // sel_start = max(0, 3) = 3, sel_end = max(3, 3) = 3
        // 3 >= 3 → no selection
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "123abc");
    }

    #[test]
    fn line_single_char_selection() {
        let spans = vec![Span::raw("abcde")];
        let result = apply_selection_to_line(spans, "abcde", 0, 0, 2, 0, 3, 0);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].content, "ab");
        assert_eq!(result[1].content, "c");
        assert_eq!(result[1].style, Style::default().patch(sel_style()));
        assert_eq!(result[2].content, "de");
    }

    #[test]
    fn line_selection_at_very_end() {
        let spans = vec![Span::raw("hello")];
        let result = apply_selection_to_line(spans, "hello", 0, 0, 4, 0, 5, 0);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "hell");
        assert_eq!(result[1].content, "o");
        assert_eq!(result[1].style, Style::default().patch(sel_style()));
    }

    #[test]
    fn line_selection_at_very_start() {
        let spans = vec![Span::raw("hello")];
        let result = apply_selection_to_line(spans, "hello", 0, 0, 0, 0, 1, 0);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "h");
        assert_eq!(result[0].style, Style::default().patch(sel_style()));
        assert_eq!(result[1].content, "ello");
    }

    #[test]
    fn spans_on_start_line_only() {
        let spans = vec![Span::raw("hello")];
        // Selection is on a single line (start == end line)
        let result = apply_selection_to_spans(spans, "hello", 0, 0, 1, 0, 4);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].content, "h");
        assert_eq!(result[1].content, "ell");
        assert_eq!(result[2].content, "o");
    }

    #[test]
    fn spans_many_small_spans() {
        let spans: Vec<Span<'static>> = "hello".chars().map(|c| Span::raw(c.to_string())).collect();
        let result = apply_selection_to_spans(spans, "hello", 0, 0, 1, 0, 4);
        assert_eq!(text(&result), "hello");
        // "h" unselected, "e","l","l" selected, "o" unselected
        assert_eq!(result[0].style, Style::default());
        assert_eq!(result[1].style, Style::default().patch(sel_style()));
        assert_eq!(result[2].style, Style::default().patch(sel_style()));
        assert_eq!(result[3].style, Style::default().patch(sel_style()));
        assert_eq!(result[4].style, Style::default());
    }

    #[test]
    fn line_many_small_spans_with_gutter() {
        let spans: Vec<Span<'static>> =
            "12|abc".chars().map(|c| Span::raw(c.to_string())).collect();
        let result = apply_selection_to_line(spans, "12|abc", 0, 0, 0, 0, 6, 3);
        assert_eq!(text(&result), "12|abc");
        // First 3 chars (gutter) unselected, rest selected
        assert_eq!(result[0].style, Style::default()); // '1'
        assert_eq!(result[1].style, Style::default()); // '2'
        assert_eq!(result[2].style, Style::default()); // '|'
        assert_eq!(result[3].style, Style::default().patch(sel_style())); // 'a'
        assert_eq!(result[4].style, Style::default().patch(sel_style())); // 'b'
        assert_eq!(result[5].style, Style::default().patch(sel_style())); // 'c'
    }

    #[test]
    fn spans_sel_covers_only_second_span() {
        let s1 = Style::default().fg(Color::Red);
        let s2 = Style::default().fg(Color::Blue);
        let spans = vec![
            Span::styled("aaa".to_string(), s1),
            Span::styled("bbb".to_string(), s2),
        ];
        let result = apply_selection_to_spans(spans, "aaabbb", 0, 0, 3, 0, 6);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "aaa");
        assert_eq!(result[0].style, s1);
        assert_eq!(result[1].content, "bbb");
        assert_eq!(result[1].style, s2.patch(sel_style()));
    }

    #[test]
    fn spans_sel_covers_only_first_span() {
        let s1 = Style::default().fg(Color::Red);
        let s2 = Style::default().fg(Color::Blue);
        let spans = vec![
            Span::styled("aaa".to_string(), s1),
            Span::styled("bbb".to_string(), s2),
        ];
        let result = apply_selection_to_spans(spans, "aaabbb", 0, 0, 0, 0, 3);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "aaa");
        assert_eq!(result[0].style, s1.patch(sel_style()));
        assert_eq!(result[1].content, "bbb");
        assert_eq!(result[1].style, s2);
    }

    #[test]
    fn spans_four_spans_select_middle_two() {
        let s1 = Style::default().fg(Color::Red);
        let s2 = Style::default().fg(Color::Green);
        let s3 = Style::default().fg(Color::Blue);
        let s4 = Style::default().fg(Color::Yellow);
        let spans = vec![
            Span::styled("aa".to_string(), s1),
            Span::styled("bb".to_string(), s2),
            Span::styled("cc".to_string(), s3),
            Span::styled("dd".to_string(), s4),
        ];
        let result = apply_selection_to_spans(spans, "aabbccdd", 0, 0, 2, 0, 6);
        assert_eq!(text(&result), "aabbccdd");
        // "aa" untouched, "bb" selected, "cc" selected, "dd" untouched
        assert_eq!(result[0].style, s1);
        assert_eq!(result[1].style, s2.patch(sel_style()));
        assert_eq!(result[2].style, s3.patch(sel_style()));
        assert_eq!(result[3].style, s4);
    }

    #[test]
    fn line_gutter_equals_line_len_no_selection() {
        let spans = vec![Span::raw("abc")];
        let result = apply_selection_to_line(spans.clone(), "abc", 0, 0, 0, 0, 3, 3);
        // gutter=3, line_len=3 → sel_start=3, sel_end=3 → no selection
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "abc");
        assert_eq!(result[0].style, Style::default());
    }

    #[test]
    fn spans_all_same_style_single_merge() {
        let style = Style::default().fg(Color::Magenta);
        let spans = vec![
            Span::styled("a".to_string(), style),
            Span::styled("b".to_string(), style),
            Span::styled("c".to_string(), style),
        ];
        let result = apply_selection_to_spans(spans, "abc", 0, 0, 0, 0, 3);
        // All selected with same base style — each span gets patched individually
        assert_eq!(text(&result), "abc");
        for s in &result {
            assert_eq!(s.style, style.patch(sel_style()));
        }
    }

    #[test]
    fn line_middle_selection_with_gutter_styled_span() {
        // Gutter span + code span, select only part of code
        let g = Style::default().fg(Color::DarkGray);
        let c = Style::default().fg(Color::White);
        let spans = vec![
            Span::styled("  1 | ".to_string(), g),
            Span::styled("abcdefgh".to_string(), c),
        ];
        let content = "  1 | abcdefgh";
        let result = apply_selection_to_line(spans, content, 0, 0, 8, 0, 12, 6);
        // sel_start = max(8, 6) = 8, sel_end = 12
        // Gutter "  1 | " (0..6) unchanged, "ab" (6..8) unchanged, "cdef" (8..12) selected, "gh" (12..14) unchanged
        assert_eq!(text(&result), content);
        assert_eq!(result[0].content, "  1 | ");
        assert_eq!(result[0].style, g);
    }

    #[test]
    fn empty_content_returns_empty_vec() {
        let spans: Vec<Span> = vec![];
        let result = apply_selection_to_spans(spans, "", 0, 0, 0, 0, 0);
        assert!(result.is_empty());
    }
}
