//! Session file monitoring and incremental parsing

use super::super::App;
use crate::backend::Backend;
use crate::events::DisplayEvent;

/// Session-file watch, parse, and display reconciliation methods.
impl App {
    /// Tell the file watcher thread to watch the current session file and
    /// worktree directory. Called after session switch (from load_session_output).
    pub fn sync_file_watches(&self) {
        let Some(ref watcher) = self.file_watcher else {
            return;
        };
        watcher.send(crate::watcher::WatchCommand::ClearAll);
        if let Some(ref path) = self.session_file_path {
            watcher.send(crate::watcher::WatchCommand::WatchSessionFile(path.clone()));
        }
        if let Some(idx) = self.selected_worktree {
            if let Some(session) = self.worktrees.get(idx) {
                if let Some(ref wt_path) = session.worktree_path {
                    watcher.send(crate::watcher::WatchCommand::WatchWorktree(
                        wt_path.to_path_buf(),
                    ));
                }
            }
        }
    }

    /// Check if session file changed (lightweight - just checks file size)
    /// Marks dirty if changed, but doesn't parse yet.
    /// Also recovers from missing-file state if the source reappears.
    pub fn check_session_file(&mut self) {
        // Auto-recovery: if source was missing and has reappeared, restore normal mode
        let Some(path) = &self.session_file_path else {
            return;
        };
        let Ok(metadata) = std::fs::metadata(path) else {
            return;
        };
        let new_size = metadata.len();

        if new_size != self.session_file_size {
            self.session_file_size = new_size;
            self.session_file_modified = metadata.modified().ok();
            self.session_file_dirty = true;
        }
    }

    /// Poll session file - does the actual parse if dirty.
    /// SKIP when Claude is actively streaming to this session — the live
    /// `handle_claude_output()` path already adds events in real-time.
    /// Polling the file too would duplicate every event (live adds to
    /// display_events, then incremental parse treats those as "existing"
    /// and appends the same events again from the file).
    pub fn poll_session_file(&mut self) -> bool {
        if !self.session_file_dirty {
            return false;
        }
        self.session_file_dirty = false;
        // Claude's live stdout already has the rich event data, so polling the
        // file while the active slot runs would duplicate events. Codex is the
        // opposite: stdout can collapse edits to `file_change`, while the JSONL
        // contains the full `apply_patch` payload. For active Codex sessions,
        // force a full reparse from disk so the richer edit diffs replace the
        // placeholder live events mid-turn.
        if self.is_active_slot_running() {
            let backend = self
                .session_file_path
                .as_deref()
                .and_then(crate::config::backend_from_session_path)
                .unwrap_or(self.backend);
            if backend != crate::backend::Backend::Codex {
                return false;
            }
            self.session_file_parse_offset = 0;
        }
        self.refresh_session_events();
        true
    }

