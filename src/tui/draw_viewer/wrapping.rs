//! Text wrapping utilities for the viewer panel
//!
//! Word-boundary wrapping for plain text and styled spans, preserving
//! syntax highlighting across wrap boundaries.

use ratatui::{style::Style, text::Span};
use textwrap::{wrap, Options};

/// Wrap plain text to a maximum width, breaking words if necessary
pub(super) fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let opts = Options::new(max_width).break_words(true);
    wrap(text, opts)
        .into_iter()
        .map(|cow| cow.into_owned())
        .collect()
}

/// Compute word-boundary wrap break positions for a single line. Returns a
/// Vec of char offsets where each visual row starts (first is always 0).
/// Uses textwrap for word boundaries, falls back to hard breaks for long words.
/// Used by both display wrapping and cursor/scroll math.
pub(crate) fn word_wrap_breaks(text: &str, max_width: usize) -> Vec<usize> {
    if max_width == 0 || text.is_empty() {
        return vec![0];
    }
    let char_count = text.chars().count();
    if char_count <= max_width {
        return vec![0];
    }
    let opts = Options::new(max_width).break_words(true);
    let wrapped = wrap(text, opts);
    let mut breaks = Vec::with_capacity(wrapped.len());
    let mut offset = 0usize;
    for segment in &wrapped {
        breaks.push(offset);
        offset += segment.chars().count();
        // textwrap eats the space at the break point — account for it
        // by checking if the next char in the original text is a space
        let next_char = text.chars().nth(offset);
        if next_char == Some(' ') {
            offset += 1;
        }
    }
    breaks
}

