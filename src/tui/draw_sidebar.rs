//! Sidebar rendering for Worktrees panel

use ratatui::{
    layout::{Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::app::{App, Focus, SidebarRowAction};
use super::util::{truncate, AZURE};

/// Build sidebar items and row→action map for mouse click handling.
/// Flat list of worktrees — no session file dropdowns or chevrons.
/// Selected item gets a highlight box (blue bg, white text) like the file tree.
fn build_sidebar_items(app: &App) -> (Vec<ListItem<'static>>, Vec<SidebarRowAction>) {
    let mut items: Vec<ListItem> = Vec::new();
    let mut row_map: Vec<SidebarRowAction> = Vec::new();
    let filter = app.sidebar_filter.to_lowercase();

    let Some(ref project) = app.project else {
        return (items, row_map);
    };

    // If project name matches filter, show everything
    let project_matches = !filter.is_empty() && project.name.to_lowercase().contains(&filter);

    for (sess_idx, session) in app.worktrees.iter().enumerate() {
        // Filter: skip worktrees that don't match
        if !filter.is_empty() && !project_matches
            && !session.name().to_lowercase().contains(&filter)
        {
            continue;
        }

        let is_selected = app.selected_worktree == Some(sess_idx);
        let status = session.status(app.is_session_running(&session.branch_name));

        // Archived get diamond icon; active worktrees use status symbol
        let (status_symbol, status_color, name_style) = if session.archived {
            let ns = if is_selected {
                Style::default().bg(Color::Blue).fg(Color::DarkGray).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            ("◇", Color::DarkGray, ns)
        } else {
            let ns = if is_selected {
                Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            (status.symbol(), status.color(), ns)
        };

        items.push(ListItem::new(Line::from(vec![
            Span::raw(" "),
            Span::styled(status_symbol, Style::default().fg(status_color)),
            Span::raw(" "),
            Span::styled(truncate(session.name(), 36), name_style),
        ])));
        row_map.push(SidebarRowAction::Worktree(sess_idx));
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
        // Count visible worktrees (Session actions in row_map), not total rows
        let match_count = app.sidebar_row_map.iter()
            .filter(|a| matches!(a, SidebarRowAction::Worktree(_)))
            .count();
        let total = app.worktrees.len();
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

    // Show project name in border title, or main browse indicator
    let (title_text, title_color) = if app.browsing_main {
        let branch = app.project.as_ref().map(|p| p.main_branch.as_str()).unwrap_or("main");
        (format!(" ★ {} (read-only) ", branch), Color::Yellow)
    } else if let Some(ref project) = app.project {
        (format!(" {} ", project.name), if is_focused { AZURE } else { Color::White })
    } else {
        (" Worktrees ".to_string(), if is_focused { AZURE } else { Color::White })
    };

    let sidebar = List::new(app.sidebar_cache.clone())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
                .title(Span::styled(title_text, Style::default().fg(title_color).add_modifier(if is_focused { Modifier::BOLD } else { Modifier::empty() })))
                .border_style(if is_focused {
                    Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                }),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_widget(sidebar, list_area);
}

/// Draw the file tree overlay in the Worktrees pane (replaces sidebar when 'f' is pressed).
/// Delegates to draw_file_tree which handles its own caching and rendering.
pub fn draw_file_tree_overlay(f: &mut Frame, app: &mut App, area: Rect) {
    super::draw_file_tree::draw_file_tree(f, app, area);
}
