//! Regressions for session navigation across live deferred-render updates.

use super::*;
use ratatui::text::Line;
use serde_json::json;

/// Builds a lightweight tool event for filling a tool-heavy live render tail.
fn tool_call(index: usize) -> DisplayEvent {
    DisplayEvent::ToolCall {
        _uuid: format!("tool-{index}"),
        tool_use_id: format!("call-{index}"),
        tool_name: "exec".to_string(),
        file_path: None,
        input: json!({"command": format!("command {index}")}),
    }
}

/// Waits for the background render thread to apply the newest result.
fn wait_for_render(app: &mut App) {
    for _ in 0..100 {
        if poll_render_result(app) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("background session render did not complete");
}

/// Builds a deferred live-session cache whose recent tail contains no prompt.
fn live_deferred_app() -> App {
    let mut app = App::new();
    app.display_events = vec![
        DisplayEvent::UserMessage {
            _uuid: "user-1".to_string(),
            content: "first prompt".to_string(),
        },
        DisplayEvent::AssistantText {
            _uuid: "assistant-1".to_string(),
            _message_id: "message-1".to_string(),
            text: "first response".to_string(),
        },
        DisplayEvent::UserMessage {
            _uuid: "user-2".to_string(),
            content: "previous prompt".to_string(),
        },
    ];
    app.display_events.extend((0..201).map(tool_call));
    app.display_events.push(DisplayEvent::AssistantText {
        _uuid: "assistant-live".to_string(),
        _message_id: "message-live".to_string(),
        text: "live response".to_string(),
    });
    app.rendered_lines_cache = vec![Line::default(); 100];
    app.session_viewport_height = 20;
    app.rendered_events_start = 3;
    app.rendered_events_count = app.display_events.len();
    app.message_bubble_positions = vec![(50, false)];
    app.session_scroll = usize::MAX;
    app.rendered_lines_dirty = false;

    // Simulate the full-reparse invalidation used during an active Codex turn.
    app.invalidate_render_cache_from_start();
    assert_eq!(app.rendered_events_start, 0);
    assert_eq!(app.rendered_cache_events_start, 3);
    app
}

/// A previous-prompt jump expands a tool-heavy live tail and lands on the
/// prior prompt instead of retaining the expansion trigger at session top.
#[test]
fn previous_prompt_from_live_deferred_tail_lands_on_prior_prompt() {
    let mut app = live_deferred_app();

    app.jump_to_prev_bubble(false);

    assert!(app.pending_session_bubble_jump.is_some());
    assert_ne!(app.session_scroll, 0);

    submit_render_request(&mut app, 80);
    wait_for_render(&mut app);

    let expected = app
        .message_bubble_positions
        .iter()
        .filter(|(_, is_user)| *is_user)
        .nth(1)
        .map(|(line_idx, _)| line_idx.saturating_sub(2))
        .expect("full render should contain the previous user prompt");
    assert_eq!(app.rendered_events_start, 0);
    assert!(app.pending_session_bubble_jump.is_none());
    assert_eq!(app.session_scroll, expected);
    assert_ne!(app.session_scroll, 0);
}

/// Moving to the bottom while expansion is pending cancels the deferred jump.
#[test]
fn bottom_navigation_supersedes_pending_previous_prompt_jump() {
    let mut app = live_deferred_app();
    app.jump_to_prev_bubble(false);

    app.scroll_session_to_bottom();
    submit_render_request(&mut app, 80);
    wait_for_render(&mut app);

    assert!(app.pending_session_bubble_jump.is_none());
    assert_eq!(app.session_scroll, usize::MAX);
}
