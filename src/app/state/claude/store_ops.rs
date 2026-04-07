//! Session store persistence operations
//!
//! Handles appending agent events to the SQLite session store after a turn
//! completes. Three paths exist depending on the slot's relationship to the
//! current view:
//! - **Viewed slot** (`store_append_from_display`): early store from live `display_events`
//! - **Exited slot** (`store_append_from_jsonl`): post-exit store with JSONL deletion
//! - **Background project** (`store_append_background`): non-active project store

use crate::backend::Backend;

use crate::app::state::App;

impl App {
    fn tool_status_from_events(
        events: &[crate::events::DisplayEvent],
    ) -> (
        std::collections::HashSet<String>,
        std::collections::HashSet<String>,
    ) {
        let mut pending = std::collections::HashSet::new();
        let mut failed = std::collections::HashSet::new();
        for event in events {
            match event {
                crate::events::DisplayEvent::ToolCall { tool_use_id, .. } => {
                    pending.insert(tool_use_id.clone());
                }
                crate::events::DisplayEvent::ToolResult {
                    tool_use_id,
                    is_error,
                    ..
                } => {
                    pending.remove(tool_use_id);
                    if *is_error {
                        failed.insert(tool_use_id.clone());
                    }
                }
                _ => {}
            }
        }
        (pending, failed)
    }

    /// Store a running slot's display events early (before exit), e.g. when
    /// a new prompt supersedes a still-running process. Removes the slot from
    /// pid_session_target so the exit handler doesn't double-store.
    pub fn store_append_from_display(&mut self, slot_id: &str) {
        let (session_id, wt_path, events_offset, _) = match self.pid_session_target.remove(slot_id)
        {
            Some(triple) => triple,
            None => return,
        };
        let end = self.display_events.len();
        if events_offset >= end {
            return;
        }
        let events = self.display_events[events_offset..end].to_vec();

        let store = if self.session_store_path.as_ref().map(|p| p.as_path()) == Some(&wt_path) {
            self.session_store.as_ref()
        } else {
            None
        };
        let temp_store;
        let store = match store {
            Some(s) => s,
            None => {
                temp_store = crate::app::session_store::SessionStore::open(&wt_path).ok();
                match temp_store.as_ref() {
                    Some(s) => s,
                    None => return,
                }
            }
        };
        let _ = store.append_events(session_id, &events);
    }

    /// Store the current turn's display events into the SQLite session store.
    /// When the slot is currently being viewed, uses live display_events.
    /// When the user switched to a different worktree, falls back to parsing
    /// the JSONL file from disk (display_events belongs to the other worktree).
    /// Deletes the source JSONL after successful ingestion.
    pub(crate) fn store_append_from_jsonl(&mut self, slot_id: &str, turn_backend: Backend) {
        // Only process if this slot was targeting a store session
        let (session_id, wt_path, events_offset, session_file_offset) =
            match self.pid_session_target.remove(slot_id) {
                Some(triple) => triple,
                None => return,
            };

        // Resolve JSONL file path for deletion
        let jsonl_path = self
            .agent_session_ids
            .get(slot_id)
            .and_then(|uuid| crate::config::session_file(&wt_path, uuid));

        // When the slot isn't being viewed, display_events belongs to a
        // different worktree — reading display_events[events_offset..] would
        // either corrupt the store (wrong events) or produce empty results
        // (offset past end). Fall back to parsing the JSONL file directly.
        let slot_owns_display = self.is_viewing_slot(slot_id);

        let mut events: Vec<crate::events::DisplayEvent> = if slot_owns_display {
            // Viewed slot — use live display_events
            if events_offset < self.display_events.len() {
                self.display_events[events_offset..].to_vec()
            } else {
                Vec::new()
            }
        } else {
            // Non-viewed slot — parse JSONL file from disk
            Self::parse_jsonl_for_store(
                &jsonl_path,
                turn_backend,
                session_file_offset,
                &wt_path,
                session_id,
            )
        };

        if events.is_empty() {
            // Still delete the JSONL (and companion dir) even if no events to store
            if let Some(p) = jsonl_path.filter(|p| p.exists()) {
                crate::config::remove_session_file(&p);
            }
            return;
        }
        if slot_owns_display && turn_backend == Backend::Codex {
            if let Some(ref path) = jsonl_path {
                if path.exists() && events_offset <= self.display_events.len() {
                    let prefix_events = &self.display_events[..events_offset];
                    let (prefix_pending, prefix_failed) =
                        Self::tool_status_from_events(prefix_events);
                    let parsed =
                        crate::app::codex_session_parser::parse_codex_session_file_incremental(
                            path,
                            session_file_offset,
                            prefix_events,
                            &prefix_pending,
                            &prefix_failed,
                        );
                    if parsed.events.len() >= prefix_events.len() {
                        events = parsed.events[events_offset..].to_vec();
                        if self.session_file_path.as_ref() == Some(path) {
                            self.display_events = parsed.events;
                            self.pending_tool_calls = parsed.pending_tools;
                            self.failed_tool_calls = parsed.failed_tools;
                            self.invalidate_render_cache();
                        }
                    }
                }
            }
        }

        // Open store at the target worktree path (may differ from current worktree
        // if the user switched away while the process was running)
        let store = if self.session_store_path.as_ref().map(|p| p.as_path()) == Some(&wt_path) {
            self.session_store.as_ref()
        } else {
            None
        };
        // Use current store if it matches, otherwise open a temporary one
        let temp_store;
        let store = match store {
            Some(s) => s,
            None => {
                temp_store = crate::app::session_store::SessionStore::open(&wt_path).ok();
                match temp_store.as_ref() {
                    Some(s) => s,
                    None => return,
                }
            }
        };

        if store.append_events(session_id, &events).is_ok() {
            // Clear the persisted UUID — ingestion complete, no recovery needed
            let _ = store.clear_session_uuid(session_id);

            // Source JSONL ingested — delete the original file and companion dir
            if let Some(ref p) = jsonl_path {
                if p.exists() {
                    crate::config::remove_session_file(p);
                }
                // Clear JSONL tracking so poll_session_file doesn't try to read the deleted file
                if self.session_file_path.as_ref() == Some(p) {
                    self.session_file_path = None;
                    self.session_file_dirty = false;
                }
            }

            // New events stored (may include user messages) — retry deferred
            // compaction spawns since a valid boundary may now exist.
            self.compaction_spawn_deferred = false;
            // Check if compaction is needed (only if not already pending or in-flight)
            if self.compaction_needed.is_none() && self.compaction_receivers.is_empty() {
                if let Ok(chars) = store.total_chars_since_compaction(session_id) {
                    if chars >= crate::app::session_store::COMPACTION_THRESHOLD {
                        self.compaction_needed = Some((session_id, wt_path));
                    }
                }
            }
            if self.current_session_id == Some(session_id) {
                // Update context percentage badge from store character count
                self.update_token_badge();
            }
        }
    }

