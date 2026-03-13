//! Session list overlay helpers
//!
//! Handles opening the session list overlay, computing message counts
//! (two-phase load), and fast JSONL message counting.

use crate::app::App;

/// Open session list overlay — scoped to the currently selected worktree only.
/// Phase 1: show the overlay + loading indicator, refresh file list (fast).
/// Phase 2 (finish_session_list_load) runs on the next event loop iteration
/// so the loading dialog renders before the expensive message count I/O starts.
pub(super) fn open_session_list(app: &mut App) {
    app.show_session_list = true;
    app.session_list_loading = true;
    app.session_list_selected = 0;
    app.session_list_scroll = 0;
    // Refresh file list immediately (cheap directory listing)
    if let Some(session) = app.current_worktree() {
        let branch = session.branch_name.clone();
        if let Some(ref wt_path) = app.worktrees[app.selected_worktree.unwrap()].worktree_path {
            let files = crate::config::list_sessions(app.backend, wt_path);
            app.session_files.insert(branch, files);
        }
    }
}

/// Phase 2 of session list loading — compute message counts (expensive I/O).
/// Called from event loop after the loading dialog has had a chance to render.
pub fn finish_session_list_load(app: &mut App) {
    if let Some(session) = app.current_worktree() {
        let branch = session.branch_name.clone();
        if let Some(files) = app.session_files.get(&branch) {
            for (session_id, path, _) in files.iter() {
                let file_size = path.metadata().map(|m| m.len()).unwrap_or(0);
                if let Some(&(_, cached_size)) = app.session_msg_counts.get(session_id.as_str()) {
                    if cached_size == file_size { continue; }
                }
                let count = count_messages_in_jsonl(path);
                app.session_msg_counts.insert(session_id.clone(), (count, file_size));
            }
        }
    }
    app.session_list_loading = false;
}