    /// Lightweight refresh of session events (no terminal/file tree reload).
    /// Uses incremental parsing — only reads new bytes appended since last parse.
    pub(in crate::app::state) fn refresh_session_events(&mut self) {
        let Some(path) = self.session_file_path.clone() else {
            return;
        };
        let previous_display_events = self.display_events.clone();

        // Track if we were at bottom before refresh (usize::MAX = follow mode)
        let was_at_bottom = self.session_scroll == usize::MAX;

        // Incremental parse: only read new bytes since last offset
        let was_full_reparse = self.session_file_parse_offset == 0;
        let parse_backend = crate::config::backend_from_session_path(&path).unwrap_or(self.backend);
        let mut parsed = match parse_backend {
            Backend::Claude => crate::app::session_parser::parse_session_file_incremental(
                &path,
                self.session_file_parse_offset,
                &self.display_events,
                &self.pending_tool_calls,
                &self.failed_tool_calls,
            ),
            Backend::Codex => {
                crate::app::codex_session_parser::parse_codex_session_file_incremental(
                    &path,
                    self.session_file_parse_offset,
                    &self.display_events,
                    &self.pending_tool_calls,
                    &self.failed_tool_calls,
                )
            }
        };
        parsed.events =
            crate::app::context_injection::strip_injected_context_from_events(parsed.events);
        if was_full_reparse && parse_backend == Backend::Codex {
            parsed.events = self.merge_live_prefix_for_active_codex_reparse(
                parsed.events,
                &previous_display_events,
            );
        }
        let turn_events_offset =
            self.active_turn_events_offset_for_display(&previous_display_events);
        let pending_confirmed_by_parse =
            self.pending_user_message.as_ref().is_some_and(|pending| {
                let start_idx = turn_events_offset.unwrap_or(0);
                parsed
                    .events
                    .iter()
                    .enumerate()
                    .skip(start_idx)
                    .any(|(_, event)| match event {
                        DisplayEvent::UserMessage { content, .. } => content == pending,
                        _ => false,
                    })
            });
        if was_full_reparse && parse_backend == Backend::Codex {
            parsed.events = self.preserve_pending_user_message(parsed.events, turn_events_offset);
            parsed.events = Self::preserve_live_suffix_when_reparse_lags(
                parsed.events,
                &previous_display_events,
            );
        }
        // Guard: if the parse returned empty events but we already had content,
        // the file was likely temporarily unavailable (locked, atomic rewrite,
        // or deleted during Claude Code compaction). Preserve existing display
        // rather than wiping the session pane. The next poll will retry.
        if parsed.events.is_empty() && !self.display_events.is_empty() && parsed.end_offset == 0 {
            return;
        }
        if was_full_reparse {
            // Full re-parse replaced ALL display_events. Use the replacement
            // helper so stale in-flight render results from the previous event
            // array cannot append duplicate bubbles after this assignment.
            self.replace_display_events_for_render(parsed.events);
        } else {
            self.display_events = parsed.events;
            self.invalidate_render_cache();
        }
        self.pending_tool_calls = parsed.pending_tools;
        self.failed_tool_calls = parsed.failed_tools;
        self.parse_total_lines = parsed.total_lines;
        self.parse_errors = parsed.parse_errors;
        self.assistant_total = parsed.assistant_total;
        self.assistant_no_message = parsed.assistant_no_message;
        self.assistant_no_content_arr = parsed.assistant_no_content_arr;
        self.assistant_text_blocks = parsed.assistant_text_blocks;
        self.awaiting_plan_approval = parsed.awaiting_plan_approval;
        // Extract latest TodoWrite and AskUserQuestion state from parsed events
        self.extract_skill_tools_from_events();
        self.session_file_parse_offset = parsed.end_offset;

        // Clear pending message once it appears in the parsed events.
        // Scan all events from the end — Claude may have emitted many
        // events (hooks, tool calls, text) after the user message.
        if pending_confirmed_by_parse {
            self.pending_user_message = None;
        } else if !was_full_reparse || parse_backend != Backend::Codex {
            if let Some(ref pending) = self.pending_user_message {
                for event in self.display_events.iter().rev() {
                    if let crate::events::DisplayEvent::UserMessage { content, .. } = event {
                        if content == pending {
                            self.pending_user_message = None;
                        }
                        break; // stop at first UserMessage either way
                    }
                }
            }
        }

        // Activity detected from session file — reset compaction inactivity watcher
        self.last_session_event_time = std::time::Instant::now();
        self.compaction_banner_injected = false;

        // If we were following bottom, stay at bottom after content update
        if was_at_bottom {
            self.session_scroll = usize::MAX;
        }
    }

