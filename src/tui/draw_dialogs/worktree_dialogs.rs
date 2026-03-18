//! Worktree management dialogs (branch picker, delete confirmation, rename)

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::app::BranchDialog;
use crate::tui::util::{truncate, AZURE};

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
    let filter_title = if dialog.filter.is_empty() {
        " Filter / New Name "
    } else {
        " Filter / New Name "
    };
    let filter = Paragraph::new(dialog.filter.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(AZURE))
            .title(filter_title),
    );
    f.render_widget(filter, dialog_chunks[0]);

    // Show cursor in filter input (use cursor_pos byte offset → char count)
    let chars_before_cursor = dialog.filter[..dialog.cursor_pos].chars().count();
    let cursor_x = dialog_chunks[0].x + 1 + chars_before_cursor as u16;
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
        Style::default()
            .bg(Color::Green)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    items.push(ListItem::new(Line::from(vec![
        Span::styled(if create_selected { "▸ " } else { "  " }, create_style),
        Span::styled(
            truncate(&create_label, dialog_width as usize - 4),
            create_style,
        ),
    ])));

    // Branch rows (index 1+)
    for (display_idx, &branch_idx) in dialog.filtered_indices.iter().enumerate() {
        let branch = &dialog.branches[branch_idx];
        let is_selected = display_idx + 1 == dialog.selected; // +1 because "Create new" is 0
        let wt_count = dialog.worktree_count(branch_idx);
        let is_active = dialog.is_checked_out(branch);

        let prefix = if is_selected { "▸ " } else { "  " };
        let tag = if wt_count > 0 {
            format!(" [{} WT]", wt_count)
        } else {
            String::new()
        };
        let max_name_width = (dialog_width as usize).saturating_sub(4 + tag.len());

        let style = if is_selected {
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
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
            .title_bottom(
                Line::from(vec![
                    Span::styled(" Enter ", Style::default().fg(Color::Green)),
                    Span::styled("create/switch  ", Style::default().fg(Color::DarkGray)),
                    Span::styled("Esc ", Style::default().fg(Color::Yellow)),
                    Span::styled("cancel ", Style::default().fg(Color::DarkGray)),
                ])
                .alignment(Alignment::Center),
            ),
    );

    f.render_widget(list, dialog_chunks[1]);
}

/// Draw the delete worktree confirmation dialog (⌘d)
pub fn draw_delete_worktree_dialog(
    f: &mut Frame,
    dialog: &crate::app::types::DeleteWorktreeDialog,
    area: Rect,
) {
    use crate::app::types::DeleteWorktreeDialog;
    let (title, lines) = match dialog {
        DeleteWorktreeDialog::Sole { name, warnings } => {
            let title = format!(" Delete '{}' ", name);
            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Delete this worktree and its branch?",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )),
            ];
            if !warnings.is_empty() {
                lines.push(Line::from(""));
                for w in warnings {
                    lines.push(Line::from(Span::styled(
                        format!("  ! {}", w),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )));
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(
                    "  y",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  Confirm delete", Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(vec![
                Span::styled(
                    "  Esc",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  Cancel", Style::default().fg(Color::DarkGray)),
            ]));
            (title, lines)
        }
        DeleteWorktreeDialog::Siblings {
            branch,
            count,
            warnings,
            ..
        } => {
            let title = format!(" Delete on '{}' ", branch);
            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!(
                        "{} other worktree{} on this branch.",
                        count,
                        if *count == 1 { "" } else { "s" }
                    ),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )),
            ];
            if !warnings.is_empty() {
                lines.push(Line::from(""));
                for w in warnings {
                    lines.push(Line::from(Span::styled(
                        format!("  ! {}", w),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )));
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(
                    "  y",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "  Delete all worktrees + branch",
                    Style::default().fg(Color::White),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled(
                    "  a",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  Archive this one only", Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(vec![
                Span::styled(
                    "  Esc",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ),
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
        .title(Span::styled(
            &title,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    let para = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Left);
    f.render_widget(para, rect);
}

/// Draw the rename worktree dialog (text input with cursor)
pub fn draw_rename_worktree_dialog(
    f: &mut Frame,
    dialog: &crate::app::types::RenameWorktreeDialog,
    area: Rect,
) {
    let title = format!(" Rename '{}' ", dialog.old_name);
    let prefix = format!("{}/", crate::models::BRANCH_PREFIX);

    // Build the input line with cursor highlight
    let before_cursor = &dialog.input[..dialog.cursor];
    let cursor_char = dialog.input[dialog.cursor..].chars().next();
    let after_cursor_start = dialog.cursor + cursor_char.map(|c| c.len_utf8()).unwrap_or(0);
    let after_cursor = &dialog.input[after_cursor_start..];

    let mut input_spans = vec![
        Span::styled(
            format!("  {}", prefix),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(before_cursor, Style::default().fg(Color::White)),
    ];
    // Cursor block
    input_spans.push(Span::styled(
        cursor_char.map(|c| c.to_string()).unwrap_or_else(|| " ".to_string()),
        Style::default()
            .fg(Color::Black)
            .bg(Color::White),
    ));
    if !after_cursor.is_empty() {
        input_spans.push(Span::styled(after_cursor, Style::default().fg(Color::White)));
    }

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "New branch name:",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(input_spans),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  Enter",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Confirm", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(
                "  Esc",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Cancel", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let w = 50u16.min(area.width.saturating_sub(4));
    let h = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let rect = Rect::new(x, y, w, h);

    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .title(Span::styled(
            &title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let para = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Left);
    f.render_widget(para, rect);
}
