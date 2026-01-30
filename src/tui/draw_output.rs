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

    // Cache viewport height for scroll operations (input handling uses this)
    app.output_viewport_height = viewport_height;

    let (title, content) = match app.view_mode {
        ViewMode::Output => {
            if !app.display_events.is_empty() {
                let inner_width = area.width.saturating_sub(2);

                // Only re-render if cache is dirty or width changed (NOT for animation tick)
                if app.rendered_lines_dirty || app.rendered_lines_width != inner_width {
                    let (lines_cache, anim_indices, bubble_positions) = render_display_events(
                        &app.display_events,
                        inner_width,
                        &app.pending_tool_calls,
                        &app.failed_tool_calls,
                        &app.syntax_highlighter,
                        app.pending_user_message.as_deref(),
                    );
                    app.rendered_lines_cache = lines_cache;
                    app.animation_line_indices = anim_indices;
                    app.message_bubble_positions = bubble_positions;
                    app.rendered_lines_width = inner_width;
                    app.rendered_lines_dirty = false;
                }

                let total = app.rendered_lines_cache.len();

                // Clamp scroll to valid range (resolves usize::MAX sentinel)
                app.clamp_output_scroll();
                let scroll = app.output_scroll;

                // Build viewport slice and patch animation colors for pending indicators
                let pulse_colors = [Color::White, Color::Gray, Color::DarkGray, Color::Gray];
                let pulse_idx = (app.animation_tick / 2) as usize % pulse_colors.len();
                let pulse_color = pulse_colors[pulse_idx];

                let mut lines: Vec<Line> = app.rendered_lines_cache.iter()
                    .skip(scroll)
                    .take(viewport_height)
                    .cloned()
                    .collect();

                // Patch animation colors for pending tool indicators in viewport
                for &(line_idx, span_idx) in &app.animation_line_indices {
                    if line_idx >= scroll && line_idx < scroll + viewport_height {
                        let viewport_idx = line_idx - scroll;
                        if let Some(line) = lines.get_mut(viewport_idx) {
                            if let Some(span) = line.spans.get_mut(span_idx) {
                                span.style = span.style.fg(pulse_color);
                            }
                        }
                    }
                }

                let title = if total > viewport_height {
                    format!(" Convo [{}/{}] ", scroll + viewport_height.min(total - scroll), total)
                } else {
                    " Convo ".to_string()
                };

                (title, lines)
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
                let max_scroll = total.saturating_sub(viewport_height);
                let scroll = if app.output_scroll == usize::MAX {
                    max_scroll
                } else {
                    app.output_scroll.min(max_scroll)
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
                // Cache diff highlighting (expensive - don't do per-frame)
                if app.diff_lines_dirty {
                    app.diff_lines_cache = app.diff_highlighter.colorize_diff(diff);
                    app.diff_lines_dirty = false;
                }

                let total = app.diff_lines_cache.len();
                let scroll = app.diff_scroll.min(total.saturating_sub(viewport_height));
                app.diff_scroll = scroll;

                // Build viewport slice directly (single clone operation)
                let lines: Vec<Line> = app.diff_lines_cache.iter()
                    .skip(scroll)
                    .take(viewport_height)
                    .map(|spans| Line::from(spans.clone()))
                    .collect();

                let title = if total > viewport_height {
                    format!(" Diff (Syntax Highlighted) [{}/{}] ", scroll + viewport_height.min(total - scroll), total)
                } else {
                    " Diff (Syntax Highlighted) ".to_string()
                };

                (title, lines)
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
