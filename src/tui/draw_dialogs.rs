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

    // Each display row is either a single binding or two paired bindings merged.
    // Paired: "j/↓ down · k/↑ up" — both key+desc on one line separated by dim ·
    enum HelpRow {
        Single { keys: String, desc: &'static str },
        Paired { keys1: String, desc1: &'static str, keys2: String, desc2: &'static str },
    }

    // Build display rows per section, merging pair_with_next bindings
    let mut section_rows: Vec<(&str, Vec<HelpRow>)> = Vec::new();
    for section in &sections {
        let mut rows = Vec::new();
        let bindings = section.bindings;
        let mut i = 0;
        while i < bindings.len() {
            if bindings[i].pair_with_next && i + 1 < bindings.len() {
                rows.push(HelpRow::Paired {
                    keys1: bindings[i].display_keys(),
                    desc1: bindings[i].description,
                    keys2: bindings[i + 1].display_keys(),
                    desc2: bindings[i + 1].description,
                });
                i += 2;
            } else {
                rows.push(HelpRow::Single {
                    keys: bindings[i].display_keys(),
                    desc: bindings[i].description,
                });
                i += 1;
            }
        }
        section_rows.push((section.title, rows));
    }

    // Max key width across all single + paired entries (for the first key column)
    let key_width = section_rows.iter()
        .flat_map(|(_, rows)| rows.iter())
        .map(|row| match row {
            HelpRow::Single { keys, .. } => keys.len(),
            HelpRow::Paired { keys1, keys2, .. } => keys1.len().max(keys2.len()),
        })
        .max()
        .unwrap_or(10) + 2;

    // Max single-entry desc width (used for column sizing)
    let desc_width = section_rows.iter()
        .flat_map(|(_, rows)| rows.iter())
        .map(|row| match row {
            HelpRow::Single { desc, .. } => desc.len(),
            // Paired rows: key1+desc1 + separator + key2+desc2 — we size off single rows
            HelpRow::Paired { desc1, desc2, .. } => desc1.len().max(desc2.len()),
        })
        .max()
        .unwrap_or(20);

    // Paired rows need extra space: key + desc + " · " + key + desc
    // Column width = max(single_width, paired_width)
    let single_width = key_width + 1 + desc_width + 2;
    let paired_width = key_width + 1 + desc_width + 3 + key_width + 1 + desc_width + 2;
    let col_width = single_width.max(paired_width);

    // Calculate how many columns fit (min 1, max 3)
    let available_width = area.width.saturating_sub(4) as usize;
    let num_cols = (available_width / col_width).clamp(1, 3);
    let actual_col_width = available_width / num_cols;

    // Distribute sections across columns (roughly equal height)
    let total_lines: usize = section_rows.iter().map(|(_, rows)| rows.len() + 2).sum();
    let target_per_col = (total_lines + num_cols - 1) / num_cols;

    let mut columns: Vec<Vec<Line>> = vec![Vec::new(); num_cols];
    let mut current_col = 0;
    let mut current_height = 0;

    let dim_style = Style::default().fg(Color::DarkGray);

    for (title, rows) in &section_rows {
        let section_height = rows.len() + 2;
        if current_height + section_height > target_per_col && current_col < num_cols - 1 && current_height > 0 {
            current_col += 1;
            current_height = 0;
        }

        columns[current_col].push(Line::from(vec![
            Span::styled(*title, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        ]));

        for row in rows {
            let line = match row {
                HelpRow::Single { keys, desc } => {
                    let key_span = Span::styled(
                        format!("{:>width$}", keys, width = key_width),
                        Style::default().fg(AZURE),
                    );
                    let desc_span = Span::raw(format!(" {}", desc));
                    Line::from(vec![key_span, desc_span])
                }
                HelpRow::Paired { keys1, desc1, keys2, desc2 } => {
                    // "  keys1 desc1 · keys2 desc2"
                    let k1 = Span::styled(
                        format!("{:>width$}", keys1, width = key_width),
                        Style::default().fg(AZURE),
                    );
                    let d1 = Span::raw(format!(" {} ", desc1));
                    let sep = Span::styled("· ", dim_style);
                    let k2 = Span::styled(keys2.clone(), Style::default().fg(AZURE));
                    let d2 = Span::raw(format!(" {}", desc2));
                    Line::from(vec![k1, d1, sep, k2, d2])
                }
            };
            columns[current_col].push(line);
        }

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

    // Build list items with number shortcuts, scope badge, and selection highlight
    let items: Vec<ListItem> = app.run_commands.iter().enumerate().map(|(idx, cmd)| {
        let is_selected = idx == picker.selected;
        let style = if is_selected {
            Style::default().bg(AZURE).fg(Color::Black).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let key_style = if is_selected {
            Style::default().bg(AZURE).fg(Color::Rgb(30, 60, 100))
        } else {
            Style::default().fg(Color::Yellow)
        };

        // Show 1-9 number shortcuts, then just spaces for 10+
        let num_hint = if idx < 9 { format!(" [{}] ", idx + 1) } else { "     ".to_string() };

        // Scope badge: G=global, P=project
        let scope_badge = if cmd.global { " G " } else { " P " };
        let scope_style = if is_selected {
            Style::default().bg(AZURE).fg(Color::Rgb(30, 60, 100))
        } else if cmd.global {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let max_name = (dialog_width as usize).saturating_sub(num_hint.len() + scope_badge.len() + 4);

        ListItem::new(Line::from(vec![
            Span::styled(num_hint, key_style),
            Span::styled(truncate(&cmd.name, max_name), style),
            Span::styled(scope_badge, scope_style),
        ]))
    }).collect();

    // Title changes when delete confirmation is pending — normal title from keybindings.rs
    let title = if let Some(del_idx) = picker.confirm_delete {
        let name = app.run_commands.get(del_idx).map(|c| c.name.as_str()).unwrap_or("?");
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

    // Outer border with title — left shows mode, right shows scope badge
    let title_text = if dialog.editing_idx.is_some() { " Edit Run Command " } else { " New Run Command " };
    let scope_label = if dialog.global { " [GLOBAL] " } else { " [PROJECT] " };
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(AZURE))
        .title(Span::styled(title_text, Style::default().fg(AZURE).add_modifier(Modifier::BOLD)))
        .title(Line::from(Span::styled(scope_label, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))).alignment(Alignment::Right));
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

    // Hint line — structural keys from keybindings.rs, Enter label changes by context
    let enter_label = if dialog.editing_name { "next" } else {
        match dialog.field_mode {
            CommandFieldMode::Command => "save",
            CommandFieldMode::Prompt => "generate",
        }
    };
    let pairs = keybindings::dialog_footer_hint_pairs();
    let hint_spans: Vec<Span> = pairs.iter().map(|(key, label)| {
        // Override "save" label with context-specific Enter label
        let display_label = if key == "Enter" { enter_label } else if key == "Tab" && dialog.editing_name { "next" } else { label };
        vec![
            Span::styled(key.as_str(), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
            Span::styled(format!(":{}  ", display_label), Style::default().fg(Color::DarkGray)),
        ]
    }).flatten().collect();
    f.render_widget(Paragraph::new(Line::from(hint_spans)).alignment(Alignment::Center), chunks[2]);
}

/// Draw preset prompt picker overlay — numbered list of saved prompts.
/// 1-9 for first 9 presets, 0 for the 10th (keyboard-order layout).
pub fn draw_preset_prompt_picker(f: &mut Frame, app: &App, area: Rect) {
    let Some(ref picker) = app.preset_prompt_picker else { return };
    let count = app.preset_prompts.len();

    // Size: fit all presets + borders
    let dialog_width = 60u16.min(area.width.saturating_sub(4));
    let dialog_height = (count as u16 + 4).min(area.height.saturating_sub(4));
    let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    // Build list items with number shortcuts and selection highlight
    let items: Vec<ListItem> = app.preset_prompts.iter().enumerate().map(|(idx, preset)| {
        let is_selected = idx == picker.selected;
        let style = if is_selected {
            Style::default().bg(AZURE).fg(Color::Black).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let key_style = if is_selected {
            Style::default().bg(AZURE).fg(Color::Rgb(30, 60, 100))
        } else {
            Style::default().fg(Color::Yellow)
        };

        // 1-9 for first 9, 0 for the 10th — keyboard order
        let num_hint = if idx < 9 {
            format!(" [{}] ", idx + 1)
        } else if idx == 9 {
            " [0] ".to_string()
        } else {
            "     ".to_string()
        };

        // Scope badge: G=global, P=project — shown after the name
        let scope_badge = if preset.global { " G " } else { " P " };
        let scope_style = if is_selected {
            Style::default().bg(AZURE).fg(Color::Rgb(30, 60, 100))
        } else if preset.global {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        // Show name + scope badge + truncated prompt preview
        let max_name = 20.min((dialog_width as usize).saturating_sub(num_hint.len() + 10));
        let preview_max = (dialog_width as usize).saturating_sub(num_hint.len() + max_name + 10);
        let preview = if preview_max > 3 {
            let p = truncate(&preset.prompt, preview_max);
            format!(" {}", p)
        } else {
            String::new()
        };
        // Selected: use a muted dark tone that's readable on AZURE bg
        let preview_style = if is_selected {
            Style::default().bg(AZURE).fg(Color::Rgb(30, 60, 100))
        } else {
            Style::default().fg(Color::DarkGray)
        };

        ListItem::new(Line::from(vec![
            Span::styled(num_hint, key_style),
            Span::styled(truncate(&preset.name, max_name), style),
            Span::styled(scope_badge, scope_style),
            Span::styled(preview, preview_style),
        ]))
    }).collect();

    // Show delete confirmation in title if pending, otherwise from keybindings.rs
    let title = if let Some(del_idx) = picker.confirm_delete {
        let name = app.preset_prompts.get(del_idx).map(|p| p.name.as_str()).unwrap_or("?");
        format!(" Delete \"{}\"? (y:yes / any:cancel) ", name)
    } else {
        keybindings::picker_title("Preset Prompts")
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(AZURE))
            .title(title)
            .style(Style::default().bg(Color::Reset)),
    );
    f.render_widget(list, dialog_area);

    // Footer hint: ⌥+number shortcut works directly from prompt mode
    let hint = " ⌥1-⌥9,⌥0 from prompt mode to skip picker ";
    let hint_y = dialog_area.y + dialog_area.height.saturating_sub(1);
    let hint_x = dialog_area.x + (dialog_area.width.saturating_sub(hint.len() as u16)) / 2;
    if hint_y < area.height && hint_x + (hint.len() as u16) <= area.x + area.width {
        let hint_rect = Rect::new(hint_x, hint_y, hint.len() as u16, 1);
        f.render_widget(Paragraph::new(Line::from(Span::styled(
            hint,
            Style::default().fg(Color::DarkGray),
        ))), hint_rect);
    }
}

/// Draw preset prompt dialog overlay (create/edit a preset prompt).
/// Two text fields: Name and Prompt, stacked vertically.
pub fn draw_preset_prompt_dialog(f: &mut Frame, app: &App) {
    let Some(ref dialog) = app.preset_prompt_dialog else { return };
    let area = f.area();

    // Two text fields stacked: name(3) + prompt(3) + hints(1) + borders(2) = 9
    let dialog_width = 60u16.min(area.width.saturating_sub(4));
    let dialog_height = 9u16.min(area.height.saturating_sub(4));
    let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    // Outer border with title — left shows mode, right shows scope badge
    let title_text = if dialog.editing_idx.is_some() { " Edit Preset " } else { " New Preset " };
    let scope_label = if dialog.global { " [GLOBAL] " } else { " [PROJECT] " };
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(AZURE))
        .title(Span::styled(title_text, Style::default().fg(AZURE).add_modifier(Modifier::BOLD)))
        .title(Line::from(Span::styled(scope_label, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))).alignment(Alignment::Right));
    let inner = outer.inner(dialog_area);
    f.render_widget(outer, dialog_area);

    // Split inner area: name(3) + prompt(3) + hints(1)
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

    // Cursor in name field
    if dialog.editing_name {
        // Convert char cursor to display position (byte-based for ASCII, char-based for unicode)
        let display_pos = dialog.name.chars().take(dialog.name_cursor).count();
        f.set_cursor_position((
            chunks[0].x + 1 + display_pos as u16,
            chunks[0].y + 1,
        ));
    }

    // Prompt field — yellow border when active
    let prompt_color = if !dialog.editing_name { Color::Yellow } else { Color::DarkGray };
    let prompt_widget = Paragraph::new(dialog.prompt.as_str())
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(prompt_color))
            .title(Span::styled(" Prompt ", Style::default().fg(prompt_color))));
    f.render_widget(prompt_widget, chunks[1]);

    // Cursor in prompt field
    if !dialog.editing_name {
        let display_pos = dialog.prompt.chars().take(dialog.prompt_cursor).count();
        f.set_cursor_position((
            chunks[1].x + 1 + display_pos as u16,
            chunks[1].y + 1,
        ));
    }

    // Hint line — structural keys from keybindings.rs, Enter label varies by context
    let enter_label = if dialog.editing_name { "next" } else { "save" };
    let pairs = keybindings::dialog_footer_hint_pairs();
    let hint_spans: Vec<Span> = pairs.iter().map(|(key, label)| {
        let display_label = if key == "Enter" { enter_label } else { label };
        vec![
            Span::styled(key.as_str(), Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
            Span::styled(format!(":{}  ", display_label), Style::default().fg(Color::DarkGray)),
        ]
    }).flatten().collect();
    f.render_widget(Paragraph::new(Line::from(hint_spans)).alignment(Alignment::Center), chunks[2]);
}
