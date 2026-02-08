//! Sidebar rendering for Worktrees panel

use ratatui::{
    layout::{Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::app::{App, Focus, SidebarRowAction};
use super::util::truncate;

/// Build sidebar items and row→action map for mouse click handling.
/// Each ListItem pushed gets a corresponding SidebarRowAction pushed to row_map.
/// When sidebar_filter is non-empty, only sessions matching the filter are shown.
fn build_sidebar_items(app: &App) -> (Vec<ListItem<'static>>, Vec<SidebarRowAction>) {
    let mut items: Vec<ListItem> = Vec::new();
    let mut row_map: Vec<SidebarRowAction> = Vec::new();
    // Load custom session names once for all lookups (only called on sidebar rebuild, not per-frame)
    let session_names = app.load_all_session_names();
    // Pre-lowercase the filter once for all comparisons
    let filter = app.sidebar_filter.to_lowercase();

    if let Some(ref project) = app.project {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(project.name.clone(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ])));
        row_map.push(SidebarRowAction::ProjectHeader);

        for (sess_idx, session) in app.sessions.iter().enumerate() {
            // Skip sessions that don't match the filter.
            // Matches on: worktree name, session file UUIDs, and custom session names.
            if !filter.is_empty() {
                let name_match = session.name().to_lowercase().contains(&filter);
                let file_match = app.session_files.get(&session.branch_name).map(|files| {
                    files.iter().any(|(sid, _, _)| {
                        sid.to_lowercase().contains(&filter)
                            || session_names.get(sid.as_str()).map(|n| n.to_lowercase().contains(&filter)).unwrap_or(false)
                    })
                }).unwrap_or(false);
                if !name_match && !file_match { continue; }
            }

            let is_selected = app.selected_session == Some(sess_idx);
            let is_expanded = app.sessions_expanded.contains(&session.branch_name);
            let status = session.status(&app.running_sessions);
            let status_color = status.color();

            // Active worktree: cyan text like project name (no background)
            let style = if is_selected {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };

            // Chevron indicates expandable dropdown
            let chevron = if is_expanded { "▼" } else { "▶" };
            let prefix = if session.archived { " ◌" } else { "" };

            items.push(ListItem::new(Line::from(vec![
                Span::styled(format!(" {}{} ", chevron, prefix), Style::default().fg(Color::DarkGray)),
                Span::styled(status.symbol(), Style::default().fg(status_color)),
                Span::raw(" "),
                Span::styled(truncate(session.name(), 34), style),
            ])));
            row_map.push(SidebarRowAction::Session(sess_idx));

            // If expanded, show session file dropdown
            if is_expanded {
                let files = app.session_files.get(&session.branch_name);
                let selected_idx = *app.session_selected_file_idx.get(&session.branch_name).unwrap_or(&0);

                if let Some(files) = files {
                    for (j, (session_id, _path, time_str)) in files.iter().enumerate() {
                        let is_file_selected = j == selected_idx;
                        // Active Claude session file: cyan text (like project/worktree)
                        let file_style = if is_file_selected {
                            Style::default().fg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        };
                        // Show custom name if available, otherwise truncated UUID
                        let id_display = if let Some(name) = session_names.get(session_id.as_str()) {
                            truncate(name, 24)
                        } else if session_id.len() > 16 {
                            format!("{}…", &session_id[..15])
                        } else {
                            session_id.clone()
                        };
                        items.push(ListItem::new(Line::from(vec![
                            Span::raw("     "),
                            Span::styled(id_display, file_style),
                            Span::raw(" "),
                            Span::styled(time_str.clone(), Style::default().fg(Color::DarkGray)),
                        ])));
                        row_map.push(SidebarRowAction::SessionFile(sess_idx, j));
                    }
                } else {
                    items.push(ListItem::new(Line::from(vec![
                        Span::raw("     "),
                        Span::styled("(no sessions)", Style::default().fg(Color::DarkGray)),
                    ])));
                    // "(no sessions)" placeholder — clicking does nothing useful,
                    // but map to the parent session so focus still works
                    row_map.push(SidebarRowAction::Session(sess_idx));
                }
            }
        }
    } else {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("No project", Style::default().fg(Color::Red)),
        ])));
        row_map.push(SidebarRowAction::ProjectHeader);
    }

    (items, row_map)
}

/// Draw the sidebar showing project and sessions
pub fn draw_sidebar(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.focus == Focus::Worktrees;

    // Only rebuild sidebar items if cache is dirty or focus changed (styling depends on focus)
    if app.sidebar_dirty || app.sidebar_focus_cached != is_focused {
        let (items, row_map) = build_sidebar_items(app);
        app.sidebar_cache = items;
        app.sidebar_row_map = row_map;
        app.sidebar_dirty = false;
        app.sidebar_focus_cached = is_focused;
    }

    // Split area: filter bar (1 line + borders = 3) at top when filter is active or has text
    let has_filter = app.sidebar_filter_active || !app.sidebar_filter.is_empty();
    let (filter_area, list_area) = if has_filter {
        let chunks = Layout::vertical([
            ratatui::layout::Constraint::Length(3),
            ratatui::layout::Constraint::Min(1),
        ]).split(area);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, area)
    };

    // Draw filter input bar when active
    if let Some(fa) = filter_area {
        let match_count = app.sidebar_cache.len().saturating_sub(1); // subtract project header
        let total = app.sessions.len();
        let title = format!(" {}/{} ", match_count, total);
        let border_color = if app.sidebar_filter_active { Color::Yellow } else { Color::DarkGray };
        let filter_widget = Paragraph::new(app.sidebar_filter.clone())
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled("🔍", Style::default()))
                .title(Line::from(Span::styled(title, Style::default().fg(Color::DarkGray))).alignment(ratatui::layout::Alignment::Right)),
            );
        f.render_widget(filter_widget, fa);

        // Show cursor in filter bar when actively typing
        if app.sidebar_filter_active {
            let cursor_x = fa.x + 1 + app.sidebar_filter.len() as u16;
            let cursor_y = fa.y + 1;
            if cursor_x < fa.right() {
                f.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }

    let sidebar = List::new(app.sidebar_cache.clone())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
                .title(if is_focused {
                    Span::styled(" Worktrees ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                } else {
                    Span::styled(" Worktrees ", Style::default().fg(Color::White))
                })
                .border_style(if is_focused {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                }),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_widget(sidebar, list_area);
}
