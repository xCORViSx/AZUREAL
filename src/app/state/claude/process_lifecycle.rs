//! Agent process lifecycle management
//!
//! Handles the full lifecycle of spawned agent processes: registration,
//! start confirmation, exit handling (including background project exits),
//! cancellation, session ID tracking, and macOS completion notifications.

use std::sync::mpsc::Receiver;

use crate::backend::Backend;
use crate::claude::AgentEvent;

use crate::app::state::App;

impl App {
    /// Called when a Claude process emits Started { pid }. The slot_id IS the
    /// PID string, already registered in register_claude() — this just confirms
    /// the process is alive and clears stale exit codes.
    pub fn handle_claude_started(&mut self, slot_id: &str, _pid: u32) {
        self.running_sessions.insert(slot_id.to_string());
        self.agent_exit_codes.remove(slot_id);
        self.invalidate_sidebar();
        let branch = self
            .branch_for_slot(slot_id)
            .unwrap_or_else(|| slot_id.to_string());
        let agent = if self.codex_slot_started_at.contains_key(slot_id) {
            "Codex"
        } else {
            "Claude"
        };
        self.set_status(format!("{} started in {}", agent, branch));
    }

    /// Called when a Claude process exits. Cleans up slot state, switches active
    /// slot if needed, and triggers session file re-parse.
    pub fn handle_claude_exited(&mut self, slot_id: &str, code: Option<i32>) {
        // Resolve branch — first in current project, then in background snapshots
        let branch = self.branch_for_slot(slot_id);
        let was_codex = self.codex_slot_started_at.contains_key(slot_id);
        let turn_backend = if was_codex {
            Backend::Codex
        } else {
            Backend::Claude
        };

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
        self.codex_slot_started_at.remove(slot_id);
        if let Some(c) = code {
            self.agent_exit_codes.insert(slot_id.to_string(), c);
        }

        // Remove slot from its branch's slot list
        if let Some(ref branch) = branch {
            if let Some(slots) = self.branch_slots.get_mut(branch) {
                slots.retain(|s| s != slot_id);
                if slots.is_empty() {
                    self.branch_slots.remove(branch);
                }
            }
        }

        // If this was the active slot, switch to next available slot or clear
        let was_active = branch
            .as_ref()
            .and_then(|b| self.active_slot.get(b))
            .map(|a| a == slot_id)
            .unwrap_or(false);

        if was_active {
            if let Some(ref branch) = branch {
                // Pick another running slot on this branch, or remove active
                let next = self
                    .branch_slots
                    .get(branch)
                    .and_then(|slots| slots.last().cloned());
                match next {
                    Some(next_slot) => {
                        self.active_slot.insert(branch.clone(), next_slot);
                    }
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
        let is_current = branch
            .as_ref()
            .and_then(|b| self.current_worktree().map(|s| s.branch_name == *b))
            .unwrap_or(false);
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
            if was_codex {
                self.session_file_parse_offset = 0;
            }
        }

        // If this was a [NewRunCmd] session, auto-reload runcmds
        if is_current && self.title_session_name.starts_with("[NewRunCmd]") {
            self.load_run_commands();
        }

        // Post-exit store flow: parse JSONL → strip injected context → append to SQLite
        self.store_append_from_jsonl(slot_id, turn_backend);

        // Fallback JSONL cleanup: if store_append_from_display already consumed
        // the pid_session_target entry (e.g. compaction, superseded prompt), the
        // JSONL was never deleted. Resolve it independently and remove it.
        if let Some(uuid) = self.agent_session_ids.get(slot_id) {
            let wt_path = branch
                .as_ref()
                .and_then(|b| {
                    self.worktrees
                        .iter()
                        .find(|wt| wt.branch_name == *b)
                        .and_then(|wt| wt.worktree_path.clone())
                })
                .or_else(|| self.current_worktree().and_then(|wt| wt.worktree_path.clone()));
            if let Some(wt) = wt_path {
                if let Some(p) = crate::config::session_file(&wt, uuid) {
                    let _ = std::fs::remove_file(&p);
                    if self.session_file_path.as_ref() == Some(&p) {
                        self.session_file_path = None;
                        self.session_file_dirty = false;
                    }
                }
            }
        }

        // If compaction is active, preserve the "Compacting context" status
        if self.auto_continue_after_compaction || self.compaction_needed.is_some() {
            // Status already set by the compaction trigger — don't overwrite
        } else if self.staged_prompt.is_some() {
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
        let branch = snapshot
            .branch_slots
            .iter()
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
        self.codex_slot_started_at.remove(slot_id);
        if let Some(c) = code {
            self.agent_exit_codes.insert(slot_id.to_string(), c);
        }

        // Re-borrow snapshot after self borrows above
        let snapshot = self.project_snapshots.get_mut(&project_path).unwrap();

        // Update snapshot's branch_slots
        let _was_active = if let Some(ref branch) = branch {
            let active = snapshot
                .active_slot
                .get(branch)
                .map(|a| a == slot_id)
                .unwrap_or(false);
            if let Some(slots) = snapshot.branch_slots.get_mut(branch) {
                slots.retain(|s| s != slot_id);
                if slots.is_empty() {
                    snapshot.branch_slots.remove(branch);
                }
            }
            if active {
                let next = snapshot
                    .branch_slots
                    .get(branch)
                    .and_then(|s| s.last().cloned());
                match next {
                    Some(next_slot) => {
                        snapshot.active_slot.insert(branch.clone(), next_slot);
                    }
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
        if let Some((session_id, wt_path, _, session_file_offset)) =
            snapshot.pid_session_target.remove(slot_id)
        {
            self.store_append_background(
                slot_id,
                session_id,
                &wt_path,
                &project_path,
                session_file_offset,
            );
        }

        // Status message
        let display = branch.as_deref().unwrap_or(slot_id);
        let project_name = &self
            .project_snapshots
            .get(&project_path)
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

    /// Send a notification when Claude finishes or compacts context.
    fn send_completion_notification(&self, branch_name: &str, slot_id: &str, code: Option<i32>) {
        let worktree = crate::models::strip_branch_prefix(branch_name);

        // Resolve session display name
        let is_current = self
            .current_worktree()
            .map(|s| s.branch_name == branch_name)
            .unwrap_or(false);
        let session_name = if is_current && !self.title_session_name.is_empty() {
            self.title_session_name.clone()
        } else {
            // Try to find Claude session UUID for this slot, then look up its name
            let session_id = self.agent_session_ids.get(slot_id).cloned();
            match session_id {
                Some(id) => {
                    let names = self.load_all_session_names();
                    names.get(&id).cloned().unwrap_or_else(|| {
                        if id.len() > 8 {
                            id[..8].to_string()
                        } else {
                            id
                        }
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

        let body = if self.auto_continue_after_compaction || self.compaction_needed.is_some() {
            "Compacting context"
        } else {
            match code {
                Some(0) => "Response complete",
                Some(_) => "Exited with error",
                None => "Process terminated",
            }
        };

        let title = label;
        let body = body.to_string();
        std::thread::spawn(move || {
            #[cfg(target_os = "windows")]
            {
                // Use PowerShell WinRT toast — notify-rust with custom app_id silently
                // fails on Windows because unregistered AppUserModelIDs are dropped.
                let icon_path = dirs::home_dir()
                    .unwrap_or_default()
                    .join(".azureal")
                    .join("Azureal_toast.png");
                let icon_xml = if icon_path.exists() {
                    format!(
                        "<image placement=\"appLogoOverride\" src=\"{}\" />",
                        icon_path.display(),
                    )
                } else {
                    String::new()
                };
                let xml = format!(
                    "<toast><visual><binding template=\"ToastGeneric\">\
                     <text>{}</text><text>{}</text>{}\
                     </binding></visual></toast>",
                    title.replace('&', "&amp;").replace('<', "&lt;"),
                    body.replace('&', "&amp;").replace('<', "&lt;"),
                    icon_xml,
                );
                let ps = format!(
                    "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null; \
                     [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom, ContentType = WindowsRuntime] | Out-Null; \
                     $xml = New-Object Windows.Data.Xml.Dom.XmlDocument; \
                     $xml.LoadXml('{}'); \
                     [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}}\\WindowsPowerShell\\v1.0\\powershell.exe').Show(\
                     [Windows.UI.Notifications.ToastNotification]::new($xml))",
                    xml.replace('\'', "''"),
                );
                use std::os::windows::process::CommandExt;
                let _ = std::process::Command::new("powershell")
                    .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
                    .creation_flags(0x08000000) // CREATE_NO_WINDOW
                    .output();
            }

            #[cfg(not(target_os = "windows"))]
            {
                let mut n = notify_rust::Notification::new();
                n.summary(&title).body(&body);

                #[cfg(target_os = "macos")]
                n.sound_name("Glass");

                let _ = n.show();
            }
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
                    let _ = Command::new("taskkill")
                        .args(["/PID", &pid.to_string(), "/F"])
                        .output();
                }
                self.set_status("Cancelled Claude");
            }
        }
    }

    /// Register a newly spawned Claude process. The PID is used as the slot key.
    /// Newest spawn becomes the active slot (its output appears in session pane).
    pub fn register_claude(
        &mut self,
        branch_name: String,
        pid: u32,
        receiver: Receiver<AgentEvent>,
        model: Option<&str>,
    ) {
        let slot = pid.to_string();
        self.agent_receivers.insert(slot.clone(), receiver);
        self.running_sessions.insert(slot.clone());
        let backend = model
            .map(crate::app::state::backend_for_model)
            .unwrap_or(crate::backend::Backend::Claude);
        match backend {
            crate::backend::Backend::Codex => {
                self.codex_slot_started_at
                    .insert(slot.clone(), std::time::Instant::now());
            }
            crate::backend::Backend::Claude => {
                self.codex_slot_started_at.remove(&slot);
            }
        }
        // Track slot→project for background event routing
        if let Some(ref project) = self.project {
            self.slot_to_project
                .insert(slot.clone(), project.path.clone());
        }
        // Track this slot under its branch (append = spawn order preserved)
        self.branch_slots
            .entry(branch_name.clone())
            .or_default()
            .push(slot.clone());
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
        self.agent_session_ids
            .insert(slot_id.to_string(), claude_session_id.clone());

        // Persist UUID in the store so orphaned JSOLNs can be recovered on restart
        if let Some((session_id, _, _, _)) = self.pid_session_target.get(slot_id) {
            if let Some(ref store) = self.session_store {
                let _ = store.set_session_uuid(*session_id, &claude_session_id);
            }
        }
    }

    /// Get the Claude session UUID for the active slot of a branch (for --resume)
    pub fn get_claude_session_id(&self, branch_name: &str) -> Option<&String> {
        // Look up the active slot's Claude session UUID
        self.active_slot
            .get(branch_name)
            .and_then(|slot| self.agent_session_ids.get(slot))
            // Fallback: check if there's a session_id stored directly by branch
            // (from load_worktrees at startup, before any slot was created)
            .or_else(|| self.agent_session_ids.get(branch_name))
    }
}
