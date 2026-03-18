//! Plan mode rendering — full-width boxed display with markdown highlighting

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::tui::markdown::parse_markdown_spans;
use crate::tui::render_wrap::wrap_text;
use crate::tui::util::AZURE;

/// Render a plan block with prominent full-width styling and markdown highlighting
pub(super) fn render_plan(
    lines: &mut Vec<Line<'static>>,
    name: &str,
    content: &str,
    width: usize,
) {
    let plan_color = Color::Green;
    let header_bg = Color::Green;
    let border = "═";
    let content_width = width.saturating_sub(4);

    // Spacing before plan
    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Top border
    lines.push(Line::from(vec![Span::styled(
        format!("╔{}╗", border.repeat(width.saturating_sub(2))),
        Style::default().fg(plan_color).add_modifier(Modifier::BOLD),
    )]));

    // Header with plan icon and name
    let header = format!(" 📋 PLAN MODE: {} ", name);
    let header_pad = width.saturating_sub(header.chars().count() + 2);
    lines.push(Line::from(vec![
        Span::styled(
            "║",
            Style::default().fg(plan_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            header,
            Style::default()
                .fg(Color::Black)
                .bg(header_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ".repeat(header_pad), Style::default().bg(header_bg)),
        Span::styled(
            "║",
            Style::default().fg(plan_color).add_modifier(Modifier::BOLD),
        ),
    ]));

    // Separator under header
    lines.push(Line::from(vec![Span::styled(
        format!("╠{}╣", "─".repeat(width.saturating_sub(2))),
        Style::default().fg(plan_color),
    )]));

    // Render markdown content with box borders
    let text_lines: Vec<&str> = content.lines().collect();
    let mut in_code_block = false;

    // Helper to push a line with box borders and padding
    let push_boxed =
        |lines: &mut Vec<Line<'static>>, mut spans: Vec<Span<'static>>, char_count: usize| {
            let pad = content_width.saturating_sub(char_count);
            spans.insert(0, Span::styled("║ ", Style::default().fg(plan_color)));
            spans.push(Span::styled(
                format!("{} ║", " ".repeat(pad)),
                Style::default().fg(plan_color),
            ));
            lines.push(Line::from(spans));
        };

    for line in &text_lines {
        let trimmed = line.trim();

        // Code block delimiters
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            let lang = trimmed.trim_start_matches('`').trim();
            let (marker, char_len) = if in_code_block && !lang.is_empty() {
                (
                    vec![
                        Span::styled("┌─ ", Style::default().fg(Color::DarkGray)),
                        Span::styled(lang.to_string(), Style::default().fg(AZURE)),
                        Span::styled(" ─", Style::default().fg(Color::DarkGray)),
                    ],
                    5 + lang.chars().count(),
                )
            } else if !in_code_block {
                (
                    vec![Span::styled(
                        "└──────",
                        Style::default().fg(Color::DarkGray),
                    )],
                    7,
                )
            } else {
                (
                    vec![Span::styled(
                        "┌──────",
                        Style::default().fg(Color::DarkGray),
                    )],
                    7,
                )
            };
            push_boxed(lines, marker, char_len);
            continue;
        }

        // Code block content
        if in_code_block {
            for wrapped in wrap_text(line, content_width.saturating_sub(2)) {
                let spans = vec![
                    Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(wrapped.clone(), Style::default().fg(Color::Yellow)),
                ];
                push_boxed(lines, spans, 2 + wrapped.chars().count());
            }
            continue;
        }

        // Headers
        if trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|&c| c == '#').count();
            let text = trimmed.trim_start_matches('#').trim();
            let (prefix, style) = match level {
                1 => (
                    "█ ",
                    Style::default()
                        .fg(AZURE)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                ),
                2 => (
                    "▓ ",
                    Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
                ),
                3 => (
                    "▒ ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                _ => ("░ ", Style::default().fg(Color::Green)),
            };
            for (i, wrapped) in wrap_text(text, content_width.saturating_sub(2))
                .into_iter()
                .enumerate()
            {
                let spans = if i == 0 {
                    vec![
                        Span::styled(prefix, style),
                        Span::styled(wrapped.clone(), style),
                    ]
                } else {
                    vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(wrapped.clone(), style),
                    ]
                };
                push_boxed(lines, spans, 2 + wrapped.chars().count());
            }
            continue;
        }

        // Bullet lists
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• ") {
            let bullet_content = trimmed
                .trim_start_matches("- ")
                .trim_start_matches("* ")
                .trim_start_matches("• ");
            for (i, wrapped) in wrap_text(bullet_content, content_width.saturating_sub(4))
                .into_iter()
                .enumerate()
            {
                let mut spans = if i == 0 {
                    vec![Span::styled("  • ", Style::default().fg(AZURE))]
                } else {
                    vec![Span::styled("    ", Style::default())]
                };
                spans.extend(parse_markdown_spans(
                    &wrapped,
                    Style::default().fg(Color::White),
                ));
                push_boxed(lines, spans, 4 + wrapped.chars().count());
            }
            continue;
        }

        // Numbered lists
        if trimmed
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            if let Some(dot_pos) = trimmed.find(". ") {
                let num = &trimmed[..dot_pos];
                let content_text = &trimmed[dot_pos + 2..];
                let prefix = format!("  {}. ", num);
                let prefix_len = prefix.chars().count();
                for (i, wrapped) in
                    wrap_text(content_text, content_width.saturating_sub(prefix_len))
                        .into_iter()
                        .enumerate()
                {
                    let mut spans = if i == 0 {
                        vec![Span::styled(prefix.clone(), Style::default().fg(AZURE))]
                    } else {
                        vec![Span::styled(" ".repeat(prefix_len), Style::default())]
                    };
                    spans.extend(parse_markdown_spans(
                        &wrapped,
                        Style::default().fg(Color::White),
                    ));
                    push_boxed(lines, spans, prefix_len + wrapped.chars().count());
                }
                continue;
            }
        }

        // Blockquotes
        if trimmed.starts_with("> ") {
            let quote_content = trimmed.trim_start_matches("> ");
            for wrapped in wrap_text(quote_content, content_width.saturating_sub(2)) {
                let mut spans = vec![Span::styled("┃ ", Style::default().fg(Color::DarkGray))];
                spans.extend(parse_markdown_spans(
                    &wrapped,
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ));
                push_boxed(lines, spans, 2 + wrapped.chars().count());
            }
            continue;
        }

        // Regular paragraph text with inline markdown
        if trimmed.is_empty() {
            push_boxed(lines, vec![], 0);
        } else {
            for wrapped in wrap_text(line, content_width) {
                let spans = parse_markdown_spans(&wrapped, Style::default().fg(Color::White));
                let char_count = wrapped.chars().count();
                push_boxed(lines, spans, char_count);
            }
        }
    }

    // Bottom border
    lines.push(Line::from(vec![Span::styled(
        format!("╚{}╝", border.repeat(width.saturating_sub(2))),
        Style::default().fg(plan_color).add_modifier(Modifier::BOLD),
    )]));

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

    /// Plan renders with double-line box borders.
    #[test]
    fn test_render_plan_borders() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "My Plan", "Step 1\nStep 2", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains('╔')));
        assert!(text.iter().any(|l| l.contains('╚')));
    }

    /// Plan renders name in header.
    #[test]
    fn test_render_plan_name_in_header() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "Refactor", "content", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("PLAN MODE: Refactor")));
    }

    /// Plan with empty content.
    #[test]
    fn test_render_plan_empty_content() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "Empty", "", 80);
        assert!(!lines.is_empty());
    }

    /// Plan with markdown headers.
    #[test]
    fn test_render_plan_markdown_headers() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "P", "# Title\n## Subtitle\n### Section", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Title")));
        assert!(text.iter().any(|l| l.contains("Subtitle")));
        assert!(text.iter().any(|l| l.contains("Section")));
    }

    /// Plan with bullet list.
    #[test]
    fn test_render_plan_bullets() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "P", "- Item one\n- Item two", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Item one")));
        assert!(text.iter().any(|l| l.contains("Item two")));
    }

    /// Plan with numbered list.
    #[test]
    fn test_render_plan_numbered_list() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "P", "1. First\n2. Second", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("First")));
        assert!(text.iter().any(|l| l.contains("Second")));
    }

    /// Plan with code block.
    #[test]
    fn test_render_plan_code_block() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "P", "```rust\nfn main() {}\n```", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("rust")));
        assert!(text.iter().any(|l| l.contains("fn main()")));
    }

    /// Plan with blockquote.
    #[test]
    fn test_render_plan_blockquote() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "P", "> A quoted line", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("A quoted line")));
    }

    /// Plan at very narrow width.
    #[test]
    fn test_render_plan_narrow() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan(&mut lines, "P", "Some content here", 10);
        assert!(!lines.is_empty());
    }
}
