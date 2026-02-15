//! Background render thread coordination
//!
//! Handles submitting render requests and polling for completed results.
//! The main event loop calls `submit_render_request()` when the cache is dirty
//! and `poll_render_result()` each frame to apply completed renders.
//! All expensive work (markdown, syntax highlighting, wrapping) happens on
//! the background thread — these functions just shuttle data back and forth.

use crate::app::{App, ViewMode};
use crate::events::DisplayEvent;
use super::super::render_thread::{PreScanState, RenderRequest};

/// On initial load of large conversations, only render this many events from the tail.
/// The user starts at the bottom so they see the most recent messages instantly.
/// Full render happens lazily when they scroll to the top.
const DEFERRED_RENDER_TAIL: usize = 200;

/// Scan already-rendered events to extract state flags needed by the render thread.
/// Runs on the main thread reading its own memory (zero allocation, no clone).
/// This lets us send only NEW events to the render thread instead of ALL events.
fn pre_scan_events(events: &[DisplayEvent]) -> PreScanState {
    let mut s = PreScanState::default();
    for event in events {
        match event {
            DisplayEvent::Init { .. } => { s.saw_init = true; }
            DisplayEvent::Hook { name, output } => {
                s.last_hook = Some((name.clone(), output.clone()));
            }
            DisplayEvent::Plan { .. } | DisplayEvent::UserMessage { .. }
            | DisplayEvent::AssistantText { .. } | DisplayEvent::ToolCall { .. }
            | DisplayEvent::ToolResult { .. } => {
                s.saw_content = true;
                s.last_hook = None;
                if let DisplayEvent::ToolCall { tool_name, input, .. } = event {
                    if tool_name == "ExitPlanMode" {
                        s.saw_exit_plan_mode = true;
                        s.saw_user_after_exit_plan = false;
                    }
                    if tool_name == "AskUserQuestion" {
                        s.saw_ask_user_question = true;
                        s.saw_user_after_ask = false;
                        s.last_ask_input = Some(input.clone());
                    }
                }
                if let DisplayEvent::UserMessage { .. } = event {
                    if s.saw_exit_plan_mode { s.saw_user_after_exit_plan = true; }
                    if s.saw_ask_user_question { s.saw_user_after_ask = true; }
                }
            }
            _ => {}
        }
    }
    s
}

/// Submit a render request to the background thread (NON-BLOCKING).
/// The main event loop calls this when `rendered_lines_dirty` is true.
/// The actual rendering happens on the render thread — the main thread
/// keeps processing events immediately after this returns.
pub fn submit_render_request(app: &mut App, convo_width: u16) {
    if app.display_events.is_empty() || app.view_mode != ViewMode::Output { return; }

    let inner_width = convo_width.saturating_sub(2);

    // Deferred render: if user scrolled to top and there are unrendered early events,
    // expand to full render now (they want to see old messages)
    if app.rendered_events_start > 0 && app.output_scroll == 0 {
        app.rendered_lines_dirty = true;
        app.rendered_events_start = 0;
        app.rendered_events_count = 0;
        app.rendered_content_line_count = 0;
    }

    // Only submit if cache is dirty or width changed
    if !app.rendered_lines_dirty && app.rendered_lines_width == inner_width { return; }

    let event_count = app.display_events.len();
    let can_incremental = app.rendered_lines_width == inner_width
        && app.rendered_events_count > 0
        && event_count > app.rendered_events_count
        && app.rendered_events_start == 0;

    // Build the render request with cloned data (the thread works on its own copy).
    // IMPORTANT: we clone (not take) the existing cache so the main thread still has
    // content to display while the render thread works. Taking would empty the cache,
    // breaking scroll-to-bottom (clamp_output_scroll sees 0 lines → jumps to top).
    let req = if can_incremental {
        let existing_lines = app.rendered_lines_cache.clone();
        let existing_anim = app.animation_line_indices.clone();
        let existing_bubbles = app.message_bubble_positions.clone();
        let existing_clickable = app.clickable_paths.clone();

        // Pre-compute state flags by scanning existing events (zero-cost: just
        // reads references in main thread's own memory). This eliminates the need
        // to clone ALL display_events — render thread only gets new events.
        let pre_scan = pre_scan_events(&app.display_events[..app.rendered_events_count]);

        // Only clone NEW events (from rendered_events_count onwards).
        // Previously cloned ALL events — the #1 cause of CPU spike during streaming.
        let new_events = app.display_events[app.rendered_events_count..].to_vec();

        RenderRequest {
            events: new_events,
            width: inner_width,
            pending_tools: app.pending_tool_calls.clone(),
            failed_tools: app.failed_tool_calls.clone(),
            pending_user_message: None,
            existing_lines,
            existing_anim,
            existing_bubbles,
            existing_clickable,
            pre_scan,
            total_events: event_count,
            deferred_start: 0,
            seq: 0,
        }
    } else {
        let deferred_start = if app.rendered_events_start == 0
            && app.rendered_events_count == 0
            && event_count > DEFERRED_RENDER_TAIL
        {
            event_count.saturating_sub(DEFERRED_RENDER_TAIL)
        } else {
            0
        };

        // Clone only the events we'll actually render (from deferred_start onwards).
        // Previously cloned ALL events even though events before deferred_start are
        // never touched — wasted cloning of potentially huge serde_json::Value fields.
        RenderRequest {
            events: app.display_events[deferred_start..].to_vec(),
            width: inner_width,
            pending_tools: app.pending_tool_calls.clone(),
            failed_tools: app.failed_tool_calls.clone(),
            pending_user_message: None,
            existing_lines: Vec::new(),
            existing_anim: Vec::new(),
            existing_bubbles: Vec::new(),
            existing_clickable: Vec::new(),
            pre_scan: PreScanState::default(),
            total_events: event_count,
            deferred_start,
            seq: 0,
        }
    };

    app.render_thread.send(req);
    app.render_in_flight = true;
    // Mark dirty as false so we don't re-submit every loop iteration.
    // If new events arrive before the render completes, invalidate_render_cache()
    // sets dirty=true again and we'll submit a new request.
    app.rendered_lines_dirty = false;
}

/// Check for completed render results from the background thread (NON-BLOCKING).
/// Returns true if new content was applied (caller should trigger a redraw).
pub fn poll_render_result(app: &mut App) -> bool {
    let Some(result) = app.render_thread.try_recv() else { return false; };

    // Discard stale results (a newer request was already applied)
    if result.seq <= app.render_seq_applied { return false; }

    // Apply the completed render to app state
    app.rendered_lines_cache = result.lines;
    app.animation_line_indices = result.anim_indices;
    app.message_bubble_positions = result.bubble_positions;
    app.clickable_paths = result.clickable_paths;
    app.rendered_lines_width = result.width;
    app.rendered_events_count = result.events_count;
    app.rendered_content_line_count = app.rendered_lines_cache.len();
    app.rendered_events_start = result.events_start;
    app.render_seq_applied = result.seq;
    app.render_in_flight = false;
    // Content shifted — stale highlight would point at wrong position
    app.clicked_path_highlight = None;

    // Invalidate viewport cache since underlying content changed
    app.output_viewport_scroll = usize::MAX;
    true
}
