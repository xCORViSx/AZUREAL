//! Session loading and discovery

use std::collections::HashSet;

use crate::git::Git;
use crate::models::{Project, Worktree};

use super::helpers::build_file_tree;
use super::App;

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

        let main_branch = Git::get_main_branch(&repo_root)?;
        self.project = Some(Project::from_path(repo_root.clone(), main_branch));

        // Load filetree hidden dirs from project azufig (persisted Options overlay state)
        let az = crate::azufig::load_project_azufig(&repo_root);
        self.file_tree_hidden_dirs = az.filetree.hidden.into_iter().collect();

        // Load auto-rebase enabled branches from project azufig
        self.auto_rebase_enabled = crate::azufig::load_auto_rebase_branches(&repo_root);

        // Untrack any files that match .gitignore but are still in the index
        // (e.g. .DS_Store committed before gitignore was added).
        Git::untrack_gitignored_files(&repo_root);

        // Detached HEAD repair and orphaned rebase cleanup now handled
        // inside load_worktrees() so every refresh (not just startup) benefits.
        self.load_worktrees()?;

        Ok(())
    }

    /// Load sessions from git worktrees and branches
    pub fn load_worktrees(&mut self) -> anyhow::Result<()> {
        let Some(project) = &self.project else { return Ok(()) };

        let worktrees = Git::list_worktrees_detailed(&project.path)?;

        // Repair detached HEADs — can happen after rebase abort, crash, or
        // interrupted operations. Without this, detached worktrees get
        // branch=None → empty-named active entry + real branch as archived.
        //
        // Two cases:
        // 1. Active rebase (e.g. RCR in progress): read branch from
        //    rebase-merge/head-name WITHOUT aborting — the rebase is intentional.
        // 2. Orphaned detached HEAD (no rebase): re-attach via checkout.
        let mut needs_refetch = false;
        let mut rebase_branches: Vec<(std::path::PathBuf, String)> = Vec::new();
        for wt in &worktrees {
            if wt.branch.is_some() { continue; }
            if Git::is_rebase_in_progress(&wt.path) {
                // Read the original branch from rebase state — don't abort
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
                    // Also check rebase-apply (for non-interactive rebases)
                    let head_name = std::path::Path::new(gd).join("rebase-apply/head-name");
                    if let Ok(content) = std::fs::read_to_string(&head_name) {
                        let branch = content.trim().strip_prefix("refs/heads/").unwrap_or(content.trim());
                        if !branch.is_empty() {
                            rebase_branches.push((wt.path.clone(), branch.to_string()));
                            continue;
                        }
                    }
                }
                // Rebase state exists but can't read branch — orphaned, abort
                let _ = Git::rebase_abort(&wt.path);
                needs_refetch = true;
                continue;
            }
            // No rebase — plain detached HEAD, re-attach
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
            Git::list_worktrees_detailed(&project.path)?
        } else {
            worktrees
        };
        // Patch in branch names recovered from active rebase state
        for (path, branch) in &rebase_branches {
            for wt in &mut worktrees {
                if wt.path == *path && wt.branch.is_none() {
                    wt.branch = Some(branch.clone());
                }
            }
        }

        let azureal_branches = Git::list_azureal_branches(&project.path)?;

        // Migrate old-encoding Claude project dirs (e.g. AZUREAL++ → AZUREAL--)
        let wt_paths: Vec<_> = worktrees.iter().map(|w| w.path.clone()).collect();
        crate::config::migrate_project_dirs(&wt_paths);

        let mut sessions = Vec::new();
        let mut active_branches: HashSet<String> = HashSet::new();

        // Store main worktree separately — it's accessed via 'm' browse mode, not the sidebar.
        // Excluded from app.worktrees so it never appears in navigation or sidebar rendering.
        for wt in &worktrees {
            if wt.is_main {
                let branch_name = wt.branch.clone().unwrap_or_else(|| project.main_branch.clone());
                let claude_id = crate::config::find_latest_claude_session(&wt.path);
                if let Some(ref id) = claude_id {
                    self.claude_session_ids.insert(branch_name.clone(), id.clone());
                }
                self.main_worktree = Some(Worktree {
                    branch_name: branch_name.clone(),
                    worktree_path: Some(wt.path.clone()),
                    claude_session_id: claude_id,
                    archived: false,
                });
                // Also load session files for main so browse mode can show sessions
                let files = crate::config::list_claude_sessions(&wt.path);
                self.session_files.insert(branch_name.clone(), files);
                self.session_selected_file_idx.entry(branch_name.clone()).or_insert(0);
                active_branches.insert(branch_name);
            }
        }

        // Add feature worktrees (azureal/* branches with active worktrees)
        for wt in &worktrees {
            if !wt.is_main {
                let branch_name = wt.branch.clone().unwrap_or_default();
                let claude_id = crate::config::find_latest_claude_session(&wt.path);
                if let Some(ref id) = claude_id {
                    self.claude_session_ids.insert(branch_name.clone(), id.clone());
                }
                sessions.push(Worktree {
                    branch_name: branch_name.clone(),
                    worktree_path: Some(wt.path.clone()),
                    claude_session_id: claude_id,
                    archived: false,
                });
                active_branches.insert(branch_name);
            }
        }

        // Add archived sessions (azureal/* branches without worktrees)
        for branch in azureal_branches {
            if !active_branches.contains(&branch) {
                sessions.push(Worktree {
                    branch_name: branch,
                    worktree_path: None,
                    claude_session_id: None,
                    archived: true,
                });
            }
        }

        self.worktrees = sessions;
        // Pre-select the worktree matching the launch directory (cwd) so the
        // user lands on the branch they're actually working in.
        let cwd = std::env::current_dir().ok();
        self.selected_worktree = if self.worktrees.is_empty() {
            None
        } else {
            cwd.and_then(|c| self.worktrees.iter().position(|w| w.worktree_path.as_ref() == Some(&c)))
                .or(Some(0))
        };

        // Eagerly load session files for all worktrees so sidebar filter can search UUIDs/names
        for session in &self.worktrees {
            if let Some(ref wt_path) = session.worktree_path {
                let files = crate::config::list_claude_sessions(wt_path);
                self.session_files.insert(session.branch_name.clone(), files);
                self.session_selected_file_idx.entry(session.branch_name.clone()).or_insert(0);
            }
        }

        self.invalidate_sidebar();

        Ok(())
    }

    pub fn load_session_output(&mut self) {
        // Restore terminal for new session (save was done before selection changed)
        self.restore_session_terminal();

        self.output_lines.clear();
        self.output_buffer.clear();
        self.output_scroll = usize::MAX; // Start at bottom (most recent messages)
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
        self.output_viewport_cache.clear();
        self.animation_line_indices.clear();
        self.message_bubble_positions.clear();
        self.clickable_paths.clear();
        self.clicked_path_highlight = None;
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

            // Try to get Claude session ID: check selected file first, then cached, then auto-discover
            let mut claude_session_id = None;

            // First check if user selected a specific session file from the dropdown
            if let Some(idx) = self.session_selected_file_idx.get(&branch_name) {
                if let Some(files) = self.session_files.get(&branch_name) {
                    if let Some((id, _, _)) = files.get(*idx) {
                        claude_session_id = Some(id.clone());
                    }
                }
            }

            // Fall back to stored session ID or cached ID
            if claude_session_id.is_none() {
                claude_session_id = session.claude_session_id.clone()
                    .or_else(|| self.claude_session_ids.get(&branch_name).cloned());
            }

            // Auto-discover Claude session ID if not set and we have a worktree
            if claude_session_id.is_none() {
                if let Some(ref wt_path) = worktree_path {
                    if let Some(discovered_id) = crate::config::find_latest_claude_session(wt_path) {
                        self.claude_session_ids.insert(branch_name.clone(), discovered_id.clone());
                        claude_session_id = Some(discovered_id);
                    }
                }
            }

            // Try loading from Claude's session files
            if let (Some(claude_id), Some(ref wt_path)) = (claude_session_id, &worktree_path) {
                if let Some(session_file) = crate::config::claude_session_file(wt_path, &claude_id) {
                    // Track file for live polling
                    self.session_file_path = Some(session_file.clone());
                    if let Ok(meta) = std::fs::metadata(&session_file) {
                        self.session_file_modified = meta.modified().ok();
                        self.session_file_size = meta.len();
                    }

                    let parsed = crate::app::session_parser::parse_session_file(&session_file);
                    self.display_events = parsed.events;
                    self.pending_tool_calls = parsed.pending_tools;
                    self.failed_tool_calls = parsed.failed_tools;
                    self.parse_total_lines = parsed.total_lines;
                    self.parse_errors = parsed.parse_errors;
                    self.assistant_total = parsed.assistant_total;
                    self.assistant_no_message = parsed.assistant_no_message;
                    self.assistant_no_content_arr = parsed.assistant_no_content_arr;
                    self.assistant_text_blocks = parsed.assistant_text_blocks;
                    self.awaiting_plan_approval = parsed.awaiting_plan_approval;
                    self.session_tokens = parsed.session_tokens;
                    self.model_context_window = parsed.context_window;
                    self.update_token_badge();
                    // Extract latest TodoWrite and AskUserQuestion state from parsed events
                    self.extract_skill_tools_from_events();
                    // Store byte offset for incremental parsing on subsequent polls
                    self.session_file_parse_offset = parsed.end_offset;

                    // Clear pending message once it appears in the parsed events.
                    // Scan all events from the end — Claude may have emitted many
                    // events (hooks, tool calls, text) after the user message, pushing
                    // it far from the tail.
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
                }
            }
        }

        // Determine if we're viewing a non-active (historic) session.
        // When true, live events from the running process are suppressed.
        self.viewing_historic_session = false;
        if let Some(session) = self.current_worktree() {
            let branch = session.branch_name.clone();
            if let Some(active_slot) = self.active_slot.get(&branch) {
                if let Some(active_sid) = self.claude_session_ids.get(active_slot) {
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

    /// Get the Claude session UUID of the currently viewed session file for a branch
    pub fn viewed_session_id(&self, branch: &str) -> Option<String> {
        self.session_selected_file_idx.get(branch)
            .and_then(|idx| self.session_files.get(branch).and_then(|f| f.get(*idx)))
            .map(|(id, _, _)| id.clone())
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
    /// Reads session_names TOML once here so draw_title_bar() is zero I/O.
    /// During RCR, the title is locked to "[RCR] <name>" and won't be overwritten.
    pub fn update_title_session_name(&mut self) {
        // RCR mode locks the title — don't let normal session logic overwrite it
        if self.rcr_session.is_some() { return; }
        let Some(session) = self.current_worktree() else {
            self.title_session_name.clear();
            return;
        };
        let branch = session.branch_name.clone();
        let names = self.load_all_session_names();
        // Resolve the active claude session ID for this worktree
        let session_id = self.session_selected_file_idx.get(&branch)
            .and_then(|idx| self.session_files.get(&branch).and_then(|f| f.get(*idx)))
            .map(|(id, _, _)| id.clone())
            .or_else(|| self.worktrees.get(self.selected_worktree?)
                .and_then(|s| s.claude_session_id.clone()))
            .or_else(|| self.claude_session_ids.get(&branch).cloned());
        self.title_session_name = match session_id {
            Some(id) => names.get(&id).cloned().unwrap_or_else(|| format_uuid_short(&id)),
            None => String::new(),
        };
    }

    /// Check if session file changed (lightweight - just checks file size)
    /// Marks dirty if changed, but doesn't parse yet
    pub fn check_session_file(&mut self) {
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
        let was_at_bottom = self.output_scroll == usize::MAX;

        // Incremental parse: only read new bytes since last offset
        let parsed = crate::app::session_parser::parse_session_file_incremental(
            &path,
            self.session_file_parse_offset,
            &self.display_events,
            &self.pending_tool_calls,
            &self.failed_tool_calls,
        );
        self.display_events = parsed.events;
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
        // Update tokens and context window if the new parse found assistant events
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
        self.last_convo_event_time = std::time::Instant::now();
        self.compaction_banner_injected = false;

        // If we were following bottom, stay at bottom after content update
        if was_at_bottom {
            self.output_scroll = usize::MAX;
        }
    }

    /// Load file tree entries for the current session's worktree
    pub fn load_file_tree(&mut self) {
        self.file_tree_entries.clear();
        self.file_tree_selected = None;
        self.file_tree_scroll = 0;

        let Some(session) = self.current_worktree() else { return };
        let Some(ref worktree_path) = session.worktree_path else { return };

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

    fn dump_debug_output_inner(&self, name_suffix: &str) -> anyhow::Result<()> {
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
        let (rendered_lines, _, _, _) = crate::tui::util::render_display_events(
            &self.display_events,
            120,
            &self.pending_tool_calls,
            &self.failed_tool_calls,
            &self.syntax_highlighter,
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
