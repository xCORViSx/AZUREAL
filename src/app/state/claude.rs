//! Claude session handling and event processing

use std::sync::mpsc::Receiver;

use crate::app::util::display_text_from_json;
use crate::claude::AgentEvent;
use crate::events::DisplayEvent;
use crate::models::OutputType;

use super::App;

impl App {
    /// Check if a slot's output should be displayed (active slot of viewed branch)
    pub fn is_viewing_slot(&self, slot_id: &str) -> bool {
        let is_rcr_slot = self.rcr_session.as_ref().map(|r| r.slot_id == slot_id).unwrap_or(false);
        !self.viewing_historic_session && (is_rcr_slot || self.current_worktree().map(|s| {
            self.active_slot.get(&s.branch_name).map(|a| a == slot_id).unwrap_or(false)
        }).unwrap_or(false))
    }

    /// Apply pre-parsed Claude output to app state. Called with results from
    /// the background AgentProcessor thread — all JSON parsing already done.
    pub fn apply_parsed_output(
        &mut self,
        events: Vec<DisplayEvent>,
        parsed_json: Option<serde_json::Value>,
        output_type: OutputType,
        data: &str,
    ) {
        for event in &events {
            match event {
                DisplayEvent::ToolCall { tool_use_id, tool_name, input, .. } => {
                    self.pending_tool_calls.insert(tool_use_id.clone());
                    self.tool_status_generation += 1;
                    if tool_name == "Task" {
                        if self.active_task_tool_ids.is_empty() {
                            self.subagent_parent_idx = self.current_todos.iter()
                                .position(|t| t.status == crate::app::TodoStatus::InProgress);
                        }
                        self.active_task_tool_ids.insert(tool_use_id.clone());
                    }
                    if tool_name == "TodoWrite" {
                        if self.active_task_tool_ids.is_empty() {
                            self.current_todos = parse_todos_from_input(input);
                            self.todo_scroll = 0;
                        } else {
                            self.subagent_todos = parse_todos_from_input(input);
                            self.todo_scroll = 0;
                        }
                    }
                    if tool_name == "AskUserQuestion" {
                        self.awaiting_ask_user_question = true;
                        self.ask_user_questions_cache = Some(input.clone());
                    }
                }
                DisplayEvent::ToolResult { tool_use_id, is_error, .. } => {
                    self.pending_tool_calls.remove(tool_use_id);
                    self.tool_status_generation += 1;
                    if self.active_task_tool_ids.remove(tool_use_id) && self.active_task_tool_ids.is_empty() {
                        self.subagent_todos.clear();
                        self.subagent_parent_idx = None;
                    }
                    if *is_error {
                        self.failed_tool_calls.insert(tool_use_id.clone());
                        self.tool_status_generation += 1;
                    }
                }
                _ => {}
            }
        }

        // Extract detected model from assistant events (for display, not badge)
        if let Some(ref json) = parsed_json {
            if let Some("assistant") = json.get("type").and_then(|t| t.as_str()) {
                if let Some(model) = json.get("message")
                    .and_then(|m| m.get("model"))
                    .and_then(|m| m.as_str())
                {
                    self.detected_model = Some(model.to_string());
                }
            }
        }

        if !events.is_empty() {
            self.display_events.extend(events);
            self.invalidate_render_cache();
            self.last_session_event_time = std::time::Instant::now();
            self.compaction_banner_injected = false;
        }

        if self.rendered_lines_cache.is_empty() {
            if let Some(json) = parsed_json {
                if let Some(display_text) = display_text_from_json(&json) {
                    self.process_session_chunk(&display_text);
                }
            } else if output_type != OutputType::Stdout && output_type != OutputType::Json {
                self.process_session_chunk(data);
            }
        }
    }

    /// Called when a Claude process emits Started { pid }. The slot_id IS the
    /// PID string, already registered in register_claude() — this just confirms
    /// the process is alive and clears stale exit codes.
    pub fn handle_claude_started(&mut self, slot_id: &str, _pid: u32) {
        self.running_sessions.insert(slot_id.to_string());
        self.agent_exit_codes.remove(slot_id);
        self.invalidate_sidebar();
        let branch = self.branch_for_slot(slot_id).unwrap_or_else(|| slot_id.to_string());
        self.set_status(format!("Claude started in {}", branch));
    }

