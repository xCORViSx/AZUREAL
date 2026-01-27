//! Dialog overlays (help, context menu, branch dialog, session creation)

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, BranchDialog};
use super::util::{calculate_cursor_position, truncate};

/// Draw help overlay
pub fn draw_help_overlay(f: &mut Frame) {
    let area = f.area();
    let help_width = 80.min(area.width - 4);
    let help_height = 30.min(area.height - 4);

    let help_area = Rect {
        x: (area.width.saturating_sub(help_width)) / 2,
        y: (area.height.saturating_sub(help_height)) / 2,
        width: help_width,
        height: help_height,
    };

    let help_text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("Azural - Keyboard Navigation Help", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        ]),
        Line::from(""),
        Line::from(vec![Span::styled("Global (Command Mode)", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]),
        Line::from("  i                Enter INPROMPT mode (focus input, start typing)"),
        Line::from("  ?                Toggle this help"),
        Line::from("  Tab              Cycle focus forward (Sessions → Output → Input)"),
        Line::from("  Shift+Tab        Cycle focus backward"),
        Line::from("  Ctrl+c           Quit application"),
        Line::from(""),
        Line::from(vec![Span::styled("Sessions Panel", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]),
        Line::from("  j / Down         Select next session"),
        Line::from("  k / Up           Select previous session"),
        Line::from("  J                Select next project"),
        Line::from("  K                Select previous project"),
        Line::from("  Space            Open context menu for session actions"),
        Line::from("  Enter            Start/resume selected session"),
        Line::from("  n                Create new session (enter prompt)"),
        Line::from("  b                Browse branches (create worktree)"),
        Line::from("  d                View diff for selected session"),
        Line::from("  r                Rebase session onto main branch"),
        Line::from("  a                Archive selected session"),
        Line::from(""),
        Line::from(vec![Span::styled("Output Panel", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]),
        Line::from("  j / Down         Scroll down one line"),
        Line::from("  k / Up           Scroll up one line"),
        Line::from("  Ctrl+d           Scroll down half page (20 lines)"),
        Line::from("  Ctrl+u           Scroll up half page (20 lines)"),
        Line::from("  Ctrl+f           Scroll down full page (40 lines)"),
        Line::from("  Ctrl+b           Scroll up full page (40 lines)"),
        Line::from("  PageDown         Scroll down 10 lines"),
        Line::from("  PageUp           Scroll up 10 lines"),
        Line::from("  g                Jump to top"),
        Line::from("  G                Jump to bottom"),
        Line::from("  o                Switch to output view"),
        Line::from("  d                Switch to diff view"),
        Line::from("  Esc              Return to Sessions panel"),
        Line::from(""),
        Line::from(vec![Span::styled("Input (Vim-style)", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]),
        Line::from("  RED border = COMMAND mode (keys are commands)"),
        Line::from("  YELLOW border = INPROMPT mode (typing to Claude)"),
        Line::from("  CYAN border = TERMINAL mode (shell commands)"),
        Line::from("  i (command)      Enter INPROMPT mode"),
        Line::from("  t (command)      Toggle terminal pane"),
        Line::from("  Esc (insert)     Exit to COMMAND mode"),
        Line::from("  Enter            Submit prompt to Claude"),
        Line::from("  Arrow keys       Navigate input text"),
        Line::from("  Ctrl+W           Delete word backward"),
        Line::from(""),
        Line::from(vec![Span::styled("Terminal Pane", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]),
        Line::from("  t                Toggle terminal on/off (command mode)"),
        Line::from("  +/-              Resize terminal height (command mode)"),
        Line::from("  j/k              Scroll line (command mode)"),
        Line::from("  J/K              Scroll page (command mode)"),
        Line::from("  Enter            Execute shell command (insert mode)"),
        Line::from("  CYAN border = Terminal mode active"),
        Line::from(""),
        Line::from(vec![Span::styled("Branch Dialog", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]),
        Line::from("  j / Down         Select next branch"),
        Line::from("  k / Up           Select previous branch"),
        Line::from("  Enter            Confirm selection"),
        Line::from("  Esc              Cancel"),
        Line::from("  Type             Filter branches"),
        Line::from(""),
        Line::from(vec![Span::styled("Press ? or q or Esc to close this help", Style::default().fg(Color::Gray).add_modifier(Modifier::ITALIC))]),
    ];

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help ")
                .border_style(Style::default().fg(Color::Cyan))
                .style(Style::default().bg(Color::Reset))
        )
        .wrap(Wrap { trim: false });

    f.render_widget(help, help_area);
}

/// Draw branch selection dialog
pub fn draw_branch_dialog(f: &mut Frame, dialog: &BranchDialog, area: Rect) {
    let dialog_width = 60.min(area.width.saturating_sub(4));
    let dialog_height = 20.min(area.height.saturating_sub(4));
    let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;

    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    let dialog_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5)])
        .split(dialog_area);

    // Filter input
    let filter_title = if dialog.filter.is_empty() { " Filter (type to search) " } else { " Filter " };
    let filter_text = if dialog.filter.is_empty() { String::new() } else { dialog.filter.clone() };
    let filter = Paragraph::new(filter_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(filter_title),
        );
    f.render_widget(filter, dialog_chunks[0]);

    // Branch list
    let items: Vec<ListItem> = dialog.filtered_indices.iter().enumerate().map(|(display_idx, &branch_idx)| {
        let branch = &dialog.branches[branch_idx];
        let is_selected = display_idx == dialog.selected;

        let style = if is_selected {
            Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
        } else if branch.contains('/') {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let prefix = if is_selected { "▸ " } else { "  " };
        ListItem::new(Line::from(vec![
            Span::raw(prefix),
            Span::styled(truncate(branch, dialog_width as usize - 4), style),
        ]))
    }).collect();

    let title = format!(" Select Branch ({}/{}) ", dialog.filtered_indices.len(), dialog.branches.len());
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(title),
    );

    f.render_widget(list, dialog_chunks[1]);
}

