//! Run command picker and editor dialogs

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::app::types::CommandFieldMode;
use crate::app::App;
use crate::tui::keybindings;
use crate::tui::util::{truncate, AZURE};

/// Draw run command picker overlay (select from saved commands)
pub fn draw_run_command_picker(f: &mut Frame, app: &App, area: Rect) {
    let Some(ref picker) = app.run_command_picker else {
        return;
    };
    let cmd_count = app.run_commands.len();

    // Size: fit all commands + title + footer + borders
    let dialog_width = 60u16.min(area.width.saturating_sub(4));
    let dialog_height = (cmd_count as u16 + 4).min(area.height.saturating_sub(4));
    let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    // Build list items with number shortcuts, scope badge, and selection highlight
    let items: Vec<ListItem> = app
        .run_commands
        .iter()
        .enumerate()
        .map(|(idx, cmd)| {
            let is_selected = idx == picker.selected;
            let style = if is_selected {
                Style::default()
                    .bg(AZURE)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let key_style = if is_selected {
                Style::default().bg(AZURE).fg(Color::Rgb(30, 60, 100))
            } else {
                Style::default().fg(Color::Yellow)
            };

            // Show 1-9 number shortcuts, then just spaces for 10+
            let num_hint = if idx < 9 {
                format!(" [{}] ", idx + 1)
            } else {
                "     ".to_string()
            };

            // Scope badge: G=global, P=project
            let scope_badge = if cmd.global { " G " } else { " P " };
            let scope_style = if is_selected {
                Style::default().bg(AZURE).fg(Color::Rgb(30, 60, 100))
            } else if cmd.global {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let max_name =
                (dialog_width as usize).saturating_sub(num_hint.len() + scope_badge.len() + 4);

            ListItem::new(Line::from(vec![
                Span::styled(num_hint, key_style),
                Span::styled(truncate(&cmd.name, max_name), style),
                Span::styled(scope_badge, scope_style),
            ]))
        })
        .collect();

    // Title changes when delete confirmation is pending — normal title from keybindings.rs
    let title = if let Some(del_idx) = picker.confirm_delete {
        let name = app
            .run_commands
            .get(del_idx)
            .map(|c| c.name.as_str())
            .unwrap_or("?");
        format!(" Delete \"{}\"? (y:yes / any:cancel) ", name)
    } else {
        keybindings::picker_title("Run Command")
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(AZURE))
            .title(title)
            .style(Style::default().bg(Color::Reset)),
    );
    f.render_widget(list, dialog_area);
}

/// Draw run command dialog overlay (create/edit a command)
pub fn draw_run_command_dialog(f: &mut Frame, app: &App) {
    let Some(ref dialog) = app.run_command_dialog else {
        return;
    };
    let area = f.area();

    // Two text fields (name + command) stacked inside an outer border
    let dialog_width = 60u16.min(area.width.saturating_sub(4));
    // outer(1) + name(3) + command(3) + hints(1) + outer(1) = 9
    let dialog_height = 9u16.min(area.height.saturating_sub(4));
    let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    // Outer border with title — left shows mode, right shows scope badge
    let title_text = if dialog.editing_idx.is_some() {
        " Edit Run Command "
    } else {
        " New Run Command "
    };
    let scope_label = if dialog.global {
        " [GLOBAL] "
    } else {
        " [PROJECT] "
    };
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(AZURE))
        .title(Span::styled(
            title_text,
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
        ))
        .title(
            Line::from(Span::styled(
                scope_label,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Right),
        );
    let inner = outer.inner(dialog_area);
    f.render_widget(outer, dialog_area);

    // Split inner area: name(3) + command(3) + hints(1)
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(inner);

    // Name field — yellow border when active, gray when inactive
    let name_color = if dialog.editing_name {
        Color::Yellow
    } else {
        Color::DarkGray
    };
    let name_widget = Paragraph::new(dialog.name.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(name_color))
            .title(Span::styled(" Name ", Style::default().fg(name_color))),
    );
    f.render_widget(name_widget, chunks[0]);

    // Cursor in name field (content at y+1, x+1 inside ALL borders)
    if dialog.editing_name {
        f.set_cursor_position((chunks[0].x + 1 + dialog.name_cursor as u16, chunks[0].y + 1));
    }

    // Command/Prompt field — yellow border when active, right-aligned mode cycle hint
    let cmd_color = if !dialog.editing_name {
        Color::Yellow
    } else {
        Color::DarkGray
    };
    let (field_title, mode_hint) = match dialog.field_mode {
        CommandFieldMode::Command => (" Command ", " Tab:Prompt "),
        CommandFieldMode::Prompt => (" Prompt ", " Tab:Command "),
    };
    let mut cmd_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(cmd_color))
        .title(Span::styled(field_title, Style::default().fg(cmd_color)));
    // Right-aligned mode cycle hint (only when field is focused)
    if !dialog.editing_name {
        cmd_block = cmd_block.title(
            Line::from(Span::styled(
                mode_hint,
                Style::default().fg(Color::DarkGray),
            ))
            .alignment(Alignment::Right),
        );
    }
    let cmd_widget = Paragraph::new(dialog.command.as_str()).block(cmd_block);
    f.render_widget(cmd_widget, chunks[1]);

    // Cursor in command field
    if !dialog.editing_name {
        f.set_cursor_position((
            chunks[1].x + 1 + dialog.command_cursor as u16,
            chunks[1].y + 1,
        ));
    }

    // Hint line — structural keys from keybindings.rs, Enter label changes by context
    let enter_label = if dialog.editing_name {
        "next"
    } else {
        match dialog.field_mode {
            CommandFieldMode::Command => "save",
            CommandFieldMode::Prompt => "generate",
        }
    };
    let pairs = keybindings::dialog_footer_hint_pairs();
    let hint_spans: Vec<Span> = pairs
        .iter()
        .map(|(key, label)| {
            // Override "save" label with context-specific Enter label
            let display_label = if key == "Enter" {
                enter_label
            } else if key == "Tab" && dialog.editing_name {
                "next"
            } else {
                label
            };
            vec![
                Span::styled(
                    key.as_str(),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(":{}  ", display_label),
                    Style::default().fg(Color::DarkGray),
                ),
            ]
        })
        .flatten()
        .collect();
    f.render_widget(
        Paragraph::new(Line::from(hint_spans)).alignment(Alignment::Center),
        chunks[2],
    );
}
