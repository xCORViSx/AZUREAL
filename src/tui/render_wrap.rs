//! Text and span wrapping utilities for TUI rendering

use ratatui::{style::Style, text::Span};
use textwrap::{wrap, Options};

/// Wrap text to fit within max_width, returning wrapped lines.
/// Fast path: if text fits in max_width, returns single-element vec without calling textwrap.
pub fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    // Fast path: text fits in one line — skip textwrap entirely
    if text.chars().count() <= max_width && !text.contains('\n') {
        return vec![text.to_string()];
    }
    let opts = Options::new(max_width).break_words(true);
    wrap(text, opts)
        .into_iter()
        .map(|cow| cow.into_owned())
        .collect()
}

/// Wrap spans to fit within max_width, preserving styles
pub fn wrap_spans(spans: Vec<Span<'static>>, max_width: usize) -> Vec<Vec<Span<'static>>> {
    if max_width == 0 {
        return vec![spans];
    }

    let mut full_text = String::new();
    let mut style_ranges: Vec<(usize, usize, Style)> = Vec::new();

    for span in &spans {
        let start = full_text.len();
        full_text.push_str(&span.content);
        let end = full_text.len();
        style_ranges.push((start, end, span.style));
    }

    if full_text.is_empty() {
        return vec![vec![]];
    }

    let opts = Options::new(max_width).break_words(true);
    let wrapped_lines: Vec<String> = wrap(&full_text, opts)
        .into_iter()
        .map(|cow| cow.into_owned())
        .collect();

    let mut result: Vec<Vec<Span<'static>>> = Vec::new();
    let mut char_offset = 0;

    for wrapped in wrapped_lines {
        let line_start = char_offset;
        let line_end = char_offset + wrapped.len();
        let mut line_spans: Vec<Span<'static>> = Vec::new();

        for &(range_start, range_end, style) in &style_ranges {
            if range_end <= line_start || range_start >= line_end {
                continue;
            }

            let overlap_start = range_start.max(line_start);
            let overlap_end = range_end.min(line_end);

            if overlap_start < overlap_end {
                let local_start = overlap_start - line_start;
                let local_end = overlap_end - line_start;
                let text: String = wrapped
                    .chars()
                    .skip(local_start)
                    .take(local_end - local_start)
                    .collect();
                if !text.is_empty() {
                    line_spans.push(Span::styled(text, style));
                }
            }
        }

        result.push(line_spans);
        char_offset = line_end;
        if char_offset < full_text.len() {
            char_offset += 1;
        }
    }

    if result.is_empty() {
        result.push(vec![]);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier};

    // ─── Helpers ──────────────────────────────────────────────────────
    fn s(text: &str, style: Style) -> Span<'static> {
        Span::styled(text.to_string(), style)
    }
    fn plain() -> Style {
        Style::default()
    }
    fn red() -> Style {
        Style::default().fg(Color::Red)
    }
    fn blue() -> Style {
        Style::default().fg(Color::Blue)
    }
    fn bold() -> Style {
        Style::default().add_modifier(Modifier::BOLD)
    }

    fn _texts_from(lines: &[Vec<Span<'static>>]) -> Vec<Vec<String>> {
        lines
            .iter()
            .map(|line| line.iter().map(|span| span.content.to_string()).collect())
            .collect()
    }

    // ═══════════════════════════════════════════════════════════════════
    // wrap_text — Empty / trivial
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn wrap_empty_string() {
        assert_eq!(wrap_text("", 80), vec![""]);
    }

    #[test]
    fn wrap_short_fits() {
        assert_eq!(wrap_text("hello", 80), vec!["hello"]);
    }

    #[test]
    fn wrap_exact_fit() {
        assert_eq!(wrap_text("hello", 5), vec!["hello"]);
    }

    #[test]
    fn wrap_one_char_under() {
        assert_eq!(wrap_text("hell", 5), vec!["hell"]);
    }

    #[test]
    fn wrap_single_char() {
        assert_eq!(wrap_text("a", 1), vec!["a"]);
    }

    #[test]
    fn wrap_single_char_wide() {
        assert_eq!(wrap_text("a", 100), vec!["a"]);
    }

    // ═══════════════════════════════════════════════════════════════════
    // wrap_text — Wrapping needed
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn wrap_two_words() {
        let result = wrap_text("hello world", 5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "hello");
        assert_eq!(result[1], "world");
    }

    #[test]
    fn wrap_three_words() {
        let result = wrap_text("aa bb cc", 5);
        assert!(result.len() >= 2);
    }

    #[test]
    fn wrap_long_word_break() {
        // break_words is true, so a long word gets broken
        let result = wrap_text("abcdefghij", 5);
        assert!(result.len() >= 2);
        // Each piece should be at most 5 chars
        for line in &result {
            assert!(line.chars().count() <= 5);
        }
    }

    #[test]
    fn wrap_width_1() {
        let result = wrap_text("abc", 1);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], "a");
        assert_eq!(result[1], "b");
        assert_eq!(result[2], "c");
    }

    #[test]
    fn wrap_width_2() {
        let result = wrap_text("abcd", 2);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn wrap_width_3_multi_word() {
        let result = wrap_text("ab cd", 3);
        assert!(result.len() >= 2);
    }

    // ═══════════════════════════════════════════════════════════════════
    // wrap_text — Newlines
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn wrap_with_newline() {
        let result = wrap_text("hello\nworld", 80);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "hello");
        assert_eq!(result[1], "world");
    }

    #[test]
    fn wrap_multiple_newlines() {
        let result = wrap_text("a\nb\nc", 80);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn wrap_newline_and_wrapping() {
        let result = wrap_text("hello world\nfoo bar", 5);
        assert!(result.len() >= 4);
    }

    #[test]
    fn wrap_trailing_newline() {
        let result = wrap_text("hello\n", 80);
        // textwrap treats trailing newline as a separate (empty) line
        assert!(result.len() >= 1);
    }

    // ═══════════════════════════════════════════════════════════════════
    // wrap_text — Various lengths
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn wrap_sentence() {
        let result = wrap_text("the quick brown fox jumps over the lazy dog", 20);
        assert!(result.len() >= 2);
        for line in &result {
            assert!(line.chars().count() <= 20);
        }
    }

    #[test]
    fn wrap_many_short_words() {
        let text = "a b c d e f g h i j k l m n o p";
        let result = wrap_text(text, 10);
        assert!(result.len() >= 2);
    }

    #[test]
    fn wrap_very_long_single_word() {
        let word = "a".repeat(100);
        let result = wrap_text(&word, 10);
        assert_eq!(result.len(), 10);
        for line in &result {
            assert!(line.chars().count() <= 10);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // wrap_text — Unicode
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn wrap_unicode_fits() {
        assert_eq!(wrap_text("日本語", 10), vec!["日本語"]);
    }

    #[test]
    fn wrap_unicode_needs_wrap() {
        let text = "日本語テキスト";
        let result = wrap_text(text, 3);
        assert!(result.len() >= 2);
    }

    #[test]
    fn wrap_emoji() {
        let result = wrap_text("🎉🎊🎈", 10);
        assert!(!result.is_empty());
    }

    #[test]
    fn wrap_mixed_ascii_unicode() {
        let result = wrap_text("hello 世界 world", 8);
        assert!(result.len() >= 2);
    }

    // ═══════════════════════════════════════════════════════════════════
    // wrap_text — Edge cases
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn wrap_only_spaces() {
        let result = wrap_text("     ", 3);
        // Spaces are whitespace; textwrap may collapse or split them
        assert!(!result.is_empty());
    }

    #[test]
    fn wrap_large_width() {
        let text = "short";
        assert_eq!(wrap_text(text, 1000), vec!["short"]);
    }

    // ═══════════════════════════════════════════════════════════════════
    // wrap_spans — Empty / trivial
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn spans_empty_vec() {
        let result = wrap_spans(vec![], 80);
        assert_eq!(result.len(), 1);
        assert!(result[0].is_empty());
    }

    #[test]
    fn spans_single_fits() {
        let spans = vec![s("hello", plain())];
        let result = wrap_spans(spans, 80);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 1);
        assert_eq!(result[0][0].content.as_ref(), "hello");
    }

    #[test]
    fn spans_single_exact_fit() {
        let spans = vec![s("hello", plain())];
        let result = wrap_spans(spans, 5);
        assert_eq!(result.len(), 1);
    }

    // ═══════════════════════════════════════════════════════════════════
    // wrap_spans — Wrapping
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn spans_single_wraps() {
        let spans = vec![s("hello world", plain())];
        let result = wrap_spans(spans, 5);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn spans_two_words_two_spans() {
        let spans = vec![s("hello ", red()), s("world", blue())];
        let result = wrap_spans(spans, 5);
        assert!(result.len() >= 2);
    }

    #[test]
    fn spans_style_preserved_on_first_line() {
        let spans = vec![s("hi", red()), s(" there", blue())];
        let result = wrap_spans(spans, 80);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0][0].style, red());
        assert_eq!(result[0][1].style, blue());
    }

    #[test]
    fn spans_style_preserved_after_wrap() {
        let spans = vec![s("hello ", red()), s("world", blue())];
        let result = wrap_spans(spans, 6);
        // "hello " fits in 6, "world" goes to next line
        if result.len() == 2 {
            // Second line should have blue style
            assert!(result[1].iter().any(|sp| sp.style == blue()));
        }
    }

    #[test]
    fn spans_long_single_span_wraps() {
        let spans = vec![s("abcdefghij", red())];
        let result = wrap_spans(spans, 5);
        assert!(result.len() >= 2);
        // All spans should be red
        for line in &result {
            for sp in line {
                assert_eq!(sp.style, red());
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // wrap_spans — Zero width
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn spans_zero_width_returns_input() {
        let spans = vec![s("hello", plain())];
        let result = wrap_spans(spans.clone(), 0);
        assert_eq!(result.len(), 1);
    }

    // ═══════════════════════════════════════════════════════════════════
    // wrap_spans — Narrow widths (1-3)
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn spans_width_1() {
        let spans = vec![s("abc", plain())];
        let result = wrap_spans(spans, 1);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn spans_width_2() {
        let spans = vec![s("abcd", plain())];
        let result = wrap_spans(spans, 2);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn spans_width_3() {
        let spans = vec![s("abcdef", plain())];
        let result = wrap_spans(spans, 3);
        assert_eq!(result.len(), 2);
    }

    // ═══════════════════════════════════════════════════════════════════
    // wrap_spans — Each char in its own span
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn spans_each_char_own_span() {
        let spans = vec![s("a", red()), s("b", blue()), s("c", bold())];
        let result = wrap_spans(spans, 80);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 3);
    }

    #[test]
    fn spans_each_char_own_span_wraps() {
        let spans = vec![s("a", red()), s("b", blue()), s("c", bold()), s("d", red())];
        let result = wrap_spans(spans, 2);
        assert!(result.len() >= 2);
    }

    // ═══════════════════════════════════════════════════════════════════
    // wrap_spans — Multiple spans, various scenarios
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn spans_all_in_one_span() {
        let spans = vec![s("the quick brown fox", plain())];
        let result = wrap_spans(spans, 10);
        assert!(result.len() >= 2);
    }

    #[test]
    fn spans_three_spans_wrap() {
        let spans = vec![
            s("hello ", red()),
            s("beautiful ", blue()),
            s("world", bold()),
        ];
        let result = wrap_spans(spans, 10);
        assert!(result.len() >= 2);
    }

    #[test]
    fn spans_empty_span_content() {
        let spans = vec![s("", plain()), s("hello", red())];
        let result = wrap_spans(spans, 80);
        // Empty span contributes nothing; only "hello" matters
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn spans_all_empty() {
        let spans = vec![s("", plain()), s("", red())];
        let result = wrap_spans(spans, 80);
        assert_eq!(result.len(), 1);
        assert!(result[0].is_empty());
    }

    #[test]
    fn spans_many_small_spans() {
        let spans: Vec<Span<'static>> = (0..20)
            .map(|i| s(&format!("w{} ", i), if i % 2 == 0 { red() } else { blue() }))
            .collect();
        let result = wrap_spans(spans, 15);
        assert!(result.len() >= 2);
    }

    #[test]
    fn spans_preserves_line_count_with_newlines_in_text() {
        // wrap_spans delegates to textwrap which handles newlines
        let spans = vec![s("line1\nline2", plain())];
        let result = wrap_spans(spans, 80);
        assert!(result.len() >= 2);
    }

    // ═══════════════════════════════════════════════════════════════════
    // wrap_spans — Style correctness
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn spans_style_split_across_lines() {
        // One long red span that wraps — both lines should be red
        let spans = vec![s("hello world", red())];
        let result = wrap_spans(spans, 6);
        for line in &result {
            for sp in line {
                assert_eq!(sp.style, red());
            }
        }
    }

    #[test]
    fn spans_different_styles_on_different_lines() {
        let spans = vec![s("aaa ", red()), s("bbb", blue())];
        let result = wrap_spans(spans, 4);
        // "aaa " fits in 4, "bbb" goes to next line
        if result.len() == 2 && !result[0].is_empty() && !result[1].is_empty() {
            assert_eq!(result[0][0].style, red());
            assert_eq!(result[1][0].style, blue());
        }
    }

    #[test]
    fn spans_bold_style_preserved() {
        let spans = vec![s("bold text", bold())];
        let result = wrap_spans(spans, 5);
        for line in &result {
            for sp in line {
                assert_eq!(sp.style, bold());
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // wrap_text — Whitespace patterns
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn wrap_consecutive_spaces_between_words() {
        // Multiple spaces between words — textwrap normalizes or preserves them
        let result = wrap_text("hello    world", 80);
        assert!(!result.is_empty());
        // Regardless of how textwrap handles consecutive spaces, the result
        // should contain both words
        let joined: String = result.join(" ");
        assert!(joined.contains("hello"));
        assert!(joined.contains("world"));
    }

    #[test]
    fn wrap_tab_characters() {
        // Tabs count as characters; textwrap handles them
        let result = wrap_text("a\tb\tc", 80);
        assert!(!result.is_empty());
        let joined: String = result.join("");
        assert!(joined.contains('a'));
        assert!(joined.contains('b'));
    }

    // ═══════════════════════════════════════════════════════════════════
    // wrap_spans — Unicode in styled spans
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn spans_unicode_styled_wrap() {
        // CJK characters in a styled span that requires wrapping
        let spans = vec![s("日本語テキスト", red())];
        let result = wrap_spans(spans, 4);
        assert!(result.len() >= 2);
        // All fragments should retain the red style
        for line in &result {
            for sp in line {
                assert_eq!(sp.style, red());
            }
        }
    }
}
