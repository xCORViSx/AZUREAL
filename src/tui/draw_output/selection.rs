//! Selectable content range calculation for session pane cache lines.
//!
//! Strips bubble chrome (gutters, borders, headers) so selection/copy
//! only includes actual message text.

use ratatui::{style::Color, text::Line};

use crate::tui::colorize::ORANGE;
use crate::tui::util::AZURE;

/// Compute the selectable content range for a session pane cache line.
/// Returns (content_start_col, content_end_col) in char indices.
/// (0, 0) means non-selectable (decoration: blank, header, border).
/// Strips bubble chrome (gutters, borders) so selection/copy only include text.
pub(crate) fn compute_line_content_bounds(line: &Line) -> (usize, usize) {
    let spans = &line.spans;
    if spans.is_empty() {
        return (0, 0);
    }
    let total: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    if total == 0 {
        return (0, 0);
    }

    let first = &spans[0];
    let last = &spans[spans.len() - 1];

    // ── Assistant bubble decorations ──

    // Bottom border: "└─────" in ORANGE
    if first.content.starts_with('└') && first.style.fg == Some(ORANGE) {
        return (0, 0);
    }

    // Header: " Claude ▶ " has ORANGE background
    if first.style.bg == Some(ORANGE) {
        return (0, 0);
    }

    // Content lines: first span is "│ " in ORANGE (gutter)
    if first.content.as_ref() == "│ " && first.style.fg == Some(ORANGE) {
        if spans.len() > 1 {
            let second = &spans[1];
            // Code fence: second span starts with ┌ or └
            if second.content.starts_with('┌') || second.content.starts_with('└') {
                return (0, 0);
            }
            // Code block content: second span is "│ " in DarkGray
            if second.content.as_ref() == "│ " && second.style.fg == Some(Color::DarkGray) {
                return (4, total);
            }
        }
        return (2, total);
    }

    // ── User bubble decorations ──

    // Bottom border: last span ends with "┘" in AZURE
    if last.content.ends_with('┘') && last.style.fg == Some(AZURE) {
        return (0, 0);
    }

    // Header: any span has AZURE background
    if spans.iter().any(|s| s.style.bg == Some(AZURE)) {
        return (0, 0);
    }

    // Content lines: last span is " │" in AZURE (right border)
    if last.content.as_ref() == " │" && last.style.fg == Some(AZURE) {
        let right = total - 2; // strip " │"
                               // Left: sum of all spans before content (offset + padding)
        let left: usize = if spans.len() >= 3 {
            spans[..spans.len() - 2]
                .iter()
                .map(|s| s.content.chars().count())
                .sum()
        } else {
            0
        };
        if left < right {
            return (left, right);
        }
        return (0, 0);
    }

    // ── Everything else: fully selectable ──
    (0, total)
}
