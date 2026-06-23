//! Background render thread coordination
//!
//! Handles submitting render requests and polling for completed results.
//! The main event loop calls `submit_render_request()` when the cache is dirty
//! and `poll_render_result()` each frame to apply completed renders.
//! All expensive work (markdown, syntax highlighting, wrapping) happens on
//! the background thread — these functions just shuttle data back and forth.

use super::super::render_thread::{PreScanState, RenderRequest};
use crate::app::{App, ViewMode};
use crate::events::DisplayEvent;

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
            DisplayEvent::Init { model, .. } => {
                s.saw_init = true;
                s.current_model = Some(model.clone());
            }
            DisplayEvent::Hook { name, output } => {
                s.last_hook = Some((name.clone(), output.clone()));
            }
            DisplayEvent::Plan { .. }
            | DisplayEvent::UserMessage { .. }
            | DisplayEvent::AssistantText { .. }
            | DisplayEvent::ToolCall { .. }
            | DisplayEvent::ToolResult { .. } => {
                s.saw_content = true;
                s.last_hook = None;
                if let DisplayEvent::ToolCall {
                    tool_name, input, ..
                } = event
                {
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
                    if s.saw_exit_plan_mode {
                        s.saw_user_after_exit_plan = true;
                    }
                    if s.saw_ask_user_question {
                        s.saw_user_after_ask = true;
                    }
                }
            }
            DisplayEvent::ModelSwitch { model } => {
                s.current_model = Some(model.clone());
            }
            _ => {}
        }
    }
    s
}

/// Scan a rendered prefix and fall back to the session model when no event
/// supplied model state for assistant bubble identity.
fn pre_scan_events_with_fallback(events: &[DisplayEvent], fallback_model: &str) -> PreScanState {
    let mut state = pre_scan_events(events);
    if state
        .current_model
        .as_deref()
        .filter(|model| !model.is_empty())
        .is_none()
        && !fallback_model.is_empty()
    {
        state.current_model = Some(fallback_model.to_string());
    }
    state
}

