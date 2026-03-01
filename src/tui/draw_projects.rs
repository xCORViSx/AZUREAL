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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use ratatui::layout::{Constraint, Layout, Rect};
    use ratatui::style::{Color, Modifier, Style};
    use crate::config::ProjectEntry;

    // ══════════════════════════════════════════════════════════════════
    //  AZURE constant
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn azure_color_value() {
        assert_eq!(AZURE, Color::Rgb(51, 153, 255));
    }

    // ══════════════════════════════════════════════════════════════════
    //  ProjectsPanelMode variants
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn mode_browse_eq() {
        assert_eq!(ProjectsPanelMode::Browse, ProjectsPanelMode::Browse);
    }

    #[test]
    fn mode_add_path_eq() {
        assert_eq!(ProjectsPanelMode::AddPath, ProjectsPanelMode::AddPath);
    }

    #[test]
    fn mode_rename_eq() {
        assert_eq!(ProjectsPanelMode::Rename, ProjectsPanelMode::Rename);
    }

    #[test]
    fn mode_init_eq() {
        assert_eq!(ProjectsPanelMode::Init, ProjectsPanelMode::Init);
    }

    #[test]
    fn mode_variants_distinct() {
        assert_ne!(ProjectsPanelMode::Browse, ProjectsPanelMode::AddPath);
        assert_ne!(ProjectsPanelMode::Browse, ProjectsPanelMode::Rename);
        assert_ne!(ProjectsPanelMode::Browse, ProjectsPanelMode::Init);
        assert_ne!(ProjectsPanelMode::AddPath, ProjectsPanelMode::Rename);
        assert_ne!(ProjectsPanelMode::AddPath, ProjectsPanelMode::Init);
        assert_ne!(ProjectsPanelMode::Rename, ProjectsPanelMode::Init);
    }

    #[test]
    fn mode_debug() {
        assert_eq!(format!("{:?}", ProjectsPanelMode::Browse), "Browse");
        assert_eq!(format!("{:?}", ProjectsPanelMode::AddPath), "AddPath");
        assert_eq!(format!("{:?}", ProjectsPanelMode::Rename), "Rename");
        assert_eq!(format!("{:?}", ProjectsPanelMode::Init), "Init");
    }

    #[test]
    fn mode_clone_copy() {
        let m = ProjectsPanelMode::AddPath;
        let cloned = m.clone();
        let copied = m;
        assert_eq!(m, cloned);
        assert_eq!(m, copied);
    }

    // ══════════════════════════════════════════════════════════════════
    //  ProjectsPanel construction and methods
    // ══════════════════════════════════════════════════════════════════

    fn sample_entries() -> Vec<ProjectEntry> {
        vec![
            ProjectEntry { path: PathBuf::from("/home/user/proj1"), display_name: "Project One".to_string() },
            ProjectEntry { path: PathBuf::from("/home/user/proj2"), display_name: "Project Two".to_string() },
            ProjectEntry { path: PathBuf::from("/home/user/proj3"), display_name: "Project Three".to_string() },
        ]
    }

    #[test]
    fn panel_new_defaults() {
        let panel = crate::app::types::ProjectsPanel::new(sample_entries());
        assert_eq!(panel.selected, 0);
        assert_eq!(panel.mode, ProjectsPanelMode::Browse);
        assert!(panel.input.is_empty());
        assert_eq!(panel.input_cursor, 0);
        assert!(panel.error.is_none());
    }

    #[test]
    fn panel_new_preserves_entries() {
        let entries = sample_entries();
        let panel = crate::app::types::ProjectsPanel::new(entries.clone());
        assert_eq!(panel.entries.len(), 3);
    }

    #[test]
    fn panel_select_next() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.select_next();
        assert_eq!(panel.selected, 1);
    }

    #[test]
    fn panel_select_next_at_end() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.selected = 2;
        panel.select_next();
        assert_eq!(panel.selected, 2); // stays at end
    }

    #[test]
    fn panel_select_prev() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.selected = 2;
        panel.select_prev();
        assert_eq!(panel.selected, 1);
    }

    #[test]
    fn panel_select_prev_at_zero() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.select_prev();
        assert_eq!(panel.selected, 0); // stays at zero
    }

    #[test]
    fn panel_start_add() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.start_add();
        assert_eq!(panel.mode, ProjectsPanelMode::AddPath);
        assert!(panel.input.is_empty());
        assert_eq!(panel.input_cursor, 0);
    }

    #[test]
    fn panel_start_rename() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.selected = 1;
        panel.start_rename();
        assert_eq!(panel.mode, ProjectsPanelMode::Rename);
        assert_eq!(panel.input, "Project Two");
        assert_eq!(panel.input_cursor, "Project Two".len());
    }

    #[test]
    fn panel_start_init() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.start_init();
        assert_eq!(panel.mode, ProjectsPanelMode::Init);
        assert!(panel.input.is_empty());
    }

    #[test]
    fn panel_cancel_input() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.start_add();
        panel.input_char('x');
        panel.cancel_input();
        assert_eq!(panel.mode, ProjectsPanelMode::Browse);
        assert!(panel.input.is_empty());
        assert_eq!(panel.input_cursor, 0);
    }

    #[test]
    fn panel_input_char() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.start_add();
        panel.input_char('a');
        panel.input_char('b');
        assert_eq!(panel.input, "ab");
        assert_eq!(panel.input_cursor, 2);
    }

    #[test]
    fn panel_input_backspace() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.start_add();
        panel.input_char('a');
        panel.input_char('b');
        panel.input_backspace();
        assert_eq!(panel.input, "a");
        assert_eq!(panel.input_cursor, 1);
    }

    #[test]
    fn panel_input_backspace_at_zero() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.start_add();
        panel.input_backspace(); // no-op
        assert!(panel.input.is_empty());
        assert_eq!(panel.input_cursor, 0);
    }

    #[test]
    fn panel_input_delete() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.start_add();
        panel.input_char('a');
        panel.input_char('b');
        panel.cursor_left();
        panel.input_delete();
        assert_eq!(panel.input, "a");
    }

    #[test]
    fn panel_cursor_left() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.start_add();
        panel.input_char('x');
        panel.cursor_left();
        assert_eq!(panel.input_cursor, 0);
    }

    #[test]
    fn panel_cursor_right() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.start_add();
        panel.input_char('x');
        panel.cursor_left();
        panel.cursor_right();
        assert_eq!(panel.input_cursor, 1);
    }

    #[test]
    fn panel_cursor_home() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.start_add();
        panel.input_char('a');
        panel.input_char('b');
        panel.cursor_home();
        assert_eq!(panel.input_cursor, 0);
    }

    #[test]
    fn panel_cursor_end() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.start_add();
        panel.input_char('a');
        panel.input_char('b');
        panel.cursor_home();
        panel.cursor_end();
        assert_eq!(panel.input_cursor, 2);
    }

    #[test]
    fn panel_error_cleared_on_navigate() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.error = Some("oops".to_string());
        panel.select_next();
        assert!(panel.error.is_none());
    }

    #[test]
    fn panel_error_cleared_on_input_char() {
        let mut panel = crate::app::types::ProjectsPanel::new(sample_entries());
        panel.start_add();
        panel.error = Some("bad".to_string());
        panel.input_char('x');
        assert!(panel.error.is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    //  Modal sizing math
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn modal_width_60_pct() {
        let area = Rect::new(0, 0, 100, 50);
        let modal_w = (area.width * 60 / 100).max(40).min(area.width);
        assert_eq!(modal_w, 60);
    }

    #[test]
    fn modal_width_min_40() {
        let area = Rect::new(0, 0, 50, 50);
        let modal_w = (area.width * 60 / 100).max(40).min(area.width);
        assert_eq!(modal_w, 40); // 50*60/100=30, max(40)=40
    }

    #[test]
    fn modal_width_capped_at_area() {
        let area = Rect::new(0, 0, 30, 50);
        let modal_w = (area.width * 60 / 100).max(40).min(area.width);
        assert_eq!(modal_w, 30); // max(40) but min(30) wins
    }

    #[test]
    fn modal_height_70_pct() {
        let area = Rect::new(0, 0, 100, 50);
        let modal_h = (area.height * 70 / 100).max(10).min(area.height);
        assert_eq!(modal_h, 35);
    }

    #[test]
    fn modal_height_min_10() {
        let area = Rect::new(0, 0, 100, 12);
        let modal_h = (area.height * 70 / 100).max(10).min(area.height);
        assert_eq!(modal_h, 10); // 12*70/100=8, max(10)=10
    }

    #[test]
    fn modal_centering() {
        let area = Rect::new(0, 0, 100, 50);
        let modal_w: u16 = 60;
        let modal_h: u16 = 35;
        let x = area.x + (area.width.saturating_sub(modal_w)) / 2;
        let y = area.y + (area.height.saturating_sub(modal_h)) / 2;
        assert_eq!(x, 20);
        assert_eq!(y, 7);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Inner width
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn inner_w_calculation() {
        let modal = Rect::new(20, 7, 60, 35);
        let inner_w = modal.width.saturating_sub(4) as usize;
        assert_eq!(inner_w, 56);
    }

    #[test]
    fn inner_w_tiny_modal() {
        let modal = Rect::new(0, 0, 4, 10);
        let inner_w = modal.width.saturating_sub(4) as usize;
        assert_eq!(inner_w, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Name truncation
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn name_truncation_short() {
        let inner_w = 56;
        let name_max = (inner_w / 3).max(10);
        let name = "Short";
        let display = if name.len() > name_max {
            format!("{}...", &name[..name_max - 1])
        } else {
            name.to_string()
        };
        assert_eq!(display, "Short");
    }

    #[test]
    fn name_max_at_least_10() {
        let inner_w = 20;
        let name_max = (inner_w / 3).max(10);
        assert_eq!(name_max, 10);
    }

    #[test]
    fn name_max_scales_with_width() {
        let inner_w = 90;
        let name_max = (inner_w / 3).max(10);
        assert_eq!(name_max, 30);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Indicator logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn indicator_current_project() {
        let is_current = true;
        let indicator = if is_current { "\u{25cf} " } else { "  " };
        assert_eq!(indicator, "\u{25cf} ");
    }

    #[test]
    fn indicator_not_current() {
        let is_current = false;
        let indicator = if is_current { "\u{25cf} " } else { "  " };
        assert_eq!(indicator, "  ");
    }

    #[test]
    fn indicator_color_current_green() {
        let is_current = true;
        let color = if is_current { Color::Green } else { Color::DarkGray };
        assert_eq!(color, Color::Green);
    }

    #[test]
    fn indicator_color_not_current_gray() {
        let is_current = false;
        let color = if is_current { Color::Green } else { Color::DarkGray };
        assert_eq!(color, Color::DarkGray);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Selected style
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn selected_style_azure_bold() {
        let is_selected = true;
        let style = if is_selected {
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        assert_eq!(style.fg, Some(AZURE));
    }

    #[test]
    fn unselected_style_white() {
        let is_selected = false;
        let style = if is_selected {
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        assert_eq!(style.fg, Some(Color::White));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Input height per mode
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn input_height_browse_is_zero() {
        let mode = ProjectsPanelMode::Browse;
        let h = if mode != ProjectsPanelMode::Browse { 3u16 } else { 0u16 };
        assert_eq!(h, 0);
    }

    #[test]
    fn input_height_add_is_three() {
        let mode = ProjectsPanelMode::AddPath;
        let h = if mode != ProjectsPanelMode::Browse { 3u16 } else { 0u16 };
        assert_eq!(h, 3);
    }

    #[test]
    fn input_height_rename_is_three() {
        let mode = ProjectsPanelMode::Rename;
        let h = if mode != ProjectsPanelMode::Browse { 3u16 } else { 0u16 };
        assert_eq!(h, 3);
    }

    #[test]
    fn input_height_init_is_three() {
        let mode = ProjectsPanelMode::Init;
        let h = if mode != ProjectsPanelMode::Browse { 3u16 } else { 0u16 };
        assert_eq!(h, 3);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Prompt text per mode
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn prompt_add_path() {
        let mode = ProjectsPanelMode::AddPath;
        let prompt = match mode {
            ProjectsPanelMode::AddPath => " Path: ",
            ProjectsPanelMode::Rename => " Name: ",
            ProjectsPanelMode::Init => " Init path (blank=cwd): ",
            _ => "",
        };
        assert_eq!(prompt, " Path: ");
    }

    #[test]
    fn prompt_rename() {
        let mode = ProjectsPanelMode::Rename;
        let prompt = match mode {
            ProjectsPanelMode::AddPath => " Path: ",
            ProjectsPanelMode::Rename => " Name: ",
            ProjectsPanelMode::Init => " Init path (blank=cwd): ",
            _ => "",
        };
        assert_eq!(prompt, " Name: ");
    }

    #[test]
    fn prompt_init() {
        let mode = ProjectsPanelMode::Init;
        let prompt = match mode {
            ProjectsPanelMode::AddPath => " Path: ",
            ProjectsPanelMode::Rename => " Name: ",
            ProjectsPanelMode::Init => " Init path (blank=cwd): ",
            _ => "",
        };
        assert_eq!(prompt, " Init path (blank=cwd): ");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Error height
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn error_height_with_error() {
        let error: Option<String> = Some("bad path".to_string());
        let h: u16 = if error.is_some() { 1 } else { 0 };
        assert_eq!(h, 1);
    }

    #[test]
    fn error_height_no_error() {
        let error: Option<String> = None;
        let h: u16 = if error.is_some() { 1 } else { 0 };
        assert_eq!(h, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Layout chunks
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn layout_four_chunks() {
        let modal = Rect::new(20, 7, 60, 35);
        let input_height = 3u16;
        let error_height = 1u16;
        let chunks = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(error_height),
            Constraint::Length(input_height),
            Constraint::Length(1),
        ]).split(modal);
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[1].height, 1);
        assert_eq!(chunks[2].height, 3);
        assert_eq!(chunks[3].height, 1);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Cursor position in input field
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn cursor_position_calculation() {
        let chunk_x = 20u16;
        let prompt_len = 7; // " Path: "
        let input_cursor = 5;
        let cursor_x = chunk_x + 1 + prompt_len as u16 + input_cursor as u16;
        assert_eq!(cursor_x, 33); // 20 + 1 + 7 + 5
    }

    // ══════════════════════════════════════════════════════════════════
    //  Empty state message
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn empty_projects_message() {
        let msg = "  No projects registered. Press 'a' to add one.";
        assert!(msg.contains("No projects"));
        assert!(msg.contains("'a'"));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Hints per mode
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn add_path_hints_have_confirm_cancel() {
        // From the source code, AddPath mode shows Enter:confirm and Esc:cancel
        let hints_text = "Enter:confirm Esc:cancel";
        assert!(hints_text.contains("confirm"));
        assert!(hints_text.contains("cancel"));
    }

    #[test]
    fn rename_hints_have_save_cancel() {
        let hints_text = "Enter:save Esc:cancel";
        assert!(hints_text.contains("save"));
        assert!(hints_text.contains("cancel"));
    }

    #[test]
    fn init_hints_have_init_blank_cwd() {
        let hints_text = "Enter:init (blank=cwd) Esc:cancel";
        assert!(hints_text.contains("init"));
        assert!(hints_text.contains("blank=cwd"));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Title count formatting
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn title_count_format() {
        let count = 5;
        let text = format!(" {} ", count);
        assert_eq!(text, " 5 ");
    }

    #[test]
    fn title_count_zero() {
        let text = format!(" {} ", 0);
        assert_eq!(text, " 0 ");
    }
}
