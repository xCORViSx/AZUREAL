//! Session pane dialog overlays.
//!
//! Small centered dialog boxes rendered on top of the session pane:
//! - New session name input
//! - RCR (conflict resolution) approval
//! - Post-merge worktree disposition

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use crate::tui::util::{AZURE, GIT_ORANGE};

/// Draw the new session name dialog — centered input box over the session pane.
pub(super) fn draw_new_session_dialog(f: &mut Frame, app: &App, area: Rect) {
    let input = &app.new_session_name_input;
    let w = (input.chars().count() as u16 + 6)
        .max(42)
        .min(area.width.saturating_sub(4));
    let h = 3u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let dialog_area = Rect::new(x, y, w, h);

    f.render_widget(Clear, dialog_area);

    let widget = Paragraph::new(input.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(AZURE))
            .title(Span::styled(
                " New Session ",
                Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
            ))
            .title(
                Line::from(Span::styled(
                    " Enter:create  Esc:cancel ",
                    Style::default().fg(Color::DarkGray),
                ))
                .alignment(Alignment::Right),
            ),
    );
    f.render_widget(widget, dialog_area);

    let cursor_x = dialog_area.x + 1 + app.new_session_name_cursor as u16;
    let cursor_y = dialog_area.y + 1;
    if cursor_x < dialog_area.right() {
        f.set_cursor_position((cursor_x, cursor_y));
    }
}

/// Draw the RCR approval dialog — a small centered green-bordered box asking
/// whether the user wants to accept the conflict resolution. Rendered over the
/// session pane after Claude exits during RCR mode.
pub fn draw_rcr_approval(f: &mut Frame, area: Rect) {
    // Size: 46 wide × 5 tall, centered within the session pane
    let w = 46u16.min(area.width.saturating_sub(2));
    let h = 5u16.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let dialog = Rect::new(x, y, w, h);

    f.render_widget(Clear, dialog);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
        .title(Span::styled(
            " RCR ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));

    let text = vec![
        Line::from(Span::styled(
            "Accept conflict resolution?",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                "[y]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Accept  "),
            Span::styled(
                "[n]",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Abort  "),
            Span::styled(
                "[Esc]",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Review"),
        ]),
    ];
    let para = Paragraph::new(text)
        .block(block)
        .alignment(Alignment::Center);
    f.render_widget(para, dialog);
}

/// Draw the post-merge dialog — asks whether to keep, archive, or delete the
/// worktree after a successful squash merge. Centered on the full screen.
pub fn draw_post_merge_dialog(
    f: &mut Frame,
    area: Rect,
    dialog_state: &crate::app::types::PostMergeDialog,
) {
    let w = 50u16.min(area.width.saturating_sub(2));
    let h = 9u16.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let rect = Rect::new(x, y, w, h);

    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD))
        .title(Span::styled(
            format!(" {} merged ", dialog_state.display_name),
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD),
        ));

    let arrow = |i: usize| {
        if dialog_state.selected == i {
            "▸ "
        } else {
            "  "
        }
    };
    let style = |i: usize, color: Color| {
        if dialog_state.selected == i {
            Style::default().fg(color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        }
    };

    let text = vec![
        Line::from(Span::styled(
            "What should happen to this worktree?",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("{}Keep — continue working on this branch", arrow(0)),
            style(0, Color::Green),
        )),
        Line::from(Span::styled(
            format!("{}Archive — remove worktree, keep branch", arrow(1)),
            style(1, Color::Yellow),
        )),
        Line::from(Span::styled(
            format!("{}Delete — remove worktree and branch", arrow(2)),
            style(2, Color::Red),
        )),
    ];
    let para = Paragraph::new(text).block(block).alignment(Alignment::Left);
    f.render_widget(para, rect);
}