/// Draw context menu overlay
pub fn draw_context_menu(f: &mut Frame, app: &App, area: Rect) {
    if let Some(ref menu) = app.context_menu {
        let menu_width = 50;
        let menu_height = menu.actions.len() as u16 + 4;

        let menu_x = (area.width.saturating_sub(menu_width)) / 2;
        let menu_y = (area.height.saturating_sub(menu_height)) / 2;

        let menu_area = Rect { x: menu_x, y: menu_y, width: menu_width, height: menu_height };

        let items: Vec<ListItem> = menu.actions.iter().enumerate().map(|(idx, action)| {
            let is_selected = idx == menu.selected;
            let style = if is_selected {
                Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let key_style = if is_selected {
                Style::default().bg(Color::Cyan).fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Yellow)
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!(" [{:>5}] ", action.key_hint()), key_style),
                Span::styled(action.label(), style),
            ]))
        }).collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(" Session Actions (↑↓ to navigate, Enter to select, Esc to close) ")
                    .style(Style::default().bg(Color::Reset)),
            );

        f.render_widget(Block::default().style(Style::default().bg(Color::Reset)), menu_area);
        f.render_widget(list, menu_area);
    }
}

/// Draw session creation modal
pub fn draw_session_creation_modal(f: &mut Frame, app: &App) {
    let area = f.area();
    let modal_width = (area.width * 4) / 5;
    let modal_height = (area.height * 3) / 5;
    let modal_x = (area.width - modal_width) / 2;
    let modal_y = (area.height - modal_height) / 2;

    let modal_area = Rect { x: modal_x, y: modal_y, width: modal_width, height: modal_height };

    let bg_block = Block::default().style(Style::default().bg(Color::Reset));
    f.render_widget(bg_block, modal_area);

    let modal_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(3)])
        .split(modal_area);

    // Title
    let title = Paragraph::new("Create New Session")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::TOP | Borders::LEFT | Borders::RIGHT));
    f.render_widget(title, modal_chunks[0]);

    // Input area
    let input_text = &app.session_creation_input;
    let lines: Vec<Line> = input_text.split('\n').map(Line::from).collect();

    let input_widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .style(Style::default().fg(Color::Yellow))
        )
        .wrap(Wrap { trim: false });
    f.render_widget(input_widget, modal_chunks[1]);

    // Cursor position
    if let Some((cursor_x, cursor_y)) = calculate_cursor_position(
        input_text,
        app.session_creation_cursor,
        modal_chunks[1].width.saturating_sub(2) as usize,
    ) {
        f.set_cursor_position((
            modal_chunks[1].x + 1 + cursor_x as u16,
            modal_chunks[1].y + cursor_y as u16,
        ));
    }

    // Info bar
    let char_count = input_text.len();
    let line_count = input_text.lines().count().max(1);
    let info_text = format!(" {} chars | {} lines | Ctrl+Enter: Submit | Esc: Cancel ", char_count, line_count);

    let info = Paragraph::new(info_text)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray))
        .block(Block::default().borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT));
    f.render_widget(info, modal_chunks[2]);
}