    /// Prefix a full Codex JSONL reparse with stored and live-only pre-turn
    /// events so active-turn reparses do not reorder prompts while SQLite lags.
    fn merge_live_prefix_for_active_codex_reparse(
        &self,
        parsed_events: Vec<DisplayEvent>,
        previous_display_events: &[DisplayEvent],
    ) -> Vec<DisplayEvent> {
        let turn_events_offset =
            self.active_turn_events_offset_for_display(previous_display_events);
        let store_events = self
            .current_session_id
            .and_then(|session_id| {
                self.session_store
                    .as_ref()
                    .and_then(|store| store.load_events(session_id).ok())
            })
            .unwrap_or_default();

        let mut merged = store_events;
        if let Some(turn_events_offset) = turn_events_offset {
            let live_prefix_end = turn_events_offset.min(previous_display_events.len());
            let live_prefix = &previous_display_events[..live_prefix_end];
            let overlap = crate::app::session_store::overlap_prefix_len(&merged, live_prefix);
            if overlap == merged.len() {
                merged.extend(live_prefix.iter().skip(overlap).cloned());
            }
        }

        let overlap = crate::app::session_store::overlap_prefix_len(&merged, &parsed_events);
        merged.extend(parsed_events.into_iter().skip(overlap));
        merged
    }

    /// Keep the optimistically rendered user prompt visible when a Codex full
    /// reparse has not yet written that prompt into the JSONL session file.
    fn preserve_pending_user_message(
        &self,
        mut parsed_events: Vec<DisplayEvent>,
        turn_events_offset: Option<usize>,
    ) -> Vec<DisplayEvent> {
        let Some(pending) = self.pending_user_message.as_ref() else {
            return parsed_events;
        };

        let insert_idx = turn_events_offset
            .unwrap_or(parsed_events.len())
            .min(parsed_events.len());

        if parsed_events
            .iter()
            .skip(insert_idx)
            .any(|event| matches!(event, DisplayEvent::UserMessage { content, .. } if content == pending))
        {
            return parsed_events;
        }

        parsed_events.insert(
            insert_idx,
            DisplayEvent::UserMessage {
                _uuid: String::new(),
                content: pending.clone(),
            },
        );
        parsed_events
    }

    /// Keep live events after a lagging full reparse when the disk-parsed
    /// replacement is only a prefix of what the pane already showed.
    fn preserve_live_suffix_when_reparse_lags(
        mut parsed_events: Vec<DisplayEvent>,
        previous_display_events: &[DisplayEvent],
    ) -> Vec<DisplayEvent> {
        let overlap =
            crate::app::session_store::overlap_prefix_len(&parsed_events, previous_display_events);
        if overlap == parsed_events.len() && previous_display_events.len() > parsed_events.len() {
            parsed_events.extend(previous_display_events.iter().skip(overlap).cloned());
        }
        parsed_events
    }

    /// Return the display-event index where the currently running turn began.
    fn active_turn_events_offset(&self) -> Option<usize> {
        let branch = self.current_worktree()?.branch_name.clone();
        let slot = self.active_slot.get(&branch)?;
        self.pid_session_target
            .get(slot)
            .map(|(_, _, events_offset, _)| *events_offset)
    }

    /// Return the best active-turn start offset for a specific display snapshot.
    fn active_turn_events_offset_for_display(
        &self,
        display_events: &[DisplayEvent],
    ) -> Option<usize> {
        let tracked_offset = self.active_turn_events_offset();
        let Some(pending) = self.pending_user_message.as_ref() else {
            return tracked_offset;
        };
        if tracked_offset.is_some_and(|idx| {
            matches!(
                display_events.get(idx),
                Some(DisplayEvent::UserMessage { content, .. }) if content == pending
            )
        }) {
            return tracked_offset;
        }
        display_events.iter().rposition(|event| {
            matches!(event, DisplayEvent::UserMessage { content, .. } if content == pending)
        })
    }
}

#[cfg(test)]
/// Tests for session-file polling and active Codex reparse reconciliation.
mod tests {
    use crate::app::state::app::App;
    use crate::events::DisplayEvent;
    use ratatui::text::Line;
    use std::path::PathBuf;

