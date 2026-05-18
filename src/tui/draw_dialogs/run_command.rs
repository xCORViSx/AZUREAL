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
use crate::tui::draw_input::{display_width, word_wrap_break_points};
use crate::tui::keybindings;
use crate::tui::util::{truncate, AZURE};

const RUN_DIALOG_FIXED_HEIGHT: u16 = 8;

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

    let dialog_width = 60u16.min(area.width.saturating_sub(4));
    let command_inner_width = dialog_width.saturating_sub(4).max(1) as usize;
    let command_rows = run_command_dialog_command_rows(&dialog.command, command_inner_width);
    let max_dialog_height = area.height.saturating_sub(4).max(1);
    let max_command_rows = max_dialog_height
        .saturating_sub(RUN_DIALOG_FIXED_HEIGHT)
        .max(1) as usize;
    let visible_command_rows = command_rows.min(max_command_rows).max(1) as u16;
    // outer borders(2) + name field(3) + command field(rows+2) + hints(1)
    let dialog_height = (visible_command_rows + RUN_DIALOG_FIXED_HEIGHT)
        .min(max_dialog_height)
        .max(1);
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

    // Split inner area: name(3) + command(dynamic) + hints(1)
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(visible_command_rows + 2),
        Constraint::Length(1),
    ])
    .split(inner);

    // Name field — yellow border when active, gray when inactive
    let name_color = if dialog.editing_name {
        Color::Yellow
    } else {
        Color::DarkGray
    };
    let name_selection = if dialog.editing_name {
        dialog.selection
    } else {
        None
    };
    let name_widget = Paragraph::new(single_line_field_content(&dialog.name, name_selection))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(name_color))
                .title(Span::styled(" Name ", Style::default().fg(name_color))),
        );
    f.render_widget(name_widget, chunks[0]);

    // Cursor in name field (content at y+1, x+1 inside ALL borders)
    if dialog.editing_name {
        let name_chars: Vec<char> = dialog.name.chars().collect();
        let cursor_col = display_width(&name_chars[..dialog.name_cursor.min(name_chars.len())]);
        let max_col = chunks[0].width.saturating_sub(2).saturating_sub(1) as usize;
        f.set_cursor_position((
            chunks[0].x + 1 + cursor_col.min(max_col) as u16,
            chunks[0].y + 1,
        ));
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
    let cmd_inner_width = chunks[1].width.saturating_sub(2).max(1) as usize;
    let cmd_selection = if !dialog.editing_name {
        dialog.selection
    } else {
        None
    };
    let (cmd_content, cursor_row, cursor_col) = wrapped_field_content(
        &dialog.command,
        dialog.command_cursor,
        cmd_selection,
        cmd_inner_width,
    );
    let visible_rows = chunks[1].height.saturating_sub(2) as usize;
    let scroll_offset = if visible_rows > 0 && cursor_row >= visible_rows {
        (cursor_row - visible_rows + 1) as u16
    } else {
        0
    };
    let cmd_widget = Paragraph::new(cmd_content)
        .scroll((scroll_offset, 0))
        .block(cmd_block);
    f.render_widget(cmd_widget, chunks[1]);

    // Cursor in command field
    if !dialog.editing_name && visible_rows > 0 {
        let adjusted_row = cursor_row.saturating_sub(scroll_offset as usize);
        let max_col = chunks[1].width.saturating_sub(2).saturating_sub(1) as usize;
        f.set_cursor_position((
            chunks[1].x + 1 + cursor_col.min(max_col) as u16,
            chunks[1].y + 1 + adjusted_row.min(visible_rows.saturating_sub(1)) as u16,
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

pub(crate) fn run_command_dialog_command_rows(text: &str, inner_width: usize) -> usize {
    wrapped_field_content(text, text.chars().count(), None, inner_width)
        .0
        .len()
        .max(1)
}

fn single_line_field_content(text: &str, selection: Option<(usize, usize)>) -> Line<'static> {
    let chars: Vec<char> = text.chars().collect();
    let normal_style = Style::default().fg(Color::White);
    let selection_style = Style::default().bg(Color::Blue).fg(Color::White);
    Line::from(styled_field_spans(
        &chars,
        0,
        chars.len(),
        normalized_selection(selection),
        normal_style,
        selection_style,
    ))
}

fn wrapped_field_content(
    text: &str,
    cursor: usize,
    selection: Option<(usize, usize)>,
    inner_width: usize,
) -> (Vec<Line<'static>>, usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return (vec![Line::from("")], 0, 0);
    }

    let target = cursor.min(chars.len());
    let breaks = word_wrap_break_points(&chars, inner_width.max(1));
    let selection = normalized_selection(selection);
    let normal_style = Style::default().fg(Color::White);
    let selection_style = Style::default().bg(Color::Blue).fg(Color::White);

    let mut rows = Vec::new();
    let mut cursor_row = 0usize;
    let mut cursor_col = 0usize;
    let mut prev = 0usize;

    for bp in breaks {
        let display_end = if bp > prev && chars.get(bp - 1) == Some(&'\n') {
            bp - 1
        } else {
            bp
        };
        if target >= prev && target < bp {
            cursor_row = rows.len();
            cursor_col = display_width(&chars[prev..target.min(display_end)]);
        }
        rows.push(Line::from(styled_field_spans(
            &chars,
            prev,
            display_end,
            selection,
            normal_style,
            selection_style,
        )));
        prev = bp;
    }

    if target >= prev {
        cursor_row = rows.len();
        cursor_col = display_width(&chars[prev..target.min(chars.len())]);
    }
    rows.push(Line::from(styled_field_spans(
        &chars,
        prev,
        chars.len(),
        selection,
        normal_style,
        selection_style,
    )));

    (rows, cursor_row, cursor_col)
}

fn normalized_selection(selection: Option<(usize, usize)>) -> Option<(usize, usize)> {
    let (start, end) = selection?;
    if start == end {
        None
    } else if start < end {
        Some((start, end))
    } else {
        Some((end, start))
    }
}

fn styled_field_spans(
    chars: &[char],
    start: usize,
    end: usize,
    selection: Option<(usize, usize)>,
    normal: Style,
    selected: Style,
) -> Vec<Span<'static>> {
    if start >= end {
        return vec![Span::raw("")];
    }

    let Some((sel_start, sel_end)) = selection else {
        return vec![Span::styled(
            chars[start..end].iter().collect::<String>(),
            normal,
        )];
    };

    let mut spans = Vec::new();
    let highlight_start = sel_start.max(start).min(end);
    let highlight_end = sel_end.min(end).max(start);

    if start < highlight_start {
        spans.push(Span::styled(
            chars[start..highlight_start].iter().collect::<String>(),
            normal,
        ));
    }
    if highlight_start < highlight_end {
        spans.push(Span::styled(
            chars[highlight_start..highlight_end]
                .iter()
                .collect::<String>(),
            selected,
        ));
    }
    if highlight_end < end {
        spans.push(Span::styled(
            chars[highlight_end..end].iter().collect::<String>(),
            normal,
        ));
    }
    if spans.is_empty() {
        spans.push(Span::styled(
            chars[start..end].iter().collect::<String>(),
            normal,
        ));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_rows_grow_when_wrapped() {
        assert_eq!(run_command_dialog_command_rows("abcdef", 3), 2);
    }

    #[test]
    fn command_rows_include_explicit_newline() {
        assert_eq!(run_command_dialog_command_rows("one\ntwo", 20), 2);
    }

    #[test]
    fn wrapped_field_cursor_tracks_wrapped_row() {
        let (_, row, col) = wrapped_field_content("abcdef", 4, None, 3);
        assert_eq!(row, 1);
        assert_eq!(col, 1);
    }

    #[test]
    fn single_line_field_marks_selection() {
        let line = single_line_field_content("abc", Some((1, 3)));
        assert_eq!(line.spans.len(), 2);
    }
}
