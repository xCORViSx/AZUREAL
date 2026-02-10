//! Dialog overlays (help, context menu, branch dialog, session creation)

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, BranchDialog};
use crate::app::types::CommandFieldMode;
use super::keybindings;
use super::util::{calculate_cursor_position, truncate, AZURE};

/// Draw help overlay with auto-sized columns from centralized keybindings
pub fn draw_help_overlay(f: &mut Frame) {
    let area = f.area();
    let sections = keybindings::help_sections();

    // Convert keybindings to display entries (key_display, description)
    let section_entries: Vec<(&str, Vec<(String, &str)>)> = sections.iter()
        .map(|s| (s.title, s.bindings.iter().map(|b| (b.display_keys(), b.description)).collect()))
        .collect();

    // Calculate max key width across all sections
    let key_width = section_entries.iter()
        .flat_map(|(_, entries)| entries.iter())
        .map(|(k, _)| k.len())
        .max()
        .unwrap_or(10) + 2; // +2 for padding

    // Calculate max description width
    let desc_width = section_entries.iter()
        .flat_map(|(_, entries)| entries.iter())
        .map(|(_, d)| d.len())
        .max()
        .unwrap_or(20);

    // Column width = key + separator + desc + padding
    let col_width = key_width + 1 + desc_width + 2;

    // Calculate how many columns fit (min 1, max 3)
    let available_width = area.width.saturating_sub(4) as usize; // -4 for border
    let num_cols = (available_width / col_width).clamp(1, 3);
    let actual_col_width = available_width / num_cols;

    // Distribute sections across columns (roughly equal height)
    let total_lines: usize = section_entries.iter().map(|(_, e)| e.len() + 2).sum(); // +2 for title + blank
    let target_per_col = (total_lines + num_cols - 1) / num_cols;

    let mut columns: Vec<Vec<Line>> = vec![Vec::new(); num_cols];
    let mut current_col = 0;
    let mut current_height = 0;

    for (title, entries) in &section_entries {
        let section_height = entries.len() + 2;
        // Move to next column if this section would overflow (unless on last column)
        if current_height + section_height > target_per_col && current_col < num_cols - 1 && current_height > 0 {
            current_col += 1;
            current_height = 0;
        }

        // Add section title
        columns[current_col].push(Line::from(vec![
            Span::styled(*title, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        ]));

        // Add entries with proper key/desc separation
        let desc_available = actual_col_width.saturating_sub(key_width + 1);
        for (key, desc) in entries {
            let key_span = Span::styled(
                format!("{:>width$}", key, width = key_width),
                Style::default().fg(AZURE)
            );
            let desc_str: String = if desc.len() > desc_available {
                format!("{}…", desc.chars().take(desc_available.saturating_sub(1)).collect::<String>())
            } else {
                (*desc).to_string()
            };
            let desc_span = Span::raw(format!(" {}", desc_str));
            columns[current_col].push(Line::from(vec![key_span, desc_span]));
        }

        // Add blank line after section
        columns[current_col].push(Line::from(""));
        current_height += section_height;
    }

    // Calculate actual height needed (max column height + title + footer + borders)
    let max_col_height = columns.iter().map(|c| c.len()).max().unwrap_or(0);
    let help_height = (max_col_height as u16 + 4).min(area.height.saturating_sub(4));

    // Calculate actual width needed
    let help_width = ((actual_col_width * num_cols) as u16 + 4).min(area.width.saturating_sub(4));

    let help_area = Rect {
        x: (area.width.saturating_sub(help_width)) / 2,
        y: (area.height.saturating_sub(help_height)) / 2,
        width: help_width,
        height: help_height,
    };

    // Clear background
    f.render_widget(Clear, help_area);

    // Create inner area for content
    let inner = Rect {
        x: help_area.x + 1,
        y: help_area.y + 1,
        width: help_area.width.saturating_sub(2),
        height: help_area.height.saturating_sub(2),
    };

    // Split into columns
    let col_constraints: Vec<Constraint> = (0..num_cols)
        .map(|_| Constraint::Ratio(1, num_cols as u32))
        .collect();
    let col_areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(col_constraints)
        .split(inner);

    // Render border
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(" Help (? to close) ")
        .border_style(Style::default().fg(AZURE))
        .style(Style::default().bg(Color::Reset));
    f.render_widget(block, help_area);

    // Render each column
    for (i, col_lines) in columns.iter().enumerate() {
        if i < col_areas.len() {
            let para = Paragraph::new(col_lines.clone());
            f.render_widget(para, col_areas[i]);
        }
    }
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
                .border_style(Style::default().fg(AZURE))
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
            .border_style(Style::default().fg(AZURE))
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
                Style::default().bg(AZURE).fg(Color::Black).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let key_style = if is_selected {
                Style::default().bg(AZURE).fg(Color::DarkGray)
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
                    .border_style(Style::default().fg(AZURE))
                    .title(" Worktree Actions (↑↓ to navigate, Enter to select, Esc to close) ")
                    .style(Style::default().bg(Color::Reset)),
            );

        f.render_widget(Block::default().style(Style::default().bg(Color::Reset)), menu_area);
        f.render_widget(list, menu_area);
    }
}

