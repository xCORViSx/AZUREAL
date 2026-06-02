//! Session file monitoring and incremental parsing

use super::super::App;
use crate::backend::Backend;
use crate::events::DisplayEvent;

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
            parsed.events = self.merge_store_prefix_for_current_session(parsed.events);
        }
        // Guard: if the parse returned empty events but we already had content,
        // the file was likely temporarily unavailable (locked, atomic rewrite,
        // or deleted during Claude Code compaction). Preserve existing display
        // rather than wiping the session pane. The next poll will retry.
        if parsed.events.is_empty() && !self.display_events.is_empty() && parsed.end_offset == 0 {
            return;
        }
        self.display_events = parsed.events;
        // Full re-parse replaced ALL display_events — reset render counters so the
        // incremental render path doesn't use stale counts that reference the old
        // event array. Without this, submit_render_request can try to slice events
        // at the old rendered_events_count, producing garbled or missing output.
        if was_full_reparse {
            self.rendered_events_count = 0;
            self.rendered_content_line_count = 0;
            self.rendered_events_start = 0;
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

        self.invalidate_render_cache();

        // Activity detected from session file — reset compaction inactivity watcher
        self.last_session_event_time = std::time::Instant::now();
        self.compaction_banner_injected = false;

        // If we were following bottom, stay at bottom after content update
        if was_at_bottom {
            self.session_scroll = usize::MAX;
        }
    }

    fn merge_store_prefix_for_current_session(
        &self,
        parsed_events: Vec<DisplayEvent>,
    ) -> Vec<DisplayEvent> {
        let Some(session_id) = self.current_session_id else {
            return parsed_events;
        };
        let Some(store) = self.session_store.as_ref() else {
            return parsed_events;
        };
        let Ok(store_events) = store.load_events(session_id) else {
            return parsed_events;
        };
        if store_events.is_empty() {
            return parsed_events;
        }

        let overlap = crate::app::session_store::overlap_prefix_len(&store_events, &parsed_events);
        let mut merged = store_events;
        merged.extend(parsed_events.into_iter().skip(overlap));
        merged
    }
}

#[cfg(test)]
mod tests {
    use crate::app::state::app::App;
    use crate::events::DisplayEvent;
    use std::path::PathBuf;

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
}
