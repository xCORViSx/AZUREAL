//! Sticky todo widget
//!
//! Renders the task progress tracker at the bottom of the convo pane.
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

/// Render the sticky todo widget — shows at the bottom of the convo pane.
/// Main agent todos show as check/bullet/circle. Subagent todos show indented
/// with "↳" prefix directly after the parent todo item.
pub fn draw_todo_widget(
    f: &mut Frame,
    todos: &[TodoItem],
    subagent_todos: &[TodoItem],
    parent_idx: Option<usize>,
    area: Rect,
    animation_tick: u64,
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

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Tasks ", Style::default().fg(AZURE).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(Color::DarkGray));

    let widget = Paragraph::new(todo_lines).block(block).wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}
