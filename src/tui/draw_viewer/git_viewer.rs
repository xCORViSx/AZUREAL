//! Git panel viewer rendering
//!
//! Renders diff content from the git actions panel state, populating
//! viewer_lines_cache for selection/copy/scroll support.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use super::selection::apply_selection_to_line;
use super::super::util::{GIT_BROWN, GIT_ORANGE};

/// Git panel viewer — populates viewer_lines_cache for selection/copy/scroll support
pub(super) fn draw_git_viewer_selectable(f: &mut Frame, app: &mut App, area: Rect, _is_focused: bool, viewport_height: usize) {
    let (diff, title_str) = match app.git_actions_panel.as_ref() {
        Some(p) => (p.viewer_diff.clone(), p.viewer_diff_title.clone()),
        None => return,
    };

    let title = match title_str {
        Some(ref t) => format!(" {} ", t),
        None => " Viewer ".to_string(),
    };

    let block = Block::default()
        .title(Span::styled(&title, Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD));

    // Clear previous frame's cells so placeholder text doesn't bleed through
    // when diff content doesn't fill the full viewport width/height.
    f.render_widget(Clear, area);

    match diff {
        Some(ref diff_text) => {
            // Build styled lines from diff (no line number gutter — gutter=0)
            let all_lines: Vec<Line<'static>> = diff_text.lines().map(|l| {
                let style = if l.starts_with('+') && !l.starts_with("+++") {
                    Style::default().fg(Color::Green)
                } else if l.starts_with('-') && !l.starts_with("---") {
                    Style::default().fg(Color::Red)
                } else if l.starts_with("@@") {
                    Style::default().fg(Color::Cyan)
                } else if l.starts_with("diff ") || l.starts_with("index ") {
                    Style::default().fg(GIT_BROWN)
                } else {
                    Style::default().fg(Color::White)
                };
                Line::from(Span::styled(format!(" {}", l), style))
            }).collect();

            // Populate cache for selection/copy infrastructure
            app.viewer_lines_cache = all_lines;
            app.clamp_viewer_scroll();
            let scroll = app.viewer_scroll;

            // Build viewport slice with selection highlighting
            let display_lines: Vec<Line> = app.viewer_lines_cache.iter()
                .enumerate()
                .skip(scroll)
                .take(viewport_height)
                .map(|(idx, line)| {
                    if let Some((sl, sc, el, ec)) = app.viewer_selection {
                        if idx >= sl && idx <= el {
                            let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                            Line::from(apply_selection_to_line(
                                line.spans.clone(), &content, idx, sl, sc, el, ec, 0,
                            ))
                        } else { line.clone() }
                    } else { line.clone() }
                })
                .collect();

            f.render_widget(Paragraph::new(display_lines).block(block), area);
        }
        None => {
            // No diff selected — clear cache and show hint
            app.viewer_lines_cache.clear();
            let hint = vec![
                Line::from(""),
                Line::from(Span::styled(
                    " Select a file or commit to view its diff",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            f.render_widget(Paragraph::new(hint).block(block), area);
        }
    }
}
