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
pub fn submit_render_request(app: &mut App, session_width: u16) {
    if app.display_events.is_empty() || app.view_mode != ViewMode::Session { return; }

    let inner_width = session_width.saturating_sub(2);

    // Deferred render: if user scrolled to top and there are unrendered early events,
    // expand to full render now (they want to see old messages)
    if app.rendered_events_start > 0 && app.session_scroll == 0 {
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
    // breaking scroll-to-bottom (clamp_session_scroll sees 0 lines → jumps to top).
    let req = if can_incremental {
        // Pre-compute state flags by scanning existing events (zero-cost: just
        // reads references in main thread's own memory). This eliminates the need
        // to clone ALL display_events — render thread only gets new events.
        let pre_scan = pre_scan_events(&app.display_events[..app.rendered_events_count]);

        // Only clone NEW events (from rendered_events_count onwards).
        let new_events = app.display_events[app.rendered_events_count..].to_vec();

        // Record existing line count so the main thread can offset new indices.
        // NO CLONE of rendered_lines_cache — the render thread produces only new
        // lines, and the main thread extends its cache on poll_render_result.
        let existing_line_count = app.rendered_lines_cache.len();

        RenderRequest {
            events: new_events,
            width: inner_width,
            pending_tools: app.pending_tool_calls.clone(),
            failed_tools: app.failed_tool_calls.clone(),
            pending_user_message: None,
            existing_line_count,
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
        RenderRequest {
            events: app.display_events[deferred_start..].to_vec(),
            width: inner_width,
            pending_tools: app.pending_tool_calls.clone(),
            failed_tools: app.failed_tool_calls.clone(),
            pending_user_message: None,
            existing_line_count: 0,
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

    if result.incremental {
        // Incremental: render thread produced ONLY new lines with indices
        // relative to 0. Offset by existing cache length and extend.
        let offset = app.rendered_lines_cache.len();

        // Trim stale animation indices (tools that completed since last render)
        app.animation_line_indices.retain(|&(idx, _, _)| idx < offset);

        // Extend cache with new content
        app.rendered_lines_cache.extend(result.lines);

        // Offset and extend indices
        for (idx, col, id) in result.anim_indices {
            app.animation_line_indices.push((idx + offset, col, id));
        }
        for (idx, is_user) in result.bubble_positions {
            app.message_bubble_positions.push((idx + offset, is_user));
        }
        for (line_idx, start_col, end_col, path, old, new, wrap_count) in result.clickable_paths {
            app.clickable_paths.push((line_idx + offset, start_col, end_col, path, old, new, wrap_count));
        }
        for (start, end, raw) in result.clickable_tables {
            app.clickable_tables.push((start + offset, end + offset, raw));
        }
    } else {
        // Full render: replace everything
        app.rendered_lines_cache = result.lines;
        app.animation_line_indices = result.anim_indices;
        app.message_bubble_positions = result.bubble_positions;
        app.clickable_paths = result.clickable_paths;
        app.clickable_tables = result.clickable_tables;
    }

    app.rendered_lines_width = result.width;
    app.rendered_events_count = result.events_count;
    app.rendered_content_line_count = app.rendered_lines_cache.len();
    app.rendered_events_start = result.events_start;
    app.render_seq_applied = result.seq;
    app.render_in_flight = false;
    // Content shifted — stale highlight would point at wrong position
    app.clicked_path_highlight = None;

    // Invalidate viewport cache since underlying content changed
    app.session_viewport_scroll = usize::MAX;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::DisplayEvent;
    use serde_json::json;

    /// Helper: create a ToolCall DisplayEvent with minimal boilerplate
    fn tool_call(name: &str, input: serde_json::Value) -> DisplayEvent {
        DisplayEvent::ToolCall {
            _uuid: "u".into(),
            tool_use_id: "tu".into(),
            tool_name: name.into(),
            file_path: None,
            input,
        }
    }

    /// Helper: create a ToolResult DisplayEvent with minimal boilerplate
    fn tool_result(name: &str, content: &str) -> DisplayEvent {
        DisplayEvent::ToolResult {
            tool_use_id: "tu".into(),
            tool_name: name.into(),
            file_path: None,
            content: content.into(),
        }
    }

    // ── 1. DEFERRED_RENDER_TAIL constant ──

    #[test]
    fn test_deferred_render_tail_is_200() {
        assert_eq!(DEFERRED_RENDER_TAIL, 200);
    }

    // ── 2. pre_scan_events: empty input ──

    #[test]
    fn test_pre_scan_empty_events() {
        let state = pre_scan_events(&[]);
        assert!(!state.saw_init);
        assert!(!state.saw_content);
        assert!(state.last_hook.is_none());
        assert!(!state.saw_exit_plan_mode);
        assert!(!state.saw_user_after_exit_plan);
        assert!(!state.saw_ask_user_question);
        assert!(!state.saw_user_after_ask);
        assert!(state.last_ask_input.is_none());
    }

    // ── 3. pre_scan_events: Init event ──

    #[test]
    fn test_pre_scan_init_sets_saw_init() {
        let events = vec![DisplayEvent::Init {
            _session_id: "s1".into(),
            cwd: "/tmp".into(),
            model: "opus".into(),
        }];
        let state = pre_scan_events(&events);
        assert!(state.saw_init);
    }

    #[test]
    fn test_pre_scan_init_does_not_set_content() {
        let events = vec![DisplayEvent::Init {
            _session_id: "s1".into(),
            cwd: "/tmp".into(),
            model: "opus".into(),
        }];
        let state = pre_scan_events(&events);
        assert!(!state.saw_content);
    }

    // ── 4. pre_scan_events: Hook event ──

    #[test]
    fn test_pre_scan_hook_sets_last_hook() {
        let events = vec![DisplayEvent::Hook {
            name: "PreTool".into(),
            output: "allowed".into(),
        }];
        let state = pre_scan_events(&events);
        assert_eq!(state.last_hook, Some(("PreTool".into(), "allowed".into())));
    }

    #[test]
    fn test_pre_scan_hook_does_not_set_content() {
        let events = vec![DisplayEvent::Hook {
            name: "PreTool".into(),
            output: "ok".into(),
        }];
        let state = pre_scan_events(&events);
        assert!(!state.saw_content);
    }

    // ── 5. pre_scan_events: UserMessage event ──

    #[test]
    fn test_pre_scan_user_message_sets_content() {
        let events = vec![DisplayEvent::UserMessage {
            _uuid: "u1".into(),
            content: "hello".into(),
        }];
        let state = pre_scan_events(&events);
        assert!(state.saw_content);
    }

    #[test]
    fn test_pre_scan_user_message_clears_last_hook() {
        let events = vec![
            DisplayEvent::Hook { name: "hook1".into(), output: "out".into() },
            DisplayEvent::UserMessage { _uuid: "u1".into(), content: "hi".into() },
        ];
        let state = pre_scan_events(&events);
        assert!(state.last_hook.is_none());
    }

    // ── 6. pre_scan_events: AssistantText event ──

    #[test]
    fn test_pre_scan_assistant_text_sets_content() {
        let events = vec![DisplayEvent::AssistantText {
            _uuid: "a1".into(),
            _message_id: "m1".into(),
            text: "response".into(),
        }];
        let state = pre_scan_events(&events);
        assert!(state.saw_content);
    }

    // ── 7. pre_scan_events: Plan event ──

    #[test]
    fn test_pre_scan_plan_sets_content() {
        let events = vec![DisplayEvent::Plan {
            name: "plan1".into(),
            content: "do something".into(),
        }];
        let state = pre_scan_events(&events);
        assert!(state.saw_content);
    }

    // ── 8. pre_scan_events: ToolCall ExitPlanMode ──

    #[test]
    fn test_pre_scan_tool_call_exit_plan_mode() {
        let events = vec![tool_call("ExitPlanMode", json!({}))];
        let state = pre_scan_events(&events);
        assert!(state.saw_exit_plan_mode);
        assert!(!state.saw_user_after_exit_plan);
    }

    #[test]
    fn test_pre_scan_exit_plan_then_user() {
        let events = vec![
            tool_call("ExitPlanMode", json!({})),
            DisplayEvent::UserMessage { _uuid: "u1".into(), content: "go".into() },
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_exit_plan_mode);
        assert!(state.saw_user_after_exit_plan);
    }

    // ── 9. pre_scan_events: ToolCall AskUserQuestion ──

    #[test]
    fn test_pre_scan_ask_user_question() {
        let input = json!({"question": "pick one"});
        let events = vec![tool_call("AskUserQuestion", input.clone())];
        let state = pre_scan_events(&events);
        assert!(state.saw_ask_user_question);
        assert!(!state.saw_user_after_ask);
        assert_eq!(state.last_ask_input, Some(input));
    }

    #[test]
    fn test_pre_scan_ask_user_then_user_message() {
        let events = vec![
            tool_call("AskUserQuestion", json!({})),
            DisplayEvent::UserMessage { _uuid: "u1".into(), content: "yes".into() },
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_ask_user_question);
        assert!(state.saw_user_after_ask);
    }

    // ── 10. pre_scan_events: ToolResult sets content ──

    #[test]
    fn test_pre_scan_tool_result_sets_content() {
        let events = vec![tool_result("Read", "file contents")];
        let state = pre_scan_events(&events);
        assert!(state.saw_content);
    }

    // ── 11. pre_scan_events: multiple hooks, last wins ──

    #[test]
    fn test_pre_scan_multiple_hooks_last_wins() {
        let events = vec![
            DisplayEvent::Hook { name: "h1".into(), output: "o1".into() },
            DisplayEvent::Hook { name: "h2".into(), output: "o2".into() },
        ];
        let state = pre_scan_events(&events);
        assert_eq!(state.last_hook, Some(("h2".into(), "o2".into())));
    }

    // ── 12. pre_scan_events: content after hook clears hook ──

    #[test]
    fn test_pre_scan_content_clears_hook() {
        let events = vec![
            DisplayEvent::Hook { name: "h1".into(), output: "o1".into() },
            DisplayEvent::AssistantText {
                _uuid: "a1".into(),
                _message_id: "m1".into(),
                text: "response".into(),
            },
        ];
        let state = pre_scan_events(&events);
        assert!(state.last_hook.is_none());
    }

    // ── 13. pre_scan_events: non-ExitPlanMode tool call ──

    #[test]
    fn test_pre_scan_non_exit_plan_tool_call() {
        let events = vec![tool_call("Read", json!({"path": "/foo"}))];
        let state = pre_scan_events(&events);
        assert!(!state.saw_exit_plan_mode);
        assert!(!state.saw_ask_user_question);
        assert!(state.saw_content);
    }

    // ── 14. pre_scan_events: Compacting/Compacted/MayBeCompacting fallthrough ──

    #[test]
    fn test_pre_scan_compacting_no_effect() {
        let events = vec![DisplayEvent::Compacting];
        let state = pre_scan_events(&events);
        assert!(!state.saw_init);
        assert!(!state.saw_content);
        assert!(state.last_hook.is_none());
    }

    #[test]
    fn test_pre_scan_compacted_no_effect() {
        let events = vec![DisplayEvent::Compacted];
        let state = pre_scan_events(&events);
        assert!(!state.saw_content);
    }

    #[test]
    fn test_pre_scan_may_be_compacting_no_effect() {
        let events = vec![DisplayEvent::MayBeCompacting];
        let state = pre_scan_events(&events);
        assert!(!state.saw_content);
    }

    // ── 15. pre_scan_events: Command event fallthrough ──

    #[test]
    fn test_pre_scan_command_no_effect() {
        let events = vec![DisplayEvent::Command { name: "/compact".into() }];
        let state = pre_scan_events(&events);
        assert!(!state.saw_content);
    }

    // ── 16. pre_scan_events: multiple ExitPlanMode ──

    #[test]
    fn test_pre_scan_multiple_exit_plan_mode() {
        let events = vec![
            tool_call("ExitPlanMode", json!({})),
            tool_call("ExitPlanMode", json!({})),
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_exit_plan_mode);
        assert!(!state.saw_user_after_exit_plan);
    }

    // ── 17. pre_scan_events: user after ask, then another ask resets ──

    #[test]
    fn test_pre_scan_ask_then_user_then_ask_resets_user_after() {
        let events = vec![
            tool_call("AskUserQuestion", json!({"q": 1})),
            DisplayEvent::UserMessage { _uuid: "u1".into(), content: "yes".into() },
            tool_call("AskUserQuestion", json!({"q": 2})),
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_ask_user_question);
        assert!(!state.saw_user_after_ask);
        assert_eq!(state.last_ask_input, Some(json!({"q": 2})));
    }

    // ── 18. submit_render_request: empty display_events is a no-op ──

    #[test]
    fn test_submit_empty_events_noop() {
        let mut app = App::new();
        app.display_events.clear();
        app.rendered_lines_dirty = true;
        submit_render_request(&mut app, 80);
        assert!(app.rendered_lines_dirty);
    }

    // ── 19. submit_render_request: not dirty and same width is no-op ──

    #[test]
    fn test_submit_not_dirty_same_width_noop() {
        let mut app = App::new();
        app.display_events.push(DisplayEvent::UserMessage {
            _uuid: "u1".into(),
            content: "hi".into(),
        });
        app.rendered_lines_dirty = false;
        app.rendered_lines_width = 78; // 80 - 2
        submit_render_request(&mut app, 80);
        assert!(!app.rendered_lines_dirty);
    }

    // ── 20. poll_render_result: no result returns false ──

    #[test]
    fn test_poll_no_result_returns_false() {
        let mut app = App::new();
        let result = poll_render_result(&mut app);
        assert!(!result);
    }

    // ── 21. pre_scan default state ──

    #[test]
    fn test_pre_scan_default() {
        let state = PreScanState::default();
        assert!(!state.saw_init);
        assert!(!state.saw_content);
        assert!(state.last_hook.is_none());
        assert!(!state.saw_exit_plan_mode);
        assert!(!state.saw_user_after_exit_plan);
        assert!(!state.saw_ask_user_question);
        assert!(!state.saw_user_after_ask);
        assert!(state.last_ask_input.is_none());
    }

    // ── 22. pre_scan with Init then Hook then Content ──

    #[test]
    fn test_pre_scan_init_hook_content_sequence() {
        let events = vec![
            DisplayEvent::Init { _session_id: "s".into(), cwd: "/".into(), model: "m".into() },
            DisplayEvent::Hook { name: "h".into(), output: "o".into() },
            DisplayEvent::AssistantText { _uuid: "a".into(), _message_id: "m".into(), text: "t".into() },
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_init);
        assert!(state.saw_content);
        assert!(state.last_hook.is_none());
    }

    // ── 23. pre_scan: exit_plan then user then another user ──

    #[test]
    fn test_pre_scan_exit_plan_then_two_users() {
        let events = vec![
            tool_call("ExitPlanMode", json!({})),
            DisplayEvent::UserMessage { _uuid: "u1".into(), content: "go".into() },
            DisplayEvent::UserMessage { _uuid: "u2".into(), content: "again".into() },
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_user_after_exit_plan);
    }

    // ── 24. pre_scan: last_ask_input tracks latest ──

    #[test]
    fn test_pre_scan_last_ask_input_tracks_latest() {
        let events = vec![
            tool_call("AskUserQuestion", json!({"first": true})),
            tool_call("AskUserQuestion", json!({"second": true})),
        ];
        let state = pre_scan_events(&events);
        assert_eq!(state.last_ask_input, Some(json!({"second": true})));
    }

    // ── 25. pre_scan: saw_init persists across events ──

    #[test]
    fn test_pre_scan_init_persists() {
        let events = vec![
            DisplayEvent::Init { _session_id: "s".into(), cwd: "/".into(), model: "m".into() },
            DisplayEvent::UserMessage { _uuid: "u".into(), content: "x".into() },
            DisplayEvent::Hook { name: "h".into(), output: "o".into() },
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_init);
    }

    // ── 26. pre_scan: hook after content sets last_hook ──

    #[test]
    fn test_pre_scan_hook_after_content() {
        let events = vec![
            DisplayEvent::UserMessage { _uuid: "u".into(), content: "x".into() },
            DisplayEvent::Hook { name: "after".into(), output: "hook".into() },
        ];
        let state = pre_scan_events(&events);
        assert_eq!(state.last_hook, Some(("after".into(), "hook".into())));
        assert!(state.saw_content);
    }

    // ── 27. submit_render_request: dirty flag cleared after submit ──

    #[test]
    fn test_submit_clears_dirty_flag() {
        let mut app = App::new();
        app.display_events.push(DisplayEvent::UserMessage {
            _uuid: "u1".into(),
            content: "hi".into(),
        });
        app.rendered_lines_dirty = true;
        submit_render_request(&mut app, 80);
        assert!(!app.rendered_lines_dirty);
    }

    // ── 28. submit_render_request: sets render_in_flight ──

    #[test]
    fn test_submit_sets_render_in_flight() {
        let mut app = App::new();
        app.display_events.push(DisplayEvent::UserMessage {
            _uuid: "u1".into(),
            content: "hi".into(),
        });
        app.rendered_lines_dirty = true;
        submit_render_request(&mut app, 80);
        assert!(app.render_in_flight);
    }

    // ── 29. pre_scan: ToolCall sets saw_content ──

    #[test]
    fn test_pre_scan_tool_call_sets_content() {
        let events = vec![tool_call("Write", json!({}))];
        let state = pre_scan_events(&events);
        assert!(state.saw_content);
    }

    // ── 30. pre_scan: single user message ──

    #[test]
    fn test_pre_scan_single_user_saw_content_true() {
        let events = vec![DisplayEvent::UserMessage {
            _uuid: "u".into(),
            content: "test".into(),
        }];
        let state = pre_scan_events(&events);
        assert!(state.saw_content);
        assert!(!state.saw_init);
    }

    // ── 31. pre_scan: Filtered event is no-op ──

    #[test]
    fn test_pre_scan_filtered_event_no_effect() {
        let events = vec![DisplayEvent::Filtered];
        let state = pre_scan_events(&events);
        assert!(!state.saw_init);
        assert!(!state.saw_content);
    }

    // ── 32. pre_scan: Complete event is no-op ──

    #[test]
    fn test_pre_scan_complete_event_no_effect() {
        let events = vec![DisplayEvent::Complete {
            _session_id: "s".into(),
            success: true,
            duration_ms: 1000,
            cost_usd: 0.05,
        }];
        let state = pre_scan_events(&events);
        assert!(!state.saw_content);
    }

    // ── 33. submit_render_request: deferred start on large event count ──

    #[test]
    fn test_submit_large_events_triggers_deferred() {
        let mut app = App::new();
        // Populate 300 events (> DEFERRED_RENDER_TAIL=200)
        for i in 0..300 {
            app.display_events.push(DisplayEvent::UserMessage {
                _uuid: format!("u{}", i),
                content: format!("msg {}", i),
            });
        }
        app.rendered_lines_dirty = true;
        app.rendered_events_count = 0;
        app.rendered_events_start = 0;
        submit_render_request(&mut app, 80);
        assert!(!app.rendered_lines_dirty);
        assert!(app.render_in_flight);
    }

    // ── 34. submit_render_request: width=0 saturating sub ──

    #[test]
    fn test_submit_zero_width_no_panic() {
        let mut app = App::new();
        app.display_events.push(DisplayEvent::UserMessage {
            _uuid: "u".into(),
            content: "hi".into(),
        });
        app.rendered_lines_dirty = true;
        submit_render_request(&mut app, 0);
        assert!(!app.rendered_lines_dirty);
    }

    // ── 35. submit_render_request: width=1 saturating sub ──

    #[test]
    fn test_submit_width_1_no_panic() {
        let mut app = App::new();
        app.display_events.push(DisplayEvent::UserMessage {
            _uuid: "u".into(),
            content: "hi".into(),
        });
        app.rendered_lines_dirty = true;
        submit_render_request(&mut app, 1);
        assert!(!app.rendered_lines_dirty);
    }

    // ── 36. pre_scan: exit_plan without user message after ──

    #[test]
    fn test_pre_scan_exit_plan_no_user_after() {
        let events = vec![
            tool_call("ExitPlanMode", json!({})),
            DisplayEvent::AssistantText {
                _uuid: "a".into(),
                _message_id: "m".into(),
                text: "done".into(),
            },
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_exit_plan_mode);
        assert!(!state.saw_user_after_exit_plan);
    }

    // ── 37. pre_scan: ask_user without user after ──

    #[test]
    fn test_pre_scan_ask_user_no_user_after() {
        let events = vec![
            tool_call("AskUserQuestion", json!({"q": "test"})),
            DisplayEvent::AssistantText {
                _uuid: "a".into(),
                _message_id: "m".into(),
                text: "waiting".into(),
            },
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_ask_user_question);
        assert!(!state.saw_user_after_ask);
    }

    // ── 38. pre_scan: ToolResult clears last_hook ──

    #[test]
    fn test_pre_scan_tool_result_clears_hook() {
        let events = vec![
            DisplayEvent::Hook { name: "h".into(), output: "o".into() },
            tool_result("Read", "contents"),
        ];
        let state = pre_scan_events(&events);
        assert!(state.last_hook.is_none());
    }

    // ── 39. pre_scan: many events stress test ──

    #[test]
    fn test_pre_scan_large_event_list() {
        let mut events = Vec::new();
        for i in 0..500 {
            events.push(DisplayEvent::UserMessage {
                _uuid: format!("u{}", i),
                content: format!("msg{}", i),
            });
        }
        let state = pre_scan_events(&events);
        assert!(state.saw_content);
    }

    // ── 40. pre_scan: interleaved hooks and content ──

    #[test]
    fn test_pre_scan_interleaved_hooks_content() {
        let events = vec![
            DisplayEvent::Hook { name: "h1".into(), output: "o1".into() },
            DisplayEvent::UserMessage { _uuid: "u1".into(), content: "a".into() },
            DisplayEvent::Hook { name: "h2".into(), output: "o2".into() },
            DisplayEvent::UserMessage { _uuid: "u2".into(), content: "b".into() },
            DisplayEvent::Hook { name: "h3".into(), output: "o3".into() },
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_content);
        // Last event is a hook, so last_hook is set
        assert_eq!(state.last_hook, Some(("h3".into(), "o3".into())));
    }

    // ── 41. poll_render_result: called twice returns false both times ──

    #[test]
    fn test_poll_twice_returns_false() {
        let mut app = App::new();
        assert!(!poll_render_result(&mut app));
        assert!(!poll_render_result(&mut app));
    }

    // ── 42. submit with session_scroll at 0 triggers deferred expansion ──

    #[test]
    fn test_submit_scroll_zero_triggers_expansion() {
        let mut app = App::new();
        for i in 0..10 {
            app.display_events.push(DisplayEvent::UserMessage {
                _uuid: format!("u{}", i),
                content: format!("msg{}", i),
            });
        }
        app.rendered_events_start = 5;
        app.session_scroll = 0;
        app.rendered_lines_dirty = true;
        submit_render_request(&mut app, 80);
        // After expansion, rendered_events_start should be reset to 0
        assert_eq!(app.rendered_events_start, 0);
    }

    // ── 43. pre_scan: exit_plan then user then exit_plan resets user flag ──

    #[test]
    fn test_pre_scan_exit_plan_user_exit_plan_resets() {
        let events = vec![
            tool_call("ExitPlanMode", json!({})),
            DisplayEvent::UserMessage { _uuid: "u".into(), content: "x".into() },
            tool_call("ExitPlanMode", json!({})),
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_exit_plan_mode);
        // Second ExitPlanMode resets the user-after flag
        assert!(!state.saw_user_after_exit_plan);
    }

    // ── 44. pre_scan: Plan clears last_hook ──

    #[test]
    fn test_pre_scan_plan_clears_hook() {
        let events = vec![
            DisplayEvent::Hook { name: "h".into(), output: "o".into() },
            DisplayEvent::Plan { name: "p".into(), content: "c".into() },
        ];
        let state = pre_scan_events(&events);
        assert!(state.last_hook.is_none());
    }

    // ── 45. pre_scan: ToolCall with file_path ──

    #[test]
    fn test_pre_scan_tool_call_with_file_path() {
        let events = vec![DisplayEvent::ToolCall {
            _uuid: "u".into(),
            tool_use_id: "tu".into(),
            tool_name: "Read".into(),
            file_path: Some("/src/main.rs".into()),
            input: json!({"path": "/src/main.rs"}),
        }];
        let state = pre_scan_events(&events);
        assert!(state.saw_content);
    }

    // ── 46. pre_scan: ToolResult with file_path ──

    #[test]
    fn test_pre_scan_tool_result_with_file_path() {
        let events = vec![DisplayEvent::ToolResult {
            tool_use_id: "tu".into(),
            tool_name: "Read".into(),
            file_path: Some("/src/main.rs".into()),
            content: "fn main() {}".into(),
        }];
        let state = pre_scan_events(&events);
        assert!(state.saw_content);
    }

    // ── 47. pre_scan: all saw_* flags can be true simultaneously ──

    #[test]
    fn test_pre_scan_all_flags_true() {
        let events = vec![
            DisplayEvent::Init { _session_id: "s".into(), cwd: "/".into(), model: "m".into() },
            tool_call("ExitPlanMode", json!({})),
            tool_call("AskUserQuestion", json!({"q": "test"})),
            DisplayEvent::UserMessage { _uuid: "u".into(), content: "yes".into() },
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_init);
        assert!(state.saw_content);
        assert!(state.saw_exit_plan_mode);
        assert!(state.saw_user_after_exit_plan);
        assert!(state.saw_ask_user_question);
        assert!(state.saw_user_after_ask);
    }

    // ── 48. pre_scan: only hooks, no content ──

    #[test]
    fn test_pre_scan_only_hooks_no_content() {
        let events = vec![
            DisplayEvent::Hook { name: "a".into(), output: "1".into() },
            DisplayEvent::Hook { name: "b".into(), output: "2".into() },
            DisplayEvent::Hook { name: "c".into(), output: "3".into() },
        ];
        let state = pre_scan_events(&events);
        assert!(!state.saw_content);
        assert_eq!(state.last_hook, Some(("c".into(), "3".into())));
    }

    // ── 49. submit: incremental path when rendered_events_count > 0 ──

    #[test]
    fn test_submit_incremental_path() {
        let mut app = App::new();
        for i in 0..5 {
            app.display_events.push(DisplayEvent::UserMessage {
                _uuid: format!("u{}", i),
                content: format!("msg{}", i),
            });
        }
        app.rendered_events_count = 3; // 3 already rendered
        app.rendered_events_start = 0;
        app.rendered_lines_width = 78; // same width as 80-2
        app.rendered_lines_dirty = true;
        submit_render_request(&mut app, 80);
        assert!(!app.rendered_lines_dirty);
        assert!(app.render_in_flight);
    }

    // ── 50. submit: rendered_events_start > 0 and scroll == 0 expansion ──

    #[test]
    fn test_submit_deferred_expansion_resets_counts() {
        let mut app = App::new();
        for i in 0..10 {
            app.display_events.push(DisplayEvent::UserMessage {
                _uuid: format!("u{}", i),
                content: format!("m{}", i),
            });
        }
        app.rendered_events_start = 3;
        app.rendered_events_count = 7;
        app.rendered_content_line_count = 14;
        app.session_scroll = 0;
        app.rendered_lines_dirty = false;
        app.rendered_lines_width = 78;
        // Trigger expansion by setting scroll=0 and rendered_events_start > 0
        submit_render_request(&mut app, 80);
        // After expansion: start=0, count reset to 0, dirty set true then cleared
        assert_eq!(app.rendered_events_start, 0);
    }
}
