//! Claude session handling and event processing
//!
//! Delegates to submodules:
//! - `event_handling`: live agent output processing (`apply_parsed_output`, `handle_claude_output`)
//! - `process_lifecycle`: spawn/exit lifecycle (`register_claude`, `handle_claude_started/exited`, `cancel_current_claude`)
//! - `store_ops`: SQLite session store persistence (`store_append_from_display`, `store_append_from_jsonl`)

mod event_handling;
mod process_lifecycle;
mod store_ops;

/// Parse TodoWrite input JSON into TodoItem vec.
/// Input structure: { "todos": [{ "content": "...", "status": "pending"|"in_progress"|"completed", "activeForm": "..." }] }
pub fn parse_todos_from_input(input: &serde_json::Value) -> Vec<super::app::TodoItem> {
    let Some(todos) = input.get("todos").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    todos
        .iter()
        .filter_map(|t| {
            let content = t.get("content")?.as_str()?.to_string();
            let active_form = t
                .get("activeForm")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let status = match t
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending")
            {
                "in_progress" => super::app::TodoStatus::InProgress,
                "completed" => super::app::TodoStatus::Completed,
                _ => super::app::TodoStatus::Pending,
            };
            Some(super::app::TodoItem {
                content,
                status,
                active_form,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::app::TodoStatus;
    use super::*;
    use serde_json::json;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// Verifies parse_todos_from_input correctly parses a real TodoWrite input
    /// with mixed statuses (in_progress, pending, completed).
    /// This test exists because TodoWrite JSON has a specific structure from
    /// Claude Code's tool calls — getting the field names or status strings wrong
    /// would silently produce empty results.
    #[test]
    fn test_parse_todos_real_data_mixed_statuses() {
        let input = json!({
            "todos": [
                {
                    "content": "Add all terminal keybindings to title bar hints",
                    "status": "in_progress",
                    "activeForm": "Adding terminal keybindings to title bar"
                },
                {
                    "content": "Remove Terminal section from help_sections()",
                    "status": "pending",
                    "activeForm": "Removing Terminal from help panel"
                },
                {
                    "content": "Build check and verify",
                    "status": "completed",
                    "activeForm": "Verifying build"
                }
            ]
        });
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos.len(), 3, "Should parse all 3 todos");
        assert_eq!(
            todos[0].content,
            "Add all terminal keybindings to title bar hints"
        );
        assert_eq!(todos[0].status, TodoStatus::InProgress);
        assert_eq!(
            todos[0].active_form,
            "Adding terminal keybindings to title bar"
        );
        assert_eq!(todos[1].status, TodoStatus::Pending);
        assert_eq!(todos[2].status, TodoStatus::Completed);
    }

    /// Verifies empty or missing "todos" array returns empty Vec (no panic).
    /// Without this, a missing "todos" field would need to be handled gracefully.
    #[test]
    fn test_parse_todos_empty_input() {
        assert!(parse_todos_from_input(&json!({})).is_empty());
        assert!(parse_todos_from_input(&json!({"todos": []})).is_empty());
        assert!(parse_todos_from_input(&json!({"todos": "not_array"})).is_empty());
    }

    /// Verifies that missing optional fields don't cause panics.
    /// activeForm is optional in the Claude schema — should default to empty string.
    #[test]
    fn test_parse_todos_missing_active_form() {
        let input = json!({
            "todos": [{"content": "Test item", "status": "pending"}]
        });
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].active_form, "");
        assert_eq!(todos[0].status, TodoStatus::Pending);
    }

    /// Verifies unknown status strings default to Pending (defensive parsing).
    /// Claude might add new statuses in the future — should not panic.
    #[test]
    fn test_parse_todos_unknown_status_defaults_pending() {
        let input = json!({
            "todos": [{"content": "x", "status": "blocked", "activeForm": ""}]
        });
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos[0].status, TodoStatus::Pending);
    }

    /// Verifies todos with missing content field are skipped (filter_map returns None).
    #[test]
    fn test_parse_todos_missing_content_skipped() {
        let input = json!({
            "todos": [
                {"status": "pending", "activeForm": "No content"},
                {"content": "Has content", "status": "pending", "activeForm": ""}
            ]
        });
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].content, "Has content");
    }

    // ── Null / wrong-type root values ───────────────────────────────────

    /// Null JSON value returns empty vec.
    #[test]
    fn test_parse_todos_null_root() {
        assert!(parse_todos_from_input(&json!(null)).is_empty());
    }

    /// Boolean JSON value returns empty vec.
    #[test]
    fn test_parse_todos_bool_root() {
        assert!(parse_todos_from_input(&json!(true)).is_empty());
    }

    /// Numeric JSON value returns empty vec.
    #[test]
    fn test_parse_todos_number_root() {
        assert!(parse_todos_from_input(&json!(42)).is_empty());
    }

    /// String JSON value returns empty vec.
    #[test]
    fn test_parse_todos_string_root() {
        assert!(parse_todos_from_input(&json!("hello")).is_empty());
    }

    /// Array at root (not an object) returns empty vec.
    #[test]
    fn test_parse_todos_array_root() {
        assert!(parse_todos_from_input(&json!([1, 2, 3])).is_empty());
    }

    /// Todos field is null.
    #[test]
    fn test_parse_todos_field_null() {
        assert!(parse_todos_from_input(&json!({"todos": null})).is_empty());
    }

    /// Todos field is a number.
    #[test]
    fn test_parse_todos_field_number() {
        assert!(parse_todos_from_input(&json!({"todos": 999})).is_empty());
    }

    /// Todos field is a boolean.
    #[test]
    fn test_parse_todos_field_bool() {
        assert!(parse_todos_from_input(&json!({"todos": false})).is_empty());
    }

    /// Todos field is an object instead of array.
    #[test]
    fn test_parse_todos_field_object() {
        assert!(parse_todos_from_input(&json!({"todos": {"a": 1}})).is_empty());
    }

    // ── Status parsing ──────────────────────────────────────────────────

    /// All three valid status strings parse correctly.
    #[test]
    fn test_parse_todos_all_valid_statuses() {
        let input = json!({
            "todos": [
                {"content": "A", "status": "pending", "activeForm": ""},
                {"content": "B", "status": "in_progress", "activeForm": ""},
                {"content": "C", "status": "completed", "activeForm": ""}
            ]
        });
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos[0].status, TodoStatus::Pending);
        assert_eq!(todos[1].status, TodoStatus::InProgress);
        assert_eq!(todos[2].status, TodoStatus::Completed);
    }

    /// Status "cancelled" defaults to Pending.
    #[test]
    fn test_parse_todos_status_cancelled() {
        let input = json!({"todos": [{"content": "x", "status": "cancelled", "activeForm": ""}]});
        assert_eq!(
            parse_todos_from_input(&input)[0].status,
            TodoStatus::Pending
        );
    }

    /// Status "done" defaults to Pending (not "completed").
    #[test]
    fn test_parse_todos_status_done() {
        let input = json!({"todos": [{"content": "x", "status": "done", "activeForm": ""}]});
        assert_eq!(
            parse_todos_from_input(&input)[0].status,
            TodoStatus::Pending
        );
    }

    /// Status "IN_PROGRESS" (uppercase) defaults to Pending (case-sensitive).
    #[test]
    fn test_parse_todos_status_case_sensitive() {
        let input = json!({"todos": [{"content": "x", "status": "IN_PROGRESS", "activeForm": ""}]});
        assert_eq!(
            parse_todos_from_input(&input)[0].status,
            TodoStatus::Pending
        );
    }

    /// Status "Pending" with capital P defaults to Pending match.
    #[test]
    fn test_parse_todos_status_capitalized() {
        let input = json!({"todos": [{"content": "x", "status": "Pending", "activeForm": ""}]});
        // "Pending" != "pending" — falls through to default
        assert_eq!(
            parse_todos_from_input(&input)[0].status,
            TodoStatus::Pending
        );
    }

    /// Missing status field defaults to Pending.
    #[test]
    fn test_parse_todos_missing_status() {
        let input = json!({"todos": [{"content": "x", "activeForm": ""}]});
        assert_eq!(
            parse_todos_from_input(&input)[0].status,
            TodoStatus::Pending
        );
    }

    /// Status is null — defaults to Pending.
    #[test]
    fn test_parse_todos_status_null() {
        let input = json!({"todos": [{"content": "x", "status": null, "activeForm": ""}]});
        assert_eq!(
            parse_todos_from_input(&input)[0].status,
            TodoStatus::Pending
        );
    }

    /// Status is a number — defaults to Pending.
    #[test]
    fn test_parse_todos_status_number() {
        let input = json!({"todos": [{"content": "x", "status": 1, "activeForm": ""}]});
        assert_eq!(
            parse_todos_from_input(&input)[0].status,
            TodoStatus::Pending
        );
    }

    /// Status is a boolean — defaults to Pending.
    #[test]
    fn test_parse_todos_status_bool() {
        let input = json!({"todos": [{"content": "x", "status": true, "activeForm": ""}]});
        assert_eq!(
            parse_todos_from_input(&input)[0].status,
            TodoStatus::Pending
        );
    }

    /// Empty string status defaults to Pending.
    #[test]
    fn test_parse_todos_status_empty_string() {
        let input = json!({"todos": [{"content": "x", "status": "", "activeForm": ""}]});
        assert_eq!(
            parse_todos_from_input(&input)[0].status,
            TodoStatus::Pending
        );
    }

    // ── Content field edge cases ────────────────────────────────────────

    /// Content is null — should be skipped.
    #[test]
    fn test_parse_todos_content_null() {
        let input = json!({"todos": [{"content": null, "status": "pending", "activeForm": ""}]});
        assert!(parse_todos_from_input(&input).is_empty());
    }

    /// Content is a number — should be skipped.
    #[test]
    fn test_parse_todos_content_number() {
        let input = json!({"todos": [{"content": 42, "status": "pending", "activeForm": ""}]});
        assert!(parse_todos_from_input(&input).is_empty());
    }

    /// Content is a boolean — should be skipped.
    #[test]
    fn test_parse_todos_content_bool() {
        let input = json!({"todos": [{"content": true, "status": "pending", "activeForm": ""}]});
        assert!(parse_todos_from_input(&input).is_empty());
    }

    /// Content is an empty string — should still be included.
    #[test]
    fn test_parse_todos_content_empty_string() {
        let input = json!({"todos": [{"content": "", "status": "pending", "activeForm": ""}]});
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].content, "");
    }

    /// Content with unicode characters.
    #[test]
    fn test_parse_todos_content_unicode() {
        let input = json!({"todos": [{"content": "日本語テスト 🚀", "status": "pending", "activeForm": ""}]});
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos[0].content, "日本語テスト 🚀");
    }

    /// Content with special characters: quotes, backslashes, newlines.
    #[test]
    fn test_parse_todos_content_special_chars() {
        let input = json!({"todos": [{"content": "Line1\nLine2\t\"quoted\"\\backslash", "status": "pending", "activeForm": ""}]});
        let todos = parse_todos_from_input(&input);
        assert!(todos[0].content.contains('\n'));
        assert!(todos[0].content.contains('\t'));
        assert!(todos[0].content.contains('"'));
        assert!(todos[0].content.contains('\\'));
    }

    /// Very long content string.
    #[test]
    fn test_parse_todos_content_very_long() {
        let long = "X".repeat(10000);
        let input = json!({"todos": [{"content": long, "status": "pending", "activeForm": ""}]});
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos[0].content.len(), 10000);
    }

    // ── activeForm field edge cases ─────────────────────────────────────

    /// activeForm with a value.
    #[test]
    fn test_parse_todos_active_form_value() {
        let input = json!({"todos": [{"content": "x", "status": "pending", "activeForm": "Doing the thing"}]});
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos[0].active_form, "Doing the thing");
    }

    /// activeForm is null — should default to empty string.
    #[test]
    fn test_parse_todos_active_form_null() {
        let input = json!({"todos": [{"content": "x", "status": "pending", "activeForm": null}]});
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos[0].active_form, "");
    }

    /// activeForm is a number — should default to empty string.
    #[test]
    fn test_parse_todos_active_form_number() {
        let input = json!({"todos": [{"content": "x", "status": "pending", "activeForm": 42}]});
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos[0].active_form, "");
    }

    /// activeForm with unicode.
    #[test]
    fn test_parse_todos_active_form_unicode() {
        let input =
            json!({"todos": [{"content": "x", "status": "pending", "activeForm": "テスト中"}]});
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos[0].active_form, "テスト中");
    }

    // ── Todo entry type edge cases ──────────────────────────────────────

    /// Todo entry is a string instead of object.
    #[test]
    fn test_parse_todos_entry_is_string() {
        let input = json!({"todos": ["not an object"]});
        assert!(parse_todos_from_input(&input).is_empty());
    }

    /// Todo entry is a number.
    #[test]
    fn test_parse_todos_entry_is_number() {
        let input = json!({"todos": [42]});
        assert!(parse_todos_from_input(&input).is_empty());
    }

    /// Todo entry is null.
    #[test]
    fn test_parse_todos_entry_is_null() {
        let input = json!({"todos": [null]});
        assert!(parse_todos_from_input(&input).is_empty());
    }

    /// Todo entry is a boolean.
    #[test]
    fn test_parse_todos_entry_is_bool() {
        let input = json!({"todos": [true]});
        assert!(parse_todos_from_input(&input).is_empty());
    }

    /// Todo entry is an array.
    #[test]
    fn test_parse_todos_entry_is_array() {
        let input = json!({"todos": [[1, 2, 3]]});
        assert!(parse_todos_from_input(&input).is_empty());
    }

    /// Mix of valid and invalid entries — only valid ones parsed.
    #[test]
    fn test_parse_todos_mixed_valid_invalid_entries() {
        let input = json!({
            "todos": [
                {"content": "Valid1", "status": "pending", "activeForm": ""},
                null,
                42,
                "string",
                {"content": "Valid2", "status": "completed", "activeForm": "Done"},
                {"status": "pending"},
                true
            ]
        });
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].content, "Valid1");
        assert_eq!(todos[1].content, "Valid2");
    }

    // ── Multiple todos ──────────────────────────────────────────────────

    /// Large number of todos parses correctly.
    #[test]
    fn test_parse_todos_fifty_items() {
        let items: Vec<serde_json::Value> = (0..50)
            .map(|i| json!({"content": format!("Todo #{}", i), "status": "pending", "activeForm": format!("Working on #{}", i)}))
            .collect();
        let input = json!({"todos": items});
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos.len(), 50);
        assert_eq!(todos[0].content, "Todo #0");
        assert_eq!(todos[49].content, "Todo #49");
        assert_eq!(todos[25].active_form, "Working on #25");
    }

    /// All items in_progress.
    #[test]
    fn test_parse_todos_all_in_progress() {
        let items: Vec<serde_json::Value> = (0..5)
            .map(|i| json!({"content": format!("Item {}", i), "status": "in_progress", "activeForm": ""}))
            .collect();
        let input = json!({"todos": items});
        let todos = parse_todos_from_input(&input);
        assert!(todos.iter().all(|t| t.status == TodoStatus::InProgress));
    }

    /// All items completed.
    #[test]
    fn test_parse_todos_all_completed() {
        let items: Vec<serde_json::Value> = (0..5)
            .map(|i| json!({"content": format!("Done {}", i), "status": "completed", "activeForm": ""}))
            .collect();
        let input = json!({"todos": items});
        let todos = parse_todos_from_input(&input);
        assert!(todos.iter().all(|t| t.status == TodoStatus::Completed));
    }

    /// Order is preserved.
    #[test]
    fn test_parse_todos_order_preserved() {
        let input = json!({
            "todos": [
                {"content": "First", "status": "pending", "activeForm": ""},
                {"content": "Second", "status": "in_progress", "activeForm": ""},
                {"content": "Third", "status": "completed", "activeForm": ""}
            ]
        });
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos[0].content, "First");
        assert_eq!(todos[1].content, "Second");
        assert_eq!(todos[2].content, "Third");
    }

    // ── Extra fields ────────────────────────────────────────────────────

    /// Extra fields in the root object are ignored.
    #[test]
    fn test_parse_todos_extra_root_fields() {
        let input = json!({
            "todos": [{"content": "x", "status": "pending", "activeForm": ""}],
            "extra": "ignored",
            "count": 1
        });
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos.len(), 1);
    }

    /// Extra fields in todo entries are ignored.
    #[test]
    fn test_parse_todos_extra_entry_fields() {
        let input = json!({
            "todos": [{
                "content": "x",
                "status": "pending",
                "activeForm": "af",
                "priority": "high",
                "id": 123,
                "nested": {"a": 1}
            }]
        });
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].content, "x");
        assert_eq!(todos[0].active_form, "af");
    }

    // ── Whitespace / formatting ─────────────────────────────────────────

    /// Content with leading/trailing whitespace is preserved (not trimmed).
    #[test]
    fn test_parse_todos_whitespace_preserved() {
        let input =
            json!({"todos": [{"content": "  spaces  ", "status": "pending", "activeForm": ""}]});
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos[0].content, "  spaces  ");
    }

    /// Status with whitespace defaults to Pending (not trimmed).
    #[test]
    fn test_parse_todos_status_whitespace() {
        let input = json!({"todos": [{"content": "x", "status": " pending ", "activeForm": ""}]});
        // " pending " != "pending" → defaults
        assert_eq!(
            parse_todos_from_input(&input)[0].status,
            TodoStatus::Pending
        );
    }

    /// activeForm with whitespace is preserved.
    #[test]
    fn test_parse_todos_active_form_whitespace() {
        let input =
            json!({"todos": [{"content": "x", "status": "pending", "activeForm": "  spaced  "}]});
        assert_eq!(parse_todos_from_input(&input)[0].active_form, "  spaced  ");
    }

    // ── Single item variations ──────────────────────────────────────────

    /// Single pending todo.
    #[test]
    fn test_parse_todos_single_pending() {
        let input =
            json!({"todos": [{"content": "Task", "status": "pending", "activeForm": "Working"}]});
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].content, "Task");
        assert_eq!(todos[0].status, TodoStatus::Pending);
        assert_eq!(todos[0].active_form, "Working");
    }

    /// Single in_progress todo.
    #[test]
    fn test_parse_todos_single_in_progress() {
        let input = json!({"todos": [{"content": "Active", "status": "in_progress", "activeForm": "Running"}]});
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos[0].status, TodoStatus::InProgress);
    }

    /// Single completed todo.
    #[test]
    fn test_parse_todos_single_completed() {
        let input = json!({"todos": [{"content": "Done", "status": "completed", "activeForm": "Finished"}]});
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos[0].status, TodoStatus::Completed);
    }

    // ── Realistic Claude Code payloads ──────────────────────────────────

    /// Realistic TodoWrite payload from a coding session.
    #[test]
    fn test_parse_todos_realistic_coding_session() {
        let input = json!({
            "todos": [
                {"content": "Read the source file", "status": "completed", "activeForm": "Reading source"},
                {"content": "Implement the feature", "status": "in_progress", "activeForm": "Implementing feature"},
                {"content": "Write unit tests", "status": "pending", "activeForm": "Writing tests"},
                {"content": "Run cargo test", "status": "pending", "activeForm": "Running tests"},
                {"content": "Update documentation", "status": "pending", "activeForm": "Updating docs"}
            ]
        });
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos.len(), 5);
        assert_eq!(
            todos
                .iter()
                .filter(|t| t.status == TodoStatus::Completed)
                .count(),
            1
        );
        assert_eq!(
            todos
                .iter()
                .filter(|t| t.status == TodoStatus::InProgress)
                .count(),
            1
        );
        assert_eq!(
            todos
                .iter()
                .filter(|t| t.status == TodoStatus::Pending)
                .count(),
            3
        );
    }

    /// Payload with content containing code snippets.
    #[test]
    fn test_parse_todos_content_with_code() {
        let input = json!({
            "todos": [{
                "content": "Fix `fn parse_todos()` in src/app/state/claude.rs",
                "status": "pending",
                "activeForm": "Fixing parse_todos"
            }]
        });
        let todos = parse_todos_from_input(&input);
        assert!(todos[0].content.contains('`'));
        assert!(todos[0].content.contains("parse_todos()"));
    }

    /// Content containing JSON-like text (nested quotes).
    #[test]
    fn test_parse_todos_content_json_like() {
        let input = json!({
            "todos": [{
                "content": "Parse {\"key\": \"value\"} from input",
                "status": "pending",
                "activeForm": ""
            }]
        });
        let todos = parse_todos_from_input(&input);
        assert!(todos[0].content.contains("{\"key\""));
    }

    /// Hundred items stress test — no panic, all parsed.
    #[test]
    fn test_parse_todos_hundred_items() {
        let items: Vec<serde_json::Value> = (0..100)
            .map(|i| {
                let status = match i % 3 {
                    0 => "pending",
                    1 => "in_progress",
                    _ => "completed",
                };
                json!({"content": format!("Task {}", i), "status": status, "activeForm": format!("Form {}", i)})
            })
            .collect();
        let input = json!({"todos": items});
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos.len(), 100);
        // Verify distribution
        let pending = todos
            .iter()
            .filter(|t| t.status == TodoStatus::Pending)
            .count();
        let in_prog = todos
            .iter()
            .filter(|t| t.status == TodoStatus::InProgress)
            .count();
        let completed = todos
            .iter()
            .filter(|t| t.status == TodoStatus::Completed)
            .count();
        assert_eq!(pending, 34);
        assert_eq!(in_prog, 33);
        assert_eq!(completed, 33);
    }

    /// Empty object todo entry (missing all fields) is skipped.
    #[test]
    fn test_parse_todos_empty_object_entry() {
        let input = json!({"todos": [{}]});
        assert!(parse_todos_from_input(&input).is_empty());
    }

    /// Content is an array (wrong type) — skipped.
    #[test]
    fn test_parse_todos_content_array() {
        let input =
            json!({"todos": [{"content": ["a", "b"], "status": "pending", "activeForm": ""}]});
        assert!(parse_todos_from_input(&input).is_empty());
    }

    /// Content is an object (wrong type) — skipped.
    #[test]
    fn test_parse_todos_content_object() {
        let input = json!({"todos": [{"content": {"nested": true}, "status": "pending", "activeForm": ""}]});
        assert!(parse_todos_from_input(&input).is_empty());
    }

    /// activeForm is a boolean (wrong type) — defaults to empty string.
    #[test]
    fn test_parse_todos_active_form_bool() {
        let input = json!({"todos": [{"content": "x", "status": "pending", "activeForm": true}]});
        assert_eq!(parse_todos_from_input(&input)[0].active_form, "");
    }

    /// activeForm is an array (wrong type) — defaults to empty string.
    #[test]
    fn test_parse_todos_active_form_array() {
        let input = json!({"todos": [{"content": "x", "status": "pending", "activeForm": [1, 2]}]});
        assert_eq!(parse_todos_from_input(&input)[0].active_form, "");
    }

    /// Status is an array (wrong type) — defaults to Pending.
    #[test]
    fn test_parse_todos_status_array() {
        let input = json!({"todos": [{"content": "x", "status": ["a"], "activeForm": ""}]});
        assert_eq!(
            parse_todos_from_input(&input)[0].status,
            TodoStatus::Pending
        );
    }

    /// Verify that content with only whitespace is a valid todo.
    #[test]
    fn test_parse_todos_whitespace_only_content() {
        let input =
            json!({"todos": [{"content": "   \t\n   ", "status": "pending", "activeForm": ""}]});
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].content, "   \t\n   ");
    }

    #[test]
    fn register_claude_tracks_only_codex_slots() {
        let mut app = super::super::App::new();
        let (_claude_tx, claude_rx) = mpsc::channel();
        let (_codex_tx, codex_rx) = mpsc::channel();

        app.register_claude("claude".into(), 10, claude_rx, None);
        app.register_claude("codex".into(), 20, codex_rx, Some("gpt-5.4"));

        assert!(!app.codex_slot_started_at.contains_key("10"));
        assert!(app.codex_slot_started_at.contains_key("20"));
    }

    #[test]
    fn apply_parsed_output_sets_codex_complete_duration_from_pid_lifetime() {
        use crate::events::DisplayEvent;
        use crate::models::OutputType;

        let mut app = super::super::App::new();
        let (_tx, rx) = mpsc::channel();
        app.register_claude("codex".into(), 42, rx, Some("gpt-5.4"));
        app.codex_slot_started_at
            .insert("42".into(), Instant::now() - Duration::from_secs(3));

        app.apply_parsed_output(
            "42",
            vec![DisplayEvent::Complete {
                _session_id: String::new(),
                success: true,
                duration_ms: 0,
                cost_usd: 0.0,
            }],
            None,
            OutputType::Json,
            "",
        );

        match app.display_events.last() {
            Some(DisplayEvent::Complete { duration_ms, .. }) => {
                assert!(*duration_ms >= 3_000);
            }
            other => panic!("expected Complete event, got {:?}", other),
        }
    }

    #[test]
    fn apply_parsed_output_preserves_existing_claude_duration() {
        use crate::events::DisplayEvent;
        use crate::models::OutputType;

        let mut app = super::super::App::new();

        app.apply_parsed_output(
            "7",
            vec![DisplayEvent::Complete {
                _session_id: String::new(),
                success: true,
                duration_ms: 1_234,
                cost_usd: 0.0,
            }],
            None,
            OutputType::Json,
            "",
        );

        match app.display_events.last() {
            Some(DisplayEvent::Complete { duration_ms, .. }) => {
                assert_eq!(*duration_ms, 1_234);
            }
            other => panic!("expected Complete event, got {:?}", other),
        }
    }

    #[test]
    fn apply_parsed_output_updates_live_context_counter_and_badge() {
        use crate::events::DisplayEvent;
        use crate::models::OutputType;

        let mut app = super::super::App::new();
        app.current_session_id = Some(1);
        app.chars_since_compaction = 200_000;

        app.apply_parsed_output(
            "7",
            vec![DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text: "x".repeat(10_000),
            }],
            None,
            OutputType::Json,
            "",
        );

        assert_eq!(app.chars_since_compaction, 210_000);
        let (text, color) = app.token_badge_cache.unwrap();
        assert!(!text.contains("100"));
        assert_eq!(color, ratatui::style::Color::Green);
    }

    #[test]
    fn handle_claude_exited_clears_codex_slot_timer() {
        let mut app = super::super::App::new();
        let (_tx, rx) = mpsc::channel();
        app.register_claude("codex".into(), 88, rx, Some("gpt-5.4"));

        app.handle_claude_exited("88", Some(0));

        assert!(!app.codex_slot_started_at.contains_key("88"));
    }

    #[test]
    fn handle_claude_exited_forces_full_codex_session_reparse() {
        let mut app = super::super::App::new();
        let (_tx, rx) = mpsc::channel();
        app.worktrees.push(crate::models::Worktree {
            branch_name: "codex".into(),
            worktree_path: None,
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.register_claude("codex".into(), 91, rx, Some("gpt-5.4"));
        app.session_file_path = Some("/tmp/codex-session.jsonl".into());
        app.session_file_parse_offset = 1234;
        app.session_file_dirty = false;

        app.handle_claude_exited("91", Some(0));

        assert!(app.session_file_dirty);
        assert_eq!(app.session_file_parse_offset, 0);
    }

    #[test]
    fn handle_claude_exited_keeps_incremental_parse_for_claude() {
        let mut app = super::super::App::new();
        let (_tx, rx) = mpsc::channel();
        app.worktrees.push(crate::models::Worktree {
            branch_name: "claude".into(),
            worktree_path: None,
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.register_claude("claude".into(), 92, rx, None);
        app.session_file_path = Some("/tmp/claude-session.jsonl".into());
        app.session_file_parse_offset = 5678;
        app.session_file_dirty = false;

        app.handle_claude_exited("92", Some(0));

        assert!(app.session_file_dirty);
        assert_eq!(app.session_file_parse_offset, 5678);
    }

    #[test]
    fn store_append_from_jsonl_reparses_codex_turn_from_session_file() {
        use crate::backend::Backend;
        use crate::events::DisplayEvent;
        use std::io::Write;

        let mut app = super::super::App::new();
        let store = crate::app::session_store::SessionStore::open_memory().unwrap();
        let wt_path = std::path::PathBuf::from("/tmp/codex-reparse");
        let sid = store.create_session("main").unwrap();
        app.session_store = Some(store);
        app.session_store_path = Some(wt_path.clone());

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let codex_session_id = format!("codex-reparse-{}-{}", std::process::id(), unique);
        let session_dir = dirs::home_dir()
            .unwrap()
            .join(".codex")
            .join("sessions")
            .join("2099")
            .join("12")
            .join("31");
        std::fs::create_dir_all(&session_dir).unwrap();
        let session_path = session_dir.join(format!("rollout-{}.jsonl", codex_session_id));
        let patch = "*** Begin Patch\n*** Update File: /tmp/codex-reparse.txt\n@@\n-old line\n+new line\n*** End Patch";
        let mut file = std::fs::File::create(&session_path).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "session_meta",
                "timestamp": "2026-01-01T00:00:00Z",
                "payload": {
                    "id": codex_session_id,
                    "cwd": wt_path,
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
                    "call_id": "call_patch",
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
                    "call_id": "call_patch",
                    "output": "Success. Updated the following files:\nM /tmp/codex-reparse.txt\n",
                }
            })
        )
        .unwrap();
        // Set up worktree + active_slot so is_viewing_slot("55") returns true
        app.worktrees.push(crate::models::Worktree {
            branch_name: "main".into(),
            worktree_path: Some(wt_path.clone()),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.active_slot.insert("main".into(), "55".into());
        app.pid_session_target
            .insert("55".into(), (sid, wt_path.clone(), 0, 0));
        app.agent_session_ids
            .insert("55".into(), codex_session_id.clone());
        app.session_file_path = Some(session_path.clone());
        app.display_events = vec![
            DisplayEvent::ToolCall {
                _uuid: String::new(),
                tool_use_id: "call_patch".into(),
                tool_name: "Edit".into(),
                file_path: Some("/tmp/codex-reparse.txt".into()),
                input: json!({ "path": "/tmp/codex-reparse.txt" }),
            },
            DisplayEvent::ToolResult {
                tool_use_id: "call_patch".into(),
                tool_name: "Edit".into(),
                file_path: Some("/tmp/codex-reparse.txt".into()),
                content: "File update: /tmp/codex-reparse.txt".into(),
                is_error: false,
            },
        ];

        app.store_append_from_jsonl("55", Backend::Codex);

        let stored = app
            .session_store
            .as_ref()
            .unwrap()
            .load_events(sid)
            .unwrap();
        let stored_tool_call = stored
            .iter()
            .find(|event| matches!(event, DisplayEvent::ToolCall { .. }))
            .expect("expected reparsed ToolCall in stored events");
        match stored_tool_call {
            DisplayEvent::ToolCall {
                tool_name,
                file_path,
                input,
                ..
            } => {
                assert_eq!(tool_name, "Edit");
                assert_eq!(file_path.as_deref(), Some("/tmp/codex-reparse.txt"));
                assert_eq!(input.get("patch").and_then(|v| v.as_str()), Some(patch));
            }
            other => panic!("expected reparsed ToolCall, got {:?}", other),
        }
        let live_tool_call = app
            .display_events
            .iter()
            .find(|event| matches!(event, DisplayEvent::ToolCall { .. }))
            .expect("expected live display to contain reparsed ToolCall");
        match live_tool_call {
            DisplayEvent::ToolCall { input, .. } => {
                assert_eq!(input.get("patch").and_then(|v| v.as_str()), Some(patch));
            }
            other => panic!("expected live display to be replaced, got {:?}", other),
        }
        assert!(app.session_file_path.is_none());
        assert!(!session_path.exists());
    }

    #[test]
    fn store_append_from_jsonl_triggers_compaction_at_threshold() {
        use crate::backend::Backend;
        use crate::events::DisplayEvent;

        let mut app = super::super::App::new();
        let store = crate::app::session_store::SessionStore::open_memory().unwrap();
        let wt_path = std::path::PathBuf::from("/tmp/compaction-backend");
        let sid = store.create_session("main").unwrap();
        app.session_store = Some(store);
        app.session_store_path = Some(wt_path.clone());
        // Set up worktree + active_slot so is_viewing_slot("55") returns true
        app.worktrees.push(crate::models::Worktree {
            branch_name: "main".into(),
            worktree_path: Some(wt_path.clone()),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.active_slot.insert("main".into(), "55".into());
        app.pid_session_target
            .insert("55".into(), (sid, wt_path.clone(), 0, 0));
        app.display_events.push(DisplayEvent::AssistantText {
            _uuid: String::new(),
            _message_id: String::new(),
            text: "x".repeat(crate::app::session_store::COMPACTION_THRESHOLD + 1),
        });

        app.store_append_from_jsonl("55", Backend::Codex);

        assert_eq!(app.compaction_needed, Some((sid, wt_path)));
    }
}