/// Draw worktree creation modal
pub fn draw_worktree_creation_modal(f: &mut Frame, app: &App) {
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
    let title = Paragraph::new("Create New Worktree")
        .alignment(Alignment::Center)
        .style(Style::default().fg(AZURE).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::TOP | Borders::LEFT | Borders::RIGHT));
    f.render_widget(title, modal_chunks[0]);

    // Input area
    let input_text = &app.worktree_creation_input;
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
        app.worktree_creation_cursor,
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

/// Draw run command picker overlay (select from saved commands)
pub fn draw_run_command_picker(f: &mut Frame, app: &App, area: Rect) {
    let Some(ref picker) = app.run_command_picker else { return };
    let cmd_count = app.run_commands.len();

    // Size: fit all commands + title + footer + borders
    let dialog_width = 60u16.min(area.width.saturating_sub(4));
    let dialog_height = (cmd_count as u16 + 4).min(area.height.saturating_sub(4));
    let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    // Build list items with number shortcuts and selection highlight
    let items: Vec<ListItem> = app.run_commands.iter().enumerate().map(|(idx, cmd)| {
        let is_selected = idx == picker.selected;
        let style = if is_selected {
            Style::default().bg(AZURE).fg(Color::Black).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let key_style = if is_selected {
            Style::default().bg(AZURE).fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Yellow)
        };

        // Show 1-9 number shortcuts, then just spaces for 10+
        let num_hint = if idx < 9 { format!(" [{}] ", idx + 1) } else { "     ".to_string() };
        let max_name = (dialog_width as usize).saturating_sub(num_hint.len() + 4);

        ListItem::new(Line::from(vec![
            Span::styled(num_hint, key_style),
            Span::styled(truncate(&cmd.name, max_name), style),
        ]))
    }).collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(AZURE))
            .title(" Run Command (j/k:nav  1-9:quick  e:edit  x:del  a:add) ")
            .style(Style::default().bg(Color::Reset)),
    );
    f.render_widget(list, dialog_area);
}

/// Draw run command dialog overlay (create/edit a command)
pub fn draw_run_command_dialog(f: &mut Frame, app: &App) {
    let Some(ref dialog) = app.run_command_dialog else { return };
    let area = f.area();

    // Two text fields (name + command) stacked inside an outer border
    let dialog_width = 60u16.min(area.width.saturating_sub(4));
    // outer(1) + name(3) + command(3) + hints(1) + outer(1) = 9
    let dialog_height = 9u16.min(area.height.saturating_sub(4));
    let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    // Outer border with title
    let title_text = if dialog.editing_idx.is_some() { " Edit Run Command " } else { " New Run Command " };
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(AZURE))
        .title(Span::styled(title_text, Style::default().fg(AZURE).add_modifier(Modifier::BOLD)));
    let inner = outer.inner(dialog_area);
    f.render_widget(outer, dialog_area);

    // Split inner area: name(3) + command(3) + hints(1)
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(1),
    ]).split(inner);

    // Name field — yellow border when active, gray when inactive
    let name_color = if dialog.editing_name { Color::Yellow } else { Color::DarkGray };
    let name_widget = Paragraph::new(dialog.name.as_str())
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(name_color))
            .title(Span::styled(" Name ", Style::default().fg(name_color))));
    f.render_widget(name_widget, chunks[0]);

    // Cursor in name field (content at y+1, x+1 inside ALL borders)
    if dialog.editing_name {
        f.set_cursor_position((
            chunks[0].x + 1 + dialog.name_cursor as u16,
            chunks[0].y + 1,
        ));
    }

    // Command/Prompt field — yellow border when active, right-aligned mode cycle hint
    let cmd_color = if !dialog.editing_name { Color::Yellow } else { Color::DarkGray };
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
            Line::from(Span::styled(mode_hint, Style::default().fg(Color::DarkGray)))
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

    // Hint line — Enter action changes by context
    let enter_hint = if dialog.editing_name {
        ":next  "
    } else {
        match dialog.field_mode {
            CommandFieldMode::Command => ":save  ",
            CommandFieldMode::Prompt => ":generate  ",
        }
    };
    let tab_hint = if dialog.editing_name { ":next  " } else { ":mode  " };
    let hints = Line::from(vec![
        Span::styled("Tab", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::styled(tab_hint, Style::default().fg(Color::DarkGray)),
        Span::styled("⇧Tab", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::styled(":back  ", Style::default().fg(Color::DarkGray)),
        Span::styled("Enter", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::styled(enter_hint, Style::default().fg(Color::DarkGray)),
        Span::styled("Esc", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::styled(":cancel", Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(hints).alignment(Alignment::Center), chunks[2]);
}
