//! Text wrapping utilities for the viewer panel
//!
//! Word-boundary wrapping for plain text and styled spans, preserving
//! syntax highlighting across wrap boundaries.

use ratatui::{
    style::Style,
    text::Span,
};
use textwrap::{wrap, Options};

/// Wrap plain text to a maximum width, breaking words if necessary
pub(super) fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if text.is_empty() { return vec![String::new()]; }
    let opts = Options::new(max_width).break_words(true);
    wrap(text, opts).into_iter().map(|cow| cow.into_owned()).collect()
}

/// Compute word-boundary wrap break positions for a single line. Returns a
/// Vec of char offsets where each visual row starts (first is always 0).
/// Uses textwrap for word boundaries, falls back to hard breaks for long words.
/// Used by both display wrapping and cursor/scroll math.
pub(crate) fn word_wrap_breaks(text: &str, max_width: usize) -> Vec<usize> {
    if max_width == 0 || text.is_empty() { return vec![0]; }
    let char_count = text.chars().count();
    if char_count <= max_width { return vec![0]; }
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
        if next_char == Some(' ') { offset += 1; }
    }
    breaks
}

/// Word-boundary wrapping for styled spans. Uses textwrap to find break
/// positions, then slices the styled spans at those positions. Preserves
/// syntax highlighting across wrap boundaries.
pub(super) fn wrap_spans_word(spans: Vec<Span<'static>>, max_width: usize) -> Vec<Vec<Span<'static>>> {
    if max_width == 0 { return vec![spans]; }
    // Flatten to (char, style) pairs and plain text for textwrap
    let mut chars_styled: Vec<(char, Style)> = Vec::new();
    let mut plain = String::new();
    for span in &spans {
        for c in span.content.chars() {
            chars_styled.push((c, span.style));
            plain.push(c);
        }
    }
    if chars_styled.is_empty() { return vec![vec![]]; }
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
                    if !buf.is_empty() { line_spans.push(Span::styled(std::mem::take(&mut buf), cur_style)); }
                    buf.push(c);
                    cur_style = style;
                }
            }
            if !buf.is_empty() { line_spans.push(Span::styled(buf, cur_style)); }
        }
        result.push(line_spans);
    }
    if result.is_empty() { result.push(vec![]); }
    result
}