    // ── check_session_file ──

    /// Missing session paths leave the dirty flag unchanged.
    #[test]
    fn check_session_file_no_path_noop() {
        let mut app = App::new();
        app.session_file_path = None;
        app.check_session_file();
        assert!(!app.session_file_dirty);
    }

    /// Nonexistent session files are ignored rather than marking a refresh.
    #[test]
    fn check_session_file_nonexistent_path_noop() {
        let mut app = App::new();
        app.session_file_path = Some(PathBuf::from("/nonexistent/path/to/session.jsonl"));
        app.check_session_file();
        assert!(!app.session_file_dirty);
    }

    // ── poll_session_file ──

    /// Clean session-file state avoids unnecessary parser work.
    #[test]
    fn poll_session_file_not_dirty_returns_false() {
        let mut app = App::new();
        app.session_file_dirty = false;
        assert!(!app.poll_session_file());
    }

    /// Active Codex sessions fully reparse JSONL so disk apply-patch payloads
    /// replace the lighter live-stream edit placeholder.
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
        app.rendered_lines_cache = vec![Line::from("old rendered line")];
        app.rendered_lines_dirty = false;
        app.rendered_events_count = 1;
        app.rendered_content_line_count = 1;
        app.rendered_events_start = 1;
        app.render_in_flight = true;
        app.session_viewport_scroll = 5;
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
        assert!(app.rendered_lines_dirty);
        assert_eq!(app.rendered_events_count, 0);
        assert_eq!(app.rendered_content_line_count, 0);
        assert_eq!(app.rendered_events_start, 0);
        assert!(!app.render_in_flight);
        assert_eq!(app.session_viewport_scroll, usize::MAX);
        assert_eq!(
            app.rendered_lines_cache,
            vec![Line::from("old rendered line")]
        );

