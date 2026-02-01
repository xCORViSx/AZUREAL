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

/// Help section with title and key-description pairs
struct HelpSection {
    title: &'static str,
    entries: Vec<(&'static str, &'static str)>,
}

/// Build all help sections
fn help_sections() -> Vec<HelpSection> {
    vec![
        HelpSection {
            title: "Global",
            entries: vec![
                ("i", "Enter INPROMPT mode"),
                ("t", "Toggle terminal pane"),
                ("?", "Toggle this help"),
                ("Tab", "Cycle focus forward"),
                ("Shift+Tab", "Cycle focus backward"),
                ("Ctrl+X", "Cancel Claude response"),
                ("Ctrl+C", "Quit application"),
            ],
        },
        HelpSection {
            title: "Worktrees",
            entries: vec![
                ("j/k", "Select worktree"),
                ("J/K", "Select project"),
                ("Space", "Context menu"),
                ("Enter", "Start/resume"),
                ("n", "New worktree"),
                ("b", "Browse branches"),
                ("d", "View diff"),
                ("r", "Rebase onto main"),
                ("a", "Archive worktree"),
            ],
        },
        HelpSection {
            title: "Filetree",
            entries: vec![
                ("j/k", "Navigate"),
                ("h/l", "Collapse/expand"),
                ("Enter", "Open/toggle"),
                ("Space", "Toggle dir"),
                ("Esc", "Back to Worktrees"),
            ],
        },
        HelpSection {
            title: "Viewer",
            entries: vec![
                ("j/k", "Scroll line"),
                ("Ctrl+d/u", "Half page"),
                ("Ctrl+f/b", "Full page"),
                ("g/G", "Top/bottom"),
                ("q", "Close viewer"),
                ("Esc", "Close and clear"),
            ],
        },
        HelpSection {
            title: "Convo",
            entries: vec![
                ("j/k", "Scroll line"),
                ("↑/↓", "Prev/next prompt"),
                ("Shift+↑/↓", "Prev/next message"),
                ("Ctrl+d/u", "Half page"),
                ("Ctrl+f/b", "Full page"),
                ("g/G", "Top/bottom"),
                ("o", "Output view"),
                ("d", "Diff view"),
                ("Esc", "Back to Worktrees"),
            ],
        },
        HelpSection {
            title: "Input",
            entries: vec![
                ("Enter", "Submit prompt"),
                ("Esc", "Exit to COMMAND"),
                ("Ctrl+W", "Delete word"),
            ],
        },
        HelpSection {
            title: "Terminal",
            entries: vec![
                ("+/-", "Resize height"),
                ("j/k", "Scroll line"),
                ("J/K", "Scroll page"),
            ],
        },
    ]
}

/// Draw help overlay with auto-sized columns
pub fn draw_help_overlay(f: &mut Frame) {
    let area = f.area();
    let sections = help_sections();

    // Calculate max key width across all sections
    let key_width = sections.iter()
        .flat_map(|s| s.entries.iter())
        .map(|(k, _)| k.len())
        .max()
        .unwrap_or(10) + 2; // +2 for padding

    // Calculate max description width
    let desc_width = sections.iter()
        .flat_map(|s| s.entries.iter())
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
    let total_lines: usize = sections.iter().map(|s| s.entries.len() + 2).sum(); // +2 for title + blank
    let target_per_col = (total_lines + num_cols - 1) / num_cols;

    let mut columns: Vec<Vec<Line>> = vec![Vec::new(); num_cols];
    let mut current_col = 0;
    let mut current_height = 0;

    for section in &sections {
        let section_height = section.entries.len() + 2;
        // Move to next column if this section would overflow (unless we're on the last column)
        if current_height + section_height > target_per_col && current_col < num_cols - 1 && current_height > 0 {
            current_col += 1;
            current_height = 0;
        }

        // Add section title
        columns[current_col].push(Line::from(vec![
            Span::styled(section.title, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        ]));

        // Add entries with proper key/desc separation
        let desc_available = actual_col_width.saturating_sub(key_width + 1);
        for (key, desc) in &section.entries {
            let key_span = Span::styled(
                format!("{:>width$}", key, width = key_width),
                Style::default().fg(Color::Cyan)
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
    let help_height = (max_col_height as u16 + 4).min(area.height.saturating_sub(4)); // +4 for title, footer, borders

    // Calculate actual width needed
    let help_width = ((actual_col_width * num_cols) as u16 + 4).min(area.width.saturating_sub(4)); // +4 for borders + padding

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
        .title(" Help (? to close) ")
        .border_style(Style::default().fg(Color::Cyan))
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
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
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
