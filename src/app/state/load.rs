//! Session loading and discovery

use std::collections::HashSet;
use std::path::PathBuf;

use crate::app::types::WorktreeRefreshResult;
use crate::backend::Backend;
use crate::git::Git;
use crate::models::{Project, Worktree};

use super::helpers::build_file_tree;
use super::App;

/// Pure computation: all git + FS I/O for worktree discovery, no App state.
/// Safe to run on a background thread. Returns data to apply to App.
pub fn compute_worktree_refresh(
    project_path: PathBuf,
    main_branch: String,
    worktrees_dir: PathBuf,
    _backend: Backend,
) -> anyhow::Result<WorktreeRefreshResult> {
    let worktrees = Git::list_worktrees_detailed(&project_path)?;

    // Repair detached HEADs (rebase state recovery, orphaned HEAD re-attach)
    let mut needs_refetch = false;
    let mut rebase_branches: Vec<(PathBuf, String)> = Vec::new();
    for wt in &worktrees {
        if wt.branch.is_some() { continue; }
        if !wt.is_main && !wt.path.starts_with(&worktrees_dir) { continue; }
        if Git::is_rebase_in_progress(&wt.path) {
            let git_dir = std::process::Command::new("git")
                .args(["rev-parse", "--git-dir"])
                .current_dir(&wt.path)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
            if let Some(ref gd) = git_dir {
                let head_name = std::path::Path::new(gd).join("rebase-merge/head-name");
                if let Ok(content) = std::fs::read_to_string(&head_name) {
                    let branch = content.trim().strip_prefix("refs/heads/").unwrap_or(content.trim());
                    if !branch.is_empty() {
                        rebase_branches.push((wt.path.clone(), branch.to_string()));
                        continue;
                    }
                }
                let head_name = std::path::Path::new(gd).join("rebase-apply/head-name");
                if let Ok(content) = std::fs::read_to_string(&head_name) {
                    let branch = content.trim().strip_prefix("refs/heads/").unwrap_or(content.trim());
                    if !branch.is_empty() {
                        rebase_branches.push((wt.path.clone(), branch.to_string()));
                        continue;
                    }
                }
            }
            let _ = Git::rebase_abort(&wt.path);
            needs_refetch = true;
            continue;
        }
        let head_ok = std::process::Command::new("git")
            .args(["symbolic-ref", "--quiet", "HEAD"])
            .current_dir(&wt.path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(true);
        if !head_ok {
            if let Ok(out) = std::process::Command::new("git")
                .args(["for-each-ref", "--points-at=HEAD", "--format=%(refname:short)", "refs/heads/"])
                .current_dir(&wt.path)
                .output()
            {
                let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if let Some(target) = branch.lines().next().filter(|b| !b.is_empty()) {
                    let _ = std::process::Command::new("git")
                        .args(["checkout", target])
                        .current_dir(&wt.path)
                        .output();
                    needs_refetch = true;
                }
            }
        }
    }
    let mut worktrees = if needs_refetch {
        Git::list_worktrees_detailed(&project_path)?
    } else {
        worktrees
    };
    for (path, branch) in &rebase_branches {
        for wt in &mut worktrees {
            if wt.path == *path && wt.branch.is_none() {
                wt.branch = Some(branch.clone());
            }
        }
    }

    let azureal_branches = Git::list_azureal_branches(&project_path)?;

    let wt_paths: Vec<_> = worktrees.iter().map(|w| w.path.clone()).collect();
    crate::config::migrate_project_dirs(&wt_paths);

    let mut result_worktrees = Vec::new();
    let mut result_main: Option<Worktree> = None;
    let mut active_branches: HashSet<String> = HashSet::new();

    // Main worktree
    for wt in &worktrees {
        if wt.is_main {
            let branch_name = wt.branch.clone().unwrap_or_else(|| main_branch.clone());
            result_main = Some(Worktree {
                branch_name: branch_name.clone(),
                worktree_path: Some(wt.path.clone()),
                claude_session_id: None,
                archived: false,
            });
            active_branches.insert(branch_name);
        }
    }

    // Feature worktrees
    for wt in &worktrees {
        if !wt.is_main && wt.path.starts_with(&worktrees_dir) {
            let branch_name = wt.branch.clone().unwrap_or_default();
            result_worktrees.push(Worktree {
                branch_name: branch_name.clone(),
                worktree_path: Some(wt.path.clone()),
                claude_session_id: None,
                archived: false,
            });
            active_branches.insert(branch_name);
        }
    }

    // Archived branches
    for branch in azureal_branches {
        if !active_branches.contains(&branch) {
            result_worktrees.push(Worktree {
                branch_name: branch,
                worktree_path: None,
                claude_session_id: None,
                archived: true,
            });
        }
    }

    Ok(WorktreeRefreshResult {
        main_worktree: result_main,
        worktrees: result_worktrees,
    })
}

impl App {
    /// Load project and sessions from git (stateless discovery).
    /// If cwd is a git repo, auto-register it in ~/.azureal/projects.txt and load it.
    /// If NOT in a git repo, open the Projects panel so user can pick a project.
    pub fn load(&mut self) -> anyhow::Result<()> {
        let cwd = std::env::current_dir()?;

        if !Git::is_git_repo(&cwd) {
            // Not in a git repo — show Projects panel with a helpful message
            self.open_projects_panel();
            if let Some(ref mut panel) = self.projects_panel {
                panel.error = Some("Project not initialized. Press i to initialize or choose another project.".to_string());
            }
            return Ok(());
        }

        let repo_root = Git::repo_root(&cwd)?;

        // Auto-register this repo in ~/.azureal/projects.txt (no-op if already there)
        crate::config::register_project(&repo_root);

        // Ensure worktrees/ is gitignored so new worktrees don't inherit the folder
        Git::ensure_worktrees_gitignored(&repo_root);

        let main_branch = Git::get_main_branch(&repo_root)?;
        self.project = Some(Project::from_path(repo_root.clone(), main_branch));

        // Session store is opened lazily on first use (ensure_session_store)
        // to avoid creating the .azs file for projects that haven't used sessions yet.

        // Load filetree hidden dirs from project azufig (persisted Options overlay state)
        let az = crate::azufig::load_project_azufig(&repo_root);
        self.file_tree_hidden_dirs = az.filetree.hidden.into_iter().collect();

        // Load auto-rebase enabled branches from project azufig
        self.auto_rebase_enabled = crate::azufig::load_auto_rebase_branches(&repo_root);

        // Untrack any files that match .gitignore but are still in the index
        // (e.g. .DS_Store committed before gitignore was added).
        Git::untrack_gitignored_files(&repo_root);

        // Prune stale remote-tracking refs so branches deleted on other machines
        // don't appear as archived worktrees. Best-effort (no-op if offline).
        Git::prune_remote_refs(&repo_root);

        // Detached HEAD repair and orphaned rebase cleanup now handled
        // inside load_worktrees() so every refresh (not just startup) benefits.
        self.load_worktrees()?;

        Ok(())
    }

    /// Load sessions from git worktrees and branches.
    /// Synchronous — used at startup and for user-triggered refreshes.
    /// The event loop uses compute_worktree_refresh() + apply_worktree_result()
    /// on a background thread instead.
    pub fn load_worktrees(&mut self) -> anyhow::Result<()> {
        let Some(project) = &self.project else { return Ok(()) };
        // Discard any in-flight background refresh — this synchronous call takes priority
        self.worktree_refresh_receiver = None;
        let result = compute_worktree_refresh(
            project.path.clone(),
            project.main_branch.clone(),
            project.worktrees_dir(),
            self.backend,
        )?;
        self.apply_worktree_result(result);
        Ok(())
    }

    /// Apply pre-computed worktree data to App state.
    /// Handles selection preservation.
    pub fn apply_worktree_result(&mut self, result: WorktreeRefreshResult) {
        // Apply main worktree
        self.main_worktree = result.main_worktree;

        // Preserve current selection by branch name
        let prev_branch = self.selected_worktree
            .and_then(|i| self.worktrees.get(i))
            .map(|w| w.branch_name.clone());

        self.worktrees = result.worktrees;

        self.selected_worktree = if self.worktrees.is_empty() {
            None
        } else if let Some(ref branch) = prev_branch {
            self.worktrees.iter().position(|w| w.branch_name == *branch)
                .or(Some(0))
        } else {
            let cwd = std::env::current_dir().ok();
            cwd.and_then(|c| self.worktrees.iter().position(|w| w.worktree_path.as_ref() == Some(&c)))
                .or(Some(0))
        };

        self.invalidate_sidebar();
    }

    pub fn load_session_output(&mut self) {
        // Open session store if the .azs file exists (don't create it)
        self.try_open_session_store();

        // Restore terminal for new session (save was done before selection changed)
        self.restore_session_terminal();

        self.session_lines.clear();
        self.session_buffer.clear();
        self.session_scroll = usize::MAX; // Start at bottom (most recent messages)
        self.display_events.clear();
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
        self.session_tokens = None;
        self.model_context_window = None;
        self.token_badge_cache = None;
        self.current_todos.clear();
        self.subagent_todos.clear();
        self.active_task_tool_ids.clear();
        self.subagent_parent_idx = None;
        self.awaiting_ask_user_question = false;
        self.ask_user_questions_cache = None;

        if let Some(session) = self.current_worktree() {
            let branch_name = session.branch_name.clone();
            let worktree_path = session.worktree_path.clone();

            // Determine store session ID:
            // 1. From session list selection (numeric string from session_files cache)
            // 2. From current_session_id (set by start_new_session or prior load)
            // 3. Auto-discover latest session from store for this branch
            let store_session_id = self.session_selected_file_idx.get(&branch_name)
                .and_then(|idx| self.session_files.get(&branch_name)
                    .and_then(|f| f.get(*idx))
                    .and_then(|(id, _, _)| id.parse::<i64>().ok()))
                .or(self.current_session_id)
                .or_else(|| self.session_store.as_ref()
                    .and_then(|store| store.list_sessions(Some(&branch_name)).ok())
                    .and_then(|sessions| sessions.last().map(|s| s.id)));

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
            let is_live = self.active_slot.get(&branch_name)
                .map(|slot| self.running_sessions.contains(slot))
                .unwrap_or(false);

            if is_live {
                // Live session: load from JSONL for real-time display
                // (store doesn't have events yet — they're ingested on exit)
                if let Some(slot) = self.active_slot.get(&branch_name).cloned() {
                    if let Some(uuid) = self.agent_session_ids.get(&slot) {
                        if let Some(ref wt_path) = worktree_path {
                            if let Some(jsonl_path) = crate::config::session_file(self.backend, wt_path, uuid) {
                                if jsonl_path.exists() {
                                    self.session_file_path = Some(jsonl_path.clone());
                                    let source_size = std::fs::metadata(&jsonl_path)
                                        .map(|m| { self.session_file_modified = m.modified().ok(); m.len() })
                                        .unwrap_or(0);
                                    self.session_file_size = source_size;

                                    let parsed = crate::app::session_parser::parse_session_file(&jsonl_path);
                                    self.display_events = parsed.events;
                                    self.pending_tool_calls = parsed.pending_tools;
                                    self.failed_tool_calls = parsed.failed_tools;
                                    self.session_tokens = parsed.session_tokens;
                                    self.model_context_window = parsed.context_window;
                                    self.update_token_badge();
                                    self.extract_skill_tools_from_events();
                                    self.session_file_parse_offset = parsed.end_offset;
                                    self.awaiting_plan_approval = parsed.awaiting_plan_approval;

                                    if let Some(ref pending) = self.pending_user_message {
                                        for event in self.display_events.iter().rev() {
                                            if let crate::events::DisplayEvent::UserMessage { content, .. } = event {
                                                if content == pending {
                                                    self.pending_user_message = None;
                                                }
                                                break;
                                            }
                                        }
                                    }
                                    self.invalidate_render_cache();
                                }
                            }
                        }
                    }
                }
                // Set current_session_id from the store target if available
                if let Some(slot) = self.active_slot.get(&branch_name) {
                    if let Some((sid, _, _)) = self.pid_session_target.get(slot) {
                        self.current_session_id = Some(*sid);
                    }
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
                                if let crate::events::DisplayEvent::UserMessage { content, .. } = event {
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
        self.viewing_historic_session = false;
        if let Some(session) = self.current_worktree() {
            let branch = session.branch_name.clone();
            if let Some(active_slot) = self.active_slot.get(&branch) {
                if let Some(active_sid) = self.agent_session_ids.get(active_slot) {
                    if let Some(viewed_sid) = self.viewed_session_id(&branch) {
                        self.viewing_historic_session = active_sid != &viewed_sid;
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
    }

    /// Get the session ID string of the currently viewed session for a branch.
    /// Returns the store ID as a string (from session_files cache) or falls
    /// back to current_session_id.
    pub fn viewed_session_id(&self, branch: &str) -> Option<String> {
        self.session_selected_file_idx.get(branch)
            .and_then(|idx| self.session_files.get(branch).and_then(|f| f.get(*idx)))
            .map(|(id, _, _)| id.clone())
            .or_else(|| self.current_session_id.map(|id| id.to_string()))
    }

    /// Tell the file watcher thread to watch the current session file and
    /// worktree directory. Called after session switch (from load_session_output).
    pub fn sync_file_watches(&self) {
        let Some(ref watcher) = self.file_watcher else { return };
        watcher.send(crate::watcher::WatchCommand::ClearAll);
        if let Some(ref path) = self.session_file_path {
            watcher.send(crate::watcher::WatchCommand::WatchSessionFile(path.clone()));
        }
        if let Some(idx) = self.selected_worktree {
            if let Some(session) = self.worktrees.get(idx) {
                if let Some(ref wt_path) = session.worktree_path {
                    watcher.send(crate::watcher::WatchCommand::WatchWorktree(wt_path.to_path_buf()));
                }
            }
        }
    }

    /// Cache the session display name for the title bar.
    /// Reads session names from store so draw_title_bar() is zero I/O.
    /// During RCR, the title is locked to "[RCR] <name>" and won't be overwritten.
    pub fn update_title_session_name(&mut self) {
        if self.rcr_session.is_some() { return; }
        let Some(session) = self.current_worktree() else {
            self.title_session_name.clear();
            return;
        };
        let branch = session.branch_name.clone();
        let names = self.load_all_session_names();
        let session_id = self.viewed_session_id(&branch);
        self.title_session_name = match session_id {
            Some(id) => names.get(&id).cloned().unwrap_or_else(|| format_uuid_short(&id)),
            None => String::new(),
        };
    }

    /// Check if session file changed (lightweight - just checks file size)
    /// Marks dirty if changed, but doesn't parse yet.
    /// Also recovers from missing-file state if the source reappears.
    pub fn check_session_file(&mut self) {
        // Auto-recovery: if source was missing and has reappeared, restore normal mode
        let Some(path) = &self.session_file_path else { return };
        let Ok(metadata) = std::fs::metadata(path) else { return };
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
        if !self.session_file_dirty { return false; }
        self.session_file_dirty = false;
        // Skip while the ACTIVE slot is streaming — its live output already
        // feeds display_events. Other concurrent slots on the same branch don't
        // affect the displayed session file, so we only gate on the active one.
        if self.is_active_slot_running() { return false; }
        self.refresh_session_events();
        true
    }

    /// Lightweight refresh of session events (no terminal/file tree reload).
    /// Uses incremental parsing — only reads new bytes appended since last parse.
    fn refresh_session_events(&mut self) {
        let Some(path) = self.session_file_path.clone() else { return };

        // Track if we were at bottom before refresh (usize::MAX = follow mode)
        let was_at_bottom = self.session_scroll == usize::MAX;

        // Incremental parse: only read new bytes since last offset
        let was_full_reparse = self.session_file_parse_offset == 0;
        let parsed = match self.backend {
            crate::backend::Backend::Claude => crate::app::session_parser::parse_session_file_incremental(
                &path,
                self.session_file_parse_offset,
                &self.display_events,
                &self.pending_tool_calls,
                &self.failed_tool_calls,
            ),
            crate::backend::Backend::Codex => crate::app::codex_session_parser::parse_codex_session_file_incremental(
                &path,
                self.session_file_parse_offset,
                &self.display_events,
                &self.pending_tool_calls,
                &self.failed_tool_calls,
            ),
        };
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
        // Update tokens, context window, and model if the new parse found assistant events
        let mut tokens_changed = false;
        if parsed.session_tokens.is_some() {
            self.session_tokens = parsed.session_tokens;
            tokens_changed = true;
        }
        if parsed.context_window.is_some() {
            self.model_context_window = parsed.context_window;
            tokens_changed = true;
        }
        if tokens_changed { self.update_token_badge(); }
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

    /// Load file tree entries for the current session's worktree
    pub fn load_file_tree(&mut self) {
        // Discard any in-flight background scan — this synchronous call takes priority
        self.file_tree_receiver = None;
        self.file_tree_entries.clear();
        self.file_tree_selected = None;
        self.file_tree_scroll = 0;

        let Some(session) = self.current_worktree() else {
            self.invalidate_file_tree();
            return;
        };
        let Some(ref worktree_path) = session.worktree_path else {
            self.invalidate_file_tree();
            return;
        };

        self.file_tree_entries = build_file_tree(worktree_path, &self.file_tree_expanded, &self.file_tree_hidden_dirs);
        if !self.file_tree_entries.is_empty() {
            self.file_tree_selected = Some(0);
        }
        self.invalidate_file_tree();
    }

    pub fn refresh_worktrees(&mut self) -> anyhow::Result<()> { self.load_worktrees() }

    /// Scan display_events backwards for the latest TodoWrite and AskUserQuestion.
    /// TodoWrite: update sticky todo widget. AskUserQuestion: check if awaiting response.
    fn extract_skill_tools_from_events(&mut self) {
        let mut found_ask = false;
        let mut saw_user_after_ask = false;
        let mut saw_user_after_todo = false;
        // Forward scan — track whether user responded after the last TodoWrite/AskUserQuestion
        for event in &self.display_events {
            match event {
                crate::events::DisplayEvent::ToolCall { tool_name, input, .. } => {
                    if tool_name == "TodoWrite" {
                        self.current_todos = super::claude::parse_todos_from_input(input);
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
                    if found_ask { saw_user_after_ask = true; }
                    saw_user_after_todo = true;
                }
                _ => {}
            }
        }
        // Clear stale todos — user sent a new prompt after the last TodoWrite
        if saw_user_after_todo { self.current_todos.clear(); }
        // Only awaiting if AskUserQuestion was called and no user responded yet
        self.awaiting_ask_user_question = found_ask && !saw_user_after_ask;
        if !found_ask { self.ask_user_questions_cache = None; }
    }

    /// Dump debug output to .azureal/debug-output[_name] (triggered by ⌃d)
    /// All user/assistant content is obfuscated so the file can be shared in bug reports
    /// without exposing sensitive project details. Tool names, event types, and structural
    /// markers are preserved for diagnostic value. Optional name suffix appended after underscore.
    pub fn dump_debug_output(&mut self, name: &str) {
        let suffix = name.trim();
        if let Err(e) = self.dump_debug_output_inner(suffix) {
            self.set_status(format!("Debug dump failed: {}", e));
        } else {
            let filename = if suffix.is_empty() { "debug-output".to_string() }
                else { format!("debug-output_{}", suffix) };
            self.set_status(format!("Debug output saved to .azureal/{}", filename));
        }
    }

    fn dump_debug_output_inner(&mut self, name_suffix: &str) -> anyhow::Result<()> {
        use std::io::Write;
        use std::collections::HashMap;
        use crate::events::DisplayEvent;

        // Deterministic word obfuscator: maps each unique word to a consistent fake word
        // so structural patterns are preserved (same word → same replacement every time).
        // Keeps punctuation, whitespace, numbers, file extensions, and structural tokens.
        struct Obfuscator {
            map: HashMap<String, String>,
            counter: usize,
        }
        impl Obfuscator {
            fn new() -> Self { Self { map: HashMap::new(), counter: 0 } }

            // Generate a fake word from a counter (aaa, aab, aac, ... aba, abb, ...)
            fn fake_word(&mut self, len: usize) -> String {
                let id = self.counter;
                self.counter += 1;
                // 3-letter base from counter, then pad/truncate to roughly match original length
                let base: String = (0..3).rev().map(|i| {
                    (b'a' + ((id / 26_usize.pow(i as u32)) % 26) as u8) as char
                }).collect();
                if len <= 3 { base[..len.min(3)].to_string() }
                else { format!("{}{}", base, "x".repeat(len.saturating_sub(3))) }
            }

            // Obfuscate a word, preserving case pattern. Skips structural tokens.
            fn word(&mut self, w: &str) -> String {
                if w.is_empty() { return String::new(); }
                // Preserve: numbers, punctuation-only tokens, very short (1-2 char) structural tokens,
                // file extensions (.rs, .md, .toml, .json, .txt, .jsonl),
                // and common programming keywords that don't leak project info
                if w.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-') { return w.to_string(); }
                if w.len() <= 2 { return w.to_string(); }
                let key = w.to_lowercase();
                if let Some(existing) = self.map.get(&key) { return existing.clone(); }
                let fake = self.fake_word(w.len());
                // Match case pattern of original: ALL_CAPS, Capitalized, lowercase
                let result = if w.chars().all(|c| c.is_uppercase() || !c.is_alphabetic()) {
                    fake.to_uppercase()
                } else if w.starts_with(|c: char| c.is_uppercase()) {
                    let mut chars = fake.chars();
                    match chars.next() {
                        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                        None => fake,
                    }
                } else { fake.clone() };
                self.map.insert(key, result.clone());
                result
            }

            // Obfuscate a full text string, preserving whitespace and punctuation structure
            fn text(&mut self, s: &str) -> String {
                let mut result = String::with_capacity(s.len());
                let mut word = String::new();
                for ch in s.chars() {
                    if ch.is_alphanumeric() || ch == '_' {
                        word.push(ch);
                    } else {
                        if !word.is_empty() {
                            result.push_str(&self.word(&word));
                            word.clear();
                        }
                        result.push(ch);
                    }
                }
                if !word.is_empty() { result.push_str(&self.word(&word)); }
                result
            }

            // Obfuscate a file path, keeping / separators and file extensions
            fn path(&mut self, p: &str) -> String {
                p.split('/').map(|seg| {
                    if seg.is_empty() { return String::new(); }
                    // Split filename from extension
                    if let Some(dot_pos) = seg.rfind('.') {
                        let (name, ext) = seg.split_at(dot_pos);
                        format!("{}{}", self.word(name), ext) // keep extension as-is
                    } else {
                        self.word(seg)
                    }
                }).collect::<Vec<_>>().join("/")
            }
        }

        let mut ob = Obfuscator::new();

        let debug_dir = crate::config::ensure_project_data_dir()?
            .ok_or_else(|| anyhow::anyhow!("Not in a git repository"))?;
        let filename = if name_suffix.is_empty() { "debug-output".to_string() }
            else { format!("debug-output_{}", name_suffix) };
        let debug_path = debug_dir.join(&filename);
        let mut file = std::fs::File::create(&debug_path)?;

        // Diagnostic header — safe metadata (no content leaked)
        writeln!(file, "=== AZUREAL DEBUG DUMP ===")?;
        writeln!(file, "Dump time: {:?}", std::time::SystemTime::now())?;
        writeln!(file, "Session file: {:?}", self.session_file_path.as_ref().map(|p| ob.path(&p.display().to_string())))?;

        // Session file health check — only structural info, no content
        if let Some(ref path) = self.session_file_path {
            if let Ok(content) = std::fs::read_to_string(path) {
                let file_size = content.len();
                let ends_with_newline = content.ends_with('\n');
                writeln!(file, "File size: {} bytes, ends with newline: {}", file_size, ends_with_newline)?;
                writeln!(file, "Last 50 chars: [redacted]")?;
                if let Some(last_line) = content.lines().last() {
                    let is_valid_json = serde_json::from_str::<serde_json::Value>(last_line).is_ok();
                    writeln!(file, "Last line valid JSON: {}", is_valid_json)?;
                    if !is_valid_json {
                        writeln!(file, "Last line length: {} chars (invalid JSON)", last_line.len())?;
                    }
                }
            }
        }
        writeln!(file, "")?;
        writeln!(file, "JSONL lines: {} (parse errors: {})", self.parse_total_lines, self.parse_errors)?;
        writeln!(file, "")?;
        writeln!(file, "=== ASSISTANT PARSING STATS ===")?;
        writeln!(file, "  Total 'assistant' events in JSONL: {}", self.assistant_total)?;
        writeln!(file, "  - No 'message' field: {}", self.assistant_no_message)?;
        writeln!(file, "  - No 'content' array: {}", self.assistant_no_content_arr)?;
        writeln!(file, "  - Text blocks created: {}", self.assistant_text_blocks)?;
        writeln!(file, "")?;
        writeln!(file, "Total display_events: {}", self.display_events.len())?;

        // Event type counts — no content leaked
        let mut user_msgs = 0;
        let mut assistant_texts = 0;
        let mut tool_calls = 0;
        let mut tool_results = 0;
        let mut hooks = 0;
        let mut other = 0;
        for event in &self.display_events {
            match event {
                DisplayEvent::UserMessage { .. } => user_msgs += 1,
                DisplayEvent::AssistantText { .. } => assistant_texts += 1,
                DisplayEvent::ToolCall { .. } => tool_calls += 1,
                DisplayEvent::ToolResult { .. } => tool_results += 1,
                DisplayEvent::Hook { .. } => hooks += 1,
                _ => other += 1,
            }
        }
        writeln!(file, "Event breakdown:")?;
        writeln!(file, "  UserMessage: {}", user_msgs)?;
        writeln!(file, "  AssistantText: {}", assistant_texts)?;
        writeln!(file, "  ToolCall: {}", tool_calls)?;
        writeln!(file, "  ToolResult: {}", tool_results)?;
        writeln!(file, "  Hook: {}", hooks)?;
        writeln!(file, "  Other: {}", other)?;
        writeln!(file, "")?;

        // Last 5 events — content obfuscated, tool names preserved for diagnostics
        writeln!(file, "=== LAST 5 EVENTS ===")?;
        let start = self.display_events.len().saturating_sub(5);
        for (i, event) in self.display_events.iter().skip(start).enumerate() {
            let preview = match event {
                DisplayEvent::UserMessage { content, .. } => {
                    let ob_text = ob.text(&content.chars().take(80).collect::<String>());
                    format!("UserMessage: {}...", ob_text)
                }
                DisplayEvent::AssistantText { text, .. } => {
                    let ob_text = ob.text(&text.chars().take(80).collect::<String>());
                    format!("AssistantText: {}...", ob_text)
                }
                DisplayEvent::ToolCall { tool_name, file_path, .. } => {
                    let ob_path = file_path.as_ref().map(|p| ob.path(p)).unwrap_or_default();
                    format!("ToolCall: {} {}", tool_name, ob_path)
                }
                DisplayEvent::ToolResult { tool_name, file_path, content, .. } => {
                    let ob_path = file_path.as_ref().map(|p| ob.path(p)).unwrap_or_default();
                    format!("ToolResult: {} {} ({}B)", tool_name, ob_path, content.len())
                }
                DisplayEvent::Hook { name, output } => {
                    format!("Hook: {} ({}B)", name, output.len())
                }
                DisplayEvent::Complete { duration_ms, cost_usd, .. } => {
                    format!("Complete: {}ms, ${:.4}", duration_ms, cost_usd)
                }
                DisplayEvent::Init { model, .. } => format!("Init: model={}", model),
                DisplayEvent::Command { name } => format!("Command: {}", name),
                DisplayEvent::Compacting => "Compacting".to_string(),
                DisplayEvent::Compacted => "Compacted".to_string(),
                DisplayEvent::MayBeCompacting => "MayBeCompacting".to_string(),
                DisplayEvent::Plan { name, .. } => format!("Plan: {}", ob.text(name)),
                DisplayEvent::Filtered => "Filtered".to_string(),
            };
            writeln!(file, "  [{}] {}", start + i, preview)?;
        }
        writeln!(file, "")?;

        // Full rendered output — every line obfuscated
        writeln!(file, "=== RENDERED OUTPUT ===")?;
        let (rendered_lines, _, _, _, _) = crate::tui::util::render_display_events(
            &self.display_events,
            120,
            &self.pending_tool_calls,
            &self.failed_tool_calls,
            &mut self.syntax_highlighter,
            None,
        );
        writeln!(file, "Total rendered lines: {}", rendered_lines.len())?;
        writeln!(file, "")?;

        for line in rendered_lines.iter() {
            let text: String = line.spans.iter().map(|span| span.content.as_ref()).collect();
            writeln!(file, "{}", ob.text(&text))?;
        }

        Ok(())
    }
}

/// Format a UUID-like session ID as "xxxxxxxx-…" (first group + dash + ellipsis)
fn format_uuid_short(id: &str) -> String {
    if let Some(dash) = id.find('-') {
        if dash >= 8 { return format!("{}-…", &id[..dash]); }
    }
    if id.len() > 12 { format!("{}…", &id[..11]) } else { id.to_string() }
}

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
        app.session_files.insert(branch.to_string(), vec![
            ("uuid-1".to_string(), PathBuf::from("/sessions/1.jsonl"), "2024-01-01".to_string()),
            ("uuid-2".to_string(), PathBuf::from("/sessions/2.jsonl"), "2024-01-02".to_string()),
        ]);
        app.session_selected_file_idx.insert(branch.to_string(), 0);
        assert_eq!(app.viewed_session_id(branch), Some("uuid-1".to_string()));
    }

    #[test]
    fn viewed_session_id_second_selection() {
        let mut app = App::new();
        let branch = "azureal/test";
        app.session_files.insert(branch.to_string(), vec![
            ("uuid-a".to_string(), PathBuf::from("/a"), "t1".to_string()),
            ("uuid-b".to_string(), PathBuf::from("/b"), "t2".to_string()),
        ]);
        app.session_selected_file_idx.insert(branch.to_string(), 1);
        assert_eq!(app.viewed_session_id(branch), Some("uuid-b".to_string()));
    }

    #[test]
    fn viewed_session_id_idx_out_of_bounds() {
        let mut app = App::new();
        let branch = "b";
        app.session_files.insert(branch.to_string(), vec![
            ("uuid-x".to_string(), PathBuf::from("/x"), "t".to_string()),
        ]);
        app.session_selected_file_idx.insert(branch.to_string(), 5); // out of bounds
        assert!(app.viewed_session_id(branch).is_none());
    }

    #[test]
    fn viewed_session_id_no_idx() {
        let mut app = App::new();
        let branch = "b";
        app.session_files.insert(branch.to_string(), vec![
            ("uuid-x".to_string(), PathBuf::from("/x"), "t".to_string()),
        ]);
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
        app.rendered_lines_cache.push(ratatui::text::Line::raw("old"));
        app.session_viewport_cache.push(ratatui::text::Line::raw("old"));
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
    fn load_session_output_clears_token_state() {
        let mut app = App::new();
        app.session_tokens = Some((100_000, 5000));
        app.model_context_window = Some(200_000);
        app.token_badge_cache = Some(("50%".to_string(), ratatui::style::Color::Green));
        app.load_session_output();
        assert!(app.session_tokens.is_none());
        assert!(app.model_context_window.is_none());
        assert!(app.token_badge_cache.is_none());
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
        app.clickable_paths.push((0, 0, 10, "/file.rs".to_string(), "".to_string(), "".to_string(), 1));
        app.clicked_path_highlight = Some((0, 0, 10, 1));
        app.load_session_output();
        assert!(app.clickable_paths.is_empty());
        assert!(app.clicked_path_highlight.is_none());
    }

    // ── load_file_tree state reset ──

    #[test]
    fn load_file_tree_clears_when_no_worktree() {
        let mut app = App::new();
        app.file_tree_entries.push(crate::app::types::FileTreeEntry {
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
        app.session_files.insert("".to_string(), vec![
            ("id".to_string(), PathBuf::from("/p"), "t".to_string()),
        ]);
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
