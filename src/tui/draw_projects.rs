//! Projects panel rendering — full-screen modal for project selection
//!
//! Shown on startup when not in a git repo, or opened with 'P' from Worktrees pane.
//! Renders a centered modal with project list, input field for Add/Rename/Init modes,
//! and a key hints bar at the bottom.

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use crate::app::types::ProjectsPanelMode;
use crate::config::display_path;
use super::keybindings;
use super::util::AZURE;

/// Draw the full-screen Projects panel modal.
/// Takes over the entire screen — caller should return early after this.
pub fn draw_projects_panel(f: &mut Frame, app: &App) {
    let Some(ref panel) = app.projects_panel else { return };
    let area = f.area();

    // Center a modal box (60% width, 70% height, min 40x10)
    let modal_w = (area.width * 60 / 100).max(40).min(area.width);
    let modal_h = (area.height * 70 / 100).max(10).min(area.height);
    let modal = Rect::new(
        area.x + (area.width.saturating_sub(modal_w)) / 2,
        area.y + (area.height.saturating_sub(modal_h)) / 2,
        modal_w,
        modal_h,
    );

    // Clear the background behind the modal
    f.render_widget(Clear, modal);

    // Build the project list lines
    let inner_w = modal.width.saturating_sub(4) as usize; // 2 border + 2 padding
    let mut lines: Vec<Line> = Vec::new();

    if panel.entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No projects registered. Press 'a' to add one.",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Check which project is currently loaded (for green dot indicator)
    let current_path = app.project.as_ref().map(|p| &p.path);

    for (i, entry) in panel.entries.iter().enumerate() {
        let is_selected = i == panel.selected;
        let is_current = current_path.map(|p| p == &entry.path).unwrap_or(false);

        // Green dot for currently loaded project, space otherwise
        let indicator = if is_current { "● " } else { "  " };
        let indicator_color = if is_current { Color::Green } else { Color::DarkGray };

        // Truncate display name and path to fit within modal width
        let path_str = display_path(&entry.path);
        let name_max = (inner_w / 3).max(10);
        let name_display = if entry.display_name.len() > name_max {
            format!("{}…", &entry.display_name[..name_max - 1])
        } else {
            entry.display_name.clone()
        };

        // Pad name to align paths
        let padded_name = format!("{:<width$}", name_display, width = name_max + 2);

        let style = if is_selected {
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        lines.push(Line::from(vec![
            Span::styled(indicator, Style::default().fg(indicator_color)),
            Span::styled(padded_name, style),
            Span::styled(path_str, Style::default().fg(Color::DarkGray)),
        ]));
    }

    // Build key hints for the bottom of the modal — browse mode from keybindings.rs
    let hints: Vec<Span> = match panel.mode {
        ProjectsPanelMode::Browse => {
            let pairs = keybindings::projects_browse_hint_pairs(app.project.is_some());
            let mut h = vec![Span::raw(" ")];
            for (key, label) in pairs {
                h.push(Span::styled(key, Style::default().fg(AZURE)));
                h.push(Span::styled(format!(":{} ", label), Style::default().fg(Color::DarkGray)));
            }
            h
        }
        ProjectsPanelMode::AddPath => vec![
            Span::styled(" Enter", Style::default().fg(AZURE)),
            Span::styled(":confirm ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc", Style::default().fg(AZURE)),
            Span::styled(":cancel", Style::default().fg(Color::DarkGray)),
        ],
        ProjectsPanelMode::Rename => vec![
            Span::styled(" Enter", Style::default().fg(AZURE)),
            Span::styled(":save ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc", Style::default().fg(AZURE)),
            Span::styled(":cancel", Style::default().fg(Color::DarkGray)),
        ],
        ProjectsPanelMode::Init => vec![
            Span::styled(" Enter", Style::default().fg(AZURE)),
            Span::styled(":init (blank=cwd) ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc", Style::default().fg(AZURE)),
            Span::styled(":cancel", Style::default().fg(Color::DarkGray)),
        ],
    };

    // Split modal: list area + error + input field + key hints
    let input_height = if panel.mode != ProjectsPanelMode::Browse { 3 } else { 0 };
    let error_height: u16 = if panel.error.is_some() { 1 } else { 0 };
    let chunks = Layout::vertical([
        Constraint::Min(3),                                  // project list
        Constraint::Length(error_height),                    // error (visible in ANY mode)
        Constraint::Length(input_height),                    // input field (only in input modes)
        Constraint::Length(1),                               // key hints
    ]).split(modal);

    // Render the project list with border
    let list_widget = Paragraph::new(lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(AZURE))
            .title(Span::styled(" Projects ", Style::default().fg(AZURE).add_modifier(Modifier::BOLD)))
            .title(Line::from(Span::styled(
                format!(" {} ", panel.entries.len()),
                Style::default().fg(Color::DarkGray),
            )).alignment(Alignment::Right))
        );
    f.render_widget(list_widget, chunks[0]);

    // Error message — shown in ALL modes (Browse, AddPath, Rename, Init)
    if let Some(ref err) = panel.error {
        let err_line = Line::from(Span::styled(
            format!("  {}", err),
            Style::default().fg(Color::Red),
        ));
        f.render_widget(Paragraph::new(err_line), chunks[1]);
    }

    // Render input field when in AddPath/Rename/Init mode
    if panel.mode != ProjectsPanelMode::Browse {
        let prompt = match panel.mode {
            ProjectsPanelMode::AddPath => " Path: ",
            ProjectsPanelMode::Rename => " Name: ",
            ProjectsPanelMode::Init => " Init path (blank=cwd): ",
            _ => "",
        };

        let input_line = Line::from(vec![
            Span::styled(prompt, Style::default().fg(AZURE)),
            Span::raw(&panel.input),
        ]);
        let input_widget = Paragraph::new(input_line)
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
            );
        f.render_widget(input_widget, chunks[2]);

        // Cursor position in input field
        let cursor_x = chunks[2].x + 1 + prompt.len() as u16 + panel.input_cursor as u16;
        let cursor_y = chunks[2].y + 1;
        if cursor_x < chunks[2].right() {
            f.set_cursor_position((cursor_x, cursor_y));
        }
    }

    // Render key hints bar
    f.render_widget(Paragraph::new(Line::from(hints)), chunks[3]);
}
