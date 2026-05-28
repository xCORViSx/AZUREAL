//! Session store persistence operations
//!
//! Handles appending agent events to the SQLite session store after a turn
//! completes. Three paths exist depending on the slot's relationship to the
//! current view:
//! - **Viewed slot** (`store_append_from_display`): early store from live `display_events`
//! - **Exited slot** (`store_append_from_jsonl`): post-exit store with JSONL deletion
//! - **Background project** (`store_append_background`): non-active project store

use crate::app::state::App;
use crate::backend::Backend;
use crate::events::DisplayEvent;

fn has_completion(events: &[DisplayEvent]) -> bool {
    events
        .iter()
        .any(|event| matches!(event, DisplayEvent::Complete { .. }))
}

fn last_completion(events: &[DisplayEvent]) -> Option<DisplayEvent> {
    events
        .iter()
        .rev()
        .find(|event| matches!(event, DisplayEvent::Complete { .. }))
        .cloned()
}

fn unique_message_chars(events: &[DisplayEvent]) -> usize {
    let mut seen = std::collections::HashSet::new();
    let mut chars = 0usize;

    for event in events {
        let key = match event {
            DisplayEvent::UserMessage { content, .. } => Some(("user", content.as_str())),
            DisplayEvent::AssistantText { text, .. } => Some(("assistant", text.as_str())),
            _ => None,
        };
        if let Some((role, text)) = key {
            if seen.insert((role, text)) {
                chars += text.len();
            }
        }
    }

    chars
}

fn with_completion_from(
    mut events: Vec<DisplayEvent>,
    completion_source: &[DisplayEvent],
) -> Vec<DisplayEvent> {
    if !has_completion(&events) {
        if let Some(completion) = last_completion(completion_source) {
            events.push(completion);
        }
    }
    events
}

