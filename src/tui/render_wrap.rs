//! Text and span wrapping utilities for TUI rendering

use ratatui::{style::Style, text::Span};
use textwrap::{wrap, Options};

/// Wrap text to fit within max_width, returning wrapped lines.
/// Fast path: if text fits in max_width, returns single-element vec without calling textwrap.
pub fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if text.is_empty() { return vec![String::new()]; }
    // Fast path: text fits in one line — skip textwrap entirely
    if text.chars().count() <= max_width && !text.contains('\n') {
        return vec![text.to_string()];
    }
    let opts = Options::new(max_width).break_words(true);
    wrap(text, opts).into_iter().map(|cow| cow.into_owned()).collect()
}

/// Wrap spans to fit within max_width, preserving styles
pub fn wrap_spans(spans: Vec<Span<'static>>, max_width: usize) -> Vec<Vec<Span<'static>>> {
    if max_width == 0 { return vec![spans]; }

    let mut full_text = String::new();
    let mut style_ranges: Vec<(usize, usize, Style)> = Vec::new();

    for span in &spans {
        let start = full_text.len();
        full_text.push_str(&span.content);
        let end = full_text.len();
        style_ranges.push((start, end, span.style));
    }

    if full_text.is_empty() { return vec![vec![]]; }

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
            if range_end <= line_start || range_start >= line_end { continue; }

            let overlap_start = range_start.max(line_start);
            let overlap_end = range_end.min(line_end);

            if overlap_start < overlap_end {
                let local_start = overlap_start - line_start;
                let local_end = overlap_end - line_start;
                let text: String = wrapped.chars().skip(local_start).take(local_end - local_start).collect();
                if !text.is_empty() {
                    line_spans.push(Span::styled(text, style));
                }
            }
        }

        result.push(line_spans);
        char_offset = line_end;
        if char_offset < full_text.len() { char_offset += 1; }
    }

    if result.is_empty() { result.push(vec![]); }
    result
}
