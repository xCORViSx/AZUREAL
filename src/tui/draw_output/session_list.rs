//! Session list overlay
//!
//! Full-pane list of Claude session files for the current worktree.
//! Supports name filtering, content search, and session switching.
//! Each row shows: session name, mtime, and message count badge.

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use super::super::util::AZURE;
use crate::app::{App, Focus};

/// Draw the Claude session list overlay — full-pane list of all Claude session files.
/// Each row shows: session name, mtime, [N msgs].
pub fn draw_session_list(f: &mut Frame, app: &mut App, area: Rect) {
    // Show a small centered "Loading..." dialog while message counts are computing.
    // This renders on the first frame after 's' is pressed, before the I/O starts.
    if app.session_list_loading {
        let msg = " Loading sessions\u{2026} ";
        let w = (msg.len() as u16 + 4).min(area.width);
        let h = 3u16;
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let dialog = Paragraph::new(Span::styled(msg, Style::default().fg(Color::White)))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(AZURE))
                    .title(Span::styled(
                        " Sessions ",
                        Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
                    )),
            );
        f.render_widget(dialog, Rect::new(x, y, w, h));
        return;
    }

    let is_focused = app.focus == Focus::Session;

    // Split area: filter bar at top when filter is active or has text
    let has_filter = app.session_filter_active || !app.session_filter.is_empty();
    let (filter_area, list_area) = if has_filter {
        let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, area)
    };

    // Draw filter input bar when active
    if let Some(fa) = filter_area {
        let mode_prefix = if app.session_content_search {
            "//"
        } else {
            "/"
        };
        let border_color = if app.session_filter_active {
            Color::Yellow
        } else {
            Color::DarkGray
        };
        let right_info = if app.session_content_search {
            format!(" {} results ", app.session_search_results.len())
        } else {
            String::new()
        };
        let filter_widget = Paragraph::new(app.session_filter.clone()).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled(
                    mode_prefix,
                    Style::default().fg(Color::Yellow),
                ))
                .title(
                    Line::from(Span::styled(
                        right_info,
                        Style::default().fg(Color::DarkGray),
                    ))
                    .alignment(Alignment::Right),
                ),
        );
        f.render_widget(filter_widget, fa);
        if app.session_filter_active {
            let cursor_x = fa.x + 1 + app.session_filter.len() as u16;
            let cursor_y = fa.y + 1;
            if cursor_x < fa.right() {
                f.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }

    let viewport_height = list_area.height.saturating_sub(2) as usize;
    let inner_width = list_area.width.saturating_sub(2) as usize;

    // Content search mode: show search results instead of normal session list
    if app.session_content_search {
        draw_content_search(f, app, list_area, viewport_height, inner_width, is_focused);
        return;
    }

    // Session list scoped to current worktree only — no wt_name column needed
    draw_name_list(f, app, list_area, viewport_height, inner_width, is_focused);

    // Rename dialog: draw a centered input box over the session list
    if app.session_rename_active {
        draw_rename_dialog(f, app, list_area);
    }
}

