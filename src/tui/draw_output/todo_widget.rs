//! Sticky todo widget
//!
//! Renders the task progress tracker at the bottom of the session pane.
//! Shows main agent todos (checkmark/bullet icons) and subagent todos
//! indented with "↳" directly after the parent task.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::{TodoItem, TodoStatus};
use super::super::util::AZURE;

/// Render the sticky todo widget — shows at the bottom of the session pane.
/// Main agent todos show as check/bullet/circle. Subagent todos show indented
/// with "↳" prefix directly after the parent todo item.
/// When content exceeds the visible area (20 lines max), a scrollbar column
/// appears on the right edge and the widget responds to mouse wheel scrolling.
pub fn draw_todo_widget(
    f: &mut Frame,
    todos: &[TodoItem],
    subagent_todos: &[TodoItem],
    parent_idx: Option<usize>,
    area: Rect,
    animation_tick: u64,
    scroll: u16,
    total_lines: u16,
) {
    let pulse_colors = [Color::Yellow, Color::LightYellow, Color::Yellow, Color::DarkGray];
    let pulse = pulse_colors[(animation_tick / 3) as usize % pulse_colors.len()];

    // Convert a single TodoItem into a Line with optional "↳ " indent prefix
    let make_line = |t: &TodoItem, is_subtask: bool| -> Line {
        let (icon, color) = match t.status {
            TodoStatus::Completed => ("\u{2713} ", Color::Green),
            TodoStatus::InProgress => ("\u{25cf} ", pulse),
            TodoStatus::Pending => ("\u{25cb} ", Color::DarkGray),
        };
        let text = if t.status == TodoStatus::InProgress && !t.active_form.is_empty() {
            &t.active_form
        } else { &t.content };
        let text_color = if t.status == TodoStatus::Completed { Color::DarkGray } else { Color::White };
        let mut spans = Vec::new();
        if is_subtask {
            spans.push(Span::styled("\u{21b3} ", Style::default().fg(Color::DarkGray)));
        }
        spans.push(Span::styled(icon, Style::default().fg(color)));
        spans.push(Span::styled(text.clone(), Style::default().fg(text_color)));
        Line::from(spans)
    };

    // Insert position: right after the parent todo item.
    // If no parent tracked, fall back to end of list (append).
    let insert_after = parent_idx.unwrap_or(todos.len().saturating_sub(1));

    let mut todo_lines: Vec<Line> = Vec::with_capacity(todos.len() + subagent_todos.len());
    for (i, t) in todos.iter().enumerate() {
        todo_lines.push(make_line(t, false));
        // Inject subagent subtasks right after the parent item
        if i == insert_after && !subagent_todos.is_empty() {
            for sub in subagent_todos {
                todo_lines.push(make_line(sub, true));
            }
        }
    }
    // Edge case: no main todos but subagent todos exist (shouldn't happen, but safe)
    if todos.is_empty() {
        for sub in subagent_todos {
            todo_lines.push(make_line(sub, true));
        }
    }

    // Content height inside borders (area minus top+bottom border)
    let content_h = area.height.saturating_sub(2);
    // Whether we need a scrollbar (content overflows the visible area)
    let needs_scrollbar = total_lines > content_h;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Tasks ", Style::default().fg(AZURE).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(Color::DarkGray));

    // Apply scroll offset so the widget starts at the right place
    let widget = Paragraph::new(todo_lines).block(block).wrap(Wrap { trim: false }).scroll((scroll, 0));
    f.render_widget(widget, area);

    // Draw scrollbar track on the rightmost column inside the border.
    // Uses block chars: █ for thumb, │ for track — same visual language as other panes.
    if needs_scrollbar && content_h > 0 {
        let track_x = area.x + area.width - 1;
        let track_top = area.y + 1;
        let max_scroll = total_lines.saturating_sub(content_h);
        // Thumb size: proportional to visible/total, minimum 1 row
        let thumb_h = ((content_h as u32 * content_h as u32) / total_lines.max(1) as u32).max(1).min(content_h as u32) as u16;
        // Thumb position: proportional to scroll offset within max_scroll
        let thumb_top = if max_scroll == 0 { 0 } else {
            ((scroll as u32 * (content_h - thumb_h) as u32) / max_scroll as u32) as u16
        };
        let track_style = Style::default().fg(Color::DarkGray);
        let thumb_style = Style::default().fg(AZURE);
        let buf = f.buffer_mut();
        for row in 0..content_h {
            let cell = &mut buf[(track_x, track_top + row)];
            if row >= thumb_top && row < thumb_top + thumb_h {
                cell.set_symbol("█");
                cell.set_style(thumb_style);
            } else {
                cell.set_symbol("│");
                cell.set_style(track_style);
            }
        }
    }
}