        let _ = std::fs::remove_file(&session_path);
    }

    /// Active Codex full reparses keep the already-stored session history in
    /// front of the current JSONL turn.
    #[test]
    fn poll_session_file_active_codex_preserves_store_prefix_after_full_reparse() {
        use std::io::Write;

        let mut app = App::new();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let wt_dir = tempfile::tempdir().unwrap();
        let wt_path = wt_dir.path().to_path_buf();
        let branch = "codex".to_string();
        let store = crate::app::session_store::SessionStore::open(&wt_path).unwrap();
        let sid = store.create_session(&branch).unwrap();
        store
            .append_events(
                sid,
                &[
                    DisplayEvent::UserMessage {
                        _uuid: String::new(),
                        content: "original request".into(),
                    },
                    DisplayEvent::AssistantText {
                        _uuid: String::new(),
                        _message_id: String::new(),
                        text: "prior assistant work".into(),
                    },
                ],
            )
            .unwrap();

        let session_dir = dirs::home_dir()
            .unwrap()
            .join(".codex")
            .join("sessions")
            .join("2099")
            .join("12")
            .join("31");
        std::fs::create_dir_all(&session_dir).unwrap();
        let session_path = session_dir.join(format!(
            "rollout-live-codex-prefix-{}-{}.jsonl",
            std::process::id(),
            unique
        ));
        let mut file = std::fs::File::create(&session_path).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "session_meta",
                "timestamp": "2026-01-01T00:00:00Z",
                "payload": {
                    "id": format!("live-codex-prefix-{}", unique),
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
                    "type": "message",
                    "role": "user",
                    "content": crate::app::context_injection::AUTO_CONTINUE_PROMPT,
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
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type":"output_text","text":"continued after compaction"}],
                }
            })
        )
        .unwrap();

        app.worktrees.push(crate::models::Worktree {
            branch_name: branch.clone(),
            worktree_path: Some(wt_path.clone()),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.active_slot.insert(branch, "55".into());
        app.running_sessions.insert("55".into());
        app.session_store = Some(store);
        app.session_store_path = Some(wt_path);
        app.current_session_id = Some(sid);
        app.session_file_path = Some(session_path.clone());
        app.session_file_dirty = true;
        app.session_file_parse_offset = 999;
        app.display_events = vec![
            DisplayEvent::UserMessage {
                _uuid: String::new(),
                content: "original request".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text: "prior assistant work".into(),
            },
        ];

        assert!(app.poll_session_file());

        assert_eq!(app.display_events.len(), 3);
        assert!(matches!(
            &app.display_events[0],
            DisplayEvent::UserMessage { content, .. } if content == "original request"
        ));
        assert!(matches!(
            &app.display_events[1],
            DisplayEvent::AssistantText { text, .. } if text == "prior assistant work"
        ));
        assert!(matches!(
            &app.display_events[2],
            DisplayEvent::AssistantText { text, .. } if text == "continued after compaction"
        ));
        assert!(!app.display_events.iter().any(|event| matches!(
            event,
            DisplayEvent::UserMessage { content, .. }
                if content.contains("azureal-internal-auto-continue")
                    || content == "Continue."
        )));

        let _ = std::fs::remove_file(&session_path);
    }

    /// Active Codex full reparses preserve the optimistic prompt bubble until
    /// the session file confirms the visible user message.
    #[test]
    fn poll_session_file_active_codex_preserves_pending_user_prompt_after_full_reparse() {
        use std::io::Write;

        let mut app = App::new();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let wt_dir = tempfile::tempdir().unwrap();
        let wt_path = wt_dir.path().to_path_buf();
        let branch = "codex".to_string();
        let store = crate::app::session_store::SessionStore::open(&wt_path).unwrap();
        let sid = store.create_session(&branch).unwrap();
        store
            .append_events(
                sid,
                &[
                    DisplayEvent::UserMessage {
                        _uuid: String::new(),
                        content: "next request".into(),
                    },
                    DisplayEvent::AssistantText {
                        _uuid: String::new(),
                        _message_id: String::new(),
                        text: "prior assistant work".into(),
                    },
                ],
            )
            .unwrap();

        let session_dir = dirs::home_dir()
            .unwrap()
            .join(".codex")
            .join("sessions")
            .join("2099")
            .join("12")
            .join("31");
        std::fs::create_dir_all(&session_dir).unwrap();
        let session_path = session_dir.join(format!(
            "rollout-live-codex-pending-{}-{}.jsonl",
            std::process::id(),
            unique
        ));
        let mut file = std::fs::File::create(&session_path).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "session_meta",
                "timestamp": "2026-01-01T00:00:00Z",
                "payload": {
                    "id": format!("live-codex-pending-{}", unique),
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
                "timestamp": "2026-01-01T00:00:02Z",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type":"output_text","text":"working on current prompt"}],
                }
            })
        )
        .unwrap();

        app.worktrees.push(crate::models::Worktree {
            branch_name: branch.clone(),
            worktree_path: Some(wt_path.clone()),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.active_slot.insert(branch, "55".into());
        app.running_sessions.insert("55".into());
        app.pid_session_target
            .insert("55".into(), (sid, wt_path.clone(), 2, 0));
        app.session_store = Some(store);
        app.session_store_path = Some(wt_path);
        app.current_session_id = Some(sid);
        app.session_file_path = Some(session_path.clone());
        app.session_file_dirty = true;
        app.session_file_parse_offset = 999;
        app.pending_user_message = Some("next request".into());
        app.display_events = vec![
            DisplayEvent::UserMessage {
                _uuid: String::new(),
                content: "next request".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text: "prior assistant work".into(),
            },
            DisplayEvent::UserMessage {
                _uuid: String::new(),
                content: "next request".into(),
            },
        ];

        assert!(app.poll_session_file());

        assert_eq!(app.display_events.len(), 4);
        assert!(matches!(
            &app.display_events[0],
            DisplayEvent::UserMessage { content, .. } if content == "next request"
        ));
        assert!(matches!(
            &app.display_events[1],
            DisplayEvent::AssistantText { text, .. } if text == "prior assistant work"
        ));
        assert!(matches!(
            &app.display_events[2],
            DisplayEvent::UserMessage { content, .. } if content == "next request"
        ));
        assert!(matches!(
            &app.display_events[3],
            DisplayEvent::AssistantText { text, .. } if text == "working on current prompt"
        ));
        assert_eq!(app.pending_user_message, Some("next request".into()));

        let _ = std::fs::remove_file(&session_path);
    }

    /// Active Codex full reparses keep live pre-turn events ahead of the new
    /// optimistic prompt when the SQLite store has not caught up yet.
    #[test]
    fn poll_session_file_active_codex_preserves_live_prefix_before_pending_prompt() {
        use std::io::Write;

        let mut app = App::new();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let wt_dir = tempfile::tempdir().unwrap();
        let wt_path = wt_dir.path().to_path_buf();
        let branch = "codex".to_string();
        let store = crate::app::session_store::SessionStore::open(&wt_path).unwrap();
        let sid = store.create_session(&branch).unwrap();
        store
            .append_events(
                sid,
                &[
                    DisplayEvent::UserMessage {
                        _uuid: String::new(),
                        content: "stored request".into(),
                    },
                    DisplayEvent::AssistantText {
                        _uuid: String::new(),
                        _message_id: String::new(),
                        text: "stored assistant".into(),
                    },
                ],
            )
            .unwrap();

        let session_dir = dirs::home_dir()
            .unwrap()
            .join(".codex")
            .join("sessions")
            .join("2099")
            .join("12")
            .join("31");
        std::fs::create_dir_all(&session_dir).unwrap();
        let session_path = session_dir.join(format!(
            "rollout-live-codex-prefix-gap-{}-{}.jsonl",
            std::process::id(),
            unique
        ));
        let mut file = std::fs::File::create(&session_path).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "session_meta",
                "timestamp": "2026-01-01T00:00:00Z",
                "payload": {
                    "id": format!("live-codex-prefix-gap-{}", unique),
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
                "timestamp": "2026-01-01T00:00:02Z",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type":"output_text","text":"working on current prompt"}],
                }
            })
        )
        .unwrap();

        app.worktrees.push(crate::models::Worktree {
            branch_name: branch.clone(),
            worktree_path: Some(wt_path.clone()),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.active_slot.insert(branch, "55".into());
        app.running_sessions.insert("55".into());
        app.pid_session_target
            .insert("55".into(), (sid, wt_path.clone(), 2, 0));
        app.session_store = Some(store);
        app.session_store_path = Some(wt_path);
        app.current_session_id = Some(sid);
        app.session_file_path = Some(session_path.clone());
        app.session_file_dirty = true;
        app.session_file_parse_offset = 999;
        app.pending_user_message = Some("current request".into());
        app.display_events = vec![
            DisplayEvent::UserMessage {
                _uuid: String::new(),
                content: "stored request".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text: "stored assistant".into(),
            },
            DisplayEvent::UserMessage {
                _uuid: String::new(),
                content: "previous live request".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text: "previous live assistant".into(),
            },
            DisplayEvent::UserMessage {
                _uuid: String::new(),
                content: "current request".into(),
            },
        ];

        assert!(app.poll_session_file());

        let labels: Vec<&str> = app
            .display_events
            .iter()
            .filter_map(|event| match event {
                DisplayEvent::UserMessage { content, .. } => Some(content.as_str()),
                DisplayEvent::AssistantText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            labels,
            vec![
                "stored request",
                "stored assistant",
                "previous live request",
                "previous live assistant",
                "current request",
                "working on current prompt",
            ]
        );

        let _ = std::fs::remove_file(&session_path);
    }
}
