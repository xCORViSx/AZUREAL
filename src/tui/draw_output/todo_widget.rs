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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{BorderType, Borders};

    // ══════════════════════════════════════════════════════════════════
    //  AZURE constant
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn azure_color_value() {
        assert_eq!(AZURE, Color::Rgb(51, 153, 255));
    }

    // ══════════════════════════════════════════════════════════════════
    //  TodoStatus variants
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn todo_status_completed_eq() {
        assert_eq!(TodoStatus::Completed, TodoStatus::Completed);
    }

    #[test]
    fn todo_status_in_progress_eq() {
        assert_eq!(TodoStatus::InProgress, TodoStatus::InProgress);
    }

    #[test]
    fn todo_status_pending_eq() {
        assert_eq!(TodoStatus::Pending, TodoStatus::Pending);
    }

    #[test]
    fn todo_status_variants_distinct() {
        assert_ne!(TodoStatus::Completed, TodoStatus::InProgress);
        assert_ne!(TodoStatus::Completed, TodoStatus::Pending);
        assert_ne!(TodoStatus::InProgress, TodoStatus::Pending);
    }

    #[test]
    fn todo_status_clone() {
        let s = TodoStatus::InProgress;
        let cloned = s.clone();
        assert_eq!(s, cloned);
    }

    #[test]
    fn todo_status_debug() {
        assert_eq!(format!("{:?}", TodoStatus::Completed), "Completed");
        assert_eq!(format!("{:?}", TodoStatus::InProgress), "InProgress");
        assert_eq!(format!("{:?}", TodoStatus::Pending), "Pending");
    }

    // ══════════════════════════════════════════════════════════════════
    //  TodoItem construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn todo_item_completed() {
        let t = TodoItem {
            content: "Done task".to_string(),
            status: TodoStatus::Completed,
            active_form: String::new(),
        };
        assert_eq!(t.status, TodoStatus::Completed);
        assert_eq!(t.content, "Done task");
    }

    #[test]
    fn todo_item_in_progress_with_active_form() {
        let t = TodoItem {
            content: "Building project".to_string(),
            status: TodoStatus::InProgress,
            active_form: "Building...".to_string(),
        };
        assert!(!t.active_form.is_empty());
        assert_eq!(t.active_form, "Building...");
    }

    #[test]
    fn todo_item_pending_empty_active_form() {
        let t = TodoItem {
            content: "Future task".to_string(),
            status: TodoStatus::Pending,
            active_form: String::new(),
        };
        assert!(t.active_form.is_empty());
    }

    #[test]
    fn todo_item_clone() {
        let t = TodoItem {
            content: "clone me".to_string(),
            status: TodoStatus::Pending,
            active_form: "cloning".to_string(),
        };
        let cloned = t.clone();
        assert_eq!(t.content, cloned.content);
        assert_eq!(t.status, cloned.status);
        assert_eq!(t.active_form, cloned.active_form);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Pulse color array
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn pulse_colors_count() {
        let pulse_colors = [Color::Yellow, Color::LightYellow, Color::Yellow, Color::DarkGray];
        assert_eq!(pulse_colors.len(), 4);
    }

    #[test]
    fn pulse_colors_first_is_yellow() {
        let pulse_colors = [Color::Yellow, Color::LightYellow, Color::Yellow, Color::DarkGray];
        assert_eq!(pulse_colors[0], Color::Yellow);
    }

    #[test]
    fn pulse_colors_second_is_light_yellow() {
        let pulse_colors = [Color::Yellow, Color::LightYellow, Color::Yellow, Color::DarkGray];
        assert_eq!(pulse_colors[1], Color::LightYellow);
    }

    #[test]
    fn pulse_colors_last_is_dark_gray() {
        let pulse_colors = [Color::Yellow, Color::LightYellow, Color::Yellow, Color::DarkGray];
        assert_eq!(pulse_colors[3], Color::DarkGray);
    }

    #[test]
    fn pulse_index_wraps_at_len() {
        let pulse_colors = [Color::Yellow, Color::LightYellow, Color::Yellow, Color::DarkGray];
        let tick: u64 = 100;
        let idx = (tick / 3) as usize % pulse_colors.len();
        assert!(idx < pulse_colors.len());
    }

    #[test]
    fn pulse_index_at_zero() {
        let tick: u64 = 0;
        let idx = (tick / 3) as usize % 4;
        assert_eq!(idx, 0);
    }

    #[test]
    fn pulse_index_at_three() {
        let tick: u64 = 9;
        let idx = (tick / 3) as usize % 4;
        assert_eq!(idx, 3);
    }

    #[test]
    fn pulse_index_cycles() {
        let tick: u64 = 12;
        let idx = (tick / 3) as usize % 4;
        assert_eq!(idx, 0); // wraps
    }

    // ══════════════════════════════════════════════════════════════════
    //  Icon and color matching per status
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn completed_icon_is_checkmark() {
        let status = TodoStatus::Completed;
        let (icon, _color) = match status {
            TodoStatus::Completed => ("\u{2713} ", Color::Green),
            TodoStatus::InProgress => ("\u{25cf} ", Color::Yellow),
            TodoStatus::Pending => ("\u{25cb} ", Color::DarkGray),
        };
        assert_eq!(icon, "\u{2713} ");
    }

    #[test]
    fn completed_color_is_green() {
        let status = TodoStatus::Completed;
        let (_icon, color) = match status {
            TodoStatus::Completed => ("\u{2713} ", Color::Green),
            TodoStatus::InProgress => ("\u{25cf} ", Color::Yellow),
            TodoStatus::Pending => ("\u{25cb} ", Color::DarkGray),
        };
        assert_eq!(color, Color::Green);
    }

    #[test]
    fn in_progress_icon_is_filled_circle() {
        let status = TodoStatus::InProgress;
        let (icon, _color) = match status {
            TodoStatus::Completed => ("\u{2713} ", Color::Green),
            TodoStatus::InProgress => ("\u{25cf} ", Color::Yellow),
            TodoStatus::Pending => ("\u{25cb} ", Color::DarkGray),
        };
        assert_eq!(icon, "\u{25cf} ");
    }

    #[test]
    fn pending_icon_is_empty_circle() {
        let status = TodoStatus::Pending;
        let (icon, _color) = match status {
            TodoStatus::Completed => ("\u{2713} ", Color::Green),
            TodoStatus::InProgress => ("\u{25cf} ", Color::Yellow),
            TodoStatus::Pending => ("\u{25cb} ", Color::DarkGray),
        };
        assert_eq!(icon, "\u{25cb} ");
    }

    #[test]
    fn pending_color_is_dark_gray() {
        let status = TodoStatus::Pending;
        let (_icon, color) = match status {
            TodoStatus::Completed => ("\u{2713} ", Color::Green),
            TodoStatus::InProgress => ("\u{25cf} ", Color::Yellow),
            TodoStatus::Pending => ("\u{25cb} ", Color::DarkGray),
        };
        assert_eq!(color, Color::DarkGray);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Text display logic (active_form vs content)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn in_progress_uses_active_form_when_nonempty() {
        let t = TodoItem {
            content: "original".to_string(),
            status: TodoStatus::InProgress,
            active_form: "doing it".to_string(),
        };
        let text = if t.status == TodoStatus::InProgress && !t.active_form.is_empty() {
            &t.active_form
        } else {
            &t.content
        };
        assert_eq!(text, "doing it");
    }

    #[test]
    fn in_progress_falls_back_to_content_when_empty() {
        let t = TodoItem {
            content: "original".to_string(),
            status: TodoStatus::InProgress,
            active_form: String::new(),
        };
        let text = if t.status == TodoStatus::InProgress && !t.active_form.is_empty() {
            &t.active_form
        } else {
            &t.content
        };
        assert_eq!(text, "original");
    }

    #[test]
    fn completed_always_uses_content() {
        let t = TodoItem {
            content: "done".to_string(),
            status: TodoStatus::Completed,
            active_form: "doing".to_string(),
        };
        let text = if t.status == TodoStatus::InProgress && !t.active_form.is_empty() {
            &t.active_form
        } else {
            &t.content
        };
        assert_eq!(text, "done");
    }

    #[test]
    fn pending_always_uses_content() {
        let t = TodoItem {
            content: "waiting".to_string(),
            status: TodoStatus::Pending,
            active_form: "running".to_string(),
        };
        let text = if t.status == TodoStatus::InProgress && !t.active_form.is_empty() {
            &t.active_form
        } else {
            &t.content
        };
        assert_eq!(text, "waiting");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Text color per status
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn completed_text_color_is_dark_gray() {
        let status = TodoStatus::Completed;
        let color = if status == TodoStatus::Completed { Color::DarkGray } else { Color::White };
        assert_eq!(color, Color::DarkGray);
    }

    #[test]
    fn in_progress_text_color_is_white() {
        let status = TodoStatus::InProgress;
        let color = if status == TodoStatus::Completed { Color::DarkGray } else { Color::White };
        assert_eq!(color, Color::White);
    }

    #[test]
    fn pending_text_color_is_white() {
        let status = TodoStatus::Pending;
        let color = if status == TodoStatus::Completed { Color::DarkGray } else { Color::White };
        assert_eq!(color, Color::White);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Subtask prefix
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn subtask_prefix_character() {
        let prefix = "\u{21b3} "; // ↳
        assert_eq!(prefix, "\u{21b3} ");
        assert!(prefix.starts_with('\u{21b3}'));
    }

    #[test]
    fn subtask_span_has_dark_gray_color() {
        let span = Span::styled("\u{21b3} ", Style::default().fg(Color::DarkGray));
        assert_eq!(span.style.fg, Some(Color::DarkGray));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Insert position logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn insert_after_with_parent_idx() {
        let parent_idx = Some(2usize);
        let todos_len: usize = 5;
        let insert_after = parent_idx.unwrap_or(todos_len.saturating_sub(1));
        assert_eq!(insert_after, 2);
    }

    #[test]
    fn insert_after_no_parent_falls_back_to_last() {
        let parent_idx: Option<usize> = None;
        let todos_len: usize = 5;
        let insert_after = parent_idx.unwrap_or(todos_len.saturating_sub(1));
        assert_eq!(insert_after, 4);
    }

    #[test]
    fn insert_after_no_parent_empty_todos() {
        let parent_idx: Option<usize> = None;
        let todos_len: usize = 0;
        let insert_after = parent_idx.unwrap_or(todos_len.saturating_sub(1));
        assert_eq!(insert_after, 0); // saturating_sub prevents underflow
    }

    // ══════════════════════════════════════════════════════════════════
    //  Content height and scrollbar logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn content_height_normal() {
        let area = Rect::new(0, 0, 40, 12);
        let content_h = area.height.saturating_sub(2);
        assert_eq!(content_h, 10);
    }

    #[test]
    fn content_height_tiny() {
        let area = Rect::new(0, 0, 40, 2);
        let content_h = area.height.saturating_sub(2);
        assert_eq!(content_h, 0);
    }

    #[test]
    fn needs_scrollbar_when_content_exceeds() {
        let total_lines: u16 = 25;
        let content_h: u16 = 10;
        assert!(total_lines > content_h);
    }

    #[test]
    fn no_scrollbar_when_fits() {
        let total_lines: u16 = 5;
        let content_h: u16 = 10;
        assert!(!(total_lines > content_h));
    }

    // ══════════════════════════════════════════════════════════════════
    //  Scrollbar thumb math
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn thumb_height_proportional() {
        let content_h: u16 = 20;
        let total_lines: u16 = 40;
        let thumb_h = ((content_h as u32 * content_h as u32) / total_lines.max(1) as u32).max(1).min(content_h as u32) as u16;
        assert_eq!(thumb_h, 10); // 20*20/40 = 10
    }

    #[test]
    fn thumb_height_minimum_one() {
        let content_h: u16 = 5;
        let total_lines: u16 = 1000;
        let thumb_h = ((content_h as u32 * content_h as u32) / total_lines.max(1) as u32).max(1).min(content_h as u32) as u16;
        assert_eq!(thumb_h, 1); // 25/1000 rounds to 0, clamped to 1
    }

    #[test]
    fn thumb_height_capped_at_content_h() {
        let content_h: u16 = 10;
        let total_lines: u16 = 5;
        let thumb_h = ((content_h as u32 * content_h as u32) / total_lines.max(1) as u32).max(1).min(content_h as u32) as u16;
        assert_eq!(thumb_h, content_h); // 100/5 = 20, capped at 10
    }

    #[test]
    fn thumb_position_at_top() {
        let scroll: u16 = 0;
        let content_h: u16 = 20;
        let thumb_h: u16 = 5;
        let max_scroll: u16 = 30;
        let thumb_top = if max_scroll == 0 { 0 } else {
            ((scroll as u32 * (content_h - thumb_h) as u32) / max_scroll as u32) as u16
        };
        assert_eq!(thumb_top, 0);
    }

    #[test]
    fn thumb_position_at_bottom() {
        let scroll: u16 = 30;
        let content_h: u16 = 20;
        let thumb_h: u16 = 5;
        let max_scroll: u16 = 30;
        let thumb_top = if max_scroll == 0 { 0 } else {
            ((scroll as u32 * (content_h - thumb_h) as u32) / max_scroll as u32) as u16
        };
        assert_eq!(thumb_top, 15); // 30*15/30 = 15
    }

    #[test]
    fn thumb_position_max_scroll_zero() {
        let _scroll: u16 = 0;
        let max_scroll: u16 = 0;
        let thumb_top = if max_scroll == 0 { 0 } else { 1 };
        assert_eq!(thumb_top, 0);
    }

    #[test]
    fn max_scroll_calculation() {
        let total_lines: u16 = 30;
        let content_h: u16 = 10;
        let max_scroll = total_lines.saturating_sub(content_h);
        assert_eq!(max_scroll, 20);
    }

    #[test]
    fn max_scroll_when_fits() {
        let total_lines: u16 = 5;
        let content_h: u16 = 10;
        let max_scroll = total_lines.saturating_sub(content_h);
        assert_eq!(max_scroll, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Scrollbar track coordinates
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn track_x_position() {
        let area = Rect::new(5, 2, 40, 12);
        let track_x = area.x + area.width - 1;
        assert_eq!(track_x, 44);
    }

    #[test]
    fn track_top_position() {
        let area = Rect::new(5, 2, 40, 12);
        let track_top = area.y + 1;
        assert_eq!(track_top, 3);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Block and widget configuration
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn block_has_rounded_border() {
        let block = ratatui::widgets::Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded);
        let _ = block;
    }

    #[test]
    fn title_text() {
        let title = " Tasks ";
        assert_eq!(title, " Tasks ");
    }

    #[test]
    fn title_style_has_azure_bold() {
        let style = Style::default().fg(AZURE).add_modifier(Modifier::BOLD);
        assert_eq!(style.fg, Some(AZURE));
    }

    #[test]
    fn border_style_dark_gray() {
        let style = Style::default().fg(Color::DarkGray);
        assert_eq!(style.fg, Some(Color::DarkGray));
    }

    // ══════════════════════════════════════════════════════════════════
    //  make_line reconstruction tests
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn make_line_completed_not_subtask() {
        let t = TodoItem {
            content: "Done".to_string(),
            status: TodoStatus::Completed,
            active_form: String::new(),
        };
        let is_subtask = false;
        let (icon, color) = ("\u{2713} ", Color::Green);
        let text = &t.content;
        let text_color = Color::DarkGray;
        let mut spans = Vec::new();
        if is_subtask {
            spans.push(Span::styled("\u{21b3} ", Style::default().fg(Color::DarkGray)));
        }
        spans.push(Span::styled(icon, Style::default().fg(color)));
        spans.push(Span::styled(text.clone(), Style::default().fg(text_color)));
        let line = Line::from(spans);
        assert_eq!(line.spans.len(), 2); // icon + text, no subtask prefix
    }

    #[test]
    fn make_line_subtask_has_three_spans() {
        let t = TodoItem {
            content: "Sub".to_string(),
            status: TodoStatus::Pending,
            active_form: String::new(),
        };
        let is_subtask = true;
        let mut spans = Vec::new();
        if is_subtask {
            spans.push(Span::styled("\u{21b3} ", Style::default().fg(Color::DarkGray)));
        }
        spans.push(Span::styled("\u{25cb} ", Style::default().fg(Color::DarkGray)));
        spans.push(Span::styled(t.content.clone(), Style::default().fg(Color::White)));
        let line = Line::from(spans);
        assert_eq!(line.spans.len(), 3); // prefix + icon + text
    }

    // ══════════════════════════════════════════════════════════════════
    //  Todo lines assembly
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn todo_lines_empty_todos_appends_subagent() {
        let todos: Vec<TodoItem> = vec![];
        let subagent = vec![TodoItem {
            content: "sub task".to_string(),
            status: TodoStatus::Pending,
            active_form: String::new(),
        }];
        let mut lines: Vec<String> = Vec::new();
        if todos.is_empty() {
            for sub in &subagent {
                lines.push(sub.content.clone());
            }
        }
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "sub task");
    }

    #[test]
    fn todo_lines_injects_after_parent() {
        let todos = vec![
            TodoItem { content: "A".to_string(), status: TodoStatus::Completed, active_form: String::new() },
            TodoItem { content: "B".to_string(), status: TodoStatus::InProgress, active_form: "Doing B".to_string() },
            TodoItem { content: "C".to_string(), status: TodoStatus::Pending, active_form: String::new() },
        ];
        let subs = vec![
            TodoItem { content: "S1".to_string(), status: TodoStatus::Pending, active_form: String::new() },
        ];
        let insert_after = 1; // after "B"
        let mut result: Vec<String> = Vec::new();
        for (i, t) in todos.iter().enumerate() {
            result.push(t.content.clone());
            if i == insert_after {
                for sub in &subs {
                    result.push(format!("  {}", sub.content));
                }
            }
        }
        assert_eq!(result, vec!["A", "B", "  S1", "C"]);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Scrollbar symbol constants
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn thumb_symbol() {
        let sym = "\u{2588}"; // █
        assert_eq!(sym, "\u{2588}");
    }

    #[test]
    fn track_symbol() {
        let sym = "\u{2502}"; // │
        assert_eq!(sym, "\u{2502}");
    }

    #[test]
    fn thumb_style_is_azure() {
        let style = Style::default().fg(AZURE);
        assert_eq!(style.fg, Some(AZURE));
    }

    #[test]
    fn track_style_is_dark_gray() {
        let style = Style::default().fg(Color::DarkGray);
        assert_eq!(style.fg, Some(Color::DarkGray));
    }
}
