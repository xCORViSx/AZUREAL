//! Sidebar rendering for Worktrees panel

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem},
    Frame,
};

use crate::app::{App, Focus};
use super::util::truncate;

/// Format a SystemTime as a relative or absolute time string
fn format_time(mtime: std::time::SystemTime) -> String {
    let Ok(dur) = std::time::SystemTime::now().duration_since(mtime) else {
        return "future".to_string();
    };
    let secs = dur.as_secs();
    if secs < 60 { return format!("{}s ago", secs); }
    if secs < 3600 { return format!("{}m ago", secs / 60); }
    if secs < 86400 { return format!("{}h ago", secs / 3600); }
    if secs < 604800 { return format!("{}d ago", secs / 86400); }
    // Older than a week: show date
    let datetime = chrono::DateTime::<chrono::Local>::from(mtime);
    datetime.format("%b %d").to_string()
}

/// Draw the sidebar showing project and sessions
pub fn draw_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let mut items: Vec<ListItem> = Vec::new();

    if let Some(ref project) = app.project {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(&project.name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ])));

        for (sess_idx, session) in app.sessions.iter().enumerate() {
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

            // If expanded, show session file dropdown
            if is_expanded {
                let files = app.session_files.get(&session.branch_name);
                let selected_idx = *app.session_selected_file_idx.get(&session.branch_name).unwrap_or(&0);

                if let Some(files) = files {
                    for (j, (session_id, _path, mtime)) in files.iter().enumerate() {
                        let is_file_selected = j == selected_idx;
                        // Active Claude session file: cyan text (like project/worktree)
                        let file_style = if is_file_selected {
                            Style::default().fg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        };
                        let time_str = format_time(*mtime);
                        // Truncate session ID for display
                        let id_display = if session_id.len() > 16 {
                            format!("{}…", &session_id[..15])
                        } else {
                            session_id.clone()
                        };
                        items.push(ListItem::new(Line::from(vec![
                            Span::raw("     "),
                            Span::styled(id_display, file_style),
                            Span::raw(" "),
                            Span::styled(time_str, Style::default().fg(Color::DarkGray)),
                        ])));
                    }
                } else {
                    items.push(ListItem::new(Line::from(vec![
                        Span::raw("     "),
                        Span::styled("(no sessions)", Style::default().fg(Color::DarkGray)),
                    ])));
                }
            }
        }
    } else {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("No project", Style::default().fg(Color::Red)),
        ])));
    }

    let is_focused = app.focus == Focus::Worktrees;
    let sidebar = List::new(items)
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

    f.render_widget(sidebar, area);
}