    /// Parse JSONL and append to store for a background (non-active) project.
    /// Opens a temporary store connection to the worktree's .azs file.
    pub(crate) fn store_append_background(
        &self,
        slot_id: &str,
        session_id: i64,
        wt_path: &std::path::Path,
        _project_path: &std::path::Path,
        session_file_offset: u64,
    ) {
        let (session_backend, jsonl_path) = match self.agent_session_ids.get(slot_id) {
            Some(uuid) => match crate::config::session_file_with_backend(wt_path, uuid) {
                Some(pair) => pair,
                None => return,
            },
            None => return,
        };
        if !jsonl_path.exists() {
            return;
        };

        if let Ok(store) = crate::app::session_store::SessionStore::open(wt_path) {
            let events: Vec<crate::events::DisplayEvent> = match session_backend {
                crate::backend::Backend::Claude => {
                    let parsed = crate::app::session_parser::parse_session_file(&jsonl_path);
                    if parsed.events.is_empty() {
                        return;
                    }
                    parsed
                        .events
                        .into_iter()
                        .map(|ev| match ev {
                            crate::events::DisplayEvent::UserMessage { _uuid, content } => {
                                let stripped =
                                    crate::app::context_injection::strip_injected_context(&content);
                                crate::events::DisplayEvent::UserMessage {
                                    _uuid,
                                    content: stripped.to_string(),
                                }
                            }
                            other => other,
                        })
                        .collect()
                }
                crate::backend::Backend::Codex => {
                    let existing_events = store.load_events(session_id).unwrap_or_default();
                    let (existing_pending, existing_failed) =
                        Self::tool_status_from_events(&existing_events);
                    let parsed =
                        crate::app::codex_session_parser::parse_codex_session_file_incremental(
                            &jsonl_path,
                            session_file_offset,
                            &existing_events,
                            &existing_pending,
                            &existing_failed,
                        );
                    if parsed.events.len() < existing_events.len() {
                        return;
                    }
                    parsed.events[existing_events.len()..].to_vec()
                }
            };
            if events.is_empty() {
                return;
            }
            if store.append_events(session_id, &events).is_ok() {
                // Source JSONL ingested — delete the original file and companion dir
                crate::config::remove_session_file(&jsonl_path);
            }
        }
    }

    /// Parse a JSONL file directly for store ingestion (non-viewed slot path).
    /// Used when the agent exits while the user is viewing a different worktree,
    /// so display_events cannot be used (it belongs to the viewed worktree).
    fn parse_jsonl_for_store(
        jsonl_path: &Option<std::path::PathBuf>,
        turn_backend: Backend,
        session_file_offset: u64,
        wt_path: &std::path::Path,
        session_id: i64,
    ) -> Vec<crate::events::DisplayEvent> {
        let Some(ref path) = jsonl_path else {
            return Vec::new();
        };
        if !path.exists() {
            return Vec::new();
        }
        match turn_backend {
            Backend::Claude => {
                let parsed = crate::app::session_parser::parse_session_file(path);
                // Strip injected context from user messages (same as
                // store_append_background and recover_orphaned_jsonls).
                parsed
                    .events
                    .into_iter()
                    .map(|ev| match ev {
                        crate::events::DisplayEvent::UserMessage { _uuid, content } => {
                            let stripped =
                                crate::app::context_injection::strip_injected_context(&content);
                            crate::events::DisplayEvent::UserMessage {
                                _uuid,
                                content: stripped.to_string(),
                            }
                        }
                        other => other,
                    })
                    .collect()
            }
            Backend::Codex => {
                // Load prior-turn events from the store for Codex prefix context
                let prefix_events = crate::app::session_store::SessionStore::open(wt_path)
                    .ok()
                    .and_then(|s| s.load_events(session_id).ok())
                    .unwrap_or_default();
                let (prefix_pending, prefix_failed) = Self::tool_status_from_events(&prefix_events);
                let parsed = crate::app::codex_session_parser::parse_codex_session_file_incremental(
                    path,
                    session_file_offset,
                    &prefix_events,
                    &prefix_pending,
                    &prefix_failed,
                );
                let offset = prefix_events.len();
                if parsed.events.len() > offset {
                    parsed.events[offset..].to_vec()
                } else {
                    Vec::new()
                }
            }
        }
    }
}