    /// Called when a Claude process exits. Cleans up slot state, switches active
    /// slot if needed, and triggers session file re-parse.
    pub fn handle_claude_exited(&mut self, slot_id: &str, code: Option<i32>) {
        // Resolve branch — first in current project, then in background snapshots
        let branch = self.branch_for_slot(slot_id);

        // If not in current project, check background project snapshots
        if branch.is_none() {
            if self.handle_background_exit(slot_id, code) {
                return;
            }
        }

        // Send macOS notification before cleaning up state
        if let Some(ref branch) = branch {
            self.send_completion_notification(branch, slot_id, code);
        }

        // Remove slot from all process-tracking maps
        self.running_sessions.remove(slot_id);
        self.agent_receivers.remove(slot_id);
        self.slot_to_project.remove(slot_id);
        if let Some(c) = code {
            self.agent_exit_codes.insert(slot_id.to_string(), c);
        }

        // Remove slot from its branch's slot list
        if let Some(ref branch) = branch {
            if let Some(slots) = self.branch_slots.get_mut(branch) {
                slots.retain(|s| s != slot_id);
                if slots.is_empty() { self.branch_slots.remove(branch); }
            }
        }

        // If this was the active slot, switch to next available slot or clear
        let was_active = branch.as_ref().and_then(|b| self.active_slot.get(b))
            .map(|a| a == slot_id).unwrap_or(false);

        if was_active {
            if let Some(ref branch) = branch {
                // Pick another running slot on this branch, or remove active
                let next = self.branch_slots.get(branch)
                    .and_then(|slots| slots.last().cloned());
                match next {
                    Some(next_slot) => { self.active_slot.insert(branch.clone(), next_slot); }
                    None => {
                        // Promote session ID from slot-key to branch-key so the
                        // fallback path in get_claude_session_id() can resume
                        // this conversation on the next prompt.
                        if let Some(sid) = self.agent_session_ids.get(slot_id).cloned() {
                            self.agent_session_ids.insert(branch.clone(), sid);
                        }
                        self.active_slot.remove(branch);
                    }
                }
            }
        }

        self.invalidate_sidebar();

        // RCR exit intercept — when the RCR Claude process exits, show the approval
        // dialog instead of re-parsing (which would clobber the streaming output the
        // user is currently viewing). The session file lives under main's path, not
        // the feature branch's, so a normal re-parse would load the wrong data.
        if let Some(ref mut rcr) = self.rcr_session {
            if rcr.slot_id == slot_id {
                rcr.approval_pending = true;
                let display = branch.as_deref().unwrap_or(slot_id);
                let exit_str = match code {
                    Some(0) => "finished".to_string(),
                    Some(c) => format!("exited: {}", c),
                    None => "exited".to_string(),
                };
                self.set_status(format!("[RCR] {} — {}", display, exit_str));
                return;
            }
        }

        // Mark as unread if user wasn't watching this session's output
        // (different branch, or same branch but this wasn't the active display slot)
        let is_current = branch.as_ref().and_then(|b| self.current_worktree().map(|s| s.branch_name == *b)).unwrap_or(false);
        if !(is_current && was_active) {
            if let Some(ref b) = branch {
                if let Some(uuid) = self.agent_session_ids.get(slot_id) {
                    self.unread_session_ids.insert(uuid.clone());
                }
                self.unread_sessions.insert(b.clone());
            }
        }

        // Post-exit: mark session file dirty for a final incremental parse
        // to finalize any pending tool calls. The JSONL will be deleted by
        // store_append_from_jsonl shortly after, which clears session_file_path.
        if is_current && was_active && self.session_file_path.is_some() {
            self.session_file_dirty = true;
        }

        // If this was a [NewRunCmd] session, auto-reload runcmds
        if is_current && self.title_session_name.starts_with("[NewRunCmd]") {
            self.load_run_commands();
        }

        // Post-exit store flow: parse JSONL → strip injected context → append to SQLite
        self.store_append_from_jsonl(slot_id);

        // If a staged prompt exists, leave it for the event loop to auto-send.
        if self.staged_prompt.is_some() {
            self.set_status("Sending staged prompt...");
        } else {
            let display = branch.as_deref().unwrap_or(slot_id);
            let exit_str = match code {
                Some(0) => "exited OK".to_string(),
                Some(c) => format!("exited: {}", c),
                None => "exited".to_string(),
            };
            self.set_status(format!("{} {}", display, exit_str));
        }
    }

