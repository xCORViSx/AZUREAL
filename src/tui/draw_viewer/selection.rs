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
    let sel_start = if visual_line_idx == sel_start_line { sel_start_col.max(gutter) } else { gutter };
    let sel_end = if visual_line_idx == sel_end_line { sel_end_col.max(gutter) } else { line_len };

    if sel_start >= sel_end || sel_end == 0 { return spans; }

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
    let sel_start = if line_idx == sel_start_line { sel_start_col } else { 0 };
    let sel_end = if line_idx == sel_end_line { sel_end_col } else { line_len };

    if sel_start >= sel_end { return spans; }

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
