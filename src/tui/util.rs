//! Small utility functions for TUI rendering
//!
//! Re-exports commonly used items from submodules:
//! - `colorize`: Output colorization (colorize_output, MessageType, etc.)
//! - `markdown`: Markdown parsing (parse_markdown_spans, etc.)
//! - `render_events`: Display event rendering
//! - `render_tools`: Tool result rendering

// Re-export commonly used items
pub use super::colorize::{colorize_output, detect_message_type, MessageType};
pub use super::render_events::{render_display_events, render_display_events_incremental};

/// Truncate a string to max length, adding ellipsis if needed
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max - 1).collect::<String>()) }
}

/// Check if output is scrolled to bottom
pub fn is_scrolled_to_bottom(output_scroll: usize, output_lines_len: usize) -> bool {
    if output_scroll == usize::MAX { return true; }
    if output_lines_len == 0 { return true; }
    output_scroll + 5 >= output_lines_len.saturating_sub(20)
}

/// Calculate the visual cursor position in a multi-line text area
pub fn calculate_cursor_position(text: &str, cursor: usize, width: usize) -> Option<(usize, usize)> {
    let mut x = 0;
    let mut y = 0;
    let mut pos = 0;

    for ch in text.chars() {
        if pos >= cursor { break; }
        if ch == '\n' {
            y += 1;
            x = 0;
        } else {
            x += 1;
            if x >= width {
                y += 1;
                x = 0;
            }
        }
        pos += ch.len_utf8();
    }

    Some((x, y))
}