/// Submit a render request to the background thread (NON-BLOCKING).
/// The main event loop calls this when `rendered_lines_dirty` is true.
/// The actual rendering happens on the render thread — the main thread
/// keeps processing events immediately after this returns.
pub fn submit_render_request(app: &mut App, session_width: u16) {
    if app.display_events.is_empty() || app.view_mode != ViewMode::Session {
        return;
    }

    let inner_width = session_width.saturating_sub(2);
    let fallback_model = app.display_model_name().to_string();

    // Deferred render: if user scrolled to top and there are unrendered early events,
    // expand to full render now (they want to see old messages).
    // Track this with a flag so the deferred_start calculation below does NOT
    // re-trigger deferred rendering (which would create an infinite loop where
    // expansion resets counters → deferred check re-fires → expansion again).
    let expanding_deferred = app.rendered_events_start > 0 && app.session_scroll == 0;
    if expanding_deferred {
        app.rendered_lines_dirty = true;
        app.rendered_events_start = 0;
        app.rendered_events_count = 0;
        app.rendered_content_line_count = 0;
    }

    // Only submit if cache is dirty or width changed
    if !app.rendered_lines_dirty && app.rendered_lines_width == inner_width {
        return;
    }

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
        let pre_scan = pre_scan_events_with_fallback(
            &app.display_events[..app.rendered_events_count],
            &fallback_model,
        );

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
            show_edit_previews: false,
            existing_line_count,
            pre_scan,
            total_events: event_count,
            deferred_start: 0,
            seq: 0,
        }
    } else {
        // Only defer on INITIAL load (fresh session, never rendered).
        // When expanding (user scrolled to top), force deferred_start=0 for full render.
        let deferred_start = if !expanding_deferred
            && app.rendered_events_start == 0
            && app.rendered_events_count == 0
            && event_count > DEFERRED_RENDER_TAIL
        {
            event_count.saturating_sub(DEFERRED_RENDER_TAIL)
        } else {
            0
        };
        let pre_scan = if deferred_start > 0 {
            pre_scan_events_with_fallback(&app.display_events[..deferred_start], &fallback_model)
        } else {
            pre_scan_events_with_fallback(&[], &fallback_model)
        };

        // Clone only the events we'll actually render (from deferred_start onwards).
        RenderRequest {
            events: app.display_events[deferred_start..].to_vec(),
            width: inner_width,
            pending_tools: app.pending_tool_calls.clone(),
            failed_tools: app.failed_tool_calls.clone(),
            pending_user_message: None,
            show_edit_previews: false,
            existing_line_count: 0,
            pre_scan,
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
    let Some(result) = app.render_thread.try_recv() else {
        return false;
    };

    // Discard stale results (a newer request has already been submitted).
    // This can happen during streaming: the event loop queues a newer render
    // while the worker is still rendering an older snapshot. Applying the older
    // incremental result would extend the cache with bubbles from stale events.
    if result.seq < app.render_thread.current_seq() {
        return false;
    }

    // A dirty cache means display_events changed after this snapshot was
    // submitted. The existing cache is less wrong than applying a render that
    // does not include the newest user prompt or assistant chunk.
    if app.rendered_lines_dirty {
        return false;
    }

    // Discard stale results (a newer request was already applied or a display
    // event replacement advanced the applied watermark to cancel in-flight work).
    if result.seq <= app.render_seq_applied {
        return false;
    }

    if result.incremental {
        // Incremental: render thread produced ONLY new lines with indices
        // relative to 0. Offset by existing cache length and extend.
        let offset = app.rendered_lines_cache.len();

        // Trim stale animation indices (tools that completed since last render)
        app.animation_line_indices
            .retain(|&(idx, _, _)| idx < offset);

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
            app.clickable_paths.push((
                line_idx + offset,
                start_col,
                end_col,
                path,
                old,
                new,
                wrap_count,
            ));
        }
        for (start, end, raw) in result.clickable_tables {
            app.clickable_tables
                .push((start + offset, end + offset, raw));
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
/// Tests for render request submission and render result polling.
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
            is_error: false,
        }
    }

    // ── 1. DEFERRED_RENDER_TAIL constant ──

    /// Verifies deferred render tail is 200.
    #[test]
    fn test_deferred_render_tail_is_200() {
        assert_eq!(DEFERRED_RENDER_TAIL, 200);
    }

    // ── 2. pre_scan_events: empty input ──

    /// Verifies pre-scan empty events.
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

    /// Verifies pre-scan init sets saw init.
    #[test]
    fn test_pre_scan_init_sets_saw_init() {
        let events = vec![DisplayEvent::Init {
            _session_id: "s1".into(),
            cwd: "/tmp".into(),
            model: "opus".into(),
        }];
        let state = pre_scan_events(&events);
        assert!(state.saw_init);
        assert_eq!(state.current_model.as_deref(), Some("opus"));
    }

    /// Verifies pre-scan init does not set content.
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

    /// Verifies pre-scan last init model wins.
    #[test]
    fn test_pre_scan_last_init_model_wins() {
        let events = vec![
            DisplayEvent::Init {
                _session_id: "s1".into(),
                cwd: "/tmp".into(),
                model: "claude-opus-4-6".into(),
            },
            DisplayEvent::Init {
                _session_id: "s2".into(),
                cwd: "/tmp".into(),
                model: "gpt-5.4".into(),
            },
        ];
        let state = pre_scan_events(&events);
        assert_eq!(state.current_model.as_deref(), Some("gpt-5.4"));
    }

    // ── 4. pre_scan_events: Hook event ──

    /// Verifies pre-scan hook sets last hook.
    #[test]
    fn test_pre_scan_hook_sets_last_hook() {
        let events = vec![DisplayEvent::Hook {
            name: "PreTool".into(),
            output: "allowed".into(),
        }];
        let state = pre_scan_events(&events);
        assert_eq!(state.last_hook, Some(("PreTool".into(), "allowed".into())));
    }

    /// Verifies pre-scan hook does not set content.
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

    /// Verifies pre-scan user message sets content.
    #[test]
    fn test_pre_scan_user_message_sets_content() {
        let events = vec![DisplayEvent::UserMessage {
            _uuid: "u1".into(),
            content: "hello".into(),
        }];
        let state = pre_scan_events(&events);
        assert!(state.saw_content);
    }

    /// Verifies pre-scan user message clears last hook.
    #[test]
    fn test_pre_scan_user_message_clears_last_hook() {
        let events = vec![
            DisplayEvent::Hook {
                name: "hook1".into(),
                output: "out".into(),
            },
            DisplayEvent::UserMessage {
                _uuid: "u1".into(),
                content: "hi".into(),
            },
        ];
        let state = pre_scan_events(&events);
        assert!(state.last_hook.is_none());
    }

    // ── 6. pre_scan_events: AssistantText event ──

    /// Verifies pre-scan assistant text sets content.
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

    /// Verifies pre-scan plan sets content.
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

    /// Verifies pre-scan tool call exit plan mode.
    #[test]
    fn test_pre_scan_tool_call_exit_plan_mode() {
        let events = vec![tool_call("ExitPlanMode", json!({}))];
        let state = pre_scan_events(&events);
        assert!(state.saw_exit_plan_mode);
        assert!(!state.saw_user_after_exit_plan);
    }

    /// Verifies pre-scan exit plan then user.
    #[test]
    fn test_pre_scan_exit_plan_then_user() {
        let events = vec![
            tool_call("ExitPlanMode", json!({})),
            DisplayEvent::UserMessage {
                _uuid: "u1".into(),
                content: "go".into(),
            },
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_exit_plan_mode);
        assert!(state.saw_user_after_exit_plan);
    }

    // ── 9. pre_scan_events: ToolCall AskUserQuestion ──

    /// Verifies pre-scan ask user question.
    #[test]
    fn test_pre_scan_ask_user_question() {
        let input = json!({"question": "pick one"});
        let events = vec![tool_call("AskUserQuestion", input.clone())];
        let state = pre_scan_events(&events);
        assert!(state.saw_ask_user_question);
        assert!(!state.saw_user_after_ask);
        assert_eq!(state.last_ask_input, Some(input));
    }

    /// Verifies pre-scan ask user then user message.
    #[test]
    fn test_pre_scan_ask_user_then_user_message() {
        let events = vec![
            tool_call("AskUserQuestion", json!({})),
            DisplayEvent::UserMessage {
                _uuid: "u1".into(),
                content: "yes".into(),
            },
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_ask_user_question);
        assert!(state.saw_user_after_ask);
    }

    // ── 10. pre_scan_events: ToolResult sets content ──

    /// Verifies pre-scan tool result sets content.
    #[test]
    fn test_pre_scan_tool_result_sets_content() {
        let events = vec![tool_result("Read", "file contents")];
        let state = pre_scan_events(&events);
        assert!(state.saw_content);
    }

    // ── 11. pre_scan_events: multiple hooks, last wins ──

    /// Verifies pre-scan multiple hooks last wins.
    #[test]
    fn test_pre_scan_multiple_hooks_last_wins() {
        let events = vec![
            DisplayEvent::Hook {
                name: "h1".into(),
                output: "o1".into(),
            },
            DisplayEvent::Hook {
                name: "h2".into(),
                output: "o2".into(),
            },
        ];
        let state = pre_scan_events(&events);
        assert_eq!(state.last_hook, Some(("h2".into(), "o2".into())));
    }

    // ── 12. pre_scan_events: content after hook clears hook ──

    /// Verifies pre-scan content clears hook.
    #[test]
    fn test_pre_scan_content_clears_hook() {
        let events = vec![
            DisplayEvent::Hook {
                name: "h1".into(),
                output: "o1".into(),
            },
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

    /// Verifies pre-scan non exit plan tool call.
    #[test]
    fn test_pre_scan_non_exit_plan_tool_call() {
        let events = vec![tool_call("Read", json!({"path": "/foo"}))];
        let state = pre_scan_events(&events);
        assert!(!state.saw_exit_plan_mode);
        assert!(!state.saw_ask_user_question);
        assert!(state.saw_content);
    }

    // ── 14. pre_scan_events: Compacting/Compacted/MayBeCompacting fallthrough ──

    /// Verifies pre-scan compacting no effect.
    #[test]
    fn test_pre_scan_compacting_no_effect() {
        let events = vec![DisplayEvent::Compacting];
        let state = pre_scan_events(&events);
        assert!(!state.saw_init);
        assert!(!state.saw_content);
        assert!(state.last_hook.is_none());
    }

    /// Verifies pre-scan compacted no effect.
    #[test]
    fn test_pre_scan_compacted_no_effect() {
        let events = vec![DisplayEvent::Compacted];
        let state = pre_scan_events(&events);
        assert!(!state.saw_content);
    }

    /// Verifies pre-scan may be compacting no effect.
    #[test]
    fn test_pre_scan_may_be_compacting_no_effect() {
        let events = vec![DisplayEvent::MayBeCompacting];
        let state = pre_scan_events(&events);
        assert!(!state.saw_content);
    }

    // ── 15. pre_scan_events: Command event fallthrough ──

    /// Verifies pre-scan command no effect.
    #[test]
    fn test_pre_scan_command_no_effect() {
        let events = vec![DisplayEvent::Command {
            name: "/compact".into(),
        }];
        let state = pre_scan_events(&events);
        assert!(!state.saw_content);
    }

    // ── 16. pre_scan_events: multiple ExitPlanMode ──

    /// Verifies pre-scan multiple exit plan mode.
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

    /// Verifies pre-scan ask then user then ask resets user after.
    #[test]
    fn test_pre_scan_ask_then_user_then_ask_resets_user_after() {
        let events = vec![
            tool_call("AskUserQuestion", json!({"q": 1})),
            DisplayEvent::UserMessage {
                _uuid: "u1".into(),
                content: "yes".into(),
            },
            tool_call("AskUserQuestion", json!({"q": 2})),
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_ask_user_question);
        assert!(!state.saw_user_after_ask);
        assert_eq!(state.last_ask_input, Some(json!({"q": 2})));
    }

    // ── 18. submit_render_request: empty display_events is a no-op ──

    /// Verifies submit empty events noop.
    #[test]
    fn test_submit_empty_events_noop() {
        let mut app = App::new();
        app.display_events.clear();
        app.rendered_lines_dirty = true;
        submit_render_request(&mut app, 80);
        assert!(app.rendered_lines_dirty);
    }

    // ── 19. submit_render_request: not dirty and same width is no-op ──

    /// Verifies submit not dirty same width noop.
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

    /// Verifies poll no result returns false.
    #[test]
    fn test_poll_no_result_returns_false() {
        let mut app = App::new();
        let result = poll_render_result(&mut app);
        assert!(!result);
    }

    // ── 21. pre_scan default state ──

    /// Verifies pre-scan default.
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

    /// Verifies pre-scan init hook content sequence.
    #[test]
    fn test_pre_scan_init_hook_content_sequence() {
        let events = vec![
            DisplayEvent::Init {
                _session_id: "s".into(),
                cwd: "/".into(),
                model: "m".into(),
            },
            DisplayEvent::Hook {
                name: "h".into(),
                output: "o".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: "a".into(),
                _message_id: "m".into(),
                text: "t".into(),
            },
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_init);
        assert!(state.saw_content);
        assert!(state.last_hook.is_none());
    }

    // ── 23. pre_scan: exit_plan then user then another user ──

    /// Verifies pre-scan exit plan then two users.
    #[test]
    fn test_pre_scan_exit_plan_then_two_users() {
        let events = vec![
            tool_call("ExitPlanMode", json!({})),
            DisplayEvent::UserMessage {
                _uuid: "u1".into(),
                content: "go".into(),
            },
            DisplayEvent::UserMessage {
                _uuid: "u2".into(),
                content: "again".into(),
            },
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_user_after_exit_plan);
    }

    // ── 24. pre_scan: last_ask_input tracks latest ──

    /// Verifies pre-scan last ask input tracks latest.
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

    /// Verifies pre-scan init persists.
    #[test]
    fn test_pre_scan_init_persists() {
        let events = vec![
            DisplayEvent::Init {
                _session_id: "s".into(),
                cwd: "/".into(),
                model: "m".into(),
            },
            DisplayEvent::UserMessage {
                _uuid: "u".into(),
                content: "x".into(),
            },
            DisplayEvent::Hook {
                name: "h".into(),
                output: "o".into(),
            },
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_init);
    }

    // ── 26. pre_scan: hook after content sets last_hook ──

    /// Verifies pre-scan hook after content.
    #[test]
    fn test_pre_scan_hook_after_content() {
        let events = vec![
            DisplayEvent::UserMessage {
                _uuid: "u".into(),
                content: "x".into(),
            },
            DisplayEvent::Hook {
                name: "after".into(),
                output: "hook".into(),
            },
        ];
        let state = pre_scan_events(&events);
        assert_eq!(state.last_hook, Some(("after".into(), "hook".into())));
        assert!(state.saw_content);
    }

    // ── 27. submit_render_request: dirty flag cleared after submit ──

    /// Verifies submit clears dirty flag.
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

    /// Verifies submit sets render in flight.
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

    /// Verifies pre-scan tool call sets content.
    #[test]
    fn test_pre_scan_tool_call_sets_content() {
        let events = vec![tool_call("Write", json!({}))];
        let state = pre_scan_events(&events);
        assert!(state.saw_content);
    }

    // ── 30. pre_scan: single user message ──

    /// Verifies pre-scan single user saw content true.
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

    /// Verifies pre-scan filtered event no effect.
    #[test]
    fn test_pre_scan_filtered_event_no_effect() {
        let events = vec![DisplayEvent::Filtered];
        let state = pre_scan_events(&events);
        assert!(!state.saw_init);
        assert!(!state.saw_content);
    }

    // ── 32. pre_scan: Complete event is no-op ──

    /// Verifies pre-scan complete event no effect.
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

    /// Verifies submit large events triggers deferred.
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

    /// Verifies submit deferred codex tail preserves model identity.
    #[test]
    fn test_submit_deferred_codex_tail_preserves_model_identity() {
        use ratatui::style::Color;

        let mut app = App::new();
        app.display_events.push(DisplayEvent::Init {
            _session_id: "s1".into(),
            cwd: "/project".into(),
            model: "gpt-5.4".into(),
        });
        for i in 0..=DEFERRED_RENDER_TAIL {
            app.display_events.push(DisplayEvent::AssistantText {
                _uuid: format!("a{}", i),
                _message_id: format!("m{}", i),
                text: format!("tail message {}", i),
            });
        }
        app.rendered_lines_dirty = true;

        submit_render_request(&mut app, 80);

        let mut applied = false;
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if poll_render_result(&mut app) {
                applied = true;
                break;
            }
        }

        assert!(applied, "expected deferred tail render result");
        assert_eq!(app.rendered_events_start, 2);

        let codex_header = app.rendered_lines_cache.iter().find_map(|line| {
            line.spans
                .iter()
                .find(|span| span.content.contains("Codex"))
                .map(|span| span.style.bg)
        });
        assert_eq!(codex_header, Some(Some(Color::Cyan)));
    }

    /// Verifies submit codex model fallback labels assistant only slice.
    #[test]
    fn test_submit_codex_model_fallback_labels_assistant_only_slice() {
        use ratatui::style::Color;

        let mut app = App::new();
        app.selected_model = Some("gpt-5.4".into());
        app.display_events.push(DisplayEvent::AssistantText {
            _uuid: "a1".into(),
            _message_id: "m1".into(),
            text: "assistant-only slice".into(),
        });
        app.rendered_lines_dirty = true;

        submit_render_request(&mut app, 80);

        let mut applied = false;
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if poll_render_result(&mut app) {
                applied = true;
                break;
            }
        }

        assert!(applied, "expected render result");
        let codex_header = app.rendered_lines_cache.iter().find_map(|line| {
            line.spans
                .iter()
                .find(|span| span.content.contains("Codex"))
                .map(|span| span.style.bg)
        });
        assert_eq!(codex_header, Some(Some(Color::Cyan)));
    }

    // ── 34. submit_render_request: width=0 saturating sub ──

    /// Verifies submit zero width no panic.
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

    /// Verifies submit width 1 no panic.
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

    /// Verifies pre-scan exit plan no user after.
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

    /// Verifies pre-scan ask user no user after.
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

    /// Verifies pre-scan tool result clears hook.
    #[test]
    fn test_pre_scan_tool_result_clears_hook() {
        let events = vec![
            DisplayEvent::Hook {
                name: "h".into(),
                output: "o".into(),
            },
            tool_result("Read", "contents"),
        ];
        let state = pre_scan_events(&events);
        assert!(state.last_hook.is_none());
    }

    // ── 39. pre_scan: many events stress test ──

    /// Verifies pre-scan large event list.
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

    /// Verifies pre-scan interleaved hooks content.
    #[test]
    fn test_pre_scan_interleaved_hooks_content() {
        let events = vec![
            DisplayEvent::Hook {
                name: "h1".into(),
                output: "o1".into(),
            },
            DisplayEvent::UserMessage {
                _uuid: "u1".into(),
                content: "a".into(),
            },
            DisplayEvent::Hook {
                name: "h2".into(),
                output: "o2".into(),
            },
            DisplayEvent::UserMessage {
                _uuid: "u2".into(),
                content: "b".into(),
            },
            DisplayEvent::Hook {
                name: "h3".into(),
                output: "o3".into(),
            },
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_content);
        // Last event is a hook, so last_hook is set
        assert_eq!(state.last_hook, Some(("h3".into(), "o3".into())));
    }

    // ── 41. poll_render_result: called twice returns false both times ──

    /// Verifies poll twice returns false.
    #[test]
    fn test_poll_twice_returns_false() {
        let mut app = App::new();
        assert!(!poll_render_result(&mut app));
        assert!(!poll_render_result(&mut app));
    }

    /// Verifies submit can queue a newer snapshot while an older render is in flight.
    #[test]
    fn test_submit_advances_render_thread_sequence() {
        let mut app = App::new();
        app.display_events.push(DisplayEvent::UserMessage {
            _uuid: "u1".into(),
            content: "first".into(),
        });
        app.rendered_lines_dirty = true;
        submit_render_request(&mut app, 80);
        let first_seq = app.render_thread.current_seq();

        app.display_events.push(DisplayEvent::UserMessage {
            _uuid: "u2".into(),
            content: "second".into(),
        });
        app.rendered_lines_dirty = true;
        app.render_in_flight = false;
        submit_render_request(&mut app, 80);

        assert!(app.render_in_flight);
        assert!(app.render_thread.current_seq() > first_seq);
        assert!(!app.rendered_lines_dirty);
    }

    /// Verifies dirty render results do not populate the visible cache.
    #[test]
    fn test_poll_discards_dirty_render_result() {
        let mut app = App::new();
        app.display_events.push(DisplayEvent::UserMessage {
            _uuid: "u1".into(),
            content: "stale".into(),
        });
        app.rendered_lines_dirty = true;
        submit_render_request(&mut app, 80);
        app.display_events.push(DisplayEvent::UserMessage {
            _uuid: "u2".into(),
            content: "fresh".into(),
        });
        app.invalidate_render_cache();

        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            if poll_render_result(&mut app) {
                break;
            }
        }

        assert!(!app.render_in_flight);
        assert!(app.rendered_lines_dirty);
        assert!(app.rendered_lines_cache.is_empty());
        assert_eq!(app.render_seq_applied, 0);
    }

    // ── 42. submit with session_scroll at 0 triggers deferred expansion ──

    /// Verifies submit scroll zero triggers expansion.
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

    /// Verifies pre-scan exit plan user exit plan resets.
    #[test]
    fn test_pre_scan_exit_plan_user_exit_plan_resets() {
        let events = vec![
            tool_call("ExitPlanMode", json!({})),
            DisplayEvent::UserMessage {
                _uuid: "u".into(),
                content: "x".into(),
            },
            tool_call("ExitPlanMode", json!({})),
        ];
        let state = pre_scan_events(&events);
        assert!(state.saw_exit_plan_mode);
        // Second ExitPlanMode resets the user-after flag
        assert!(!state.saw_user_after_exit_plan);
    }

    // ── 44. pre_scan: Plan clears last_hook ──

    /// Verifies pre-scan plan clears hook.
    #[test]
    fn test_pre_scan_plan_clears_hook() {
        let events = vec![
            DisplayEvent::Hook {
                name: "h".into(),
                output: "o".into(),
            },
            DisplayEvent::Plan {
                name: "p".into(),
                content: "c".into(),
            },
        ];
        let state = pre_scan_events(&events);
        assert!(state.last_hook.is_none());
    }

    // ── 45. pre_scan: ToolCall with file_path ──

    /// Verifies pre-scan tool call with file path.
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

    /// Verifies pre-scan tool result with file path.
    #[test]
    fn test_pre_scan_tool_result_with_file_path() {
        let events = vec![DisplayEvent::ToolResult {
            tool_use_id: "tu".into(),
            tool_name: "Read".into(),
            file_path: Some("/src/main.rs".into()),
            content: "fn main() {}".into(),
            is_error: false,
        }];
        let state = pre_scan_events(&events);
        assert!(state.saw_content);
    }

    // ── 47. pre_scan: all saw_* flags can be true simultaneously ──

    /// Verifies pre-scan all flags true.
    #[test]
    fn test_pre_scan_all_flags_true() {
        let events = vec![
            DisplayEvent::Init {
                _session_id: "s".into(),
                cwd: "/".into(),
                model: "m".into(),
            },
            tool_call("ExitPlanMode", json!({})),
            tool_call("AskUserQuestion", json!({"q": "test"})),
            DisplayEvent::UserMessage {
                _uuid: "u".into(),
                content: "yes".into(),
            },
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

    /// Verifies pre-scan only hooks no content.
    #[test]
    fn test_pre_scan_only_hooks_no_content() {
        let events = vec![
            DisplayEvent::Hook {
                name: "a".into(),
                output: "1".into(),
            },
            DisplayEvent::Hook {
                name: "b".into(),
                output: "2".into(),
            },
            DisplayEvent::Hook {
                name: "c".into(),
                output: "3".into(),
            },
        ];
        let state = pre_scan_events(&events);
        assert!(!state.saw_content);
        assert_eq!(state.last_hook, Some(("c".into(), "3".into())));
    }

    // ── 49. submit: incremental path when rendered_events_count > 0 ──

    /// Verifies submit incremental path.
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

    /// Verifies submit deferred expansion resets counts.
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

    // ── 51. expansion with >200 events does NOT re-defer ──
    //
    // Regression test: before the fix, expansion reset rendered_events_start=0
    // and rendered_events_count=0, which satisfied the deferred_start condition
    // again (event_count > DEFERRED_RENDER_TAIL), creating an infinite loop
    // where the user could never scroll to see early messages.

    /// Verifies expansion does not redefer large sessions.
    #[test]
    fn test_expansion_does_not_redefer_large_sessions() {
        let mut app = App::new();
        // Populate more events than DEFERRED_RENDER_TAIL
        for i in 0..300 {
            app.display_events.push(DisplayEvent::UserMessage {
                _uuid: format!("u{}", i),
                content: format!("msg {}", i),
            });
        }
        // Simulate deferred state: only tail 200 events were rendered
        app.rendered_events_start = 100;
        app.rendered_events_count = 200;
        app.rendered_content_line_count = 400;
        app.session_scroll = 0; // user scrolled to top
        app.rendered_lines_dirty = false;
        app.rendered_lines_width = 78;

        submit_render_request(&mut app, 80);

        // Expansion should have reset to full render (start=0)
        assert_eq!(app.rendered_events_start, 0);
        assert_eq!(app.rendered_events_count, 0);
        // The render was submitted (dirty cleared, in_flight set)
        assert!(!app.rendered_lines_dirty);
        assert!(app.render_in_flight);
    }

    /// Verifies submit does not render inline edit patch preview.
    #[test]
    fn test_submit_does_not_render_inline_edit_patch_preview() {
        let mut app = App::new();
        app.display_events.push(DisplayEvent::ToolCall {
            _uuid: String::new(),
            tool_use_id: "patch-1".into(),
            tool_name: "Edit".into(),
            file_path: Some("/tmp/inline-preview.txt".into()),
            input: json!({
                "patch": "*** Begin Patch\n*** Update File: /tmp/inline-preview.txt\n@@\n-old line\n+new line\n*** End Patch"
            }),
        });
        app.rendered_lines_dirty = true;

        submit_render_request(&mut app, 80);

        let mut applied = false;
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if poll_render_result(&mut app) {
                applied = true;
                break;
            }
        }

        assert!(applied, "expected render result");
        let rendered = app
            .rendered_lines_cache
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!rendered.contains("-old line"));
        assert!(!rendered.contains("+new line"));
        assert_eq!(app.clickable_paths.len(), 1);
        assert_eq!(app.clickable_paths[0].4, "old line");
        assert_eq!(app.clickable_paths[0].5, "new line");
    }
}
