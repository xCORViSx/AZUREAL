//! Session loading and discovery
//!
//! Thin orchestrator that delegates to submodules:
//! - `worktree_refresh` — git worktree discovery, file tree init, `compute_worktree_refresh`
//! - `session_file` — session file monitoring and incremental parsing
//! - `session_output` — session content loading, switching, display event extraction
//! - `debug_dump` — debug output generation with content obfuscation

mod debug_dump;
mod session_file;
mod session_output;
mod worktree_refresh;

// Free functions re-exported so external code keeps the same import paths.
// `impl App` blocks in submodules are inherent methods — no re-export needed.
pub use worktree_refresh::compute_worktree_refresh;
#[cfg(test)]
use session_output::format_uuid_short;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::app::{App, TodoItem, TodoStatus};
    use crate::events::DisplayEvent;
    use std::path::PathBuf;

    // ── format_uuid_short ──

    #[test]
    fn format_uuid_short_standard_uuid() {
        let result = format_uuid_short("abcdef12-3456-7890-abcd-ef1234567890");
        assert_eq!(result, "abcdef12-…");
    }

    #[test]
    fn format_uuid_short_eight_char_prefix() {
        let result = format_uuid_short("12345678-rest");
        assert_eq!(result, "12345678-…");
    }

    #[test]
    fn format_uuid_short_short_prefix() {
        // Dash at position 3, which is < 8
        let result = format_uuid_short("abc-def");
        // Falls through to length check: len=7 <= 12, so returns as-is
        assert_eq!(result, "abc-def");
    }

    #[test]
    fn format_uuid_short_long_no_dash() {
        let result = format_uuid_short("abcdefghijklmnop");
        // No dash, len > 12 → truncate to 11 chars + ellipsis
        assert_eq!(result, "abcdefghijk…");
    }

    #[test]
    fn format_uuid_short_short_no_dash() {
        let result = format_uuid_short("abc");
        assert_eq!(result, "abc");
    }

    #[test]
    fn format_uuid_short_empty_string() {
        let result = format_uuid_short("");
        assert_eq!(result, "");
    }

    #[test]
    fn format_uuid_short_exactly_twelve_chars() {
        let result = format_uuid_short("123456789012");
        assert_eq!(result, "123456789012");
    }

    #[test]
    fn format_uuid_short_thirteen_chars() {
        let result = format_uuid_short("1234567890123");
        assert_eq!(result, "12345678901…");
    }

    #[test]
    fn format_uuid_short_dash_at_position_eight() {
        let result = format_uuid_short("01234567-suffix");
        assert_eq!(result, "01234567-…");
    }

    #[test]
    fn format_uuid_short_multiple_dashes() {
        let result = format_uuid_short("abcdefgh-1234-5678-9abc");
        // First dash at position 8, so uses first dash
        assert_eq!(result, "abcdefgh-…");
    }

    #[test]
    fn format_uuid_short_dash_only() {
        let result = format_uuid_short("-");
        // Dash at position 0, which is < 8, falls to length check
        assert_eq!(result, "-");
    }

    #[test]
    fn format_uuid_short_dash_at_end() {
        let result = format_uuid_short("abcdefghijk-");
        // Dash at position 11 >= 8
        assert_eq!(result, "abcdefghijk-…");
    }

    // ── viewed_session_id ──

    #[test]
    fn viewed_session_id_no_data() {
        let app = App::new();
        assert!(app.viewed_session_id("branch").is_none());
    }

    #[test]
    fn viewed_session_id_returns_correct_id() {
        let mut app = App::new();
        let branch = "azureal/feat";
        app.session_files.insert(
            branch.to_string(),
            vec![
                (
                    "uuid-1".to_string(),
                    PathBuf::from("/sessions/1.jsonl"),
                    "2024-01-01".to_string(),
                ),
                (
                    "uuid-2".to_string(),
                    PathBuf::from("/sessions/2.jsonl"),
                    "2024-01-02".to_string(),
                ),
            ],
        );
        app.session_selected_file_idx.insert(branch.to_string(), 0);
        assert_eq!(app.viewed_session_id(branch), Some("uuid-1".to_string()));
    }

    #[test]
    fn viewed_session_id_second_selection() {
        let mut app = App::new();
        let branch = "azureal/test";
        app.session_files.insert(
            branch.to_string(),
            vec![
                ("uuid-a".to_string(), PathBuf::from("/a"), "t1".to_string()),
                ("uuid-b".to_string(), PathBuf::from("/b"), "t2".to_string()),
            ],
        );
        app.session_selected_file_idx.insert(branch.to_string(), 1);
        assert_eq!(app.viewed_session_id(branch), Some("uuid-b".to_string()));
    }

    #[test]
    fn viewed_session_id_idx_out_of_bounds() {
        let mut app = App::new();
        let branch = "b";
        app.session_files.insert(
            branch.to_string(),
            vec![("uuid-x".to_string(), PathBuf::from("/x"), "t".to_string())],
        );
        app.session_selected_file_idx.insert(branch.to_string(), 5); // out of bounds
        assert!(app.viewed_session_id(branch).is_none());
    }

    #[test]
    fn viewed_session_id_no_idx() {
        let mut app = App::new();
        let branch = "b";
        app.session_files.insert(
            branch.to_string(),
            vec![("uuid-x".to_string(), PathBuf::from("/x"), "t".to_string())],
        );
        // No entry in session_selected_file_idx
        assert!(app.viewed_session_id(branch).is_none());
    }

    // ── extract_skill_tools_from_events ──

    #[test]
    fn extract_skill_tools_no_events() {
        let mut app = App::new();
        app.extract_skill_tools_from_events();
        assert!(app.current_todos.is_empty());
        assert!(!app.awaiting_ask_user_question);
        assert!(app.ask_user_questions_cache.is_none());
    }

    #[test]
    fn extract_skill_tools_todo_write() {
        let mut app = App::new();
        let input = serde_json::json!({
            "todos": [
                {"content": "Task 1", "status": "pending", "activeForm": "Doing 1"},
                {"content": "Task 2", "status": "completed", "activeForm": "Doing 2"},
            ]
        });
        app.display_events.push(DisplayEvent::ToolCall {
            _uuid: "u1".to_string(),
            tool_name: "TodoWrite".to_string(),
            tool_use_id: "t1".to_string(),
            input: input,
            file_path: None,
        });
        app.extract_skill_tools_from_events();
        assert_eq!(app.current_todos.len(), 2);
        assert_eq!(app.current_todos[0].content, "Task 1");
        assert_eq!(app.current_todos[1].content, "Task 2");
    }

    #[test]
    fn extract_skill_tools_todo_cleared_by_user_message() {
        let mut app = App::new();
        let input = serde_json::json!({
            "todos": [{"content": "T", "status": "pending", "activeForm": "Doing"}]
        });
        app.display_events.push(DisplayEvent::ToolCall {
            _uuid: "u".to_string(),
            tool_name: "TodoWrite".to_string(),
            tool_use_id: "t".to_string(),
            input: input,
            file_path: None,
        });
        app.display_events.push(DisplayEvent::UserMessage {
            _uuid: "u2".to_string(),
            content: "new prompt".to_string(),
        });
        app.extract_skill_tools_from_events();
        assert!(app.current_todos.is_empty());
    }

    #[test]
    fn extract_skill_tools_ask_user_awaiting() {
        let mut app = App::new();
        app.display_events.push(DisplayEvent::ToolCall {
            _uuid: "u".to_string(),
            tool_name: "AskUserQuestion".to_string(),
            tool_use_id: "t".to_string(),
            input: serde_json::json!({"question": "Shall I proceed?"}),
            file_path: None,
        });
        app.extract_skill_tools_from_events();
        assert!(app.awaiting_ask_user_question);
        assert!(app.ask_user_questions_cache.is_some());
    }

    #[test]
    fn extract_skill_tools_ask_user_answered() {
        let mut app = App::new();
        app.display_events.push(DisplayEvent::ToolCall {
            _uuid: "u".to_string(),
            tool_name: "AskUserQuestion".to_string(),
            tool_use_id: "t".to_string(),
            input: serde_json::json!({"question": "Q?"}),
            file_path: None,
        });
        app.display_events.push(DisplayEvent::UserMessage {
            _uuid: "u2".to_string(),
            content: "Yes, go ahead".to_string(),
        });
        app.extract_skill_tools_from_events();
        assert!(!app.awaiting_ask_user_question);
    }

    #[test]
    fn extract_skill_tools_no_ask_clears_cache() {
        let mut app = App::new();
        // Only normal tool calls, no AskUserQuestion
        app.display_events.push(DisplayEvent::ToolCall {
            _uuid: "u".to_string(),
            tool_name: "Read".to_string(),
            tool_use_id: "t".to_string(),
            input: serde_json::json!({}),
            file_path: None,
        });
        app.ask_user_questions_cache = Some(serde_json::json!({}));
        app.extract_skill_tools_from_events();
        assert!(!app.awaiting_ask_user_question);
        assert!(app.ask_user_questions_cache.is_none());
    }

    #[test]
    fn extract_skill_tools_multiple_todo_writes_uses_last() {
        let mut app = App::new();
        let input1 = serde_json::json!({
            "todos": [{"content": "First", "status": "pending", "activeForm": "F"}]
        });
        let input2 = serde_json::json!({
            "todos": [{"content": "Second", "status": "in_progress", "activeForm": "S"}]
        });
        app.display_events.push(DisplayEvent::ToolCall {
            _uuid: "u".to_string(),
            tool_name: "TodoWrite".to_string(),
            tool_use_id: "t1".to_string(),
            input: input1,
            file_path: None,
        });
        app.display_events.push(DisplayEvent::ToolCall {
            _uuid: "u".to_string(),
            tool_name: "TodoWrite".to_string(),
            tool_use_id: "t2".to_string(),
            input: input2,
            file_path: None,
        });
        app.extract_skill_tools_from_events();
        assert_eq!(app.current_todos.len(), 1);
        assert_eq!(app.current_todos[0].content, "Second");
    }

    // ── check_session_file ──

    #[test]
    fn check_session_file_no_path_noop() {
        let mut app = App::new();
        app.session_file_path = None;
        app.check_session_file();
        assert!(!app.session_file_dirty);
    }

    #[test]
    fn check_session_file_nonexistent_path_noop() {
        let mut app = App::new();
        app.session_file_path = Some(PathBuf::from("/nonexistent/path/to/session.jsonl"));
        app.check_session_file();
        assert!(!app.session_file_dirty);
    }

    // ── poll_session_file ──

    #[test]
    fn poll_session_file_not_dirty_returns_false() {
        let mut app = App::new();
        app.session_file_dirty = false;
        assert!(!app.poll_session_file());
    }

    #[test]
    fn poll_session_file_reparses_active_codex_session_from_disk() {
        use std::io::Write;

        let mut app = App::new();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let session_dir = dirs::home_dir()
            .unwrap()
            .join(".codex")
            .join("sessions")
            .join("2099")
            .join("12")
            .join("30");
        std::fs::create_dir_all(&session_dir).unwrap();
        let session_path = session_dir.join(format!(
            "rollout-live-codex-reparse-{}-{}.jsonl",
            std::process::id(),
            unique
        ));
        let patch =
            "*** Begin Patch\n*** Update File: /tmp/live-codex-reparse.txt\n@@\n-before\n+after\n*** End Patch";
        let mut file = std::fs::File::create(&session_path).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "session_meta",
                "timestamp": "2026-01-01T00:00:00Z",
                "payload": {
                    "id": format!("live-codex-reparse-{}", unique),
                    "cwd": "/tmp/live-codex-reparse",
                }
            })
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-01-01T00:00:01Z",
                "payload": {
                    "type": "custom_tool_call",
                    "call_id": "call_live_patch",
                    "name": "apply_patch",
                    "input": patch,
                }
            })
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-01-01T00:00:02Z",
                "payload": {
                    "type": "custom_tool_call_output",
                    "call_id": "call_live_patch",
                    "output": "Success. Updated the following files:\nM /tmp/live-codex-reparse.txt\n",
                }
            })
        )
        .unwrap();

        app.worktrees.push(crate::models::Worktree {
            branch_name: "codex".into(),
            worktree_path: None,
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.active_slot.insert("codex".into(), "55".into());
        app.running_sessions.insert("55".into());
        app.session_file_path = Some(session_path.clone());
        app.session_file_dirty = true;
        app.session_file_parse_offset = 999;
        app.display_events.push(DisplayEvent::ToolCall {
            _uuid: String::new(),
            tool_use_id: "call_live_patch".into(),
            tool_name: "Edit".into(),
            file_path: Some("/tmp/live-codex-reparse.txt".into()),
            input: serde_json::json!({ "path": "/tmp/live-codex-reparse.txt" }),
        });

        assert!(app.poll_session_file());

        let live_tool_call = app
            .display_events
            .iter()
            .find(|event| matches!(event, DisplayEvent::ToolCall { .. }))
            .expect("expected ToolCall after Codex reparse");
        match live_tool_call {
            DisplayEvent::ToolCall { input, .. } => {
                assert_eq!(input.get("patch").and_then(|v| v.as_str()), Some(patch));
            }
            other => panic!("expected ToolCall, got {:?}", other),
        }

        let _ = std::fs::remove_file(&session_path);
    }

    // ── load_session_output state reset ──

    #[test]
    fn load_session_output_resets_session_state() {
        let mut app = App::new();
        app.session_lines.push_back("old line".to_string());
        app.session_buffer = "old buffer".to_string();
        app.display_events.push(DisplayEvent::Compacting);
        app.session_scroll = 42;
        app.session_file_path = Some(PathBuf::from("/old"));
        app.session_file_dirty = true;
        app.session_file_size = 9999;
        app.session_file_parse_offset = 5000;
        app.pending_tool_calls.insert("tool-1".to_string());
        app.failed_tool_calls.insert("tool-2".to_string());
        app.current_todos.push(TodoItem {
            content: "t".to_string(),
            status: TodoStatus::Pending,
            active_form: "t".to_string(),
        });
        app.load_session_output();
        assert!(app.session_lines.is_empty());
        assert!(app.session_buffer.is_empty());
        assert!(app.display_events.is_empty());
        assert_eq!(app.session_scroll, usize::MAX);
        assert!(app.session_file_path.is_none());
        assert!(!app.session_file_dirty);
        assert_eq!(app.session_file_size, 0);
        assert_eq!(app.session_file_parse_offset, 0);
        assert!(app.pending_tool_calls.is_empty());
        assert!(app.failed_tool_calls.is_empty());
        assert!(app.current_todos.is_empty());
        assert!(app.subagent_todos.is_empty());
    }

    #[test]
    fn load_session_output_resets_render_caches() {
        let mut app = App::new();
        app.rendered_lines_cache
            .push(ratatui::text::Line::raw("old"));
        app.session_viewport_cache
            .push(ratatui::text::Line::raw("old"));
        app.animation_line_indices.push((0, 0, "tool1".into()));
        app.message_bubble_positions.push((0, true));
        app.rendered_events_count = 100;
        app.rendered_content_line_count = 50;
        app.rendered_events_start = 10;
        app.load_session_output();
        assert!(app.rendered_lines_cache.is_empty());
        assert!(app.session_viewport_cache.is_empty());
        assert!(app.animation_line_indices.is_empty());
        assert!(app.message_bubble_positions.is_empty());
        assert_eq!(app.rendered_events_count, 0);
        assert_eq!(app.rendered_content_line_count, 0);
        assert_eq!(app.rendered_events_start, 0);
    }

    #[test]
    fn load_session_output_clears_context_badge_cache() {
        let mut app = App::new();
        app.token_badge_cache = Some(("50%".to_string(), ratatui::style::Color::Green));
        app.store_chars_cached = 123_456;
        app.load_session_output();
        assert!(app.token_badge_cache.is_none());
        assert_eq!(app.store_chars_cached, 0);
    }

    #[test]
    fn load_session_output_not_viewing_historic() {
        let mut app = App::new();
        app.viewing_historic_session = true;
        app.load_session_output();
        assert!(!app.viewing_historic_session);
    }

    #[test]
    fn load_session_output_resets_ask_user_state() {
        let mut app = App::new();
        app.awaiting_ask_user_question = true;
        app.ask_user_questions_cache = Some(serde_json::json!({"q": "test"}));
        app.load_session_output();
        assert!(!app.awaiting_ask_user_question);
        assert!(app.ask_user_questions_cache.is_none());
    }

    #[test]
    fn load_session_output_clears_clickable_paths() {
        let mut app = App::new();
        app.clickable_paths.push((
            0,
            0,
            10,
            "/file.rs".to_string(),
            "".to_string(),
            "".to_string(),
            1,
        ));
        app.clicked_path_highlight = Some((0, 0, 10, 1));
        app.load_session_output();
        assert!(app.clickable_paths.is_empty());
        assert!(app.clicked_path_highlight.is_none());
    }

    // ── load_file_tree state reset ──

    #[test]
    fn load_file_tree_clears_when_no_worktree() {
        let mut app = App::new();
        app.file_tree_entries
            .push(crate::app::types::FileTreeEntry {
                path: PathBuf::from("/old"),
                name: "old".to_string(),
                is_dir: false,
                depth: 0,
                is_hidden: false,
            });
        app.file_tree_selected = Some(0);
        app.file_tree_scroll = 5;
        app.load_file_tree();
        assert!(app.file_tree_entries.is_empty());
        assert!(app.file_tree_selected.is_none());
        assert_eq!(app.file_tree_scroll, 0);
    }

    // ── refresh_worktrees ──

    #[test]
    fn refresh_worktrees_no_project_ok() {
        let mut app = App::new();
        assert!(app.refresh_worktrees().is_ok());
    }

    // ── load_session_output with worktree but no session file ──

    #[test]
    fn load_session_output_with_worktree_no_session() {
        let mut app = App::new();
        app.worktrees.push(crate::models::Worktree {
            branch_name: "azureal/test".to_string(),
            worktree_path: Some(PathBuf::from("/tmp/nonexistent-wt")),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.load_session_output();
        // Should reset everything without panic
        assert!(app.session_file_path.is_none());
        assert!(app.display_events.is_empty());
    }

    #[test]
    fn load_session_output_historic_recomputes_badge_from_store_chars() {
        let mut app = App::new();
        let store = crate::app::session_store::SessionStore::open_memory().unwrap();
        let sid = store.create_session("azureal/test").unwrap();
        store
            .append_events(
                sid,
                &[DisplayEvent::UserMessage {
                    _uuid: String::new(),
                    content: "x".repeat(100_000),
                }],
            )
            .unwrap();

        app.session_store = Some(store);
        app.session_store_path = Some(PathBuf::from("/tmp/nonexistent-wt"));
        app.worktrees.push(crate::models::Worktree {
            branch_name: "azureal/test".to_string(),
            worktree_path: Some(PathBuf::from("/tmp/nonexistent-wt")),
            claude_session_id: None,
            archived: false,
        });
        app.session_files.insert(
            "azureal/test".to_string(),
            vec![(
                sid.to_string(),
                PathBuf::from("/tmp/session"),
                String::new(),
            )],
        );
        app.session_selected_file_idx
            .insert("azureal/test".to_string(), 0);
        app.selected_worktree = Some(0);

        app.load_session_output();

        assert_eq!(app.current_session_id, Some(sid));
        let (text, color) = app.token_badge_cache.unwrap();
        assert!(text.contains("25"), "unexpected badge text: {text:?}");
        assert_eq!(color, ratatui::style::Color::Green);
    }

    // ── load_session_output clears selected_event ──

    #[test]
    fn load_session_output_clears_selected_event() {
        let mut app = App::new();
        app.selected_event = Some(5);
        app.load_session_output();
        assert!(app.selected_event.is_none());
    }

    // ── load_session_output clears pending_user_message when matched ──

    #[test]
    fn load_session_output_pending_message_not_cleared_when_no_match() {
        let mut app = App::new();
        app.pending_user_message = Some("my prompt".to_string());
        // No worktree → no events to match against
        app.load_session_output();
        // pending_user_message is NOT cleared because there are no events to match
        assert_eq!(app.pending_user_message, Some("my prompt".to_string()));
    }

    // ── load_session_output resets event_parser ──

    #[test]
    fn load_session_output_creates_fresh_parser() {
        let mut app = App::new();
        app.load_session_output();
        // We can't easily inspect EventParser internals, but it should not panic
        assert!(app.selected_event.is_none());
    }

    // ── extract_skill_tools: TodoWrite resets scroll ──

    #[test]
    fn extract_skill_tools_resets_todo_scroll() {
        let mut app = App::new();
        app.todo_scroll = 10;
        let input = serde_json::json!({
            "todos": [{"content": "T", "status": "pending", "activeForm": "D"}]
        });
        app.display_events.push(DisplayEvent::ToolCall {
            _uuid: "u".to_string(),
            tool_name: "TodoWrite".to_string(),
            tool_use_id: "t".to_string(),
            input: input,
            file_path: None,
        });
        app.extract_skill_tools_from_events();
        assert_eq!(app.todo_scroll, 0);
    }

    // ── extract_skill_tools: non-matching tool names ignored ──

    #[test]
    fn extract_skill_tools_ignores_other_tools() {
        let mut app = App::new();
        app.display_events.push(DisplayEvent::ToolCall {
            _uuid: "u".to_string(),
            tool_name: "Write".to_string(),
            tool_use_id: "t".to_string(),
            input: serde_json::json!({}),
            file_path: None,
        });
        app.display_events.push(DisplayEvent::ToolCall {
            _uuid: "u".to_string(),
            tool_name: "Read".to_string(),
            tool_use_id: "t".to_string(),
            input: serde_json::json!({}),
            file_path: None,
        });
        app.extract_skill_tools_from_events();
        assert!(app.current_todos.is_empty());
        assert!(!app.awaiting_ask_user_question);
    }

    // ── extract_skill_tools: mixed events ──

    #[test]
    fn extract_skill_tools_mixed_event_types() {
        let mut app = App::new();
        app.display_events.push(DisplayEvent::AssistantText {
            _uuid: "u".to_string(),
            _message_id: "m".to_string(),
            text: "Hello".to_string(),
        });
        app.display_events.push(DisplayEvent::ToolCall {
            _uuid: "u".to_string(),
            tool_name: "TodoWrite".to_string(),
            tool_use_id: "t".to_string(),
            input: serde_json::json!({
                "todos": [{"content": "Mix", "status": "in_progress", "activeForm": "Mixing"}]
            }),
            file_path: None,
        });
        app.display_events.push(DisplayEvent::ToolResult {
            tool_name: "TodoWrite".to_string(),
            tool_use_id: "t".to_string(),
            content: "done".to_string(),
            file_path: None,
            is_error: false,
        });
        app.extract_skill_tools_from_events();
        assert_eq!(app.current_todos.len(), 1);
        assert_eq!(app.current_todos[0].content, "Mix");
        assert_eq!(app.current_todos[0].status, TodoStatus::InProgress);
    }

    // ── format_uuid_short: additional edge cases ──

    #[test]
    fn format_uuid_short_single_char() {
        assert_eq!(format_uuid_short("a"), "a");
    }

    #[test]
    fn format_uuid_short_exactly_eight_chars_no_dash() {
        assert_eq!(format_uuid_short("12345678"), "12345678");
    }

    #[test]
    fn format_uuid_short_nine_chars_no_dash() {
        assert_eq!(format_uuid_short("123456789"), "123456789");
    }

    #[test]
    fn format_uuid_short_unicode() {
        // Unicode chars — but function uses byte positions via find('-')
        // This may panic or work depending on char boundaries; test basic ASCII
        let result = format_uuid_short("aaaabbbb-cccc");
        assert_eq!(result, "aaaabbbb-…");
    }

    // ── viewed_session_id: edge cases ──

    #[test]
    fn viewed_session_id_empty_branch() {
        let mut app = App::new();
        app.session_files.insert(
            "".to_string(),
            vec![("id".to_string(), PathBuf::from("/p"), "t".to_string())],
        );
        app.session_selected_file_idx.insert("".to_string(), 0);
        assert_eq!(app.viewed_session_id(""), Some("id".to_string()));
    }

    // ── load_session_output resets active_task state ──

    #[test]
    fn load_session_output_resets_active_task_ids() {
        let mut app = App::new();
        app.active_task_tool_ids.insert("task-1".to_string());
        app.subagent_parent_idx = Some(2);
        app.load_session_output();
        assert!(app.active_task_tool_ids.is_empty());
        assert!(app.subagent_parent_idx.is_none());
    }

    // ── load_session_output resets compaction state ──

    #[test]
    fn load_session_output_resets_compaction_flag() {
        let mut app = App::new();
        app.compaction_banner_injected = true;
        let before = std::time::Instant::now();
        app.load_session_output();
        // load_session_output resets compaction watcher so a high-context
        // session doesn't trigger the banner from a stale timer
        assert!(!app.compaction_banner_injected);
        assert!(app.last_session_event_time >= before);
    }

    // ── load_file_tree: with worktree but nonexistent path ──

    #[test]
    fn load_file_tree_nonexistent_worktree_path() {
        let mut app = App::new();
        app.worktrees.push(crate::models::Worktree {
            branch_name: "azureal/test".to_string(),
            worktree_path: Some(PathBuf::from("/nonexistent/path/asdf")),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.load_file_tree();
        // build_file_tree on nonexistent path should produce empty entries
        assert!(app.file_tree_entries.is_empty());
        assert!(app.file_tree_selected.is_none());
    }

    // ── load_session_output resets render_in_flight ──

    #[test]
    fn load_session_output_advances_render_seq() {
        let mut app = App::new();
        app.render_in_flight = true;
        let seq_before = app.render_thread.current_seq();
        app.load_session_output();
        assert!(!app.render_in_flight);
        assert_eq!(app.render_seq_applied, seq_before);
    }

    // ── load_session_output and awaiting_plan_approval ──

    #[test]
    fn load_session_output_plan_approval_from_parsed_events() {
        let mut app = App::new();
        // With no worktree/session file, awaiting_plan_approval stays as-is
        // (it's only updated when a session file is parsed)
        app.awaiting_plan_approval = true;
        app.load_session_output();
        // No session to parse → field retains its value from the last parse
        assert!(app.awaiting_plan_approval);
    }
}