    /// Store the current turn's display events into the SQLite session store.
    /// Uses the live display_events (which match what the user saw) rather than
    /// re-parsing the JSONL file. Deletes the source JSONL after successful ingestion.
    fn store_append_from_jsonl(&mut self, slot_id: &str) {
        // Only process if this slot was targeting a store session
        let (session_id, wt_path, events_offset) = match self.pid_session_target.remove(slot_id) {
            Some(triple) => triple,
            None => return,
        };

        // Resolve JSONL file path for deletion
        let jsonl_path = self.agent_session_ids.get(slot_id)
            .and_then(|uuid| crate::config::session_file(self.backend, &wt_path, uuid));

        // Collect current turn's events from display_events (after the offset)
        let events: Vec<crate::events::DisplayEvent> = if events_offset < self.display_events.len() {
            self.display_events[events_offset..].to_vec()
        } else {
            Vec::new()
        };

        if events.is_empty() {
            // Still delete the JSONL even if no events to store
            if let Some(p) = jsonl_path.filter(|p| p.exists()) {
                let _ = std::fs::remove_file(&p);
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
            // Source JSONL ingested — delete the original file
            if let Some(ref p) = jsonl_path {
                if p.exists() {
                    let _ = std::fs::remove_file(p);
                }
                // Clear JSONL tracking so poll_session_file doesn't try to read the deleted file
                if self.session_file_path.as_ref() == Some(p) {
                    self.session_file_path = None;
                    self.session_file_dirty = false;
                }
            }

            // Check if compaction is needed (only if not already pending)
            if self.compaction_needed.is_none() {
                if let Ok(chars) = store.total_chars_since_compaction(session_id) {
                    if chars >= crate::app::session_store::COMPACTION_THRESHOLD {
                        self.compaction_needed = Some((session_id, wt_path));
                    }
                }
            }
            // Update context percentage badge from store character count
            self.update_token_badge();
        }
    }

    /// Parse JSONL and append to store for a background (non-active) project.
    /// Opens a temporary store connection to the worktree's .azs file.
    fn store_append_background(&self, slot_id: &str, session_id: i64, wt_path: &std::path::Path, _project_path: &std::path::Path) {
        let jsonl_path = match self.agent_session_ids.get(slot_id) {
            Some(uuid) => crate::config::session_file(self.backend, wt_path, uuid),
            None => return,
        };
        let jsonl_path = match jsonl_path {
            Some(p) if p.exists() => p,
            _ => return,
        };

        let parsed = crate::app::session_parser::parse_session_file(&jsonl_path);
        if parsed.events.is_empty() {
            return;
        }

        let events: Vec<crate::events::DisplayEvent> = parsed.events.into_iter().map(|ev| {
            match ev {
                crate::events::DisplayEvent::UserMessage { _uuid, content } => {
                    let stripped = crate::app::context_injection::strip_injected_context(&content);
                    crate::events::DisplayEvent::UserMessage {
                        _uuid,
                        content: stripped.to_string(),
                    }
                }
                other => other,
            }
        }).collect();

        if let Ok(store) = crate::app::session_store::SessionStore::open(wt_path) {
            if store.append_events(session_id, &events).is_ok() {
                // Source JSONL ingested — delete the original file
                let _ = std::fs::remove_file(&jsonl_path);
            }
        }
    }

    /// Handle a Claude process exit for a background (non-active) project.
    /// Updates the saved snapshot's branch_slots/active_slot/unread state.
    /// Returns true if the slot was found in a background snapshot.
    fn handle_background_exit(&mut self, slot_id: &str, code: Option<i32>) -> bool {
        // Find which snapshot owns this slot
        let project_path = self.slot_to_project.get(slot_id).cloned();
        let project_path = match project_path {
            Some(p) => p,
            None => return false,
        };
        let snapshot = match self.project_snapshots.get_mut(&project_path) {
            Some(s) => s,
            None => return false,
        };

        // Find branch in snapshot
        let branch = snapshot.branch_slots.iter()
            .find(|(_, slots)| slots.contains(&slot_id.to_string()))
            .map(|(b, _)| b.clone());

        // Send notification
        if let Some(ref branch) = branch {
            self.send_completion_notification(branch, slot_id, code);
        }

        // Global cleanup
        self.running_sessions.remove(slot_id);
        self.agent_receivers.remove(slot_id);
        self.slot_to_project.remove(slot_id);
        if let Some(c) = code {
            self.agent_exit_codes.insert(slot_id.to_string(), c);
        }

        // Re-borrow snapshot after self borrows above
        let snapshot = self.project_snapshots.get_mut(&project_path).unwrap();

        // Update snapshot's branch_slots
        let _was_active = if let Some(ref branch) = branch {
            let active = snapshot.active_slot.get(branch).map(|a| a == slot_id).unwrap_or(false);
            if let Some(slots) = snapshot.branch_slots.get_mut(branch) {
                slots.retain(|s| s != slot_id);
                if slots.is_empty() { snapshot.branch_slots.remove(branch); }
            }
            if active {
                let next = snapshot.branch_slots.get(branch).and_then(|s| s.last().cloned());
                match next {
                    Some(next_slot) => { snapshot.active_slot.insert(branch.clone(), next_slot); }
                    None => {
                        if let Some(sid) = self.agent_session_ids.get(slot_id).cloned() {
                            self.agent_session_ids.insert(branch.clone(), sid);
                        }
                        snapshot.active_slot.remove(branch);
                    }
                }
            }
            active
        } else {
            false
        };

        // Mark as unread in the snapshot (user will see it when they switch back)
        if let Some(ref b) = branch {
            if let Some(uuid) = self.agent_session_ids.get(slot_id) {
                snapshot.unread_session_ids.insert(uuid.clone());
            }
            snapshot.unread_sessions.insert(b.clone());
        }

        // Post-exit store flow for background project
        if let Some((session_id, wt_path, _)) = snapshot.pid_session_target.remove(slot_id) {
            self.store_append_background(slot_id, session_id, &wt_path, &project_path);
        }

        // Status message
        let display = branch.as_deref().unwrap_or(slot_id);
        let project_name = &self.project_snapshots.get(&project_path)
            .map(|s| s.project.name.clone())
            .unwrap_or_default();
        let exit_str = match code {
            Some(0) => "exited OK".to_string(),
            Some(c) => format!("exited: {}", c),
            None => "exited".to_string(),
        };
        self.set_status(format!("[{}] {} {}", project_name, display, exit_str));

        true
    }

    /// Send a macOS notification when Claude finishes.
    fn send_completion_notification(&self, branch_name: &str, slot_id: &str, code: Option<i32>) {
        let worktree = crate::models::strip_branch_prefix(branch_name);

        // Resolve session display name
        let is_current = self.current_worktree().map(|s| s.branch_name == branch_name).unwrap_or(false);
        let session_name = if is_current && !self.title_session_name.is_empty() {
            self.title_session_name.clone()
        } else {
            // Try to find Claude session UUID for this slot, then look up its name
            let session_id = self.agent_session_ids.get(slot_id).cloned();
            match session_id {
                Some(id) => {
                    let names = self.load_all_session_names();
                    names.get(&id).cloned().unwrap_or_else(|| {
                        if id.len() > 8 { id[..8].to_string() } else { id }
                    })
                }
                None => String::new(),
            }
        };

        let label = if session_name.is_empty() {
            worktree.to_string()
        } else {
            format!("{}:{}", worktree, session_name)
        };

        let body = match code {
            Some(0) => "Response complete",
            Some(_) => "Exited with error",
            None => "Process terminated",
        };

        let title = label;
        let body = body.to_string();
        std::thread::spawn(move || {
            let _ = notify_rust::Notification::new()
                .summary(&title)
                .body(&body)
                .sound_name("Glass")
                .show();
        });
    }

    /// Cancel the active Claude process for the current session.
    /// Only kills the active slot — other concurrent sessions keep running.
    pub fn cancel_current_claude(&mut self) {
        let branch_name = match self.current_worktree() {
            Some(s) => s.branch_name.clone(),
            None => return,
        };
        // The active slot's key IS the PID string — parse it back to u32
        if let Some(slot) = self.active_slot.get(&branch_name).cloned() {
            if let Ok(pid) = slot.parse::<u32>() {
                #[cfg(unix)]
                {
                    use std::process::Command;
                    let _ = Command::new("kill").arg(pid.to_string()).status();
                }
                #[cfg(windows)]
                {
                    use std::process::Command;
                    let _ = Command::new("taskkill").args(["/PID", &pid.to_string(), "/F"]).output();
                }
                self.set_status("Cancelled Claude");
            }
        }
    }

    /// Handle Claude output. Only processes events from the active slot (the one
    /// displayed in the session pane). Non-active slots' output is silently drained
    /// by the event loop to prevent channel backup.
    pub fn handle_claude_output(&mut self, slot_id: &str, output_type: OutputType, data: String) {
        // Only display output from the active slot of the currently viewed branch.
        // Also suppress when the user is viewing a different session file (historic).
        // During RCR, always show output if the slot matches the RCR session — the
        // worktree's branch_name may be empty (detached HEAD during rebase).
        let is_rcr_slot = self.rcr_session.as_ref().map(|r| r.slot_id == slot_id).unwrap_or(false);
        let is_viewing = !self.viewing_historic_session && (is_rcr_slot || self.current_worktree().map(|s| {
            self.active_slot.get(&s.branch_name).map(|a| a == slot_id).unwrap_or(false)
        }).unwrap_or(false));
        if is_viewing {
            // Single JSON parse: EventParser returns both events AND the raw parsed
            // JSON value. We reuse that value for token/model extraction below instead
            // of calling serde_json::from_str again (was the #1 remaining CPU cost).
            let (events, parsed_json) = self.event_parser.parse(&data);

            for event in &events {
                match event {
                    DisplayEvent::ToolCall { tool_use_id, tool_name, input, .. } => {
                        self.pending_tool_calls.insert(tool_use_id.clone());
                        self.tool_status_generation += 1;
                        // Track subagent (Task) tool calls — while active, TodoWrite
                        // events go to subagent_todos instead of overwriting main todos.
                        // On first Task spawn, snapshot which main todo is in_progress
                        // so subtasks render directly beneath that parent item.
                        if tool_name == "Task" {
                            if self.active_task_tool_ids.is_empty() {
                                self.subagent_parent_idx = self.current_todos.iter()
                                    .position(|t| t.status == crate::app::TodoStatus::InProgress);
                            }
                            self.active_task_tool_ids.insert(tool_use_id.clone());
                        }
                        // TodoWrite: route to subagent_todos if a Task is active,
                        // otherwise update the main agent's current_todos
                        if tool_name == "TodoWrite" {
                            if self.active_task_tool_ids.is_empty() {
                                self.current_todos = parse_todos_from_input(input);
                                self.todo_scroll = 0;
                            } else {
                                self.subagent_todos = parse_todos_from_input(input);
                                self.todo_scroll = 0;
                            }
                        }
                        // AskUserQuestion: flag for special input handling
                        if tool_name == "AskUserQuestion" {
                            self.awaiting_ask_user_question = true;
                            self.ask_user_questions_cache = Some(input.clone());
                        }
                    }
                    DisplayEvent::ToolResult { tool_use_id, is_error, .. } => {
                        self.pending_tool_calls.remove(tool_use_id);
                        self.tool_status_generation += 1;
                        // When a Task (subagent) completes, clear subagent state
                        if self.active_task_tool_ids.remove(tool_use_id) && self.active_task_tool_ids.is_empty() {
                            self.subagent_todos.clear();
                            self.subagent_parent_idx = None;
                        }
                        if *is_error {
                            self.failed_tool_calls.insert(tool_use_id.clone());
                            self.tool_status_generation += 1;
                        }
                    }
                    _ => {}
                }
            }

            // Reuse the JSON value that EventParser already parsed — zero additional
            // serde_json::from_str calls. EventParser returns it alongside events.

            // Extract detected model from live stream events (for display, not badge)
            if let Some(ref json) = parsed_json {
                if let Some("assistant") = json.get("type").and_then(|t| t.as_str()) {
                    if let Some(model) = json.get("message")
                        .and_then(|m| m.get("model"))
                        .and_then(|m| m.as_str())
                    {
                        self.detected_model = Some(model.to_string());
                    }
                }
            }

            // Only extend + invalidate when we actually got events. Many stdout lines
            // (progress, hook_started) produce 0 events — skip the work entirely.
            if !events.is_empty() {
                self.display_events.extend(events);
                self.invalidate_render_cache();
                // Activity detected — reset compaction inactivity watcher
                self.last_session_event_time = std::time::Instant::now();
                self.compaction_banner_injected = false;
            }

            // Feed the fallback session_lines only when the rendered cache is empty
            // (before first render completes). Once we have rendered content, the session
            // pane draws from rendered_lines_cache and session_lines is never read —
            // skip the display_text_from_json + process_session_chunk work entirely.
            if self.rendered_lines_cache.is_empty() {
                if let Some(json) = parsed_json {
                    if let Some(display_text) = display_text_from_json(&json) {
                        self.process_session_chunk(&display_text);
                    }
                } else if output_type != OutputType::Stdout && output_type != OutputType::Json {
                    self.process_session_chunk(&data);
                }
            }

        }
    }

    /// Register a newly spawned Claude process. The PID is used as the slot key.
    /// Newest spawn becomes the active slot (its output appears in session pane).
    pub fn register_claude(&mut self, branch_name: String, pid: u32, receiver: Receiver<AgentEvent>) {
        let slot = pid.to_string();
        self.agent_receivers.insert(slot.clone(), receiver);
        self.running_sessions.insert(slot.clone());
        // Track slot→project for background event routing
        if let Some(ref project) = self.project {
            self.slot_to_project.insert(slot.clone(), project.path.clone());
        }
        // Track this slot under its branch (append = spawn order preserved)
        self.branch_slots.entry(branch_name.clone()).or_default().push(slot.clone());
        // Newest spawn becomes active — its output shows in session pane
        self.active_slot.insert(branch_name, slot);
        // New process = user wants live output, not a historic view
        self.viewing_historic_session = false;
        // Reset compaction inactivity watcher so the 30s timer starts from NOW,
        // not from the last event of the previous response (which may be >30s ago)
        self.last_session_event_time = std::time::Instant::now();
        self.compaction_banner_injected = false;
        self.invalidate_sidebar();
    }

    /// Store Claude's real session UUID, keyed by slot_id (PID string).
    /// Also propagates to RcrSession if this slot is the active RCR process.
    pub fn set_claude_session_id(&mut self, slot_id: &str, claude_session_id: String) {
        self.check_pending_session_name(slot_id, &claude_session_id);
        // Keep RCR session_id in sync so we can --resume and clean up the file
        if let Some(ref mut rcr) = self.rcr_session {
            if rcr.slot_id == slot_id {
                rcr.session_id = Some(claude_session_id.clone());
            }
        }
        self.agent_session_ids.insert(slot_id.to_string(), claude_session_id);
    }

    /// Get the Claude session UUID for the active slot of a branch (for --resume)
    pub fn get_claude_session_id(&self, branch_name: &str) -> Option<&String> {
        // Look up the active slot's Claude session UUID
        self.active_slot.get(branch_name)
            .and_then(|slot| self.agent_session_ids.get(slot))
            // Fallback: check if there's a session_id stored directly by branch
            // (from load_worktrees at startup, before any slot was created)
            .or_else(|| self.agent_session_ids.get(branch_name))
    }

}

/// Parse TodoWrite input JSON into TodoItem vec.
/// Input structure: { "todos": [{ "content": "...", "status": "pending"|"in_progress"|"completed", "activeForm": "..." }] }
pub fn parse_todos_from_input(input: &serde_json::Value) -> Vec<super::app::TodoItem> {
    let Some(todos) = input.get("todos").and_then(|v| v.as_array()) else { return Vec::new() };
    todos.iter().filter_map(|t| {
        let content = t.get("content")?.as_str()?.to_string();
        let active_form = t.get("activeForm").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let status = match t.get("status").and_then(|v| v.as_str()).unwrap_or("pending") {
            "in_progress" => super::app::TodoStatus::InProgress,
            "completed" => super::app::TodoStatus::Completed,
            _ => super::app::TodoStatus::Pending,
        };
        Some(super::app::TodoItem { content, status, active_form })
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::app::TodoStatus;
    use serde_json::json;

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
        assert_eq!(todos[0].content, "Add all terminal keybindings to title bar hints");
        assert_eq!(todos[0].status, TodoStatus::InProgress);
        assert_eq!(todos[0].active_form, "Adding terminal keybindings to title bar");
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
        assert_eq!(parse_todos_from_input(&input)[0].status, TodoStatus::Pending);
    }

    /// Status "done" defaults to Pending (not "completed").
    #[test]
    fn test_parse_todos_status_done() {
        let input = json!({"todos": [{"content": "x", "status": "done", "activeForm": ""}]});
        assert_eq!(parse_todos_from_input(&input)[0].status, TodoStatus::Pending);
    }

    /// Status "IN_PROGRESS" (uppercase) defaults to Pending (case-sensitive).
    #[test]
    fn test_parse_todos_status_case_sensitive() {
        let input = json!({"todos": [{"content": "x", "status": "IN_PROGRESS", "activeForm": ""}]});
        assert_eq!(parse_todos_from_input(&input)[0].status, TodoStatus::Pending);
    }

    /// Status "Pending" with capital P defaults to Pending match.
    #[test]
    fn test_parse_todos_status_capitalized() {
        let input = json!({"todos": [{"content": "x", "status": "Pending", "activeForm": ""}]});
        // "Pending" != "pending" — falls through to default
        assert_eq!(parse_todos_from_input(&input)[0].status, TodoStatus::Pending);
    }

    /// Missing status field defaults to Pending.
    #[test]
    fn test_parse_todos_missing_status() {
        let input = json!({"todos": [{"content": "x", "activeForm": ""}]});
        assert_eq!(parse_todos_from_input(&input)[0].status, TodoStatus::Pending);
    }

    /// Status is null — defaults to Pending.
    #[test]
    fn test_parse_todos_status_null() {
        let input = json!({"todos": [{"content": "x", "status": null, "activeForm": ""}]});
        assert_eq!(parse_todos_from_input(&input)[0].status, TodoStatus::Pending);
    }

    /// Status is a number — defaults to Pending.
    #[test]
    fn test_parse_todos_status_number() {
        let input = json!({"todos": [{"content": "x", "status": 1, "activeForm": ""}]});
        assert_eq!(parse_todos_from_input(&input)[0].status, TodoStatus::Pending);
    }

    /// Status is a boolean — defaults to Pending.
    #[test]
    fn test_parse_todos_status_bool() {
        let input = json!({"todos": [{"content": "x", "status": true, "activeForm": ""}]});
        assert_eq!(parse_todos_from_input(&input)[0].status, TodoStatus::Pending);
    }

    /// Empty string status defaults to Pending.
    #[test]
    fn test_parse_todos_status_empty_string() {
        let input = json!({"todos": [{"content": "x", "status": "", "activeForm": ""}]});
        assert_eq!(parse_todos_from_input(&input)[0].status, TodoStatus::Pending);
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
        let input = json!({"todos": [{"content": "x", "status": "pending", "activeForm": "テスト中"}]});
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
        let input = json!({"todos": [{"content": "  spaces  ", "status": "pending", "activeForm": ""}]});
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos[0].content, "  spaces  ");
    }

    /// Status with whitespace defaults to Pending (not trimmed).
    #[test]
    fn test_parse_todos_status_whitespace() {
        let input = json!({"todos": [{"content": "x", "status": " pending ", "activeForm": ""}]});
        // " pending " != "pending" → defaults
        assert_eq!(parse_todos_from_input(&input)[0].status, TodoStatus::Pending);
    }

    /// activeForm with whitespace is preserved.
    #[test]
    fn test_parse_todos_active_form_whitespace() {
        let input = json!({"todos": [{"content": "x", "status": "pending", "activeForm": "  spaced  "}]});
        assert_eq!(parse_todos_from_input(&input)[0].active_form, "  spaced  ");
    }

    // ── Single item variations ──────────────────────────────────────────

    /// Single pending todo.
    #[test]
    fn test_parse_todos_single_pending() {
        let input = json!({"todos": [{"content": "Task", "status": "pending", "activeForm": "Working"}]});
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
        assert_eq!(todos.iter().filter(|t| t.status == TodoStatus::Completed).count(), 1);
        assert_eq!(todos.iter().filter(|t| t.status == TodoStatus::InProgress).count(), 1);
        assert_eq!(todos.iter().filter(|t| t.status == TodoStatus::Pending).count(), 3);
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
        let pending = todos.iter().filter(|t| t.status == TodoStatus::Pending).count();
        let in_prog = todos.iter().filter(|t| t.status == TodoStatus::InProgress).count();
        let completed = todos.iter().filter(|t| t.status == TodoStatus::Completed).count();
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
        let input = json!({"todos": [{"content": ["a", "b"], "status": "pending", "activeForm": ""}]});
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
        assert_eq!(parse_todos_from_input(&input)[0].status, TodoStatus::Pending);
    }

    /// Verify that content with only whitespace is a valid todo.
    #[test]
    fn test_parse_todos_whitespace_only_content() {
        let input = json!({"todos": [{"content": "   \t\n   ", "status": "pending", "activeForm": ""}]});
        let todos = parse_todos_from_input(&input);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].content, "   \t\n   ");
    }
}
