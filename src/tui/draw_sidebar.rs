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
/// Each ListItem pushed gets a corresponding SidebarRowAction pushed to row_map.
///
/// Hierarchical filter: searches project name, worktree names, session names/UUIDs simultaneously.
/// Matching items are shown with their parent hierarchy preserved — e.g. a matching session file
/// appears under its worktree and project header even if those parents don't match the filter.
fn build_sidebar_items(app: &App) -> (Vec<ListItem<'static>>, Vec<SidebarRowAction>) {
    let mut items: Vec<ListItem> = Vec::new();
    let mut row_map: Vec<SidebarRowAction> = Vec::new();
    // Load custom session names once for all lookups (only called on sidebar rebuild, not per-frame)
    let session_names = app.load_all_session_names();
    // Pre-lowercase the filter once for all comparisons
    let filter = app.sidebar_filter.to_lowercase();

    let Some(ref project) = app.project else {
        return (items, row_map);
    };

    // If project name matches filter, show everything (no filtering below)
    let project_matches = !filter.is_empty() && project.name.to_lowercase().contains(&filter);

    for (sess_idx, session) in app.sessions.iter().enumerate() {
        // When filter is active, figure out what matched at each level:
        // - worktree_matches: the worktree name itself matches → show normally
        // - matching_file_indices: specific session files that match → auto-expand and show only those
        // - project_matches: project name matches → show everything
        let (show_worktree, matching_file_indices) = if filter.is_empty() || project_matches {
            (true, None) // no filter or project-level match — show all
        } else {
            let wt_matches = session.name().to_lowercase().contains(&filter);
            // Check which specific session files match
            let file_indices: Vec<usize> = app.session_files.get(&session.branch_name)
                .map(|files| {
                    files.iter().enumerate().filter_map(|(j, (sid, _, _))| {
                        let sid_match = sid.to_lowercase().contains(&filter);
                        let name_match = session_names.get(sid.as_str())
                            .map(|n| n.to_lowercase().contains(&filter))
                            .unwrap_or(false);
                        if sid_match || name_match { Some(j) } else { None }
                    }).collect()
                })
                .unwrap_or_default();

            if wt_matches {
                (true, None) // worktree matches → show it normally (all files if expanded)
            } else if !file_indices.is_empty() {
                (true, Some(file_indices)) // only children match → show worktree as parent + matching files
            } else {
                (false, None) // nothing matches → skip
            }
        };
        if !show_worktree { continue; }

        let is_selected = app.selected_session == Some(sess_idx);
        // Auto-expand when filter matched at the session file level
        let is_expanded = matching_file_indices.is_some()
            || app.sessions_expanded.contains(&session.branch_name);
        let status = session.status(&app.running_sessions);
        let status_color = status.color();

        // Active worktree: cyan text like project name (no background)
        let style = if is_selected {
            Style::default().fg(AZURE)
        } else {
            Style::default()
        };

        let chevron = if is_expanded { "▼" } else { "▶" };
        let prefix = if session.archived { " ◌" } else { "" };

        items.push(ListItem::new(Line::from(vec![
            Span::styled(format!(" {}{} ", chevron, prefix), Style::default().fg(Color::DarkGray)),
            Span::styled(status.symbol(), Style::default().fg(status_color)),
            Span::raw(" "),
            Span::styled(truncate(session.name(), 34), style),
        ])));
        row_map.push(SidebarRowAction::Session(sess_idx));

        // Show session file dropdown when expanded (either manually or auto-expanded by filter)
        if is_expanded {
            let files = app.session_files.get(&session.branch_name);
            let selected_idx = *app.session_selected_file_idx.get(&session.branch_name).unwrap_or(&0);

            if let Some(files) = files {
                for (j, (session_id, _path, time_str)) in files.iter().enumerate() {
                    // When filter matched specific files, only show those
                    if let Some(ref indices) = matching_file_indices {
                        if !indices.contains(&j) { continue; }
                    }
                    let is_file_selected = j == selected_idx;
                    let file_style = if is_file_selected {
                        Style::default().fg(AZURE)
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
            } else if matching_file_indices.is_none() {
                // Only show "(no sessions)" for manually expanded, not filter-expanded
                items.push(ListItem::new(Line::from(vec![
                    Span::raw("     "),
                    Span::styled("(no sessions)", Style::default().fg(Color::DarkGray)),
                ])));
                row_map.push(SidebarRowAction::Session(sess_idx));
            }
        }
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
            .filter(|a| matches!(a, SidebarRowAction::Session(_)))
            .count();
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

    // Show project name in border title: " Worktrees (projectname) "
    let title_text = if let Some(ref project) = app.project {
        format!(" {} ", project.name)
    } else {
        " Worktrees ".to_string()
    };

    let sidebar = List::new(app.sidebar_cache.clone())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
                .title(if is_focused {
                    Span::styled(title_text, Style::default().fg(AZURE).add_modifier(Modifier::BOLD))
                } else {
                    Span::styled(title_text, Style::default().fg(Color::White))
                })
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
