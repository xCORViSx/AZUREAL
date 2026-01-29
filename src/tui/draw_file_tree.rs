//! FileTree panel rendering
//!
//! Shows the directory structure for the selected session's worktree.
//! Supports expand/collapse of directories and file selection.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::app::{App, Focus};
use super::util::truncate;

/// Draw the file tree panel showing the session's worktree files
pub fn draw_file_tree(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.focus == Focus::FileTree;
    let viewport_height = area.height.saturating_sub(2) as usize;

    // Build display lines from file tree entries
    let mut lines: Vec<Line> = Vec::new();

    if app.file_tree_entries.is_empty() {
        // Show placeholder when no worktree or empty
        if app.current_session().and_then(|s| s.worktree_path.as_ref()).is_none() {
            lines.push(Line::from(Span::styled(
                "No worktree",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "Empty directory",
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else {
        for (idx, entry) in app.file_tree_entries.iter().enumerate() {
            let is_selected = app.file_tree_selected == Some(idx);

            // Indent based on depth
            let indent = "  ".repeat(entry.depth);

            // Icon for file or directory
            let (icon, icon_color) = if entry.is_dir {
                let expanded = app.file_tree_expanded.contains(&entry.path);
                if expanded {
                    ("▼ ", Color::Yellow)
                } else {
                    ("▶ ", Color::Yellow)
                }
            } else {
                // File icon based on extension
                let icon = match entry.path.extension().and_then(|e| e.to_str()) {
                    Some("rs") => "🦀",
                    Some("toml") => "⚙ ",
                    Some("md") => "📝",
                    Some("json") => "{}",
                    Some("yaml") | Some("yml") => "📋",
                    Some("lock") => "🔒",
                    _ => "  ",
                };
                (icon, Color::White)
            };

            // Build the line
            let mut spans = vec![
                Span::raw(indent),
                Span::styled(icon, Style::default().fg(icon_color)),
            ];

            // Name styling based on selection and file type
            let name_style = if is_selected {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else if entry.is_dir {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            // Truncate name to fit (38 - indent - icon)
            let max_name_len = 38usize.saturating_sub(entry.depth * 2 + 2);
            spans.push(Span::styled(truncate(&entry.name, max_name_len), name_style));

            lines.push(Line::from(spans));
        }
    }

    // Auto-scroll to keep selection visible
    let total = lines.len();
    let max_scroll = total.saturating_sub(viewport_height);
    let scroll = if let Some(selected) = app.file_tree_selected {
        if selected < app.file_tree_scroll {
            selected // Selection above viewport, scroll up
        } else if selected >= app.file_tree_scroll + viewport_height {
            selected.saturating_sub(viewport_height - 1) // Selection below viewport, scroll down
        } else {
            app.file_tree_scroll // Selection in view, keep current scroll
        }
    } else {
        app.file_tree_scroll
    }.min(max_scroll);
    app.file_tree_scroll = scroll;

    let display_lines: Vec<Line> = lines.into_iter().skip(scroll).take(viewport_height).collect();

    // Title with scroll indicator
    let title = if total > viewport_height {
        format!(" Files [{}/{}] ", scroll + display_lines.len().min(total - scroll), total)
    } else {
        " Files ".to_string()
    };

    let widget = Paragraph::new(display_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(if is_focused { BorderType::Double } else { BorderType::Plain })
            .title(if is_focused {
                Span::styled(title, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(title, Style::default().fg(Color::White))
            })
            .border_style(if is_focused {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            }),
    );

    f.render_widget(widget, area);
}
