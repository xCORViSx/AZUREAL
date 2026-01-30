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
use textwrap::{wrap, Options};

use crate::app::{App, Focus, ViewerMode};

/// Draw the viewer panel showing file content or diff detail
pub fn draw_viewer(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.focus == Focus::Viewer;
    let viewport_height = area.height.saturating_sub(2) as usize;
    let viewport_width = area.width.saturating_sub(2) as usize;

    // Cache viewport height for scroll operations (input handling uses this)
    app.viewer_viewport_height = viewport_height;

    let (title, lines) = match app.viewer_mode {
        ViewerMode::Empty => {
            let placeholder = vec![
                Line::from(""),
                Line::from(Span::styled("Select a file from the tree", Style::default().fg(Color::DarkGray))),
                Line::from(Span::styled("or a diff from output", Style::default().fg(Color::DarkGray))),
            ];
            (" Viewer ".to_string(), placeholder)
        }
        ViewerMode::File => {
            let path_str = app.viewer_path.as_ref()
                .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
                .unwrap_or_else(|| "File".to_string());

            if app.viewer_content.is_some() {
                // Only re-render if cache is dirty or width changed
                if app.viewer_lines_dirty || app.viewer_lines_width != viewport_width {
                    let content = app.viewer_content.as_ref().unwrap();
                    let highlighted = app.syntax_highlighter.highlight_file(content, &path_str);
                    let original_line_count = highlighted.len();
                    let line_num_width = original_line_count.to_string().len().max(3);
                    let content_width = viewport_width.saturating_sub(line_num_width + 3);

                    let mut all_lines: Vec<Line> = Vec::new();
                    let mut line_numbers: Vec<usize> = Vec::new();
                    for (line_idx, spans) in highlighted.into_iter().enumerate() {
                        let wrapped = wrap_spans(spans, content_width);
                        for (wrap_idx, wrapped_spans) in wrapped.into_iter().enumerate() {
                            let line_num = if wrap_idx == 0 {
                                format!("{:>width$} │ ", line_idx + 1, width = line_num_width)
                            } else {
                                format!("{:>width$} │ ", "", width = line_num_width)
                            };
                            let mut all_spans = vec![Span::styled(line_num, Style::default().fg(Color::DarkGray))];
                            all_spans.extend(wrapped_spans);
                            all_lines.push(Line::from(all_spans));
                            line_numbers.push(line_idx + 1); // 1-indexed original line
                        }
                    }

                    app.viewer_lines_cache = all_lines;
                    app.viewer_line_numbers = line_numbers;
                    app.viewer_original_line_count = original_line_count;
                    app.viewer_lines_width = viewport_width;
                    app.viewer_lines_dirty = false;
                }

                let total = app.viewer_original_line_count;

                // Clamp scroll to valid range (resolves usize::MAX sentinel)
                app.clamp_viewer_scroll();
                let scroll = app.viewer_scroll;

                // Build viewport slice directly (single clone operation)
                let display_lines: Vec<Line> = app.viewer_lines_cache.iter()
                    .skip(scroll)
                    .take(viewport_height)
                    .cloned()
                    .collect();

                // Find the last visible original line number for the title
                let last_visible_idx = (scroll + display_lines.len()).saturating_sub(1);
                let last_visible_line = app.viewer_line_numbers.get(last_visible_idx).copied().unwrap_or(total);
                let title = if app.viewer_lines_cache.len() > viewport_height {
                    format!(" {} [{}/{}] ", path_str, last_visible_line, total)
                } else {
                    format!(" {} ({} lines) ", path_str, total)
                };

                (title, display_lines)
            } else {
                (format!(" {} ", path_str), vec![Line::from("No content")])
            }
        }
        ViewerMode::Diff => {
            if app.viewer_content.is_some() {
                // Cache diff lines too (wrapping is expensive)
                if app.viewer_lines_dirty || app.viewer_lines_width != viewport_width {
                    let content = app.viewer_content.as_ref().unwrap();
                    let mut all_lines: Vec<Line> = Vec::new();
                    for line in content.lines() {
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

                        for wrapped in wrap_text(line, viewport_width) {
                            all_lines.push(Line::from(Span::styled(wrapped, style)));
                        }
                    }

                    app.viewer_lines_cache = all_lines;
                    app.viewer_lines_width = viewport_width;
                    app.viewer_lines_dirty = false;
                }

                let total = app.viewer_lines_cache.len();

                // Clamp scroll to valid range (resolves usize::MAX sentinel)
                app.clamp_viewer_scroll();
                let scroll = app.viewer_scroll;

                // Build viewport slice directly (single clone operation)
                let display_lines: Vec<Line> = app.viewer_lines_cache.iter()
                    .skip(scroll)
                    .take(viewport_height)
                    .cloned()
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

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if text.is_empty() { return vec![String::new()]; }
    let opts = Options::new(max_width).break_words(true);
    wrap(text, opts).into_iter().map(|cow| cow.into_owned()).collect()
}

fn wrap_spans(spans: Vec<Span<'static>>, max_width: usize) -> Vec<Vec<Span<'static>>> {
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
