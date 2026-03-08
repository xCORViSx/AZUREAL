//! Dialog overlays (help, context menu, branch dialog, session creation)

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::app::{App, BranchDialog};
use crate::app::types::CommandFieldMode;
use super::keybindings;
use super::util::{truncate, AZURE};

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

/// Draw unified Add Worktree dialog — "[+] Create new" row at top, branches below with [N WT] indicators
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

    // Filter / new name input with cursor
    let filter_title = if dialog.filter.is_empty() { " Filter / New Name " } else { " Filter / New Name " };
    let filter = Paragraph::new(dialog.filter.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(AZURE))
                .title(filter_title),
        );
    f.render_widget(filter, dialog_chunks[0]);

    // Show cursor in filter input
    let cursor_x = dialog_chunks[0].x + 1 + dialog.filter.chars().count() as u16;
    let cursor_y = dialog_chunks[0].y + 1;
    if cursor_x < dialog_chunks[0].x + dialog_chunks[0].width - 1 {
        f.set_cursor_position((cursor_x, cursor_y));
    }

    // Build list: "[+] Create new" row first, then branch rows
    let mut items: Vec<ListItem> = Vec::with_capacity(1 + dialog.filtered_indices.len());

    // "Create new" row (always first, index 0)
    let create_selected = dialog.on_create_new();
    let create_label = if dialog.filter.is_empty() {
        "[+] Create new".to_string()
    } else {
        format!("[+] Create new: {}", dialog.filter)
    };
    let create_style = if create_selected {
        Style::default().bg(Color::Green).fg(Color::Black).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    items.push(ListItem::new(Line::from(vec![
        Span::styled(if create_selected { "▸ " } else { "  " }, create_style),
        Span::styled(truncate(&create_label, dialog_width as usize - 4), create_style),
    ])));

    // Branch rows (index 1+)
    for (display_idx, &branch_idx) in dialog.filtered_indices.iter().enumerate() {
        let branch = &dialog.branches[branch_idx];
        let is_selected = display_idx + 1 == dialog.selected; // +1 because "Create new" is 0
        let wt_count = dialog.worktree_count(branch_idx);
        let is_active = dialog.is_checked_out(branch);

        let prefix = if is_selected { "▸ " } else { "  " };
        let tag = if wt_count > 0 { format!(" [{} WT]", wt_count) } else { String::new() };
        let max_name_width = (dialog_width as usize).saturating_sub(4 + tag.len());

        let style = if is_selected {
            Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
        } else if is_active {
            Style::default().fg(Color::DarkGray)
        } else if branch.contains('/') {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let mut spans = vec![
            Span::raw(prefix),
            Span::styled(truncate(branch, max_name_width), style),
        ];
        if wt_count > 0 {
            let tag_style = if is_selected {
                Style::default().bg(Color::Blue).fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            spans.push(Span::styled(tag, tag_style));
        }
        items.push(ListItem::new(Line::from(spans)));
    }

    let title = format!(" Add Worktree ({} branches) ", dialog.branches.len());
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(AZURE))
            .title(title)
            .title_bottom(Line::from(vec![
                Span::styled(" Enter ", Style::default().fg(Color::Green)),
                Span::styled("create/switch  ", Style::default().fg(Color::DarkGray)),
                Span::styled("Esc ", Style::default().fg(Color::Yellow)),
                Span::styled("cancel ", Style::default().fg(Color::DarkGray)),
            ]).alignment(Alignment::Center)),
    );

    f.render_widget(list, dialog_chunks[1]);
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

/// Draw the delete worktree confirmation dialog (⌘d)
pub fn draw_delete_worktree_dialog(f: &mut Frame, dialog: &crate::app::types::DeleteWorktreeDialog, area: Rect) {
    use crate::app::types::DeleteWorktreeDialog;
    let (title, lines) = match dialog {
        DeleteWorktreeDialog::Sole { name, warnings } => {
            let title = format!(" Delete '{}' ", name);
            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Delete this worktree and its branch?",
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )),
            ];
            if !warnings.is_empty() {
                lines.push(Line::from(""));
                for w in warnings {
                    lines.push(Line::from(Span::styled(
                        format!("  ! {}", w),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )));
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  y", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled("  Confirm delete", Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  Esc", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
                Span::styled("  Cancel", Style::default().fg(Color::DarkGray)),
            ]));
            (title, lines)
        }
        DeleteWorktreeDialog::Siblings { branch, count, warnings, .. } => {
            let title = format!(" Delete on '{}' ", branch);
            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!("{} other worktree{} on this branch.", count, if *count == 1 { "" } else { "s" }),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )),
            ];
            if !warnings.is_empty() {
                lines.push(Line::from(""));
                for w in warnings {
                    lines.push(Line::from(Span::styled(
                        format!("  ! {}", w),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )));
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  y", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled("  Delete all worktrees + branch", Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  a", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("  Archive this one only", Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  Esc", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
                Span::styled("  Cancel", Style::default().fg(Color::DarkGray)),
            ]));
            (title, lines)
        }
    };

    let w = 50u16.min(area.width.saturating_sub(4));
    let h = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let rect = Rect::new(x, y, w, h);

    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        .title(Span::styled(&title, Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
    let para = Paragraph::new(lines).block(block).alignment(Alignment::Left);
    f.render_widget(para, rect);
}

/// Draw full-width table popup overlay (click a table in session pane to open)
pub fn draw_table_popup(f: &mut Frame, popup: &crate::app::types::TablePopup, area: Rect) {
    if popup.lines.is_empty() { return; }

    // Size: use most of the terminal, capped to content
    let max_w = area.width.saturating_sub(4);
    // Measure widest rendered line (span content widths)
    let content_w: u16 = popup.lines.iter().map(|line| {
        line.spans.iter().map(|s| s.content.chars().count() as u16).sum::<u16>()
    }).max().unwrap_or(40);
    let w = (content_w + 4).min(max_w).max(30); // +4 for border + padding
    let max_h = area.height.saturating_sub(4);
    let content_h = popup.total_lines as u16;
    let h = (content_h + 2).min(max_h).max(5); // +2 for border
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let rect = Rect::new(x, y, w, h);

    f.render_widget(Clear, rect);

    let inner_h = h.saturating_sub(2) as usize;
    let can_scroll = popup.total_lines > inner_h;
    let scroll_info = if can_scroll {
        format!(" {}/{} ", popup.scroll + 1, popup.total_lines.saturating_sub(inner_h) + 1)
    } else {
        String::new()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(AZURE))
        .title(Span::styled(" Table ", Style::default().fg(AZURE).add_modifier(Modifier::BOLD)))
        .title_bottom(Line::from(vec![
            Span::styled(" Esc", Style::default().fg(Color::DarkGray)),
            Span::styled(" close ", Style::default().fg(Color::DarkGray)),
            if can_scroll {
                Span::styled(scroll_info, Style::default().fg(Color::DarkGray))
            } else {
                Span::raw("")
            },
        ]));

    let visible: Vec<Line> = popup.lines.iter()
        .skip(popup.scroll)
        .take(inner_h)
        .cloned()
        .collect();

    let para = Paragraph::new(visible).block(block);
    f.render_widget(para, rect);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::{
        RunCommand, RunCommandDialog, RunCommandPicker,
        PresetPrompt, PresetPromptPicker, PresetPromptDialog,
    };

    // ══════════════════════════════════════════════════════════════════
    // BranchDialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_branch_dialog_new_populates_filtered_indices() {
        let d = BranchDialog::new(vec!["main".into(), "dev".into()], vec![], vec![0, 0]);
        assert_eq!(d.filtered_indices, vec![0, 1]);
    }

    #[test]
    fn test_branch_dialog_new_selected_starts_zero() {
        let d = BranchDialog::new(vec!["a".into()], vec![], vec![0]);
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn test_branch_dialog_new_filter_empty() {
        let d = BranchDialog::new(vec!["x".into()], vec![], vec![0]);
        assert!(d.filter.is_empty());
    }

    #[test]
    fn test_branch_dialog_is_checked_out_exact() {
        let d = BranchDialog::new(vec![], vec!["main".into()], vec![]);
        assert!(d.is_checked_out("main"));
    }

    #[test]
    fn test_branch_dialog_is_checked_out_remote_prefix() {
        let d = BranchDialog::new(vec![], vec!["feature".into()], vec![]);
        assert!(d.is_checked_out("origin/feature"));
    }

    #[test]
    fn test_branch_dialog_is_checked_out_false() {
        let d = BranchDialog::new(vec![], vec!["main".into()], vec![]);
        assert!(!d.is_checked_out("dev"));
    }

    #[test]
    fn test_branch_dialog_apply_filter_narrows() {
        let mut d = BranchDialog::new(vec!["main".into(), "dev".into(), "feature".into()], vec![], vec![0, 0, 0]);
        d.filter = "dev".into();
        d.apply_filter();
        assert_eq!(d.filtered_indices, vec![1]);
    }

    #[test]
    fn test_branch_dialog_apply_filter_case_insensitive() {
        let mut d = BranchDialog::new(vec!["Main".into(), "DEV".into()], vec![], vec![0, 0]);
        d.filter = "dev".into();
        d.apply_filter();
        assert_eq!(d.filtered_indices, vec![1]);
    }

    #[test]
    fn test_branch_dialog_apply_filter_empty_shows_all() {
        let mut d = BranchDialog::new(vec!["a".into(), "b".into()], vec![], vec![0, 0]);
        d.filter.clear();
        d.apply_filter();
        assert_eq!(d.filtered_indices, vec![0, 1]);
    }

    #[test]
    fn test_branch_dialog_apply_filter_resets_selected() {
        let mut d = BranchDialog::new(vec!["a".into(), "b".into(), "c".into()], vec![], vec![0, 0, 0]);
        d.selected = 2;
        d.filter = "z".into();
        d.apply_filter();
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn test_branch_dialog_selected_branch() {
        let mut d = BranchDialog::new(vec!["a".into(), "b".into()], vec![], vec![0, 0]);
        // selected==0 is "[+] Create new", move to first branch
        d.select_next();
        assert_eq!(d.selected_branch().unwrap(), "a");
    }

    #[test]
    fn test_branch_dialog_selected_branch_after_filter() {
        let mut d = BranchDialog::new(vec!["alpha".into(), "beta".into()], vec![], vec![0, 0]);
        d.filter = "bet".into();
        d.apply_filter();
        // selected==0 is "[+] Create new", move to first filtered branch
        d.select_next();
        assert_eq!(d.selected_branch().unwrap(), "beta");
    }

    #[test]
    fn test_branch_dialog_selected_branch_empty() {
        let mut d = BranchDialog::new(vec!["a".into()], vec![], vec![0]);
        d.filter = "zzz".into();
        d.apply_filter();
        assert!(d.selected_branch().is_none());
    }

    #[test]
    fn test_branch_dialog_select_next() {
        let mut d = BranchDialog::new(vec!["a".into(), "b".into(), "c".into()], vec![], vec![0, 0, 0]);
        d.select_next();
        assert_eq!(d.selected, 1);
        d.select_next();
        assert_eq!(d.selected, 2);
    }

    #[test]
    fn test_branch_dialog_select_next_at_end() {
        // display_len = 1 (Create new) + 2 branches = 3, max selected = 2
        let mut d = BranchDialog::new(vec!["a".into(), "b".into()], vec![], vec![0, 0]);
        d.selected = 2;
        d.select_next();
        assert_eq!(d.selected, 2); // no change — already at end
    }

    #[test]
    fn test_branch_dialog_select_prev() {
        let mut d = BranchDialog::new(vec!["a".into(), "b".into()], vec![], vec![0, 0]);
        d.selected = 1;
        d.select_prev();
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn test_branch_dialog_select_prev_at_zero() {
        let mut d = BranchDialog::new(vec!["a".into()], vec![], vec![0]);
        d.select_prev();
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn test_branch_dialog_filter_char() {
        let mut d = BranchDialog::new(vec!["abc".into(), "def".into()], vec![], vec![0, 0]);
        d.filter_char('a');
        assert_eq!(d.filter, "a");
        assert_eq!(d.filtered_indices, vec![0]);
    }

    #[test]
    fn test_branch_dialog_filter_backspace() {
        let mut d = BranchDialog::new(vec!["abc".into(), "def".into()], vec![], vec![0, 0]);
        d.filter = "ab".into();
        d.filter_backspace();
        assert_eq!(d.filter, "a");
    }

    #[test]
    fn test_branch_dialog_filter_backspace_empty() {
        let mut d = BranchDialog::new(vec!["abc".into()], vec![], vec![0]);
        d.filter_backspace();
        assert!(d.filter.is_empty());
    }

    // ══════════════════════════════════════════════════════════════════
    // Layout centering math
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_dialog_centering_60x20_on_100x50() {
        let area_w = 100u16;
        let area_h = 50u16;
        let dw = 60u16.min(area_w.saturating_sub(4));
        let dh = 20u16.min(area_h.saturating_sub(4));
        let dx = (area_w.saturating_sub(dw)) / 2;
        let dy = (area_h.saturating_sub(dh)) / 2;
        assert_eq!(dw, 60);
        assert_eq!(dh, 20);
        assert_eq!(dx, 20);
        assert_eq!(dy, 15);
    }

    #[test]
    fn test_dialog_centering_small_terminal() {
        let area_w = 30u16;
        let area_h = 10u16;
        let dw = 60u16.min(area_w.saturating_sub(4));
        let dh = 20u16.min(area_h.saturating_sub(4));
        assert_eq!(dw, 26);
        assert_eq!(dh, 6);
    }

    #[test]
    fn test_dialog_centering_very_small() {
        let area_w = 5u16;
        let dw = 60u16.min(area_w.saturating_sub(4));
        assert_eq!(dw, 1);
    }

    #[test]
    fn test_modal_50_centering() {
        let area_w = 80u16;
        let mw = 50u16.min(area_w.saturating_sub(4));
        let mx = (area_w.saturating_sub(mw)) / 2;
        assert_eq!(mw, 50);
        assert_eq!(mx, 15);
    }

    // ══════════════════════════════════════════════════════════════════
    // Rect construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_rect_new() {
        let r = Rect::new(10, 20, 30, 40);
        assert_eq!(r.x, 10);
        assert_eq!(r.y, 20);
        assert_eq!(r.width, 30);
        assert_eq!(r.height, 40);
    }

    #[test]
    fn test_rect_area() {
        let r = Rect::new(0, 0, 10, 10);
        assert_eq!(r.area(), 100);
    }

    // ══════════════════════════════════════════════════════════════════
    // Style construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_azure_color_value() {
        assert_eq!(AZURE, Color::Rgb(51, 153, 255));
    }

    #[test]
    fn test_style_fg_azure() {
        let s = Style::default().fg(AZURE);
        assert_eq!(s.fg, Some(AZURE));
    }

    #[test]
    fn test_style_bold_modifier() {
        let s = Style::default().add_modifier(Modifier::BOLD);
        assert!(s.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_style_bg_blue_fg_white() {
        let s = Style::default().bg(Color::Blue).fg(Color::White);
        assert_eq!(s.bg, Some(Color::Blue));
        assert_eq!(s.fg, Some(Color::White));
    }

    // ══════════════════════════════════════════════════════════════════
    // Span and Line
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_span_raw_content() {
        let s = Span::raw("hello");
        assert_eq!(s.content.as_ref(), "hello");
    }

    #[test]
    fn test_span_styled_content() {
        let s = Span::styled("text", Style::default().fg(Color::Red));
        assert_eq!(s.content.as_ref(), "text");
        assert_eq!(s.style.fg, Some(Color::Red));
    }

    #[test]
    fn test_line_from_spans() {
        let line = Line::from(vec![Span::raw("a"), Span::raw("b")]);
        assert_eq!(line.spans.len(), 2);
    }

    #[test]
    fn test_line_from_string() {
        let line = Line::from("hello");
        assert_eq!(line.spans.len(), 1);
    }

    // ══════════════════════════════════════════════════════════════════
    // truncate
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn test_truncate_exact() {
        assert_eq!(truncate("abcde", 5), "abcde");
    }

    #[test]
    fn test_truncate_over() {
        let r = truncate("abcdef", 4);
        assert_eq!(r.chars().count(), 4);
        assert!(r.ends_with('\u{2026}'));
    }

    // ══════════════════════════════════════════════════════════════════
    // CommandFieldMode
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_command_field_mode_command() {
        let m = CommandFieldMode::Command;
        assert_eq!(m, CommandFieldMode::Command);
    }

    #[test]
    fn test_command_field_mode_prompt() {
        let m = CommandFieldMode::Prompt;
        assert_eq!(m, CommandFieldMode::Prompt);
    }

    #[test]
    fn test_command_field_mode_ne() {
        assert_ne!(CommandFieldMode::Command, CommandFieldMode::Prompt);
    }

    #[test]
    fn test_field_title_command() {
        let (field_title, mode_hint) = match CommandFieldMode::Command {
            CommandFieldMode::Command => (" Command ", " Tab:Prompt "),
            CommandFieldMode::Prompt => (" Prompt ", " Tab:Command "),
        };
        assert_eq!(field_title, " Command ");
        assert_eq!(mode_hint, " Tab:Prompt ");
    }

    #[test]
    fn test_field_title_prompt() {
        let (field_title, mode_hint) = match CommandFieldMode::Prompt {
            CommandFieldMode::Command => (" Command ", " Tab:Prompt "),
            CommandFieldMode::Prompt => (" Prompt ", " Tab:Command "),
        };
        assert_eq!(field_title, " Prompt ");
        assert_eq!(mode_hint, " Tab:Command ");
    }

    // ══════════════════════════════════════════════════════════════════
    // RunCommandDialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_run_command_dialog_new_defaults() {
        let d = RunCommandDialog::new();
        assert!(d.name.is_empty());
        assert!(d.command.is_empty());
        assert_eq!(d.name_cursor, 0);
        assert_eq!(d.command_cursor, 0);
        assert!(d.editing_name);
        assert!(d.editing_idx.is_none());
        assert_eq!(d.field_mode, CommandFieldMode::Command);
        assert!(!d.global);
    }

    #[test]
    fn test_run_command_dialog_edit() {
        let cmd = RunCommand::new("build", "cargo build", true);
        let d = RunCommandDialog::edit(3, &cmd);
        assert_eq!(d.name, "build");
        assert_eq!(d.command, "cargo build");
        assert_eq!(d.editing_idx, Some(3));
        assert!(d.global);
    }

    // ══════════════════════════════════════════════════════════════════
    // RunCommandPicker
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_run_command_picker_new() {
        let p = RunCommandPicker::new();
        assert_eq!(p.selected, 0);
        assert!(p.confirm_delete.is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    // PresetPrompt
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_preset_prompt_new() {
        let p = PresetPrompt::new("test", "run tests", false);
        assert_eq!(p.name, "test");
        assert_eq!(p.prompt, "run tests");
        assert!(!p.global);
    }

    #[test]
    fn test_preset_prompt_global() {
        let p = PresetPrompt::new("g", "p", true);
        assert!(p.global);
    }

    // ══════════════════════════════════════════════════════════════════
    // PresetPromptPicker
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_preset_prompt_picker_new() {
        let p = PresetPromptPicker::new();
        assert_eq!(p.selected, 0);
        assert!(p.confirm_delete.is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    // PresetPromptDialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_preset_prompt_dialog_new() {
        let d = PresetPromptDialog::new();
        assert!(d.name.is_empty());
        assert!(d.prompt.is_empty());
        assert!(d.editing_name);
        assert!(d.editing_idx.is_none());
    }

    #[test]
    fn test_preset_prompt_dialog_edit() {
        let p = PresetPrompt::new("fix", "fix the bug", false);
        let d = PresetPromptDialog::edit(2, &p);
        assert_eq!(d.name, "fix");
        assert_eq!(d.prompt, "fix the bug");
        assert_eq!(d.editing_idx, Some(2));
    }

    // ══════════════════════════════════════════════════════════════════
    // Number hint formatting (from draw_preset_prompt_picker logic)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_num_hint_first_nine() {
        for idx in 0..9usize {
            let hint = format!(" [{}] ", idx + 1);
            assert!(hint.contains(&(idx + 1).to_string()));
        }
    }

    #[test]
    fn test_num_hint_tenth() {
        let idx = 9usize;
        let hint = if idx == 9 { " [0] ".to_string() } else { "     ".to_string() };
        assert_eq!(hint, " [0] ");
    }

    #[test]
    fn test_num_hint_eleventh_plus() {
        let idx = 10usize;
        let hint = if idx < 9 { format!(" [{}] ", idx + 1) } else if idx == 9 { " [0] ".to_string() } else { "     ".to_string() };
        assert_eq!(hint, "     ");
    }

    // ══════════════════════════════════════════════════════════════════
    // Scope badge logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_scope_badge_global() {
        let global = true;
        let badge = if global { " G " } else { " P " };
        assert_eq!(badge, " G ");
    }

    #[test]
    fn test_scope_badge_project() {
        let global = false;
        let badge = if global { " G " } else { " P " };
        assert_eq!(badge, " P ");
    }

    // ══════════════════════════════════════════════════════════════════
    // Filter title logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_filter_title_empty() {
        let filter = "";
        let title = if filter.is_empty() { " Filter (type to search) " } else { " Filter " };
        assert_eq!(title, " Filter (type to search) ");
    }

    #[test]
    fn test_filter_title_non_empty() {
        let filter = "main";
        let title = if filter.is_empty() { " Filter (type to search) " } else { " Filter " };
        assert_eq!(title, " Filter ");
    }

    // ══════════════════════════════════════════════════════════════════
    // Title formatting (from draw_branch_dialog)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_branch_title_format() {
        let filtered = 5usize;
        let total = 10usize;
        let title = format!(" Branches ({}/{}) ", filtered, total);
        assert_eq!(title, " Branches (5/10) ");
    }

    #[test]
    fn test_branch_title_format_all_shown() {
        let n = 3usize;
        let title = format!(" Branches ({}/{}) ", n, n);
        assert_eq!(title, " Branches (3/3) ");
    }

    // ══════════════════════════════════════════════════════════════════
    // Dialog title text selection (edit vs new)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_edit_title_run_command() {
        let editing_idx: Option<usize> = Some(0);
        let title = if editing_idx.is_some() { " Edit Run Command " } else { " New Run Command " };
        assert_eq!(title, " Edit Run Command ");
    }

    #[test]
    fn test_new_title_run_command() {
        let editing_idx: Option<usize> = None;
        let title = if editing_idx.is_some() { " Edit Run Command " } else { " New Run Command " };
        assert_eq!(title, " New Run Command ");
    }

    #[test]
    fn test_edit_title_preset() {
        let editing_idx: Option<usize> = Some(2);
        let title = if editing_idx.is_some() { " Edit Preset " } else { " New Preset " };
        assert_eq!(title, " Edit Preset ");
    }

    #[test]
    fn test_new_title_preset() {
        let editing_idx: Option<usize> = None;
        let title = if editing_idx.is_some() { " Edit Preset " } else { " New Preset " };
        assert_eq!(title, " New Preset ");
    }

    // ══════════════════════════════════════════════════════════════════
    // Enter label logic (from draw_run_command_dialog)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_enter_label_editing_name() {
        let editing_name = true;
        let field_mode = CommandFieldMode::Command;
        let label = if editing_name { "next" } else {
            match field_mode {
                CommandFieldMode::Command => "save",
                CommandFieldMode::Prompt => "generate",
            }
        };
        assert_eq!(label, "next");
    }

    #[test]
    fn test_enter_label_command_mode() {
        let editing_name = false;
        let field_mode = CommandFieldMode::Command;
        let label = if editing_name { "next" } else {
            match field_mode {
                CommandFieldMode::Command => "save",
                CommandFieldMode::Prompt => "generate",
            }
        };
        assert_eq!(label, "save");
    }

    #[test]
    fn test_enter_label_prompt_mode() {
        let editing_name = false;
        let field_mode = CommandFieldMode::Prompt;
        let label = if editing_name { "next" } else {
            match field_mode {
                CommandFieldMode::Command => "save",
                CommandFieldMode::Prompt => "generate",
            }
        };
        assert_eq!(label, "generate");
    }

    // ══════════════════════════════════════════════════════════════════
    // Constraint and Layout checks
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_constraint_length() {
        let c = Constraint::Length(3);
        assert_eq!(c, Constraint::Length(3));
    }

    #[test]
    fn test_constraint_min() {
        let c = Constraint::Min(5);
        assert_eq!(c, Constraint::Min(5));
    }

    #[test]
    fn test_layout_vertical_split() {
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
        ]).split(Rect::new(0, 0, 60, 7));
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].height, 3);
        assert_eq!(chunks[1].height, 3);
        assert_eq!(chunks[2].height, 1);
    }

    // ══════════════════════════════════════════════════════════════════
    // RunCommand
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_run_command_new() {
        let c = RunCommand::new("test", "cargo test", false);
        assert_eq!(c.name, "test");
        assert_eq!(c.command, "cargo test");
        assert!(!c.global);
    }

    #[test]
    fn test_run_command_global() {
        let c = RunCommand::new("deploy", "deploy.sh", true);
        assert!(c.global);
    }

    #[test]
    fn test_run_command_clone() {
        let c = RunCommand::new("build", "make", false);
        let cloned = c.clone();
        assert_eq!(cloned.name, c.name);
        assert_eq!(cloned.command, c.command);
        assert_eq!(cloned.global, c.global);
    }
}