/// Prefer the parsed JSONL events because they contain the richest tool data,
/// but never let them replace live/cache events that contain user or assistant
/// text the parser failed to recover. This protects the already-visible turn
/// before the source JSONL is deleted.
fn choose_store_events(
    parsed_events: Vec<DisplayEvent>,
    live_events: Vec<DisplayEvent>,
) -> Vec<DisplayEvent> {
    if parsed_events.is_empty() {
        return live_events;
    }
    if live_events.is_empty() {
        return parsed_events;
    }

    let parsed_message_chars = unique_message_chars(&parsed_events);
    let live_message_chars = unique_message_chars(&live_events);
    if live_message_chars > parsed_message_chars {
        return with_completion_from(live_events, &parsed_events);
    }

    let parsed_has_completion = has_completion(&parsed_events);
    let live_has_completion = has_completion(&live_events);
    if live_has_completion && !parsed_has_completion && live_events.len() >= parsed_events.len() {
        return live_events;
    }
    if !parsed_has_completion && live_events.len() > parsed_events.len() {
        return live_events;
    }

    parsed_events
}

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

        let cached_live_branch = self.branch_for_slot(slot_id).or_else(|| {
            self.worktrees
                .iter()
                .chain(self.main_worktree.iter())
                .find(|wt| wt.worktree_path.as_deref() == Some(wt_path.as_path()))
                .map(|wt| wt.branch_name.clone())
        });
        let cached_live_events = cached_live_branch
            .as_ref()
            .and_then(|branch| self.live_display_events_cache.get(branch).cloned())
            .map(crate::app::context_injection::strip_injected_context_from_events);

        // When the slot isn't being viewed, display_events belongs to a
        // different worktree — reading display_events[events_offset..] would
        // either corrupt the store (wrong events) or produce empty results
        // (offset past end). Fall back to parsing the JSONL file directly.
        let slot_owns_display = self.is_viewing_slot(slot_id)
            || (self.current_session_id == Some(session_id)
                && self.session_store_path.as_ref().map(|p| p.as_path()) == Some(&wt_path));

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

        if !slot_owns_display {
            if let Some(cached_events) = cached_live_events {
                if events_offset <= cached_events.len() {
                    let cached_suffix: Vec<_> =
                        cached_events.into_iter().skip(events_offset).collect();
                    if !cached_suffix.is_empty() {
                        events = choose_store_events(events, cached_suffix);
                    }
                }
            }
        }

        if slot_owns_display && turn_backend == Backend::Claude {
            if let Some(ref path) = jsonl_path {
                if path.exists() && events_offset <= self.display_events.len() {
                    let prefix_events: Vec<_> = self.display_events[..events_offset].to_vec();
                    let parsed = crate::app::session_parser::parse_session_file(path);
                    let parsed_events =
                        crate::app::context_injection::strip_injected_context_from_events(
                            parsed.events,
                        );
                    let overlap = crate::app::session_store::overlap_prefix_len(
                        &prefix_events,
                        &parsed_events,
                    );
                    let parsed_suffix: Vec<_> = parsed_events.into_iter().skip(overlap).collect();
                    if !parsed_suffix.is_empty() {
                        events = choose_store_events(parsed_suffix, events);
                    }
                    if !events.is_empty() && self.session_file_path.as_ref() == Some(path) {
                        self.display_events = prefix_events;
                        self.display_events.extend(events.clone());
                        let (pending, failed) = Self::tool_status_from_events(&self.display_events);
                        self.pending_tool_calls = pending;
                        self.failed_tool_calls = failed;
                        self.invalidate_render_cache();
                    }
                }
            }
        } else if slot_owns_display && turn_backend == Backend::Codex {
            if let Some(ref path) = jsonl_path {
                if path.exists() && events_offset <= self.display_events.len() {
                    let prefix_events: Vec<_> = self.display_events[..events_offset].to_vec();
                    let parsed = crate::app::codex_session_parser::parse_codex_session_file(path);
                    let parsed_events =
                        crate::app::context_injection::strip_injected_context_from_events(
                            parsed.events,
                        );
                    let overlap = crate::app::session_store::overlap_prefix_len(
                        &prefix_events,
                        &parsed_events,
                    );
                    let parsed_suffix: Vec<_> = parsed_events.into_iter().skip(overlap).collect();
                    if !parsed_suffix.is_empty() {
                        events = choose_store_events(parsed_suffix, events);
                    }
                    if !events.is_empty() && self.session_file_path.as_ref() == Some(path) {
                        self.display_events = prefix_events;
                        self.display_events.extend(events.clone());
                        let (pending, failed) = Self::tool_status_from_events(&self.display_events);
                        self.pending_tool_calls = pending;
                        self.failed_tool_calls = failed;
                        self.invalidate_render_cache();
                    }
                }
            }
        }

        if events.is_empty() {
            // Still delete the JSONL (and companion dir) even if no events to store
            if let Some(p) = jsonl_path.filter(|p| p.exists()) {
                crate::config::remove_session_file(&p);
            }
            return;
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
            if let Some(branch) = cached_live_branch {
                self.live_display_events_cache.remove(&branch);
            }
        }
    }

    /// Parse JSONL and append to store for a background (non-active) project.
    /// Opens a temporary store connection to the worktree's .azs file.
    pub(crate) fn store_append_background(
        &mut self,
        slot_id: &str,
        session_id: i64,
        wt_path: &std::path::Path,
        project_path: &std::path::Path,
        cache_branch: Option<&str>,
        _session_file_offset: u64,
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
            let existing_events = store.load_events(session_id).unwrap_or_default();
            let mut events: Vec<crate::events::DisplayEvent> = match session_backend {
                crate::backend::Backend::Claude => {
                    let parsed = crate::app::session_parser::parse_session_file(&jsonl_path);
                    crate::app::context_injection::strip_injected_context_from_events(parsed.events)
                }
                crate::backend::Backend::Codex => {
                    let parsed =
                        crate::app::codex_session_parser::parse_codex_session_file(&jsonl_path);
                    let parsed_events =
                        crate::app::context_injection::strip_injected_context_from_events(
                            parsed.events,
                        );
                    let overlap = crate::app::session_store::overlap_prefix_len(
                        &existing_events,
                        &parsed_events,
                    );
                    parsed_events.into_iter().skip(overlap).collect()
                }
            };
            if let Some(cached_events) = cache_branch
                .and_then(|branch| {
                    self.project_snapshots
                        .get(project_path)
                        .and_then(|snapshot| snapshot.live_display_events_cache.get(branch))
                })
                .cloned()
                .map(crate::app::context_injection::strip_injected_context_from_events)
            {
                let overlap =
                    crate::app::session_store::overlap_prefix_len(&existing_events, &cached_events);
                let cached_suffix: Vec<_> = cached_events.into_iter().skip(overlap).collect();
                if !cached_suffix.is_empty() {
                    events = choose_store_events(events, cached_suffix);
                }
            }
            if events.is_empty() {
                return;
            }
            if store.append_events(session_id, &events).is_ok() {
                // Source JSONL ingested — delete the original file and companion dir
                crate::config::remove_session_file(&jsonl_path);
                if let Some(branch) = cache_branch {
                    if let Some(snapshot) = self.project_snapshots.get_mut(project_path) {
                        snapshot.live_display_events_cache.remove(branch);
                    }
                }
            }
        }
    }

    /// Parse a JSONL file directly for store ingestion (non-viewed slot path).
    /// Used when the agent exits while the user is viewing a different worktree,
    /// so display_events cannot be used (it belongs to the viewed worktree).
    fn parse_jsonl_for_store(
        jsonl_path: &Option<std::path::PathBuf>,
        turn_backend: Backend,
        _session_file_offset: u64,
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
                crate::app::context_injection::strip_injected_context_from_events(parsed.events)
            }
            Backend::Codex => {
                // Load prior-turn events from the store for prefix overlap.
                // Codex session files are per-rollout and the saved byte offset
                // can belong to a previous deleted JSONL, so parse the file from
                // byte 0 and dedupe against SQLite instead of seeking.
                let prefix_events = crate::app::session_store::SessionStore::open(wt_path)
                    .ok()
                    .and_then(|s| s.load_events(session_id).ok())
                    .unwrap_or_default();
                let parsed = crate::app::codex_session_parser::parse_codex_session_file(path);
                let parsed_events =
                    crate::app::context_injection::strip_injected_context_from_events(
                        parsed.events,
                    );
                let overlap =
                    crate::app::session_store::overlap_prefix_len(&prefix_events, &parsed_events);
                parsed_events.into_iter().skip(overlap).collect()
            }
        }
    }
}
