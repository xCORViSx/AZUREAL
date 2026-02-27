//! Viewer dialog overlays
//!
//! Save confirmation and discard confirmation dialogs shown during edit mode.

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

/// Draw post-save dialog (after saving from Edit diff view)
pub(super) fn draw_save_dialog(f: &mut Frame, area: Rect) {
    let dialog_width = 45u16;
    let dialog_height = 8u16;
    let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
    let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    let chunks = Layout::default()
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
        ])
        .margin(1)
        .split(dialog_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(Span::styled(" File Saved ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(Color::Green));

    f.render_widget(block, dialog_area);

    let msg = Paragraph::new("Where would you like to go?")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::White));
    f.render_widget(msg, chunks[0]);

    let options = Paragraph::new("(d)iff view  (f)ile view  (Esc)continue")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray));
    f.render_widget(options, chunks[1]);
}

/// Draw discard confirmation dialog
pub(super) fn draw_discard_dialog(f: &mut Frame, area: Rect, from_edit_diff: bool) {
    let dialog_width = if from_edit_diff { 50u16 } else { 40u16 };
    let dialog_height = if from_edit_diff { 9u16 } else { 7u16 };
    let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
    let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(Span::styled(" Unsaved Changes ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(Color::Yellow));

    f.render_widget(block, dialog_area);

    if from_edit_diff {
        let chunks = Layout::default()
            .constraints([
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Length(2),
            ])
            .margin(1)
            .split(dialog_area);

        let msg = Paragraph::new("Discard changes?")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::White));
        f.render_widget(msg, chunks[0]);

        let options1 = Paragraph::new("(y)es discard  (n)o cancel")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(options1, chunks[1]);

        let options2 = Paragraph::new("(s)ave → diff  (f)save → file")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(options2, chunks[2]);
    } else {
        let chunks = Layout::default()
            .constraints([
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Length(2),
            ])
            .margin(1)
            .split(dialog_area);

        let msg = Paragraph::new("Discard changes?")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::White));
        f.render_widget(msg, chunks[0]);

        let options = Paragraph::new("(y)es  (n)o  (s)ave and exit")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(options, chunks[1]);
    }
}
