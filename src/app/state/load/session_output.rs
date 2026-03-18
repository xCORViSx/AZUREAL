//! Session content loading, switching, and display event extraction

use super::super::App;

/// Format a UUID-like session ID as "xxxxxxxx-…" (first group + dash + ellipsis)
pub(crate) fn format_uuid_short(id: &str) -> String {
    if let Some(dash) = id.find('-') {
        if dash >= 8 {
            return format!("{}-…", &id[..dash]);
        }
    }
    if id.len() > 12 {
        format!("{}…", &id[..11])
    } else {
        id.to_string()
    }
}

impl App {
    pub fn load_session_output(&mut self) {
        // Open session store if the .azs file exists (don't create it)
        self.try_open_session_store();
        // Recover any orphaned JSOLNs from a previous crash/restart
        self.recover_orphaned_jsonls();

        // Restore terminal for new session (save was done before selection changed)
        self.restore_session_terminal();

        self.display_events.clear();

        self.session_lines.clear();
        self.session_buffer.clear();
        self.session_scroll = usize::MAX; // Start at bottom (most recent messages)
        self.session_file_path = None;
        self.session_file_modified = None;
        self.session_file_size = 0;
        self.session_file_dirty = false;
        self.session_file_parse_offset = 0;
        self.invalidate_render_cache();
        // Immediately clear rendered content so no stale lines from the
        // previous session flash while the new render is in flight.
        self.rendered_lines_cache.clear();
        self.session_viewport_cache.clear();
        self.animation_line_indices.clear();
        self.message_bubble_positions.clear();
        self.clickable_paths.clear();
        self.clickable_tables.clear();
        self.table_popup = None;
        self.clicked_path_highlight = None;
        self.file_tree_lines_cache.clear();
        self.clear_viewer();
        // Discard any in-flight render result from the previous session.
        // The render thread may still be processing old events — advancing
        // render_seq_applied ensures poll_render_result rejects stale results.
        self.render_seq_applied = self.render_thread.current_seq();
        self.render_in_flight = false;
        // Reset deferred render state so the new session gets fast initial load
        self.rendered_events_count = 0;
        self.rendered_content_line_count = 0;
        self.rendered_events_start = 0;
        self.event_parser = crate::events::EventParser::new();
        self.agent_processor_needs_reset = true;
        self.selected_event = None;
        self.pending_tool_calls.clear();
        self.failed_tool_calls.clear();
        self.token_badge_cache = None;
        self.store_chars_cached = 0;
        self.chars_since_compaction = 0;
        self.current_todos.clear();
        self.subagent_todos.clear();
        self.active_task_tool_ids.clear();
        self.subagent_parent_idx = None;
        self.awaiting_ask_user_question = false;
        self.ask_user_questions_cache = None;

        if let Some(session) = self.current_worktree() {
            let branch_name = session.branch_name.clone();
            let worktree_path = session.worktree_path.clone();

            // Reset current_session_id — it belongs to the previous worktree.
            // Will be set below if a session is found for this worktree.
            self.current_session_id = None;

            // Determine store session ID:
            // 1. From session list selection (numeric string from session_files cache)
            // 2. Auto-discover latest session from store for this branch
            let store_session_id = self
                .session_selected_file_idx
                .get(&branch_name)
                .and_then(|idx| {
                    self.session_files
                        .get(&branch_name)
                        .and_then(|f| f.get(*idx))
                        .and_then(|(id, _, _)| id.parse::<i64>().ok())
                })
                .or_else(|| {
                    self.session_store
                        .as_ref()
                        .and_then(|store| store.list_sessions(Some(&branch_name)).ok())
                        .and_then(|sessions| sessions.last().map(|s| s.id))
                });

            // Clear unread for the viewed session
            if self.git_actions_panel.is_none() {
                if let Some(sid) = store_session_id {
                    self.unread_session_ids.remove(&sid.to_string());
                }
                if self.unread_session_ids.is_empty() {
                    self.unread_sessions.remove(&branch_name);
                }
            }

            // Check if there's an active Claude process on this branch
            let is_live = self
                .active_slot
                .get(&branch_name)
                .map(|slot| self.running_sessions.contains(slot))
                .unwrap_or(false);

            // Detect if user explicitly selected a different session than the
            // active live one (e.g. via the session list). Honor that selection
            // by taking the historic path even though a live session is running.
            let active_store_id = self
                .active_slot
                .get(&branch_name)
                .and_then(|slot| self.pid_session_target.get(slot))
                .map(|(sid, _, _, _)| *sid);
            let viewing_explicit_historic = is_live
                && self.session_selected_file_idx.contains_key(&branch_name)
                && store_session_id.is_some()
                && store_session_id != active_store_id;

            if is_live && !viewing_explicit_historic {
                // Live session: re-parse the JSONL file from disk to capture ALL
                // events, including those generated while viewing another worktree.
                // The live_display_events_cache only snapshots at switch-away time
                // and misses everything produced after that.
                let slot = self.active_slot.get(&branch_name).cloned();

                // Set current_session_id from the slot's session target
                if let Some(ref slot) = slot {
                    if let Some((sid, _, _, _)) = self.pid_session_target.get(slot) {
                        self.current_session_id = Some(*sid);
                    }
                }

                let mut restored_from_jsonl = false;
                // Chars from JSONL events (not yet in store) — computed before
                // events are consumed so the context badge is accurate.
                let mut jsonl_chars: usize = 0;
                if let Some(ref slot) = slot {
                    if let Some(uuid) = self.agent_session_ids.get(slot) {
                        if let Some(ref wt_path) = worktree_path {
                            if let Some(jsonl_path) = crate::config::session_file(wt_path, uuid) {
                                if jsonl_path.exists() {
                                    // Prior turns from SQLite store
                                    let store_events = self
                                        .current_session_id
                                        .and_then(|sid| {
                                            self.session_store
                                                .as_ref()
                                                .and_then(|s| s.load_events(sid).ok())
                                        })
                                        .unwrap_or_default();

                                    // Current turn from JSONL file
                                    let parsed =
                                        crate::app::session_parser::parse_session_file(&jsonl_path);

                                    if !parsed.events.is_empty() || !store_events.is_empty() {
                                        let events_offset = store_events.len();
                                        // Count JSONL chars before consuming parsed events
                                        jsonl_chars = parsed
                                            .events
                                            .iter()
                                            .map(crate::app::session_store::event_char_len)
                                            .sum();
                                        self.display_events = store_events;
                                        self.display_events.extend(parsed.events);
                                        self.pending_tool_calls = parsed.pending_tools;
                                        self.failed_tool_calls = parsed.failed_tools;
                                        self.session_file_parse_offset = parsed.end_offset;
                                        self.session_file_size = std::fs::metadata(&jsonl_path)
                                            .map(|m| m.len())
                                            .unwrap_or(0);

                                        // Update events_offset so store_append_from_jsonl
                                        // slices at the correct boundary on exit.
                                        if let Some(target) =
                                            self.pid_session_target.get_mut(slot.as_str())
                                        {
                                            target.2 = events_offset;
                                        }
                                        restored_from_jsonl = true;
                                    }
                                    self.session_file_path = Some(jsonl_path);
                                }
                            }
                        }
                    }
                }

                if !restored_from_jsonl {
                    // Fall back to cache if JSONL re-parse was not possible
                    if let Some(cached) = self.live_display_events_cache.remove(&branch_name) {
                        self.display_events = cached;
                    }
                    // Set up JSONL file watching
                    if let Some(ref slot) = slot {
                        if let Some(uuid) = self.agent_session_ids.get(slot) {
                            if let Some(ref wt_path) = worktree_path {
                                if let Some(jsonl_path) =
                                    crate::config::session_file(wt_path, uuid)
                                {
                                    if jsonl_path.exists() {
                                        self.session_file_path = Some(jsonl_path);
                                    }
                                }
                            }
                        }
                    }
                }

                // Discard stale cache entry (JSONL re-parse has better data)
                self.live_display_events_cache.remove(&branch_name);

                self.invalidate_render_cache();
                // Sync badge from store (authoritative for prior turns),
                // then add current turn's JSONL chars that haven't been
                // stored yet so the context percentage reflects reality.
                self.update_token_badge();
                if jsonl_chars > 0 {
                    self.chars_since_compaction += jsonl_chars;
                    self.update_token_badge_live();
                }
            } else if let Some(sid) = store_session_id {
                // Historic session: load from SQLite store
                self.current_session_id = Some(sid);
                if let Some(ref store) = self.session_store {
                    if let Ok(events) = store.load_events(sid) {
                        self.display_events = events;
                        self.invalidate_render_cache();
                        self.update_token_badge();

                        if let Some(ref pending) = self.pending_user_message {
                            for event in self.display_events.iter().rev() {
                                if let crate::events::DisplayEvent::UserMessage {
                                    content, ..
                                } = event
                                {
                                    if content == pending {
                                        self.pending_user_message = None;
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Reset compaction watcher so loading a high-context session doesn't
        // immediately trigger the banner (stale last_session_event_time from
        // a previous session would satisfy the 30s threshold on first tick).
        self.last_session_event_time = std::time::Instant::now();
        self.compaction_banner_injected = false;

        // Determine if we're viewing a non-active (historic) session.
        // When true, live events from the running process are suppressed.
        // Compare by store session ID: the active slot's pid_session_target
        // vs the currently viewed session's store ID.
        self.viewing_historic_session = false;
        if let Some(session) = self.current_worktree() {
            let branch = session.branch_name.clone();
            if let Some(active_slot) = self.active_slot.get(&branch) {
                let active_store_id = self
                    .pid_session_target
                    .get(active_slot)
                    .map(|(sid, _, _, _)| *sid);
                if let Some(active_sid) = active_store_id {
                    if let Some(viewed_sid) = self.current_session_id {
                        self.viewing_historic_session = active_sid != viewed_sid;
                    }
                }
            }
        }

        // Cache the session title for the title bar (avoids file I/O on every draw frame)
        self.update_title_session_name();

        // Load file tree for new session
        self.load_file_tree();

        // Register file watches for the new session file and worktree
        self.sync_file_watches();

        // Update the OS terminal title to reflect current project and branch
        self.update_terminal_title();

        // Restore selected_model + backend from the newly loaded session's
        // events so worktree/project switches pick up the correct model.
        self.restore_model_from_session();
    }

    /// Get the session ID string of the currently viewed session for a branch.
    /// Returns the store ID as a string (from session_files cache) or falls
    /// back to current_session_id.
    pub fn viewed_session_id(&self, branch: &str) -> Option<String> {
        self.session_selected_file_idx
            .get(branch)
            .and_then(|idx| self.session_files.get(branch).and_then(|f| f.get(*idx)))
            .map(|(id, _, _)| id.clone())
            .or_else(|| self.current_session_id.map(|id| id.to_string()))
    }

    /// Cache the session display name for the title bar.
    /// Reads session names from store so draw_title_bar() is zero I/O.
    /// During RCR, the title is locked to "[RCR] <name>" and won't be overwritten.
    pub fn update_title_session_name(&mut self) {
        if self.rcr_session.is_some() {
            return;
        }
        let Some(session) = self.current_worktree() else {
            self.title_session_name.clear();
            return;
        };
        let branch = session.branch_name.clone();
        let names = self.load_all_session_names();
        let session_id = self.viewed_session_id(&branch);
        self.title_session_name = match session_id {
            Some(id) => names
                .get(&id)
                .cloned()
                .unwrap_or_else(|| format_uuid_short(&id)),
            None => String::new(),
        };
    }

    /// Scan display_events backwards for the latest TodoWrite and AskUserQuestion.
    /// TodoWrite: update sticky todo widget. AskUserQuestion: check if awaiting response.
    pub(in crate::app::state) fn extract_skill_tools_from_events(&mut self) {
        let mut found_ask = false;
        let mut saw_user_after_ask = false;
        let mut saw_user_after_todo = false;
        // Forward scan — track whether user responded after the last TodoWrite/AskUserQuestion
        for event in &self.display_events {
            match event {
                crate::events::DisplayEvent::ToolCall {
                    tool_name, input, ..
                } => {
                    if tool_name == "TodoWrite" {
                        self.current_todos = super::super::claude::parse_todos_from_input(input);
                        self.todo_scroll = 0;
                        saw_user_after_todo = false;
                    }
                    if tool_name == "AskUserQuestion" {
                        self.ask_user_questions_cache = Some(input.clone());
                        found_ask = true;
                        saw_user_after_ask = false;
                    }
                }
                crate::events::DisplayEvent::UserMessage { .. } => {
                    if found_ask {
                        saw_user_after_ask = true;
                    }
                    saw_user_after_todo = true;
                }
                _ => {}
            }
        }
        // Clear stale todos — user sent a new prompt after the last TodoWrite
        if saw_user_after_todo {
            self.current_todos.clear();
        }
        // Only awaiting if AskUserQuestion was called and no user responded yet
        self.awaiting_ask_user_question = found_ask && !saw_user_after_ask;
        if !found_ask {
            self.ask_user_questions_cache = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::format_uuid_short;
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
        let result = format_uuid_short("aaaabbbb-cccc");
        assert_eq!(result, "aaaabbbb-…");
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

    // ── load_session_output ──

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

    #[test]
    fn load_session_output_clears_selected_event() {
        let mut app = App::new();
        app.selected_event = Some(5);
        app.load_session_output();
        assert!(app.selected_event.is_none());
    }

    #[test]
    fn load_session_output_pending_message_not_cleared_when_no_match() {
        let mut app = App::new();
        app.pending_user_message = Some("my prompt".to_string());
        // No worktree → no events to match against
        app.load_session_output();
        // pending_user_message is NOT cleared because there are no events to match
        assert_eq!(app.pending_user_message, Some("my prompt".to_string()));
    }

    #[test]
    fn load_session_output_creates_fresh_parser() {
        let mut app = App::new();
        app.load_session_output();
        // We can't easily inspect EventParser internals, but it should not panic
        assert!(app.selected_event.is_none());
    }

    #[test]
    fn load_session_output_resets_active_task_ids() {
        let mut app = App::new();
        app.active_task_tool_ids.insert("task-1".to_string());
        app.subagent_parent_idx = Some(2);
        app.load_session_output();
        assert!(app.active_task_tool_ids.is_empty());
        assert!(app.subagent_parent_idx.is_none());
    }

    #[test]
    fn load_session_output_resets_compaction_flag() {
        let mut app = App::new();
        app.compaction_banner_injected = true;
        let before = std::time::Instant::now();
        app.load_session_output();
        assert!(!app.compaction_banner_injected);
        assert!(app.last_session_event_time >= before);
    }

    #[test]
    fn load_session_output_advances_render_seq() {
        let mut app = App::new();
        app.render_in_flight = true;
        let seq_before = app.render_thread.current_seq();
        app.load_session_output();
        assert!(!app.render_in_flight);
        assert_eq!(app.render_seq_applied, seq_before);
    }

    #[test]
    fn load_session_output_plan_approval_from_parsed_events() {
        let mut app = App::new();
        app.awaiting_plan_approval = true;
        app.load_session_output();
        assert!(app.awaiting_plan_approval);
    }
}