/// Count message bubbles in a JSONL session file for the session list [N msgs] badge.
/// Uses fast string scanning (no JSON parsing) — "type":"user" and "type":"assistant"
/// have zero false positives in Claude Code's compact JSON output.
/// Skips isMeta, tool_result arrays, command hooks, and compaction summaries.
/// ParentUuid dedup skipped for speed (rare rewind case, off by ≤2).
fn count_messages_in_jsonl(path: &std::path::Path) -> usize {
    let Ok(content) = std::fs::read_to_string(path) else { return 0; };
    let mut count = 0usize;
    for line in content.lines() {
        if line.contains("\"type\":\"user\"") {
            // Skip system-generated meta messages
            if line.contains("\"isMeta\":true") { continue; }
            // Skip tool_result lines — only string content creates bubbles
            // Tool result user lines contain {"type":"tool_result",...} blocks
            if line.contains("\"type\":\"tool_result\"") { continue; }
            // Skip non-bubble user events the parser also skips
            if line.contains("<local-command-caveat>") { continue; }
            if line.contains("<local-command-stdout>") { continue; }
            if line.contains("<command-name>") { continue; }
            if line.contains("This session is being continued from a previous conversation") { continue; }
            count += 1;
        } else if line.contains("\"type\":\"assistant\"") {
            // Only count lines with a text content block (those become AssistantText bubbles)
            if line.contains("\"type\":\"text\"") { count += 1; }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Helper: write content to a temp file and return the PathBuf.
    /// Uses a unique file name per call to avoid collisions.
    fn temp_jsonl(content: &str) -> std::path::PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let tid = std::thread::current().id();
        let path = std::env::temp_dir().join(format!("azureal_test_{:?}_{}.jsonl", tid, id));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        path
    }

    // ── 1. count_messages_in_jsonl: empty file ──

    #[test]
    fn test_count_empty_file() {
        let f = temp_jsonl("");
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 2. count_messages_in_jsonl: single user message ──

    #[test]
    fn test_count_single_user_message() {
        let f = temp_jsonl(r#"{"type":"user","content":"hello"}"#);
        assert_eq!(count_messages_in_jsonl(&f), 1);
    }

    // ── 3. count_messages_in_jsonl: user + assistant ──

    #[test]
    fn test_count_user_and_assistant() {
        let content = r#"{"type":"user","content":"hi"}
{"type":"assistant","content":[{"type":"text","text":"hello"}]}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 2);
    }

    // ── 4. count_messages_in_jsonl: assistant without text block ──

    #[test]
    fn test_count_assistant_no_text_block() {
        let content = r#"{"type":"assistant","content":[{"type":"tool_use"}]}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 5. count_messages_in_jsonl: skips isMeta ──

    #[test]
    fn test_count_skips_is_meta() {
        let content = r#"{"type":"user","isMeta":true,"content":"system"}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 6. count_messages_in_jsonl: skips tool_result ──

    #[test]
    fn test_count_skips_tool_result() {
        let content = r#"{"type":"user","content":[{"type":"tool_result","output":"ok"}]}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 7. count_messages_in_jsonl: skips local-command-caveat ──

    #[test]
    fn test_count_skips_local_command_caveat() {
        let content = r#"{"type":"user","content":"<local-command-caveat>test</local-command-caveat>"}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 8. count_messages_in_jsonl: skips local-command-stdout ──

    #[test]
    fn test_count_skips_local_command_stdout() {
        let content = r#"{"type":"user","content":"<local-command-stdout>output</local-command-stdout>"}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 9. count_messages_in_jsonl: skips command-name ──

    #[test]
    fn test_count_skips_command_name() {
        let content = r#"{"type":"user","content":"<command-name>compact</command-name>"}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 10. count_messages_in_jsonl: skips continuation message ──

    #[test]
    fn test_count_skips_continuation() {
        let content = r#"{"type":"user","content":"This session is being continued from a previous conversation"}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 11. count_messages_in_jsonl: multiple user messages ──

    #[test]
    fn test_count_multiple_users() {
        let content = r#"{"type":"user","content":"hi"}
{"type":"user","content":"there"}
{"type":"user","content":"again"}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 3);
    }

    // ── 12. count_messages_in_jsonl: multiple assistants with text ──

    #[test]
    fn test_count_multiple_assistants() {
        let content = r#"{"type":"assistant","content":[{"type":"text","text":"a"}]}
{"type":"assistant","content":[{"type":"text","text":"b"}]}
{"type":"assistant","content":[{"type":"text","text":"c"}]}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 3);
    }

    // ── 13. count_messages_in_jsonl: mixed valid and skipped ──

    #[test]
    fn test_count_mixed_valid_and_skipped() {
        let content = r#"{"type":"user","content":"real"}
{"type":"user","isMeta":true,"content":"meta"}
{"type":"user","content":"also real"}
{"type":"assistant","content":[{"type":"text","text":"response"}]}
{"type":"assistant","content":[{"type":"tool_use"}]}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 3);
    }

    // ── 14. count_messages_in_jsonl: non-existent file ──

    #[test]
    fn test_count_nonexistent_file() {
        assert_eq!(count_messages_in_jsonl(std::path::Path::new("/nonexistent/path")), 0);
    }

    // ── 15. count_messages_in_jsonl: other event types ──

    #[test]
    fn test_count_other_event_types() {
        let content = r#"{"type":"system","content":"init"}
{"type":"tool","name":"read"}
{"something":"else"}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 16. open_session_list: sets show flag ──

    #[test]
    fn test_open_session_list_sets_show() {
        let mut app = App::new();
        open_session_list(&mut app);
        assert!(app.show_session_list);
    }

    #[test]
    fn test_open_session_list_sets_loading() {
        let mut app = App::new();
        open_session_list(&mut app);
        assert!(app.session_list_loading);
    }

    #[test]
    fn test_open_session_list_resets_selected() {
        let mut app = App::new();
        app.session_list_selected = 5;
        open_session_list(&mut app);
        assert_eq!(app.session_list_selected, 0);
    }

    #[test]
    fn test_open_session_list_resets_scroll() {
        let mut app = App::new();
        app.session_list_scroll = 10;
        open_session_list(&mut app);
        assert_eq!(app.session_list_scroll, 0);
    }

    // ── 17. open_session_list: no worktree → no crash ──

    #[test]
    fn test_open_session_list_no_worktree_no_crash() {
        let mut app = App::new();
        open_session_list(&mut app);
        assert!(app.show_session_list);
    }

    // ── 18. finish_session_list_load: clears loading ──

    #[test]
    fn test_finish_clears_loading() {
        let mut app = App::new();
        app.session_list_loading = true;
        finish_session_list_load(&mut app);
        assert!(!app.session_list_loading);
    }

    // ── 19. finish_session_list_load: no worktree → no crash ──

    #[test]
    fn test_finish_no_worktree_no_crash() {
        let mut app = App::new();
        finish_session_list_load(&mut app);
        assert!(!app.session_list_loading);
    }

    // ── 20. count_messages_in_jsonl: single line with both user and text type ──

    #[test]
    fn test_count_user_with_text_type_in_same_line() {
        // A line that has both "type":"user" and "type":"text" — user match fires first
        let content = r#"{"type":"user","content":[{"type":"text","text":"hi"}]}"#;
        let f = temp_jsonl(content);
        // The user match fires because "type":"user" is found first, and "type":"text" is also present
        // but since user check happens first in the if-else chain, it counts as user (1)
        assert_eq!(count_messages_in_jsonl(&f), 1);
    }

    // ── 21. count_messages_in_jsonl: large file ──

    #[test]
    fn test_count_large_file() {
        let mut content = String::new();
        for i in 0..100 {
            content.push_str(&format!(r#"{{"type":"user","content":"msg {}"}}"#, i));
            content.push('\n');
            content.push_str(&format!(r#"{{"type":"assistant","content":[{{"type":"text","text":"reply {}"}}]}}"#, i));
            content.push('\n');
        }
        let f = temp_jsonl(&content);
        assert_eq!(count_messages_in_jsonl(&f), 200);
    }

    // ── 22. count_messages_in_jsonl: assistant with empty content ──

    #[test]
    fn test_count_assistant_empty_content() {
        let content = r#"{"type":"assistant","content":[]}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 23. count_messages_in_jsonl: partial type match does not match ──

    #[test]
    fn test_count_partial_type_no_match() {
        // "type":"user_invalid" does NOT contain the exact string "type":"user"
        // because JSON quoting means the value is "user_invalid" not "user"
        // The contains check is for the literal string "type":"user" (with closing quote)
        let content = r#"{"type":"user_invalid","content":"nope"}"#;
        let f = temp_jsonl(content);
        // "\"type\":\"user\"" is NOT a substring of "\"type\":\"user_invalid\""
        // because the closing quote after "user" is followed by "_" not "\"" in the invalid case
        // Wait — actually let me think about this: the raw string contains:
        // "type":"user_invalid" — does this contain "type":"user"?
        // Looking for substring: `"type":"user"` in `"type":"user_invalid"`
        // chars: ..."type":"user_invalid"...
        //        ..."type":"user"...
        // No! Because after "user" in the source, the next char is '_', not '"'
        // So `contains("\"type\":\"user\"")` is FALSE.
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 24. open_session_list followed by open again resets ──

    #[test]
    fn test_open_session_list_twice_resets() {
        let mut app = App::new();
        open_session_list(&mut app);
        app.session_list_selected = 3;
        app.session_list_scroll = 5;
        open_session_list(&mut app);
        assert_eq!(app.session_list_selected, 0);
        assert_eq!(app.session_list_scroll, 0);
    }

    // ── 25. finish then open: loading sequence ──

    #[test]
    fn test_finish_then_open_loading_sequence() {
        let mut app = App::new();
        finish_session_list_load(&mut app);
        assert!(!app.session_list_loading);
        open_session_list(&mut app);
        assert!(app.session_list_loading);
    }

    // ── 26. count_messages_in_jsonl: blank lines ──

    #[test]
    fn test_count_blank_lines_ignored() {
        let content = "\n\n\n";
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 27. count_messages_in_jsonl: user line with all skip markers ──

    #[test]
    fn test_count_user_with_multiple_skip_markers() {
        // A line that has both isMeta and tool_result — should be skipped
        let content = r#"{"type":"user","isMeta":true,"content":[{"type":"tool_result"}]}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 28. count_messages_in_jsonl: assistant with tool_use and text ──

    #[test]
    fn test_count_assistant_with_tool_use_and_text() {
        let content = r#"{"type":"assistant","content":[{"type":"tool_use"},{"type":"text","text":"also"}]}"#;
        let f = temp_jsonl(content);
        // "type":"text" is present, so it counts
        assert_eq!(count_messages_in_jsonl(&f), 1);
    }

    // ── 29. count_messages_in_jsonl: interleaved conversation ──

    #[test]
    fn test_count_interleaved_conversation() {
        let content = r#"{"type":"user","content":"q1"}
{"type":"assistant","content":[{"type":"text","text":"a1"}]}
{"type":"user","content":"q2"}
{"type":"assistant","content":[{"type":"text","text":"a2"}]}
{"type":"user","content":"q3"}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 5);
    }

    // ── 30. count_messages_in_jsonl: user with leading whitespace in content ──

    #[test]
    fn test_count_user_with_whitespace_content() {
        let content = r#"{"type":"user","content":"   spaces   "}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 1);
    }

    // ── 31. open_session_list: show_session_list already true ──

    #[test]
    fn test_open_when_already_open() {
        let mut app = App::new();
        app.show_session_list = true;
        app.session_list_selected = 7;
        open_session_list(&mut app);
        assert!(app.show_session_list);
        assert_eq!(app.session_list_selected, 0); // reset
    }

    // ── 32. finish_session_list_load: already not loading ──

    #[test]
    fn test_finish_already_not_loading() {
        let mut app = App::new();
        app.session_list_loading = false;
        finish_session_list_load(&mut app);
        assert!(!app.session_list_loading);
    }

    // ── 33. count_messages_in_jsonl: only assistants ──

    #[test]
    fn test_count_only_assistants() {
        let content = r#"{"type":"assistant","content":[{"type":"text","text":"a"}]}
{"type":"assistant","content":[{"type":"text","text":"b"}]}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 2);
    }

    // ── 34. count_messages_in_jsonl: only users ──

    #[test]
    fn test_count_only_users() {
        let content = r#"{"type":"user","content":"a"}
{"type":"user","content":"b"}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 2);
    }

    // ── 35. count_messages_in_jsonl: all skip patterns in separate lines ──

    #[test]
    fn test_count_all_skip_patterns() {
        let content = r#"{"type":"user","isMeta":true,"content":"skip1"}
{"type":"user","content":[{"type":"tool_result"}]}
{"type":"user","content":"<local-command-caveat>x</local-command-caveat>"}
{"type":"user","content":"<local-command-stdout>x</local-command-stdout>"}
{"type":"user","content":"<command-name>x</command-name>"}
{"type":"user","content":"This session is being continued from a previous conversation"}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 36. count: unicode in messages ──

    #[test]
    fn test_count_unicode_messages() {
        let content = r#"{"type":"user","content":"こんにちは"}
{"type":"assistant","content":[{"type":"text","text":"日本語"}]}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 2);
    }

    // ── 37. open_session_list sets all expected fields ──

    #[test]
    fn test_open_session_list_all_fields() {
        let mut app = App::new();
        app.session_list_selected = 10;
        app.session_list_scroll = 20;
        app.session_list_loading = false;
        app.show_session_list = false;
        open_session_list(&mut app);
        assert!(app.show_session_list);
        assert!(app.session_list_loading);
        assert_eq!(app.session_list_selected, 0);
        assert_eq!(app.session_list_scroll, 0);
    }

    // ── 38. count: very long single line ──

    #[test]
    fn test_count_very_long_line() {
        let long_content = "x".repeat(10000);
        let content = format!(r#"{{"type":"user","content":"{}"}}"#, long_content);
        let f = temp_jsonl(&content);
        assert_eq!(count_messages_in_jsonl(&f), 1);
    }

    // ── 39. count: line with type but no user/assistant ──

    #[test]
    fn test_count_type_system_ignored() {
        let content = r#"{"type":"system","content":"init"}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 40. count: single newline file ──

    #[test]
    fn test_count_single_newline() {
        let f = temp_jsonl("\n");
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 41. count: tab-separated fields still match ──

    #[test]
    fn test_count_tab_in_content() {
        let content = "{\t\"type\":\"user\",\"content\":\"tabbed\"}";
        let f = temp_jsonl(content);
        // Tab before "type" means `"type":"user"` is still a substring
        assert_eq!(count_messages_in_jsonl(&f), 1);
    }

    // ── 42. count: user line with isMeta false counts ──

    #[test]
    fn test_count_is_meta_false_counts() {
        let content = r#"{"type":"user","isMeta":false,"content":"real"}"#;
        let f = temp_jsonl(content);
        // "isMeta":false does NOT contain "isMeta":true, so it counts
        assert_eq!(count_messages_in_jsonl(&f), 1);
    }

    // ── 43. count: multiple assistants some with text some without ──

    #[test]
    fn test_count_assistants_mixed_text() {
        let content = r#"{"type":"assistant","content":[{"type":"text","text":"a"}]}
{"type":"assistant","content":[{"type":"tool_use"}]}
{"type":"assistant","content":[{"type":"text","text":"b"}]}
{"type":"assistant","content":[]}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 2);
    }

    // ── 44. open_session_list: session_msg_counts untouched ──

    #[test]
    fn test_open_session_list_preserves_msg_counts() {
        let mut app = App::new();
        app.session_msg_counts.insert("sess1".to_string(), (10, 500));
        open_session_list(&mut app);
        assert_eq!(app.session_msg_counts.get("sess1"), Some(&(10, 500)));
    }

    // ── 45. count: 1000 user messages ──

    #[test]
    fn test_count_1000_messages() {
        let mut content = String::new();
        for _ in 0..1000 {
            content.push_str(r#"{"type":"user","content":"m"}"#);
            content.push('\n');
        }
        let f = temp_jsonl(&content);
        assert_eq!(count_messages_in_jsonl(&f), 1000);
    }

    // ── 46. count: JSON with escaped quotes ──

    #[test]
    fn test_count_escaped_quotes_in_content() {
        let content = r#"{"type":"user","content":"he said \"hello\""}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 1);
    }

    // ── 47. count: assistant type substring in content doesn't false positive ──

    #[test]
    fn test_count_assistant_in_content_no_double_count() {
        // A user message whose content happens to mention "type":"assistant"
        let content = r#"{"type":"user","content":"the type:\"assistant\" is cool"}"#;
        let f = temp_jsonl(content);
        // Contains "type":"user" -> counts as user (1)
        assert_eq!(count_messages_in_jsonl(&f), 1);
    }

    // ── 48. finish_session_list_load: msg_counts preserved from prior runs ──

    #[test]
    fn test_finish_preserves_existing_msg_counts() {
        let mut app = App::new();
        app.session_msg_counts.insert("old".into(), (5, 100));
        app.session_list_loading = true;
        finish_session_list_load(&mut app);
        assert_eq!(app.session_msg_counts.get("old"), Some(&(5, 100)));
    }

    // ── 49. count: user message with newlines in content ──

    #[test]
    fn test_count_newline_in_content() {
        let content = r#"{"type":"user","content":"line1\nline2"}"#;
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 1);
    }

    // ── 50. count: empty JSON objects ──

    #[test]
    fn test_count_empty_json_objects() {
        let content = "{}\n{}\n{}";
        let f = temp_jsonl(content);
        assert_eq!(count_messages_in_jsonl(&f), 0);
    }

    // ── 51. open + finish + open cycle ──

    #[test]
    fn test_open_finish_open_cycle() {
        let mut app = App::new();
        open_session_list(&mut app);
        assert!(app.session_list_loading);
        finish_session_list_load(&mut app);
        assert!(!app.session_list_loading);
        open_session_list(&mut app);
        assert!(app.session_list_loading);
    }
}