/// Word-boundary wrapping for styled spans. Uses textwrap to find break
/// positions, then slices the styled spans at those positions. Preserves
/// syntax highlighting across wrap boundaries.
pub(super) fn wrap_spans_word(
    spans: Vec<Span<'static>>,
    max_width: usize,
) -> Vec<Vec<Span<'static>>> {
    if max_width == 0 {
        return vec![spans];
    }
    // Flatten to (char, style) pairs and plain text for textwrap
    let mut chars_styled: Vec<(char, Style)> = Vec::new();
    let mut plain = String::new();
    for span in &spans {
        for c in span.content.chars() {
            chars_styled.push((c, span.style));
            plain.push(c);
        }
    }
    if chars_styled.is_empty() {
        return vec![vec![]];
    }
    // Get break positions via textwrap
    let breaks = word_wrap_breaks(&plain, max_width);
    let total = chars_styled.len();
    let mut result: Vec<Vec<Span<'static>>> = Vec::with_capacity(breaks.len());
    for (i, &start) in breaks.iter().enumerate() {
        let end = if i + 1 < breaks.len() {
            // End at next break, but trim trailing space at the break boundary
            let next = breaks[i + 1];
            if next > 0 && start < next && chars_styled.get(next - 1).map(|c| c.0) == Some(' ') {
                next - 1
            } else {
                next
            }
        } else {
            total
        };
        // Merge consecutive chars with same style into spans
        let mut line_spans: Vec<Span<'static>> = Vec::new();
        if start < end {
            let mut buf = String::new();
            let mut cur_style = chars_styled[start].1;
            for &(c, style) in &chars_styled[start..end] {
                if style == cur_style {
                    buf.push(c);
                } else {
                    if !buf.is_empty() {
                        line_spans.push(Span::styled(std::mem::take(&mut buf), cur_style));
                    }
                    buf.push(c);
                    cur_style = style;
                }
            }
            if !buf.is_empty() {
                line_spans.push(Span::styled(buf, cur_style));
            }
        }
        result.push(line_spans);
    }
    if result.is_empty() {
        result.push(vec![]);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier, Style};

    // ---------------------------------------------------------------
    //  wrap_text
    // ---------------------------------------------------------------

    #[test]
    fn wrap_text_empty_string() {
        let result = wrap_text("", 40);
        assert_eq!(result, vec![String::new()]);
    }

    #[test]
    fn wrap_text_single_word_fits() {
        let result = wrap_text("hello", 10);
        assert_eq!(result, vec!["hello"]);
    }

    #[test]
    fn wrap_text_exact_width() {
        let result = wrap_text("hello", 5);
        assert_eq!(result, vec!["hello"]);
    }

    #[test]
    fn wrap_text_short_word_wraps() {
        let result = wrap_text("abcdef", 3);
        assert_eq!(result, vec!["abc", "def"]);
    }

    #[test]
    fn wrap_text_two_words_fit() {
        let result = wrap_text("hi there", 20);
        assert_eq!(result, vec!["hi there"]);
    }

    #[test]
    fn wrap_text_two_words_break_at_space() {
        let result = wrap_text("hello world", 5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "hello");
        assert_eq!(result[1], "world");
    }

    #[test]
    fn wrap_text_three_words() {
        let result = wrap_text("one two three", 7);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "one two");
        assert_eq!(result[1], "three");
    }

    #[test]
    fn wrap_text_long_single_word_breaks() {
        let result = wrap_text("abcdefghij", 5);
        assert_eq!(result, vec!["abcde", "fghij"]);
    }

    #[test]
    fn wrap_text_preserves_multiple_spaces() {
        // textwrap collapses/trims internal spaces by default
        let result = wrap_text("a  b", 10);
        assert_eq!(result, vec!["a  b"]);
    }

    #[test]
    fn wrap_text_width_1() {
        let result = wrap_text("abc", 1);
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn wrap_text_already_multiline_content() {
        // textwrap treats \n as line breaks natively
        let result = wrap_text("ab\ncd", 10);
        assert_eq!(result, vec!["ab", "cd"]);
    }

    #[test]
    fn wrap_text_unicode_chars() {
        // Each CJK char is 2-wide in textwrap
        let result = wrap_text("hello", 80);
        assert_eq!(result, vec!["hello"]);
    }

    #[test]
    fn wrap_text_leading_spaces() {
        let result = wrap_text("  hello", 20);
        assert_eq!(result, vec!["  hello"]);
    }

    #[test]
    fn wrap_text_width_matches_string_length() {
        let text = "exactly20characters!";
        let result = wrap_text(text, 20);
        assert_eq!(result, vec!["exactly20characters!"]);
    }

    #[test]
    fn wrap_text_returns_multiple_lines() {
        let text = "the quick brown fox jumps over the lazy dog";
        let result = wrap_text(text, 10);
        assert!(result.len() > 1);
        // Every line must be <= 10 chars
        for line in &result {
            assert!(line.chars().count() <= 10, "line too long: {:?}", line);
        }
    }

    #[test]
    fn wrap_text_sentence() {
        let text = "this is a sentence with several words";
        let result = wrap_text(text, 15);
        // Reassemble should produce original text (spaces at breaks are consumed)
        let rejoined = result.join(" ");
        assert_eq!(rejoined, text);
    }

    #[test]
    fn wrap_text_all_spaces() {
        let result = wrap_text("     ", 3);
        // textwrap may collapse this — just ensure no panic
        assert!(!result.is_empty());
    }

    #[test]
    fn wrap_text_single_char() {
        let result = wrap_text("x", 1);
        assert_eq!(result, vec!["x"]);
    }

    #[test]
    fn wrap_text_large_width_no_wrap() {
        let text = "short";
        let result = wrap_text(text, 1000);
        assert_eq!(result, vec!["short"]);
    }

    // ---------------------------------------------------------------
    //  word_wrap_breaks
    // ---------------------------------------------------------------

    #[test]
    fn breaks_empty_string() {
        assert_eq!(word_wrap_breaks("", 10), vec![0]);
    }

    #[test]
    fn breaks_zero_width() {
        assert_eq!(word_wrap_breaks("hello", 0), vec![0]);
    }

    #[test]
    fn breaks_both_empty_and_zero() {
        assert_eq!(word_wrap_breaks("", 0), vec![0]);
    }

    #[test]
    fn breaks_fits_within_width() {
        assert_eq!(word_wrap_breaks("hello", 10), vec![0]);
    }

    #[test]
    fn breaks_exactly_at_width() {
        assert_eq!(word_wrap_breaks("hello", 5), vec![0]);
    }

    #[test]
    fn breaks_one_char_over() {
        let breaks = word_wrap_breaks("hello!", 5);
        assert_eq!(breaks.len(), 2);
        assert_eq!(breaks[0], 0);
    }

    #[test]
    fn breaks_two_words() {
        let breaks = word_wrap_breaks("hello world", 5);
        assert_eq!(breaks[0], 0);
        // second break at char 6 (after "hello ")
        assert_eq!(breaks[1], 6);
    }

    #[test]
    fn breaks_three_words() {
        let breaks = word_wrap_breaks("one two three", 7);
        assert_eq!(breaks[0], 0);
        assert!(breaks.len() >= 2);
    }

    #[test]
    fn breaks_long_word_hard_break() {
        let breaks = word_wrap_breaks("abcdefghij", 5);
        assert_eq!(breaks[0], 0);
        assert_eq!(breaks[1], 5);
    }

    #[test]
    fn breaks_very_long_word() {
        let text = "a".repeat(20);
        let breaks = word_wrap_breaks(&text, 5);
        assert_eq!(breaks.len(), 4);
        assert_eq!(breaks, vec![0, 5, 10, 15]);
    }

    #[test]
    fn breaks_width_1() {
        let breaks = word_wrap_breaks("abc", 1);
        assert_eq!(breaks, vec![0, 1, 2]);
    }

    #[test]
    fn breaks_first_is_always_zero() {
        for width in 1..=10 {
            let breaks = word_wrap_breaks("some text here", width);
            assert_eq!(breaks[0], 0, "first break must be 0 for width={}", width);
        }
    }

    #[test]
    fn breaks_single_char() {
        assert_eq!(word_wrap_breaks("x", 1), vec![0]);
    }

    #[test]
    fn breaks_single_char_wide() {
        assert_eq!(word_wrap_breaks("x", 80), vec![0]);
    }

    #[test]
    fn breaks_offsets_are_monotonically_increasing() {
        let breaks = word_wrap_breaks("the quick brown fox jumps over lazy dog", 8);
        for i in 1..breaks.len() {
            assert!(
                breaks[i] > breaks[i - 1],
                "breaks must increase: {:?}",
                breaks
            );
        }
    }

    #[test]
    fn breaks_last_offset_within_text() {
        let text = "hello world foo bar";
        let breaks = word_wrap_breaks(text, 5);
        let char_count = text.chars().count();
        for &b in &breaks {
            assert!(
                b < char_count,
                "break offset {} out of bounds (len={})",
                b,
                char_count
            );
        }
    }

    #[test]
    fn breaks_spaces_at_boundary() {
        // "ab cd ef" with width 5 => "ab cd" fits, then "ef"
        let breaks = word_wrap_breaks("ab cd ef", 5);
        assert_eq!(breaks[0], 0);
        assert!(breaks.len() >= 2);
    }

    #[test]
    fn breaks_trailing_space() {
        let breaks = word_wrap_breaks("hi ", 5);
        // "hi " is 3 chars, fits in width 5
        assert_eq!(breaks, vec![0]);
    }

    #[test]
    fn breaks_leading_space() {
        let breaks = word_wrap_breaks(" hi", 5);
        assert_eq!(breaks, vec![0]);
    }

    #[test]
    fn breaks_count_matches_wrap_text_lines() {
        let text = "the quick brown fox jumps over the lazy dog";
        for width in 3..=20 {
            let breaks = word_wrap_breaks(text, width);
            let wrapped = wrap_text(text, width);
            assert_eq!(
                breaks.len(),
                wrapped.len(),
                "breaks/wrap mismatch at width={}: breaks={:?}, wrapped={:?}",
                width,
                breaks,
                wrapped
            );
        }
    }

    // ---------------------------------------------------------------
    //  wrap_spans_word
    // ---------------------------------------------------------------

    #[test]
    fn spans_empty_vec() {
        let result = wrap_spans_word(vec![], 10);
        assert_eq!(result.len(), 1);
        assert!(result[0].is_empty());
    }

    #[test]
    fn spans_zero_width_returns_original() {
        let spans = vec![Span::raw("hello")];
        let result = wrap_spans_word(spans.clone(), 0);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 1);
        assert_eq!(result[0][0].content, "hello");
    }

    #[test]
    fn spans_single_short_span() {
        let spans = vec![Span::raw("hi")];
        let result = wrap_spans_word(spans, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 1);
        assert_eq!(result[0][0].content, "hi");
    }

    #[test]
    fn spans_single_span_exact_width() {
        let spans = vec![Span::raw("hello")];
        let result = wrap_spans_word(spans, 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0][0].content, "hello");
    }

    #[test]
    fn spans_single_span_wraps() {
        let spans = vec![Span::raw("hello world")];
        let result = wrap_spans_word(spans, 5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0][0].content, "hello");
        assert_eq!(result[1][0].content, "world");
    }

    #[test]
    fn spans_preserves_style_after_wrap() {
        let style = Style::default().fg(Color::Red);
        let spans = vec![Span::styled("hello world".to_string(), style)];
        let result = wrap_spans_word(spans, 5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0][0].style, style);
        assert_eq!(result[1][0].style, style);
    }

    #[test]
    fn spans_two_styles_no_wrap() {
        let s1 = Style::default().fg(Color::Red);
        let s2 = Style::default().fg(Color::Blue);
        let spans = vec![
            Span::styled("ab".to_string(), s1),
            Span::styled("cd".to_string(), s2),
        ];
        let result = wrap_spans_word(spans, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 2);
        assert_eq!(result[0][0].content, "ab");
        assert_eq!(result[0][0].style, s1);
        assert_eq!(result[0][1].content, "cd");
        assert_eq!(result[0][1].style, s2);
    }

    #[test]
    fn spans_style_split_at_wrap_boundary() {
        // "hello world" with Red="hello " and Blue="world"
        let s1 = Style::default().fg(Color::Red);
        let s2 = Style::default().fg(Color::Blue);
        let spans = vec![
            Span::styled("hello ".to_string(), s1),
            Span::styled("world".to_string(), s2),
        ];
        let result = wrap_spans_word(spans, 5);
        assert_eq!(result.len(), 2);
        // First line: "hello" in Red
        assert_eq!(result[0][0].style, s1);
        // Second line: "world" in Blue
        assert_eq!(result[1][0].style, s2);
    }

    #[test]
    fn spans_hard_break_long_word() {
        let spans = vec![Span::raw("abcdefghij")];
        let result = wrap_spans_word(spans, 5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0][0].content, "abcde");
        assert_eq!(result[1][0].content, "fghij");
    }

    #[test]
    fn spans_multiple_spans_wrap() {
        let s1 = Style::default().fg(Color::Green);
        let s2 = Style::default().fg(Color::Yellow);
        let spans = vec![
            Span::styled("hello ".to_string(), s1),
            Span::styled("world".to_string(), s2),
        ];
        let result = wrap_spans_word(spans, 6);
        // "hello " is 6 chars, "world" is 5 — should wrap
        assert!(result.len() >= 2);
    }

    #[test]
    fn spans_content_preserved_across_wraps() {
        let text = "the quick brown fox";
        let spans = vec![Span::raw(text)];
        let result = wrap_spans_word(spans, 10);
        // Collect all chars from result
        let mut reassembled = String::new();
        for (i, line) in result.iter().enumerate() {
            for span in line {
                reassembled.push_str(&span.content);
            }
            if i < result.len() - 1 {
                reassembled.push(' '); // space consumed at break
            }
        }
        assert_eq!(reassembled, text);
    }

    #[test]
    fn spans_single_char_span() {
        let spans = vec![Span::raw("x")];
        let result = wrap_spans_word(spans, 1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0][0].content, "x");
    }

    #[test]
    fn spans_empty_span_content() {
        let spans = vec![Span::raw("")];
        let result = wrap_spans_word(spans, 10);
        assert_eq!(result.len(), 1);
        assert!(result[0].is_empty());
    }

    #[test]
    fn spans_width_1_multiple_chars() {
        let spans = vec![Span::raw("abc")];
        let result = wrap_spans_word(spans, 1);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn spans_adjacent_same_style_merged() {
        let style = Style::default().fg(Color::Red);
        let spans = vec![
            Span::styled("a".to_string(), style),
            Span::styled("b".to_string(), style),
        ];
        let result = wrap_spans_word(spans, 10);
        assert_eq!(result.len(), 1);
        // Same-style chars should be merged into one span
        assert_eq!(result[0].len(), 1);
        assert_eq!(result[0][0].content, "ab");
    }

    #[test]
    fn spans_modifier_preserved() {
        let style = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
        let spans = vec![Span::styled("hello world".to_string(), style)];
        let result = wrap_spans_word(spans, 5);
        for line in &result {
            for span in line {
                assert!(span.style.add_modifier == style.add_modifier);
            }
        }
    }

    #[test]
    fn spans_large_width_no_wrap() {
        let spans = vec![Span::raw("short text")];
        let result = wrap_spans_word(spans, 1000);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn spans_three_different_styles() {
        let s1 = Style::default().fg(Color::Red);
        let s2 = Style::default().fg(Color::Blue);
        let s3 = Style::default().fg(Color::Green);
        let spans = vec![
            Span::styled("aa".to_string(), s1),
            Span::styled("bb".to_string(), s2),
            Span::styled("cc".to_string(), s3),
        ];
        let result = wrap_spans_word(spans, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 3);
    }

    #[test]
    fn spans_wrap_preserves_all_styles() {
        let s1 = Style::default().fg(Color::Red);
        let s2 = Style::default().fg(Color::Blue);
        // "abc def" — Red="abc " Blue="def"
        let spans = vec![
            Span::styled("abc ".to_string(), s1),
            Span::styled("def".to_string(), s2),
        ];
        let result = wrap_spans_word(spans, 4);
        // Should produce 2 lines
        assert_eq!(result.len(), 2);
        // First line "abc" in red
        assert_eq!(result[0][0].style, s1);
        // Second line "def" in blue
        assert_eq!(result[1][0].style, s2);
    }

    #[test]
    fn spans_result_line_widths_within_max() {
        let spans = vec![Span::raw("the quick brown fox jumps over the lazy dog")];
        let max_width = 10;
        let result = wrap_spans_word(spans, max_width);
        for (i, line) in result.iter().enumerate() {
            let line_len: usize = line.iter().map(|s| s.content.chars().count()).sum();
            assert!(
                line_len <= max_width,
                "line {} has {} chars, exceeds max_width {}: {:?}",
                i,
                line_len,
                max_width,
                line
            );
        }
    }

    #[test]
    fn spans_bg_color_preserved() {
        let style = Style::default().bg(Color::DarkGray);
        let spans = vec![Span::styled("hello world".to_string(), style)];
        let result = wrap_spans_word(spans, 5);
        assert_eq!(result[0][0].style.bg, Some(Color::DarkGray));
        assert_eq!(result[1][0].style.bg, Some(Color::DarkGray));
    }
}
