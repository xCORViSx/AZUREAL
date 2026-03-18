//! Preset prompt picker and editor dialogs

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::app::App;
use crate::tui::keybindings;
use crate::tui::util::{truncate, AZURE};

/// Draw preset prompt picker overlay — numbered list of saved prompts.
/// 1-9 for first 9 presets, 0 for the 10th (keyboard-order layout).
pub fn draw_preset_prompt_picker(f: &mut Frame, app: &App, area: Rect) {
    let Some(ref picker) = app.preset_prompt_picker else {
        return;
    };
    let count = app.preset_prompts.len();

    // Size: fit all presets + borders
    let dialog_width = 60u16.min(area.width.saturating_sub(4));
    let dialog_height = (count as u16 + 4).min(area.height.saturating_sub(4));
    let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    // Build list items with number shortcuts and selection highlight
    let items: Vec<ListItem> = app
        .preset_prompts
        .iter()
        .enumerate()
        .map(|(idx, preset)| {
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
            let preview_max =
                (dialog_width as usize).saturating_sub(num_hint.len() + max_name + 10);
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
        })
        .collect();

    // Show delete confirmation in title if pending, otherwise from keybindings.rs
    let title = if let Some(del_idx) = picker.confirm_delete {
        let name = app
            .preset_prompts
            .get(del_idx)
            .map(|p| p.name.as_str())
            .unwrap_or("?");
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
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                hint,
                Style::default().fg(Color::DarkGray),
            ))),
            hint_rect,
        );
    }
}

/// Draw preset prompt dialog overlay (create/edit a preset prompt).
/// Two text fields: Name and Prompt, stacked vertically.
pub fn draw_preset_prompt_dialog(f: &mut Frame, app: &App) {
    let Some(ref dialog) = app.preset_prompt_dialog else {
        return;
    };
    let area = f.area();

    // Two text fields stacked: name(3) + prompt(3) + hints(1) + borders(2) = 9
    let dialog_width = 60u16.min(area.width.saturating_sub(4));
    let dialog_height = 9u16.min(area.height.saturating_sub(4));
    let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    // Outer border with title — left shows mode, right shows scope badge
    let title_text = if dialog.editing_idx.is_some() {
        " Edit Preset "
    } else {
        " New Preset "
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

    // Split inner area: name(3) + prompt(3) + hints(1)
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

    // Cursor in name field
    if dialog.editing_name {
        // Convert char cursor to display position (byte-based for ASCII, char-based for unicode)
        let display_pos = dialog.name.chars().take(dialog.name_cursor).count();
        f.set_cursor_position((chunks[0].x + 1 + display_pos as u16, chunks[0].y + 1));
    }

    // Prompt field — yellow border when active
    let prompt_color = if !dialog.editing_name {
        Color::Yellow
    } else {
        Color::DarkGray
    };
    let prompt_widget = Paragraph::new(dialog.prompt.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(prompt_color))
            .title(Span::styled(" Prompt ", Style::default().fg(prompt_color))),
    );
    f.render_widget(prompt_widget, chunks[1]);

    // Cursor in prompt field
    if !dialog.editing_name {
        let display_pos = dialog.prompt.chars().take(dialog.prompt_cursor).count();
        f.set_cursor_position((chunks[1].x + 1 + display_pos as u16, chunks[1].y + 1));
    }

    // Hint line — structural keys from keybindings.rs, Enter label varies by context
    let enter_label = if dialog.editing_name { "next" } else { "save" };
    let pairs = keybindings::dialog_footer_hint_pairs();
    let hint_spans: Vec<Span> = pairs
        .iter()
        .map(|(key, label)| {
            let display_label = if key == "Enter" { enter_label } else { label };
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
