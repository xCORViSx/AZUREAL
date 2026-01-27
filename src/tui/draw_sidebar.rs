//! Sidebar rendering for Sessions panel

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem},
    Frame,
};

use crate::app::{App, Focus};
use super::util::truncate;

/// Draw the sidebar showing projects and sessions
pub fn draw_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let mut items: Vec<ListItem> = Vec::new();

    for (proj_idx, project) in app.projects.iter().enumerate() {
        let is_selected_proj = proj_idx == app.selected_project;
        let proj_style = if is_selected_proj {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };

        items.push(ListItem::new(Line::from(vec![
            Span::styled("▸ ", proj_style),
            Span::styled(&project.name, proj_style),
        ])));

        if is_selected_proj {
            for (sess_idx, session) in app.sessions.iter().enumerate() {
                let is_selected = app.selected_session == Some(sess_idx);
                let status_color = session.status.color();

                let style = if is_selected && app.focus == Focus::Sessions {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    Style::default()
                };

                items.push(ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(session.status.symbol(), Style::default().fg(status_color)),
                    Span::raw(" "),
                    Span::styled(truncate(&session.name, 22), style),
                ])));
            }
        }
    }

    let is_focused = app.focus == Focus::Sessions;
    let sidebar = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
                .title(if is_focused {
                    Span::styled(" Sessions ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                } else {
                    Span::styled(" Sessions ", Style::default().fg(Color::White))
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
