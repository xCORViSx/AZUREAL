//! Session loading and discovery

use std::collections::HashSet;

use crate::git::Git;
use crate::models::{Project, Session};

use super::helpers::build_file_tree;
use super::App;

impl App {
    /// Load project and sessions from git (stateless discovery)
    pub fn load(&mut self) -> anyhow::Result<()> {
        let cwd = std::env::current_dir()?;

        // Find git repo root
        if !Git::is_git_repo(&cwd) {
            return Ok(());
        }

        let repo_root = Git::repo_root(&cwd)?;
        let main_branch = Git::get_main_branch(&repo_root)?;

        self.project = Some(Project::from_path(repo_root, main_branch));
        self.load_sessions()?;

        Ok(())
    }

    /// Load sessions from git worktrees and branches
    pub fn load_sessions(&mut self) -> anyhow::Result<()> {
        let Some(project) = &self.project else { return Ok(()) };

        let worktrees = Git::list_worktrees_detailed(&project.path)?;
        let azureal_branches = Git::list_azureal_branches(&project.path)?;

        let mut sessions = Vec::new();
        let mut active_branches: HashSet<String> = HashSet::new();

        // First, add main worktree
        for wt in &worktrees {
            if wt.is_main {
                let branch_name = wt.branch.clone().unwrap_or_else(|| project.main_branch.clone());
                let claude_id = crate::config::find_latest_claude_session(&wt.path);
                if let Some(ref id) = claude_id {
                    self.claude_session_ids.insert(branch_name.clone(), id.clone());
                }
                sessions.push(Session {
                    branch_name: branch_name.clone(),
                    worktree_path: Some(wt.path.clone()),
                    claude_session_id: claude_id,
                    archived: false,
                });
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
                sessions.push(Session {
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
                sessions.push(Session {
                    branch_name: branch,
                    worktree_path: None,
                    claude_session_id: None,
                    archived: true,
                });
            }
        }

        self.sessions = sessions;
        self.selected_session = if self.sessions.is_empty() { None } else { Some(0) };
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
        self.session_file_parse_offset = 0;
        self.invalidate_render_cache();
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

        if let Some(session) = self.current_session() {
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

        // Load file tree for new session
        self.load_file_tree();
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
        // Live stream already provides events — polling would duplicate them
        if self.is_current_session_running() { return false; }
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
        // Update tokens and context window if the new parse found assistant events
        if parsed.session_tokens.is_some() {
            self.session_tokens = parsed.session_tokens;
        }
        if parsed.context_window.is_some() {
            self.model_context_window = parsed.context_window;
        }
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

        let Some(session) = self.current_session() else { return };
        let Some(ref worktree_path) = session.worktree_path else { return };

        self.file_tree_entries = build_file_tree(worktree_path, &self.file_tree_expanded);
        if !self.file_tree_entries.is_empty() {
            self.file_tree_selected = Some(0);
        }
        self.invalidate_file_tree();
    }

    pub fn refresh_sessions(&mut self) -> anyhow::Result<()> { self.load_sessions() }

    /// Dump debug output to .azureal/debug-output.txt (triggered by Ctrl+Opt+Cmd+D)
    pub fn dump_debug_output(&mut self) {
        if let Err(e) = self.dump_debug_output_inner() {
            self.set_status(format!("Debug dump failed: {}", e));
        } else {
            self.set_status("Debug output saved to .azureal/debug-output.txt");
        }
    }

    fn dump_debug_output_inner(&self) -> anyhow::Result<()> {
        use std::io::Write;
        use crate::events::DisplayEvent;

        // Use project data dir (.azureal/ in git root) - only creates when actually writing
        let debug_dir = crate::config::ensure_project_data_dir()?
            .ok_or_else(|| anyhow::anyhow!("Not in a git repository"))?;
        let debug_path = debug_dir.join("debug-output.txt");
        let mut file = std::fs::File::create(&debug_path)?;

        // Diagnostic header
        writeln!(file, "=== AZUREAL DEBUG DUMP ===")?;
        writeln!(file, "Dump time: {:?}", std::time::SystemTime::now())?;
        writeln!(file, "Session file: {:?}", self.session_file_path)?;

        // Check if session file looks complete (ends with newline and valid JSON)
        if let Some(ref path) = self.session_file_path {
            if let Ok(content) = std::fs::read_to_string(path) {
                let file_size = content.len();
                let ends_with_newline = content.ends_with('\n');
                let last_50_chars: String = content.chars().rev().take(50).collect::<String>().chars().rev().collect();
                writeln!(file, "File size: {} bytes, ends with newline: {}", file_size, ends_with_newline)?;
                writeln!(file, "Last 50 chars: {:?}", last_50_chars)?;

                // Check if last line looks like valid JSON
                if let Some(last_line) = content.lines().last() {
                    let is_valid_json = serde_json::from_str::<serde_json::Value>(last_line).is_ok();
                    writeln!(file, "Last line valid JSON: {}", is_valid_json)?;
                    if !is_valid_json {
                        writeln!(file, "Last line (truncated): {:?}", &last_line.chars().take(100).collect::<String>())?;
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

        // Count event types
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

        // Show last 5 events with preview
        writeln!(file, "=== LAST 5 EVENTS ===")?;
        let start = self.display_events.len().saturating_sub(5);
        for (i, event) in self.display_events.iter().skip(start).enumerate() {
            let preview = match event {
                DisplayEvent::UserMessage { content, .. } => format!("UserMessage: {}...", &content.chars().take(50).collect::<String>()),
                DisplayEvent::AssistantText { text, .. } => format!("AssistantText: {}...", &text.chars().take(50).collect::<String>()),
                DisplayEvent::ToolCall { tool_name, .. } => format!("ToolCall: {}", tool_name),
                DisplayEvent::ToolResult { tool_name, .. } => format!("ToolResult: {}", tool_name),
                DisplayEvent::Hook { name, output } => format!("Hook: {} -> {}", name, &output.chars().take(30).collect::<String>()),
                DisplayEvent::Complete { .. } => "Complete".to_string(),
                DisplayEvent::Error { message } => format!("Error: {}", &message.chars().take(50).collect::<String>()),
                _ => format!("{:?}", event).chars().take(50).collect(),
            };
            writeln!(file, "  [{}] {}", start + i, preview)?;
        }
        writeln!(file, "")?;

        writeln!(file, "=== RENDERED OUTPUT ===")?;
        let (rendered_lines, _, _) = crate::tui::util::render_display_events(
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
            writeln!(file, "{}", text)?;
        }

        Ok(())
    }
}
