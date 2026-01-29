//! Convopanel rendering

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, Focus, ViewMode};
use crate::models::RebaseState;
use super::util::{colorize_output, detect_message_type, render_display_events, MessageType};

/// Draw the main output/diff panel
pub fn draw_output(f: &mut Frame, app: &mut App, area: Rect) {
    let viewport_height = area.height.saturating_sub(2) as usize;

    let (title, content) = match app.view_mode {
        ViewMode::Output => {
            if !app.display_events.is_empty() {
                let inner_width = area.width.saturating_sub(2);

                // Only re-render if cache is dirty or width changed
                if app.rendered_lines_dirty || app.rendered_lines_width != inner_width {
                    app.rendered_lines_cache = render_display_events(
                        &app.display_events,
                        inner_width,
                        &app.pending_tool_calls,
                        &app.failed_tool_calls,
                        app.animation_tick,
                        &app.syntax_highlighter,
                    );
                    app.rendered_lines_width = inner_width;
                    app.rendered_lines_dirty = false;
                }

                let total = app.rendered_lines_cache.len();

                let scroll = if app.output_scroll == usize::MAX {
                    total.saturating_sub(viewport_height)
                } else {
                    app.output_scroll.min(total.saturating_sub(viewport_height))
                };
                app.output_scroll = scroll;

                let lines: Vec<Line> = app.rendered_lines_cache.iter().skip(scroll).take(viewport_height).cloned().collect();

                let scroll_indicator = if total > viewport_height {
                    format!(" Convo [{}/{}] ", scroll + viewport_height.min(total - scroll), total)
                } else {
                    " Convo ".to_string()
                };

                (scroll_indicator, lines)
            } else {
                // Fallback: using output_lines with colorize_output
                let mut all_lines: Vec<Line> = Vec::new();
                let mut last_msg_type = MessageType::Other;

                for line in app.output_lines.iter() {
                    let msg_type = detect_message_type(line);

                    // Add spacing when transitioning between user and assistant
                    if (last_msg_type == MessageType::User && msg_type == MessageType::Assistant)
                        || (last_msg_type == MessageType::Assistant && msg_type == MessageType::User)
                    {
                        all_lines.push(Line::from(""));
                        all_lines.push(Line::from(""));
                    }

                    all_lines.push(colorize_output(line));

                    if msg_type != MessageType::Other {
                        last_msg_type = msg_type;
                    }
                }

                if !app.output_buffer.is_empty() {
                    let msg_type = detect_message_type(&app.output_buffer);
                    if (last_msg_type == MessageType::User && msg_type == MessageType::Assistant)
                        || (last_msg_type == MessageType::Assistant && msg_type == MessageType::User)
                    {
                        all_lines.push(Line::from(""));
                        all_lines.push(Line::from(""));
                    }
                    all_lines.push(colorize_output(&app.output_buffer));
                }

                let total = all_lines.len();
                let scroll = if app.output_scroll == usize::MAX {
                    total.saturating_sub(viewport_height)
                } else {
                    app.output_scroll.min(total.saturating_sub(viewport_height))
                };
                app.output_scroll = scroll;

                let lines: Vec<Line> = all_lines.into_iter().skip(scroll).take(viewport_height).collect();

                let scroll_indicator = if total > viewport_height {
                    format!(" Convo [{}/{}] ", scroll + viewport_height.min(total - scroll), total)
                } else {
                    " Convo ".to_string()
                };

                (scroll_indicator, lines)
            }
        }
        ViewMode::Diff => {
            if let Some(ref diff) = app.diff_text {
                let highlighted = app.diff_highlighter.colorize_diff(diff);
                let total = highlighted.len();
                let scroll = app.diff_scroll.min(total.saturating_sub(viewport_height));
                app.diff_scroll = scroll;

                let lines: Vec<Line> = highlighted.into_iter()
                    .skip(scroll)
                    .take(viewport_height)
                    .map(Line::from)
                    .collect();

                let scroll_indicator = if total > viewport_height {
                    format!(" Diff (Syntax Highlighted) [{}/{}] ", scroll + viewport_height.min(total - scroll), total)
                } else {
                    " Diff (Syntax Highlighted) ".to_string()
                };

                (scroll_indicator, lines)
            } else {
                (" Diff ".to_string(), vec![Line::from("No diff available")])
            }
        }
        ViewMode::Messages => {
            (" Messages ".to_string(), vec![Line::from("Messages view not implemented")])
        }
        ViewMode::Rebase => {
            (" Rebase ".to_string(), draw_rebase_content(app))
        }
    };

    let is_focused = app.focus == Focus::Output;
    let output = Paragraph::new(content)
        .block(
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
        )
        .wrap(Wrap { trim: false });

    f.render_widget(output, area);
}

/// Draw rebase status content
fn draw_rebase_content(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    if let Some(ref status) = app.rebase_status {
        let state_color = status.state.color();
        lines.push(Line::from(vec![
            Span::styled("State: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{} {}", status.state.symbol(), status.state.as_str()),
                Style::default().fg(state_color),
            ),
        ]));

        if let (Some(current), Some(total)) = (status.current_step, status.total_steps) {
            lines.push(Line::from(vec![
                Span::styled("Progress: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(format!("{}/{}", current, total)),
            ]));
        }

        if let Some(ref head) = status.head_name {
            lines.push(Line::from(vec![
                Span::styled("Rebasing: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(head.clone(), Style::default().fg(Color::Green)),
            ]));
        }

        if let Some(ref onto) = status.onto_branch {
            lines.push(Line::from(vec![
                Span::styled("Onto: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(onto.clone(), Style::default().fg(Color::Cyan)),
            ]));
        }

        if let Some(ref commit) = status.current_commit {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Current commit: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(commit.clone(), Style::default().fg(Color::Yellow)),
            ]));
            if let Some(ref msg) = status.current_commit_message {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::raw(msg.clone()),
                ]));
            }
        }

        if !status.conflicted_files.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(
                    format!("Conflicts ({}):", status.conflicted_files.len()),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            ]));

            for (idx, file) in status.conflicted_files.iter().enumerate() {
                let is_selected = app.selected_conflict == Some(idx);
                let style = if is_selected {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    Style::default().fg(Color::Red)
                };
                let prefix = if is_selected { "▸ " } else { "  " };
                lines.push(Line::from(vec![
                    Span::raw(prefix),
                    Span::styled(file.clone(), style),
                ]));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Commands: ", Style::default().add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from("  o: use ours (keep our changes)"));
            lines.push(Line::from("  t: use theirs (accept incoming)"));
            lines.push(Line::from("  Enter: view conflict diff"));
            lines.push(Line::from("  c: continue rebase"));
            lines.push(Line::from("  s: skip this commit"));
            lines.push(Line::from("  A: abort rebase"));
        } else if status.state == RebaseState::InProgress {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("No conflicts. ", Style::default().fg(Color::Green)),
                Span::raw("Press 'c' to continue."),
            ]));
        }
    } else {
        lines.push(Line::from("No rebase in progress"));
    }

    lines
}
