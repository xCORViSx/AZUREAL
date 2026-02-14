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

use crate::app::{App, Focus};
use super::super::util::AZURE;

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
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(AZURE))
                .title(Span::styled(" Sessions ", Style::default().fg(AZURE).add_modifier(Modifier::BOLD))));
        f.render_widget(dialog, Rect::new(x, y, w, h));
        return;
    }

    let is_focused = app.focus == Focus::Output;

    // Split area: filter bar at top when filter is active or has text
    let has_filter = app.session_filter_active || !app.session_filter.is_empty();
    let (filter_area, list_area) = if has_filter {
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
        ]).split(area);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, area)
    };

    // Draw filter input bar when active
    if let Some(fa) = filter_area {
        let mode_prefix = if app.session_content_search { "//" } else { "/" };
        let border_color = if app.session_filter_active { Color::Yellow } else { Color::DarkGray };
        let right_info = if app.session_content_search {
            format!(" {} results ", app.session_search_results.len())
        } else {
            String::new()
        };
        let filter_widget = Paragraph::new(app.session_filter.clone())
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled(mode_prefix, Style::default().fg(Color::Yellow)))
                .title(Line::from(Span::styled(right_info, Style::default().fg(Color::DarkGray))).alignment(Alignment::Right)),
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
}

/// Render content search results (triggered by "//" prefix in filter)
fn draw_content_search(
    f: &mut Frame, app: &mut App, list_area: Rect,
    viewport_height: usize, inner_width: usize, is_focused: bool,
) {
    let session_names = app.load_all_session_names();
    let mut rows: Vec<Line<'static>> = Vec::new();
    for (idx, (_row, session_id, preview)) in app.session_search_results.iter().enumerate() {
        let is_selected = idx == app.session_list_selected;
        let name_display = session_names.get(session_id.as_str())
            .cloned()
            .unwrap_or_else(|| session_id.chars().take(12).collect::<String>());
        let bg = if is_selected { Style::default().bg(AZURE).fg(Color::Black) } else { Style::default() };
        let name_style = if is_selected {
            Style::default().bg(AZURE).fg(Color::Black).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        // Truncate preview to fit
        let prefix_len = name_display.chars().count() + 4; // " name | "
        let preview_space = inner_width.saturating_sub(prefix_len);
        let trunc_preview: String = preview.chars().take(preview_space).collect();

        rows.push(Line::from(vec![
            Span::styled(format!(" {} ", name_display), name_style),
            Span::styled("\u{2502} ", if is_selected { bg } else { Style::default().fg(Color::DarkGray) }),
            Span::styled(trunc_preview, if is_selected { bg } else { Style::default().fg(Color::DarkGray) }),
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
        app.session_list_scroll = app.session_list_selected.saturating_sub(viewport_height - 1);
    }
    app.session_list_scroll = app.session_list_scroll.min(max_scroll);
    let display: Vec<Line> = rows.into_iter().skip(app.session_list_scroll).take(viewport_height).collect();
    let title = format!(" Search [{}/{}] ", app.session_list_selected.saturating_add(1).min(total.max(1)), total.max(1));
    let border_style = if is_focused {
        Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
    } else { Style::default().fg(Color::White) };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
        .title(Span::styled(title, border_style))
        .border_style(border_style);
    f.render_widget(Paragraph::new(display).block(block), list_area);
}

/// Render the normal session name list (with optional name filter)
fn draw_name_list(
    f: &mut Frame, app: &mut App, list_area: Rect,
    viewport_height: usize, inner_width: usize, is_focused: bool,
) {
    let session_names = app.load_all_session_names();
    let filter_lower = app.session_filter.to_lowercase();
    let filtering = !filter_lower.is_empty();
    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut total_unfiltered = 0usize;

    let branch = app.current_session().map(|s| s.branch_name.clone());
    let files = branch.as_deref().and_then(|b| app.session_files.get(b));

    if let Some(files) = files {
        for (session_id, _path, time_str) in files.iter() {
            total_unfiltered += 1;
            let name_display = session_names.get(session_id.as_str())
                .cloned()
                .unwrap_or_else(|| session_id.clone());

            // Name filter: skip rows that don't match session name or session id
            if filtering {
                let matches = name_display.to_lowercase().contains(&filter_lower)
                    || session_id.to_lowercase().contains(&filter_lower);
                if !matches { continue; }
            }

            let msg_count = app.session_msg_counts.get(session_id).map(|&(c, _)| c).unwrap_or(0);
            let msg_badge = format!("[{} msgs]", msg_count);
            let suffix = format!(" {} {} ", time_str, msg_badge);
            // Row: " ● session_name    mtime [N msgs]"
            let name_space = inner_width.saturating_sub(3 + suffix.chars().count());
            let truncated_name = if name_display.chars().count() > name_space {
                let trunc: String = name_display.chars().take(name_space.saturating_sub(1)).collect();
                format!("{}\u{2026}", trunc)
            } else {
                name_display
            };
            let pad = name_space.saturating_sub(truncated_name.chars().count());

            let is_selected = rows.len() == app.session_list_selected;
            let name_style = if is_selected {
                Style::default().bg(AZURE).fg(Color::Black).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let bg_style = if is_selected {
                Style::default().bg(AZURE).fg(Color::Black)
            } else {
                Style::default()
            };

            // Green dot for running sessions, dim circle for idle
            let running = app.is_claude_session_running(session_id);
            let (dot, dot_color) = if running { ("●", Color::Green) } else { ("○", Color::DarkGray) };

            rows.push(Line::from(vec![
                Span::styled(" ", bg_style),
                Span::styled(dot, if is_selected { bg_style } else { Style::default().fg(dot_color) }),
                Span::styled(" ", bg_style),
                Span::styled(truncated_name, name_style),
                Span::styled(" ".repeat(pad), bg_style),
                Span::styled(format!(" {} ", time_str), if is_selected { bg_style } else { Style::default().fg(Color::DarkGray) }),
                Span::styled(msg_badge, if is_selected { bg_style } else { Style::default().fg(AZURE) }),
            ]));
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
        app.session_list_scroll = app.session_list_selected.saturating_sub(viewport_height - 1);
    }
    app.session_list_scroll = app.session_list_scroll.min(max_scroll);

    let display: Vec<Line> = rows.into_iter()
        .skip(app.session_list_scroll)
        .take(viewport_height)
        .collect();

    // Title includes worktree name since list is scoped to current worktree
    let wt_label = app.current_session().map(|s| s.name().to_string()).unwrap_or_default();
    let title = if filtering {
        format!(" {} [{}/{} of {}] ", wt_label, app.session_list_selected.saturating_add(1).min(total.max(1)), total, total_unfiltered)
    } else {
        format!(" {} [{}/{}] ", wt_label, app.session_list_selected + 1, total.max(1))
    };
    let border_style = if is_focused {
        Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
        .title(Span::styled(title, border_style))
        .border_style(border_style);

    let widget = Paragraph::new(display).block(block);
    f.render_widget(widget, list_area);
}