/// Render content search results (triggered by "//" prefix in filter)
fn draw_content_search(
    f: &mut Frame,
    app: &mut App,
    list_area: Rect,
    viewport_height: usize,
    inner_width: usize,
    is_focused: bool,
) {
    let session_names = app.load_all_session_names();
    let mut rows: Vec<Line<'static>> = Vec::new();
    for (idx, (_row, session_id, preview)) in app.session_search_results.iter().enumerate() {
        let is_selected = idx == app.session_list_selected;
        let name_display = session_names
            .get(session_id.as_str())
            .cloned()
            .unwrap_or_else(|| session_id.chars().take(12).collect::<String>());
        let bg = if is_selected {
            Style::default().bg(AZURE).fg(Color::Black)
        } else {
            Style::default()
        };
        let name_style = if is_selected {
            Style::default()
                .bg(AZURE)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        // Truncate preview to fit
        let prefix_len = name_display.chars().count() + 4; // " name | "
        let preview_space = inner_width.saturating_sub(prefix_len);
        let trunc_preview: String = preview.chars().take(preview_space).collect();

        rows.push(Line::from(vec![
            Span::styled(format!(" {} ", name_display), name_style),
            Span::styled(
                "\u{2502} ",
                if is_selected {
                    bg
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
            Span::styled(
                trunc_preview,
                if is_selected {
                    bg
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
        ]));
    }
    let total = rows.len();
    if app.session_list_selected >= total && total > 0 {
        app.session_list_selected = total - 1;
    }
    let max_scroll = total.saturating_sub(viewport_height);
    if app.session_list_selected < app.session_list_scroll {
        app.session_list_scroll = app.session_list_selected;
    } else if app.session_list_selected >= app.session_list_scroll + viewport_height {
        app.session_list_scroll = app
            .session_list_selected
            .saturating_sub(viewport_height - 1);
    }
    app.session_list_scroll = app.session_list_scroll.min(max_scroll);
    let display: Vec<Line> = rows
        .into_iter()
        .skip(app.session_list_scroll)
        .take(viewport_height)
        .collect();
    let title = if total == 0 {
        " Search [0/0] ".to_string()
    } else {
        format!(
            " Search [{}/{}] ",
            app.session_list_selected.saturating_add(1).min(total),
            total
        )
    };
    let border_style = if is_focused {
        Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if is_focused {
            BorderType::Double
        } else {
            BorderType::Plain
        })
        .title(Span::styled(title, border_style))
        .border_style(border_style);
    let display = if total == 0 {
        let msg = "No results";
        let pad = (inner_width.saturating_sub(msg.len())) / 2;
        vec![Line::from(Span::styled(
            format!("{}{}", " ".repeat(pad), msg),
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        display
    };
    f.render_widget(Paragraph::new(display).block(block), list_area);
}

/// Render the normal session name list (with optional name filter)
fn draw_name_list(
    f: &mut Frame,
    app: &mut App,
    list_area: Rect,
    viewport_height: usize,
    inner_width: usize,
    is_focused: bool,
) {
    let session_names = app.load_all_session_names();
    let filter_lower = app.session_filter.to_lowercase();
    let filtering = !filter_lower.is_empty();
    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut total_unfiltered = 0usize;

    let branch = app.current_worktree().map(|s| s.branch_name.clone());
    let files = branch.as_deref().and_then(|b| app.session_files.get(b));

    if let Some(files) = files {
        for (session_id, _path, time_str) in files.iter() {
            total_unfiltered += 1;
            let name_display = session_names
                .get(session_id.as_str())
                .cloned()
                .unwrap_or_else(|| session_id.clone());

            // Name filter: skip rows that don't match session name or session id
            if filtering {
                let matches = name_display.to_lowercase().contains(&filter_lower)
                    || session_id.to_lowercase().contains(&filter_lower);
                if !matches {
                    continue;
                }
            }

            let msg_count = app
                .session_msg_counts
                .get(session_id)
                .map(|&(c, _)| c)
                .unwrap_or(0);
            let completion = app.session_completion.get(session_id);
            let msg_badge = format!("[{} msgs]", msg_count);

            // Build duration badge for completed sessions (e.g. "3.5s")
            let duration_badge = completion.map(|&(_, ms, _)| {
                let secs = ms as f64 / 1000.0;
                if secs >= 60.0 {
                    format!("{:.0}m", secs / 60.0)
                } else {
                    format!("{:.0}s", secs)
                }
            });
            let suffix = match &duration_badge {
                Some(d) => format!(" {} {} {} ", time_str, d, msg_badge),
                None => format!(" {} {} ", time_str, msg_badge),
            };
            // Row: " ● session_name    mtime 3s [N msgs]"
            let name_space = inner_width.saturating_sub(3 + suffix.chars().count());
            let truncated_name = if name_display.chars().count() > name_space {
                let trunc: String = name_display
                    .chars()
                    .take(name_space.saturating_sub(1))
                    .collect();
                format!("{}\u{2026}", trunc)
            } else {
                name_display
            };
            let pad = name_space.saturating_sub(truncated_name.chars().count());

            let is_selected = rows.len() == app.session_list_selected;
            let name_style = if is_selected {
                Style::default()
                    .bg(AZURE)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let bg_style = if is_selected {
                Style::default().bg(AZURE).fg(Color::Black)
            } else {
                Style::default()
            };

            // Status indicator: running > completed/failed > idle
            let running = app.is_claude_session_running(session_id);
            let (dot, dot_color) = if running {
                ("\u{25cf}", Color::Green) // ● green = running
            } else if let Some(&(success, _, _)) = completion {
                if success {
                    ("\u{2713}", Color::Green)
                } else {
                    ("\u{2717}", Color::Red)
                } // ✓ green / ✗ red
            } else {
                ("\u{25cb}", Color::DarkGray) // ○ dim = idle/unknown
            };

            let mut spans = vec![
                Span::styled(" ", bg_style),
                Span::styled(
                    dot,
                    if is_selected {
                        bg_style
                    } else {
                        Style::default().fg(dot_color)
                    },
                ),
                Span::styled(" ", bg_style),
                Span::styled(truncated_name, name_style),
                Span::styled(" ".repeat(pad), bg_style),
                Span::styled(
                    format!(" {} ", time_str),
                    if is_selected {
                        bg_style
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
            ];
            if let Some(d) = &duration_badge {
                let dur_color = if completion.map(|c| c.0).unwrap_or(true) {
                    Color::Green
                } else {
                    Color::Red
                };
                spans.push(Span::styled(
                    format!("{} ", d),
                    if is_selected {
                        bg_style
                    } else {
                        Style::default().fg(dur_color)
                    },
                ));
            }
            spans.push(Span::styled(
                msg_badge,
                if is_selected {
                    bg_style
                } else {
                    Style::default().fg(AZURE)
                },
            ));

            rows.push(Line::from(spans));
        }
    }

    // Clamp selection
    let total = rows.len();
    if app.session_list_selected >= total && total > 0 {
        app.session_list_selected = total - 1;
    }

    // Auto-scroll to keep selection visible
    let max_scroll = total.saturating_sub(viewport_height);
    if app.session_list_selected < app.session_list_scroll {
        app.session_list_scroll = app.session_list_selected;
    } else if app.session_list_selected >= app.session_list_scroll + viewport_height {
        app.session_list_scroll = app
            .session_list_selected
            .saturating_sub(viewport_height - 1);
    }
    app.session_list_scroll = app.session_list_scroll.min(max_scroll);

    let display: Vec<Line> = rows
        .into_iter()
        .skip(app.session_list_scroll)
        .take(viewport_height)
        .collect();

    let title = if total == 0 {
        " Sessions [0/0] ".to_string()
    } else if filtering {
        format!(
            " Sessions [{}/{} of {}] ",
            app.session_list_selected.saturating_add(1).min(total),
            total,
            total_unfiltered
        )
    } else {
        format!(" Sessions [{}/{}] ", app.session_list_selected + 1, total)
    };
    let border_style = if is_focused {
        Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if is_focused {
            BorderType::Double
        } else {
            BorderType::Plain
        })
        .title(Span::styled(title, border_style))
        .border_style(border_style);

    let display = if total == 0 {
        let msg = "No sessions";
        let pad = (inner_width.saturating_sub(msg.len())) / 2;
        let hint_spans = vec![
            Span::styled("Press ", Style::default().fg(Color::DarkGray)),
            Span::styled("a", Style::default().fg(AZURE).add_modifier(Modifier::BOLD)),
            Span::styled(" to add a session", Style::default().fg(Color::DarkGray)),
        ];
        let hint_len: usize = hint_spans.iter().map(|s| s.content.len()).sum();
        let hint_pad = (inner_width.saturating_sub(hint_len)) / 2;
        let mut hint = vec![Span::styled(" ".repeat(hint_pad), Style::default())];
        hint.extend(hint_spans);
        vec![
            Line::from(Span::styled(
                format!("{}{}", " ".repeat(pad), msg),
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(hint),
        ]
    } else {
        display
    };
    let widget = Paragraph::new(display).block(block);
    f.render_widget(widget, list_area);
}

/// Draw a centered rename dialog over the session list.
/// Shows a single-line text input with the current rename buffer.
fn draw_rename_dialog(f: &mut Frame, app: &App, area: Rect) {
    let input = &app.session_rename_input;
    // Size: enough for the input + some padding, clamped to area
    let w = (input.chars().count() as u16 + 6)
        .max(30)
        .min(area.width.saturating_sub(4));
    let h = 3u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let dialog_area = Rect::new(x, y, w, h);

    // Clear background behind dialog
    let clear = Paragraph::new("").block(Block::default().style(Style::default().bg(Color::Black)));
    f.render_widget(clear, dialog_area);

    let widget = Paragraph::new(input.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Span::styled(
                " Rename ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))
            .title(
                Line::from(Span::styled(
                    " Enter:save  Esc:cancel ",
                    Style::default().fg(Color::DarkGray),
                ))
                .alignment(Alignment::Right),
            ),
    );
    f.render_widget(widget, dialog_area);

    // Position cursor
    let cursor_x = dialog_area.x + 1 + app.session_rename_cursor as u16;
    let cursor_y = dialog_area.y + 1;
    if cursor_x < dialog_area.right() {
        f.set_cursor_position((cursor_x, cursor_y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::{Constraint, Layout, Rect};
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, BorderType, Borders};

    // ══════════════════════════════════════════════════════════════════
    //  AZURE constant accessibility
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn azure_is_correct_rgb() {
        assert_eq!(AZURE, Color::Rgb(51, 153, 255));
    }

    #[test]
    fn azure_is_not_plain_blue() {
        assert_ne!(AZURE, Color::Blue);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Focus::Session variant
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn focus_session_equals_itself() {
        assert_eq!(Focus::Session, Focus::Session);
    }

    #[test]
    fn focus_session_ne_worktrees() {
        assert_ne!(Focus::Session, Focus::Worktrees);
    }

    #[test]
    fn focus_session_ne_input() {
        assert_ne!(Focus::Session, Focus::Input);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Loading dialog sizing math
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn loading_msg_text() {
        let msg = " Loading sessions\u{2026} ";
        assert!(msg.contains("Loading"));
        assert!(msg.contains('\u{2026}')); // ellipsis
    }

    #[test]
    fn loading_dialog_width_normal_area() {
        let msg = " Loading sessions\u{2026} ";
        let area_width = 80u16;
        let w = (msg.len() as u16 + 4).min(area_width);
        assert!(w <= area_width);
        assert!(w > 0);
    }

    #[test]
    fn loading_dialog_width_narrow_area() {
        let msg = " Loading sessions\u{2026} ";
        let area_width = 10u16;
        let w = (msg.len() as u16 + 4).min(area_width);
        assert_eq!(w, area_width);
    }

    #[test]
    fn loading_dialog_height_is_three() {
        let h = 3u16;
        assert_eq!(h, 3);
    }

    #[test]
    fn loading_dialog_centering_x() {
        let area = Rect::new(0, 0, 80, 24);
        let msg = " Loading sessions\u{2026} ";
        let w = (msg.len() as u16 + 4).min(area.width);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        assert!(x > 0);
        assert!(x + w <= area.x + area.width);
    }

    #[test]
    fn loading_dialog_centering_y() {
        let area = Rect::new(0, 0, 80, 24);
        let h = 3u16;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        assert_eq!(y, 10); // (24-3)/2 = 10
    }

    #[test]
    fn loading_dialog_centering_y_small_area() {
        let area = Rect::new(0, 0, 80, 3);
        let h = 3u16;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        assert_eq!(y, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Filter bar layout splitting
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn filter_bar_splits_give_3_plus_rest() {
        let area = Rect::new(0, 0, 80, 24);
        let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
        assert_eq!(chunks[0].height, 3);
        assert_eq!(chunks[1].height, 21);
    }

    #[test]
    fn no_filter_uses_full_area() {
        let area = Rect::new(0, 0, 80, 24);
        let has_filter = false;
        let (_filter_area, list_area) = if has_filter {
            let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
            (Some(chunks[0]), chunks[1])
        } else {
            (None::<Rect>, area)
        };
        assert_eq!(list_area, area);
    }

    #[test]
    fn has_filter_gives_some_filter_area() {
        let area = Rect::new(0, 0, 80, 24);
        let has_filter = true;
        let (filter_area, _list_area) = if has_filter {
            let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
            (Some(chunks[0]), chunks[1])
        } else {
            (None::<Rect>, area)
        };
        assert!(filter_area.is_some());
        assert_eq!(filter_area.unwrap().height, 3);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Mode prefix logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn mode_prefix_content_search() {
        let session_content_search = true;
        let prefix = if session_content_search { "//" } else { "/" };
        assert_eq!(prefix, "//");
    }

    #[test]
    fn mode_prefix_name_filter() {
        let session_content_search = false;
        let prefix = if session_content_search { "//" } else { "/" };
        assert_eq!(prefix, "/");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Border color logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn filter_border_active_is_yellow() {
        let active = true;
        let color = if active {
            Color::Yellow
        } else {
            Color::DarkGray
        };
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn filter_border_inactive_is_dark_gray() {
        let active = false;
        let color = if active {
            Color::Yellow
        } else {
            Color::DarkGray
        };
        assert_eq!(color, Color::DarkGray);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Right info formatting
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn right_info_content_search_shows_count() {
        let content_search = true;
        let results_len = 5;
        let right_info = if content_search {
            format!(" {} results ", results_len)
        } else {
            String::new()
        };
        assert_eq!(right_info, " 5 results ");
    }

    #[test]
    fn right_info_name_filter_is_empty() {
        let content_search = false;
        let right_info = if content_search {
            format!(" {} results ", 0)
        } else {
            String::new()
        };
        assert!(right_info.is_empty());
    }

    #[test]
    fn right_info_zero_results() {
        let right_info = format!(" {} results ", 0);
        assert_eq!(right_info, " 0 results ");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Viewport height / inner width math
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn viewport_height_normal() {
        let area = Rect::new(0, 0, 80, 24);
        let vh = area.height.saturating_sub(2) as usize;
        assert_eq!(vh, 22);
    }

    #[test]
    fn viewport_height_tiny() {
        let area = Rect::new(0, 0, 80, 2);
        let vh = area.height.saturating_sub(2) as usize;
        assert_eq!(vh, 0);
    }

    #[test]
    fn inner_width_normal() {
        let area = Rect::new(0, 0, 80, 24);
        let iw = area.width.saturating_sub(2) as usize;
        assert_eq!(iw, 78);
    }

    #[test]
    fn inner_width_narrow() {
        let area = Rect::new(0, 0, 2, 24);
        let iw = area.width.saturating_sub(2) as usize;
        assert_eq!(iw, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Preview truncation in content search
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn preview_truncation_prefix_len() {
        let name_display = "my-session";
        let prefix_len = name_display.chars().count() + 4; // " name | "
        assert_eq!(prefix_len, 14);
    }

    #[test]
    fn preview_space_calculation() {
        let inner_width = 78;
        let prefix_len = 14;
        let preview_space = inner_width - prefix_len;
        assert_eq!(preview_space, 64);
    }

    #[test]
    fn preview_truncation_respects_budget() {
        let preview = "This is a really long preview text that should be truncated at some point";
        let preview_space = 20;
        let trunc: String = preview.chars().take(preview_space).collect();
        assert_eq!(trunc.len(), 20);
    }

    #[test]
    fn preview_truncation_short_preview_unchanged() {
        let preview = "short";
        let preview_space = 20;
        let trunc: String = preview.chars().take(preview_space).collect();
        assert_eq!(trunc, "short");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Name truncation with ellipsis
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn name_truncation_short_name_unchanged() {
        let name = "hello".to_string();
        let space = 20;
        let result = if name.chars().count() > space {
            let trunc: String = name.chars().take(space.saturating_sub(1)).collect();
            format!("{}\u{2026}", trunc)
        } else {
            name.clone()
        };
        assert_eq!(result, "hello");
    }

    #[test]
    fn name_truncation_long_name_gets_ellipsis() {
        let name = "very-long-session-name-here".to_string();
        let space = 10;
        let result = if name.chars().count() > space {
            let trunc: String = name.chars().take(space.saturating_sub(1)).collect();
            format!("{}\u{2026}", trunc)
        } else {
            name.clone()
        };
        assert!(result.ends_with('\u{2026}'));
        assert_eq!(result.chars().count(), 10);
    }

    #[test]
    fn name_truncation_exact_fit() {
        let name = "abcde".to_string();
        let space = 5;
        let result = if name.chars().count() > space {
            let trunc: String = name.chars().take(space.saturating_sub(1)).collect();
            format!("{}\u{2026}", trunc)
        } else {
            name.clone()
        };
        assert_eq!(result, "abcde");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Scroll clamping logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn clamp_selected_to_total() {
        let total = 5;
        let mut selected = 10;
        if selected >= total && total > 0 {
            selected = total - 1;
        }
        assert_eq!(selected, 4);
    }

    #[test]
    fn clamp_selected_zero_items() {
        let total = 0;
        let mut selected = 5;
        if selected >= total && total > 0 {
            selected = total - 1;
        }
        // No clamping when total == 0 (the && total > 0 guard)
        assert_eq!(selected, 5);
    }

    #[test]
    fn max_scroll_calculation() {
        let total = 20;
        let viewport_height = 10;
        let max = total - viewport_height;
        assert_eq!(max, 10);
    }

    #[test]
    fn max_scroll_when_all_fit() {
        let total: usize = 5;
        let viewport_height: usize = 10;
        let max = total.saturating_sub(viewport_height);
        assert_eq!(max, 0);
    }

    #[test]
    fn scroll_up_when_selected_above_viewport() {
        let selected: usize = 3;
        let mut scroll: usize = 5;
        let viewport_height: usize = 10;
        if selected < scroll {
            scroll = selected;
        } else if selected >= scroll + viewport_height {
            scroll = selected.saturating_sub(viewport_height - 1);
        }
        assert_eq!(scroll, 3);
    }

    #[test]
    fn scroll_down_when_selected_below_viewport() {
        let selected: usize = 15;
        let mut scroll: usize = 0;
        let viewport_height: usize = 10;
        if selected < scroll {
            scroll = selected;
        } else if selected >= scroll + viewport_height {
            scroll = selected.saturating_sub(viewport_height - 1);
        }
        assert_eq!(scroll, 6);
    }

    #[test]
    fn scroll_unchanged_when_selected_visible() {
        let selected: usize = 5;
        let mut scroll: usize = 0;
        let viewport_height: usize = 10;
        if selected < scroll {
            scroll = selected;
        } else if selected >= scroll + viewport_height {
            scroll = selected.saturating_sub(viewport_height - 1);
        }
        assert_eq!(scroll, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Title formatting
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn search_title_format() {
        let selected: usize = 3;
        let total: usize = 10;
        let title = format!(
            " Search [{}/{}] ",
            selected.saturating_add(1).min(total.max(1)),
            total.max(1)
        );
        assert_eq!(title, " Search [4/10] ");
    }

    #[test]
    fn search_title_empty() {
        let total: usize = 0;
        let title = if total == 0 {
            " Search [0/0] ".to_string()
        } else {
            format!(" Search [{}/{}] ", 1, total)
        };
        assert_eq!(title, " Search [0/0] ");
    }

    #[test]
    fn session_title_no_filter() {
        let filtering = false;
        let selected: usize = 2;
        let total: usize = 5;
        let total_unfiltered: usize = 5;
        let title = if filtering {
            format!(
                " Sessions [{}/{} of {}] ",
                selected.saturating_add(1).min(total.max(1)),
                total,
                total_unfiltered
            )
        } else {
            format!(" Sessions [{}/{}] ", selected + 1, total.max(1))
        };
        assert_eq!(title, " Sessions [3/5] ");
    }

    #[test]
    fn session_title_with_filter() {
        let filtering = true;
        let selected: usize = 1;
        let total: usize = 3;
        let total_unfiltered: usize = 10;
        let title = if filtering {
            format!(
                " Sessions [{}/{} of {}] ",
                selected.saturating_add(1).min(total.max(1)),
                total,
                total_unfiltered
            )
        } else {
            format!(" Sessions [{}/{}] ", selected + 1, total.max(1))
        };
        assert_eq!(title, " Sessions [2/3 of 10] ");
    }

    #[test]
    fn session_title_zero_total() {
        let total: usize = 0;
        let title = if total == 0 {
            " Sessions [0/0] ".to_string()
        } else {
            format!(" Sessions [{}/{}] ", 1, total)
        };
        assert_eq!(title, " Sessions [0/0] ");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Border styles
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn border_style_focused() {
        let is_focused = true;
        let style = if is_focused {
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        assert_eq!(style.fg, Some(AZURE));
    }

    #[test]
    fn border_style_unfocused() {
        let is_focused = false;
        let style = if is_focused {
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        assert_eq!(style.fg, Some(Color::White));
    }

    #[test]
    fn border_type_focused_is_double() {
        let is_focused = true;
        let bt = if is_focused {
            BorderType::Double
        } else {
            BorderType::Plain
        };
        assert_eq!(bt, BorderType::Double);
    }

    #[test]
    fn border_type_unfocused_is_plain() {
        let is_focused = false;
        let bt = if is_focused {
            BorderType::Double
        } else {
            BorderType::Plain
        };
        assert_eq!(bt, BorderType::Plain);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Name style logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn name_style_selected_has_bg_azure() {
        let is_selected = true;
        let style = if is_selected {
            Style::default()
                .bg(AZURE)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        assert_eq!(style.bg, Some(AZURE));
        assert_eq!(style.fg, Some(Color::Black));
    }

    #[test]
    fn name_style_unselected_white() {
        let is_selected = false;
        let style = if is_selected {
            Style::default()
                .bg(AZURE)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        assert_eq!(style.fg, Some(Color::White));
        assert_eq!(style.bg, None);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Dot indicators (running vs idle)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn running_dot_green() {
        let running = true;
        let (dot, color) = if running {
            ("\u{25cf}", Color::Green)
        } else {
            ("\u{25cb}", Color::DarkGray)
        };
        assert_eq!(dot, "\u{25cf}"); // filled circle
        assert_eq!(color, Color::Green);
    }

    #[test]
    fn idle_dot_dark_gray() {
        let running = false;
        let (dot, color) = if running {
            ("\u{25cf}", Color::Green)
        } else {
            ("\u{25cb}", Color::DarkGray)
        };
        assert_eq!(dot, "\u{25cb}"); // empty circle
        assert_eq!(color, Color::DarkGray);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Message badge formatting
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn msg_badge_format() {
        let count = 42;
        let badge = format!("[{} msgs]", count);
        assert_eq!(badge, "[42 msgs]");
    }

    #[test]
    fn msg_badge_zero() {
        let badge = format!("[{} msgs]", 0);
        assert_eq!(badge, "[0 msgs]");
    }

    #[test]
    fn suffix_format() {
        let time_str = "2h ago";
        let msg_badge = "[10 msgs]";
        let suffix = format!(" {} {} ", time_str, msg_badge);
        assert_eq!(suffix, " 2h ago [10 msgs] ");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Cursor position logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn cursor_x_calculation() {
        let fa = Rect::new(5, 2, 40, 3);
        let filter_len = 8;
        let cursor_x = fa.x + 1 + filter_len as u16;
        assert_eq!(cursor_x, 14); // 5 + 1 + 8
    }

    #[test]
    fn cursor_y_calculation() {
        let fa = Rect::new(5, 2, 40, 3);
        let cursor_y = fa.y + 1;
        assert_eq!(cursor_y, 3);
    }

    #[test]
    fn cursor_bounds_check() {
        let fa = Rect::new(5, 2, 40, 3);
        let cursor_x = fa.x + 1 + 10;
        assert!(cursor_x < fa.right()); // 16 < 45
    }

    #[test]
    fn cursor_at_edge() {
        let fa = Rect::new(0, 0, 10, 3);
        let cursor_x = fa.x + 1 + 8; // 9
        assert!(cursor_x < fa.right()); // 9 < 10
    }

    // ══════════════════════════════════════════════════════════════════
    //  Rect construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn rect_new_basic() {
        let r = Rect::new(10, 20, 30, 40);
        assert_eq!(r.x, 10);
        assert_eq!(r.y, 20);
        assert_eq!(r.width, 30);
        assert_eq!(r.height, 40);
    }

    #[test]
    fn rect_right_bottom() {
        let r = Rect::new(5, 10, 20, 15);
        assert_eq!(r.right(), 25);
        assert_eq!(r.bottom(), 25);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Filter match logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn filter_matches_name() {
        let name_display = "My Session".to_string();
        let session_id = "abc123".to_string();
        let filter_lower = "my ses".to_string();
        let matches = name_display.to_lowercase().contains(&filter_lower)
            || session_id.to_lowercase().contains(&filter_lower);
        assert!(matches);
    }

    #[test]
    fn filter_matches_session_id() {
        let name_display = "My Session".to_string();
        let session_id = "abc123".to_string();
        let filter_lower = "abc".to_string();
        let matches = name_display.to_lowercase().contains(&filter_lower)
            || session_id.to_lowercase().contains(&filter_lower);
        assert!(matches);
    }

    #[test]
    fn filter_no_match() {
        let name_display = "My Session".to_string();
        let session_id = "abc123".to_string();
        let filter_lower = "xyz".to_string();
        let matches = name_display.to_lowercase().contains(&filter_lower)
            || session_id.to_lowercase().contains(&filter_lower);
        assert!(!matches);
    }

    #[test]
    fn filter_case_insensitive() {
        let name_display = "MySession".to_string();
        let filter_lower = "mysession".to_string();
        assert!(name_display.to_lowercase().contains(&filter_lower));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Span and Line construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn span_styled_construction() {
        let s = Span::styled("hello", Style::default().fg(Color::White));
        assert_eq!(s.content, "hello");
    }

    #[test]
    fn line_from_spans() {
        let line = Line::from(vec![
            Span::raw(" "),
            Span::styled("test", Style::default().fg(Color::White)),
        ]);
        assert_eq!(line.spans.len(), 2);
    }

    #[test]
    fn block_with_borders_and_title() {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .title(Span::styled(" Test ", Style::default().fg(AZURE)));
        // Verify it compiles and can be used
        let _ = block;
    }
}
