//! Chat bubble rendering — user/assistant message bubbles and completion banners

use ratatui::{
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::app::state::backend_for_model;
use crate::backend::Backend;
use crate::tui::colorize::ORANGE;
use crate::tui::render_wrap::wrap_text;
use crate::tui::util::{truncate, AZURE};

pub(super) fn assistant_identity(model: Option<&str>) -> (&'static str, Color) {
    match model.map(backend_for_model).unwrap_or(Backend::Claude) {
        Backend::Codex => ("Codex", Color::Cyan),
        Backend::Claude => ("Claude", ORANGE),
    }
}

pub(super) fn render_assistant_header_line(
    model: Option<&str>,
    bubble_width: usize,
) -> Line<'static> {
    let model = model.filter(|m| !m.is_empty());
    let (assistant_name, assistant_color) = assistant_identity(model);
    let header_style = Style::default()
        .fg(Color::Black)
        .bg(assistant_color)
        .add_modifier(Modifier::BOLD);
    let model_style = Style::default()
        .fg(Color::Rgb(60, 60, 60))
        .bg(assistant_color);
    let fill_style = Style::default().bg(assistant_color);

    let left = format!(" {} ▶ ", assistant_name);
    let right = model
        .map(|model| {
            let max_model_width = bubble_width.saturating_sub(left.chars().count() + 3).max(1);
            format!(" {} ", truncate(model, max_model_width))
        })
        .unwrap_or_default();
    let gap = bubble_width.saturating_sub(left.chars().count() + right.chars().count());

    let mut spans = vec![
        Span::styled(left, header_style),
        Span::styled(" ".repeat(gap), fill_style),
    ];
    if !right.is_empty() {
        spans.push(Span::styled(right, model_style));
    }

    Line::from(spans)
}

pub(super) fn render_user_message(
    lines: &mut Vec<Line<'static>>,
    content: &str,
    bubble_width: usize,
    total_width: usize,
) {
    lines.push(Line::from(""));
    lines.push(Line::from(""));

    let header = " You ◀ ".to_string();
    let header_pad = " ".repeat(bubble_width.saturating_sub(header.len()));
    let right_offset = total_width.saturating_sub(bubble_width);
    let offset_str = " ".repeat(right_offset);

    lines.push(Line::from(vec![
        Span::raw(offset_str.clone()),
        Span::styled(header_pad, Style::default().bg(AZURE)),
        Span::styled(
            header,
            Style::default()
                .fg(Color::Black)
                .bg(AZURE)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    let content_width = bubble_width.saturating_sub(2);
    for wrapped in wrap_text(content, content_width) {
        let pad = bubble_width.saturating_sub(wrapped.chars().count() + 2);
        lines.push(Line::from(vec![
            Span::raw(offset_str.clone()),
            Span::styled(" ".repeat(pad), Style::default()),
            Span::styled(wrapped, Style::default().fg(Color::White)),
            Span::styled(" │", Style::default().fg(AZURE)),
        ]));
    }

    lines.push(Line::from(vec![
        Span::raw(offset_str),
        Span::styled(
            format!("{}┘", "─".repeat(bubble_width - 1)),
            Style::default().fg(AZURE),
        ),
    ]));
}

pub(super) fn render_complete(
    lines: &mut Vec<Line<'static>>,
    duration_ms: u64,
    _cost_usd: f64,
    success: bool,
) {
    lines.push(Line::from(""));
    let (status, color) = if success {
        ("Completed", Color::Green)
    } else {
        ("Failed", Color::Red)
    };
    lines.push(
        Line::from(vec![
            Span::styled(
                format!(" ● {} ", status),
                Style::default()
                    .fg(Color::Black)
                    .bg(color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {:.1}s ", duration_ms as f64 / 1000.0),
                Style::default().fg(Color::White),
            ),
        ])
        .alignment(Alignment::Center),
    );
    lines.push(Line::from(""));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_to_text(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    // ── render_user_message tests ───────────────────────────────────────

    /// User message renders with "You" header.
    #[test]
    fn test_render_user_message_basic() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_user_message(&mut lines, "Hello Claude", 40, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("You")));
        assert!(text.iter().any(|l| l.contains("Hello Claude")));
    }

    /// User message renders bottom border.
    #[test]
    fn test_render_user_message_bottom_border() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_user_message(&mut lines, "test", 40, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains('┘')));
    }

    /// User message with empty content.
    #[test]
    fn test_render_user_message_empty() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_user_message(&mut lines, "", 40, 80);
        assert!(!lines.is_empty());
    }

    /// User message wraps long text.
    #[test]
    fn test_render_user_message_wraps() {
        let long = "A ".repeat(100);
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_user_message(&mut lines, &long, 40, 80);
        // Should have more lines due to wrapping
        assert!(lines.len() > 5);
    }

    /// User message with unicode.
    #[test]
    fn test_render_user_message_unicode() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_user_message(&mut lines, "こんにちは世界", 40, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("こんにちは世界")));
    }

    /// User message with newlines.
    #[test]
    fn test_render_user_message_newlines() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_user_message(&mut lines, "Line1\nLine2\nLine3", 40, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Line1")));
    }

    /// User message at minimum bubble width.
    #[test]
    fn test_render_user_message_min_bubble() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_user_message(&mut lines, "Hi", 5, 10);
        assert!(!lines.is_empty());
    }

    // ── render_complete tests ───────────────────────────────────────────

    /// Successful completion renders green Completed.
    #[test]
    fn test_render_complete_success() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_complete(&mut lines, 5000, 0.0123, true);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Completed")));
        assert!(text.iter().any(|l| l.contains("5.0s")));
    }

    /// Failed completion renders red Failed.
    #[test]
    fn test_render_complete_failure() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_complete(&mut lines, 1000, 0.05, false);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Failed")));
    }

    /// Zero duration.
    #[test]
    fn test_render_complete_zero_duration() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_complete(&mut lines, 0, 0.0, true);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("0.0s")));
    }

    /// Large duration in milliseconds.
    #[test]
    fn test_render_complete_large_duration() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_complete(&mut lines, 120000, 1.5, true);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("120.0s")));
    }

    /// Zero cost (cost not rendered, just ensure no panic).
    #[test]
    fn test_render_complete_zero_cost() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_complete(&mut lines, 100, 0.0, true);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Completed")));
    }

    /// Produces exactly 3 lines (empty, content, empty).
    #[test]
    fn test_render_complete_line_count() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_complete(&mut lines, 1000, 0.01, true);
        assert_eq!(lines.len(), 3);
    }

    // ── render_assistant_header_line tests ──────────────────────────────

    #[test]
    fn test_render_assistant_header_line_truncates_model_to_fit() {
        let line = render_assistant_header_line(Some("claude-opus-4-6-extra-long-model"), 16);
        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(text.chars().count(), 16);
        assert!(text.starts_with(" Claude ▶ "));
        assert!(text.ends_with(" cl… "));
    }

    #[test]
    fn test_render_assistant_header_line_model_span_is_subdued() {
        let line = render_assistant_header_line(Some("gpt-5.4"), 24);
        let model_span = line
            .spans
            .iter()
            .find(|span| span.content.contains("gpt-5.4"))
            .expect("expected model span");
        assert_eq!(model_span.style.fg, Some(Color::Rgb(60, 60, 60)));
        assert!(!model_span.style.add_modifier.contains(Modifier::BOLD));
    }
}
