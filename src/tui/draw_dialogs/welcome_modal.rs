//! Welcome modal shown when a project has no worktrees

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::tui::keybindings;
use crate::tui::util::AZURE;

/// Draw the welcome modal — shown when a project is loaded but has no worktrees
/// and main is not being browsed. Only Browse Main, Add Worktree, and Quit are accepted.
pub fn draw_welcome_modal(f: &mut Frame) {
    let area = f.area();

    // Resolve keybindings dynamically — panels are global, worktree mutations use W leader
    let main_key =
        keybindings::find_key_for_action(&keybindings::GLOBAL, keybindings::Action::BrowseMain)
            .unwrap_or_else(|| "M".into());
    let wt_key = format!(
        "W{}",
        keybindings::find_key_for_action(
            &keybindings::WORKTREES,
            keybindings::Action::AddWorktree
        )
        .unwrap_or_else(|| "a".into())
    );
    let proj_key =
        keybindings::find_key_for_action(&keybindings::GLOBAL, keybindings::Action::OpenProjects)
            .unwrap_or_else(|| "P".into());
    let quit_key =
        keybindings::find_key_for_action(&keybindings::GLOBAL, keybindings::Action::Quit)
            .unwrap_or_else(|| "Ctrl+Q".into());

    let key_style = Style::default().fg(AZURE).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);
    let white = Style::default().fg(Color::White);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("No worktrees", white)).alignment(Alignment::Center),
        Line::from(""),
        Line::from(vec![
            Span::styled(&main_key, key_style),
            Span::styled("  Browse main branch", dim),
        ])
        .alignment(Alignment::Center),
        Line::from(vec![
            Span::styled(&wt_key, key_style),
            Span::styled("  Create a worktree", dim),
        ])
        .alignment(Alignment::Center),
        Line::from(vec![
            Span::styled(&proj_key, key_style),
            Span::styled("  Open projects", dim),
        ])
        .alignment(Alignment::Center),
        Line::from(vec![
            Span::styled(&quit_key, key_style),
            Span::styled("  Quit", dim),
        ])
        .alignment(Alignment::Center),
        Line::from(""),
    ];

    let h = lines.len() as u16 + 2; // +2 for borders
    let w = 30u16.min(area.width.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let rect = Rect::new(x, y, w, h);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(AZURE))
        .title(Span::styled(
            " AZUREAL ",
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center);

    f.render_widget(Clear, rect);
    f.render_widget(Paragraph::new(lines).block(block), rect);
}
