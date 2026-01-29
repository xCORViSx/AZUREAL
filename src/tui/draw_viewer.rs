//! Viewer panel rendering
//!
//! Shows file content when a file is selected from FileTree,
//! or diff detail when a diff is selected from Output.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::app::{App, Focus, ViewerMode};

/// Draw the viewer panel showing file content or diff detail
pub fn draw_viewer(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.focus == Focus::Viewer;
    let viewport_height = area.height.saturating_sub(2) as usize;
    let viewport_width = area.width.saturating_sub(2) as usize;

    let (title, lines) = match app.viewer_mode {
        ViewerMode::Empty => {
            let placeholder = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Select a file from the tree",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "or a diff from output",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            (" Viewer ".to_string(), placeholder)
        }
        ViewerMode::File => {
            let path_str = app.viewer_path.as_ref()
                .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
                .unwrap_or_else(|| "File".to_string());

            if let Some(ref content) = app.viewer_content {
                // Syntax highlight the file
                let highlighted = app.syntax_highlighter.highlight_file(content, &path_str);
                let total = highlighted.len();
                let scroll = app.viewer_scroll.min(total.saturating_sub(viewport_height));
                app.viewer_scroll = scroll;

                // Build display lines with line numbers
                let line_num_width = total.to_string().len().max(3);
                let display_lines: Vec<Line> = highlighted
                    .into_iter()
                    .enumerate()
                    .skip(scroll)
                    .take(viewport_height)
                    .map(|(i, spans)| {
                        let line_num = format!("{:>width$} │ ", i + 1, width = line_num_width);
                        let mut all_spans = vec![
                            Span::styled(line_num, Style::default().fg(Color::DarkGray))
                        ];
                        all_spans.extend(spans);

                        // Truncate long lines (approximate - spans make this tricky)
                        let content_width = viewport_width.saturating_sub(line_num_width + 3);
                        truncate_line_spans(&mut all_spans, content_width + line_num_width + 3);

                        Line::from(all_spans)
                    })
                    .collect();

                let title = if total > viewport_height {
                    format!(" {} [{}/{}] ", path_str, scroll + 1, total)
                } else {
                    format!(" {} ({} lines) ", path_str, total)
                };

                (title, display_lines)
            } else {
                (format!(" {} ", path_str), vec![Line::from("No content")])
            }
        }
        ViewerMode::Diff => {
            if let Some(ref content) = app.viewer_content {
                let all_lines: Vec<Line> = content
                    .lines()
                    .map(|line| {
                        let style = if line.starts_with('+') && !line.starts_with("+++") {
                            Style::default().fg(Color::Green)
                        } else if line.starts_with('-') && !line.starts_with("---") {
                            Style::default().fg(Color::Red)
                        } else if line.starts_with("@@") {
                            Style::default().fg(Color::Cyan)
                        } else if line.starts_with("diff ") || line.starts_with("index ") {
                            Style::default().fg(Color::Yellow)
                        } else {
                            Style::default()
                        };
                        Line::from(Span::styled(truncate_str(line, viewport_width), style))
                    })
                    .collect();

                let total = all_lines.len();
                let scroll = app.viewer_scroll.min(total.saturating_sub(viewport_height));
                app.viewer_scroll = scroll;
                let display_lines: Vec<Line> = all_lines
                    .into_iter()
                    .skip(scroll)
                    .take(viewport_height)
                    .collect();

                let title = if total > viewport_height {
                    format!(" Diff [{}/{}] ", scroll + 1, total)
                } else {
                    " Diff ".to_string()
                };

                (title, display_lines)
            } else {
                (" Diff ".to_string(), vec![Line::from("No diff selected")])
            }
        }
    };

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
            .title(if is_focused {
                Span::styled(title, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(title, Style::default().fg(Color::White))
            })
            .border_style(if is_focused {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            }),
    );

    f.render_widget(widget, area);
}

/// Truncate a string to max_width, adding ellipsis if needed
fn truncate_str(s: &str, max_width: usize) -> String {
    if s.len() <= max_width {
        s.to_string()
    } else if max_width > 3 {
        format!("{}...", &s[..max_width - 3])
    } else {
        s[..max_width].to_string()
    }
}

/// Truncate line spans to approximate max width
fn truncate_line_spans(spans: &mut Vec<Span<'static>>, max_width: usize) {
    let mut total_len = 0;
    let mut truncate_at = spans.len();

    for (i, span) in spans.iter().enumerate() {
        let span_len = span.content.chars().count();
        if total_len + span_len > max_width {
            truncate_at = i;
            break;
        }
        total_len += span_len;
    }

    if truncate_at < spans.len() {
        spans.truncate(truncate_at + 1);
        if let Some(last) = spans.last_mut() {
            let remaining = max_width.saturating_sub(total_len);
            if remaining > 3 {
                let content: String = last.content.chars().take(remaining - 3).collect();
                *last = Span::styled(format!("{}...", content), last.style);
            }
        }
    }
}
