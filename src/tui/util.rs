//! Small utility functions for TUI rendering
//!
//! The AZURE constant defines the app's signature color (#007FFF) used
//! everywhere Cyan was previously used, aligning with the "Azureal" name.

/// Azure blue (#3399FF) — the app's signature accent color, replacing all
/// uses of ANSI Cyan for a cohesive visual identity matching the name "Azureal".
pub const AZURE: ratatui::style::Color = ratatui::style::Color::Rgb(51, 153, 255);

/// Git brand orange (#F05032) — used for Git Actions panel border and accents
pub const GIT_ORANGE: ratatui::style::Color = ratatui::style::Color::Rgb(240, 80, 50);

/// Git brown (#A0522D, sienna) — warm secondary color for Git panel text elements
/// (headers, key hints, separators, footer) instead of generic gray
pub const GIT_BROWN: ratatui::style::Color = ratatui::style::Color::Rgb(160, 82, 45);
//
// Re-exports commonly used items from submodules:
// - `colorize`: Output colorization (colorize_output, MessageType, etc.)
// - `markdown`: Markdown parsing (parse_markdown_spans, etc.)
// - `render_events`: Display event rendering
// - `render_tools`: Tool result rendering

// Re-export commonly used items
pub use super::colorize::{colorize_output, detect_message_type, MessageType};
pub use super::render_events::render_display_events;

/// Truncate a string to max length, adding ellipsis if needed
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max - 1).collect::<String>()) }
}

