//! Viewport cache building for the session pane.
//!
//! Extracts a viewport slice from the pre-rendered line cache and applies
//! real-time overlays: tool status indicators, text selection, clicked path
//! highlighting, and search match highlighting. Also computes the title
//! with message position counter.

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use super::selection::compute_line_content_bounds;
use crate::app::App;
use crate::tui::colorize::ORANGE;

/// Rebuild the viewport cache from the rendered lines cache.
///
/// Clones the visible slice, patches tool status indicators, applies
/// text selection / path / search highlighting, computes the title
/// string, and writes everything back to `app.session_viewport_*` fields.
pub(super) fn rebuild_viewport_cache(app: &mut App, scroll: usize, viewport_height: usize) {
    // Clone viewport slice from the pre-rendered line cache
    let mut lines: Vec<Line> = app
        .rendered_lines_cache
        .iter()
        .skip(scroll)
        .take(viewport_height)
        .cloned()
        .collect();

    // Patch tool status indicators based on current state.
    // The render cache bakes in the status at render time, but tools may
    // complete or fail between renders. This patches both text and color
    // so circles update immediately without a full re-render.
    if !app.animation_line_indices.is_empty() {
        let pulse_colors = [Color::White, Color::Gray, Color::DarkGray, Color::Gray];
        let pulse_color = pulse_colors[(app.animation_tick / 2) as usize % pulse_colors.len()];
        for (line_idx, span_idx, tool_use_id) in &app.animation_line_indices {
            if *line_idx >= scroll && *line_idx < scroll + viewport_height {
                let viewport_idx = line_idx - scroll;
                if let Some(line) = lines.get_mut(viewport_idx) {
                    if let Some(span) = line.spans.get_mut(*span_idx) {
                        let is_pending = app.pending_tool_calls.contains(tool_use_id);
                        let is_failed = app.failed_tool_calls.contains(tool_use_id);
                        if is_pending {
                            span.content = "○ ".into();
                            span.style = span.style.fg(pulse_color);
                        } else if is_failed {
                            span.content = "✗ ".into();
                            span.style = span.style.fg(Color::Red);
                        } else {
                            span.content = "● ".into();
                            span.style = span.style.fg(Color::Green);
                        }
                    }
                }
            }
        }
    }

    // Apply text selection highlighting if active.
    // Clamp to per-line content bounds so bubble chrome
    // (gutters, borders, headers) is never highlighted.
    if let Some((sl, sc, el, ec)) = app.session_selection {
        for (vi, line) in lines.iter_mut().enumerate() {
            let ci = scroll + vi;
            if ci >= sl && ci <= el {
                let (cb_start, cb_end) = compute_line_content_bounds(line);
                if cb_start >= cb_end {
                    continue;
                }
                let eff_sc = if ci == sl { sc.max(cb_start) } else { cb_start };
                let eff_ec = if ci == el { ec.min(cb_end) } else { cb_end };
                if eff_sc >= eff_ec {
                    continue;
                }
                let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                let new_spans = crate::tui::draw_viewer::apply_selection_to_line(
                    line.spans.clone(),
                    &text,
                    0,
                    0,
                    eff_sc,
                    0,
                    eff_ec,
                    0,
                );
                *line = Line::from(new_spans);
            }
        }
    }
    app.session_selection_cached = app.session_selection;

    // Apply inverted highlight on clicked file path (orange bg, black fg)
    // Covers all wrapped lines of the path (first line uses column range,
    // continuation lines highlight all content)
    if let Some((hl, hsc, hec, wlc)) = app.clicked_path_highlight {
        let hl_style = Style::default().bg(ORANGE).fg(Color::Black);
        for row in 0..wlc {
            let cache_line = hl + row;
            if cache_line < scroll || cache_line >= scroll + viewport_height {
                continue;
            }
            let vi = cache_line - scroll;
            let Some(line) = lines.get_mut(vi) else {
                continue;
            };
            let (start, end) = if row == 0 {
                (hsc, hec)
            } else {
                (
                    hsc,
                    line.spans.iter().map(|s| s.content.chars().count()).sum(),
                )
            };
            let mut new_spans: Vec<Span<'static>> = Vec::new();
            let mut col = 0usize;
            for span in line.spans.iter() {
                let span_len = span.content.chars().count();
                let span_end = col + span_len;
                if span_end <= start || col >= end {
                    new_spans.push(span.clone());
                } else {
                    let chars: Vec<char> = span.content.chars().collect();
                    let hs = start.saturating_sub(col);
                    let he = (end - col).min(span_len);
                    if hs > 0 {
                        let before: String = chars[..hs].iter().collect();
                        new_spans.push(Span::styled(before, span.style));
                    }
                    let mid: String = chars[hs..he].iter().collect();
                    new_spans.push(Span::styled(mid, hl_style));
                    if he < span_len {
                        let after: String = chars[he..].iter().collect();
                        new_spans.push(Span::styled(after, span.style));
                    }
                }
                col = span_end;
            }
            *line = Line::from(new_spans);
        }
    }

    // Apply session find match highlighting (yellow bg for matches,
    // bright yellow for current match — same span-splitting technique)
    if !app.session_find_matches.is_empty() {
        let match_style = Style::default().bg(Color::DarkGray).fg(Color::Yellow);
        let current_style = Style::default().bg(Color::Yellow).fg(Color::Black);
        for (mi, &(line_idx, sc, ec)) in app.session_find_matches.iter().enumerate() {
            if line_idx < scroll || line_idx >= scroll + viewport_height {
                continue;
            }
            let vi = line_idx - scroll;
            let Some(line) = lines.get_mut(vi) else {
                continue;
            };
            let style = if mi == app.session_find_current {
                current_style
            } else {
                match_style
            };
            let mut new_spans: Vec<Span<'static>> = Vec::new();
            let mut col = 0usize;
            for span in line.spans.iter() {
                let span_len = span.content.chars().count();
                let span_end = col + span_len;
                if span_end <= sc || col >= ec {
                    new_spans.push(span.clone());
                } else {
                    let chars: Vec<char> = span.content.chars().collect();
                    let hs = sc.saturating_sub(col);
                    let he = (ec - col).min(span_len);
                    if hs > 0 {
                        new_spans.push(Span::styled(
                            chars[..hs].iter().collect::<String>(),
                            span.style,
                        ));
                    }
                    new_spans.push(Span::styled(
                        chars[hs..he].iter().collect::<String>(),
                        style,
                    ));
                    if he < span_len {
                        new_spans.push(Span::styled(
                            chars[he..].iter().collect::<String>(),
                            span.style,
                        ));
                    }
                }
                col = span_end;
            }
            *line = Line::from(new_spans);
        }
    }

    // Build title with message count
    // Total counts ALL display events (not just rendered tail from deferred render)
    // so the denominator is accurate even before the user scrolls to top
    let total_msgs = app
        .display_events
        .iter()
        .filter(|e| {
            matches!(
                e,
                crate::events::DisplayEvent::UserMessage { .. }
                    | crate::events::DisplayEvent::AssistantText { .. }
            )
        })
        .count();
    let title = if total_msgs > 0 {
        let current_line = scroll.saturating_add(3);
        // Current position from rendered bubble positions (only covers rendered tail)
        // Add the unrendered bubble count as offset so numbering is correct
        let rendered_bubbles = app.message_bubble_positions.len();
        let unrendered_offset = total_msgs.saturating_sub(rendered_bubbles);
        let current_msg = app
            .message_bubble_positions
            .iter()
            .enumerate()
            .rev()
            .find(|(_, (line_idx, _))| *line_idx <= current_line)
            .map(|(idx, _)| idx + 1 + unrendered_offset)
            .unwrap_or(1);
        format!(" Session [{}/{}] ", current_msg, total_msgs)
    } else {
        " Session ".to_string()
    };

    app.session_viewport_cache = lines;
    app.session_viewport_scroll = scroll;
    app.session_viewport_anim_tick = app.animation_tick;
    app.session_viewport_status_gen = app.tool_status_generation;
    app.session_viewport_title = title;
}
