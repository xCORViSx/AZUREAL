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
        let azural_branches = Git::list_azural_branches(&project.path)?;

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

        // Add feature worktrees (azural/* branches with active worktrees)
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

        // Add archived sessions (azural/* branches without worktrees)
        for branch in azural_branches {
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

        Ok(())
    }

    pub fn load_session_output(&mut self) {
        // Restore terminal for new session (save was done before selection changed)
        self.restore_session_terminal();

        self.output_lines.clear();
        self.output_buffer.clear();
        self.output_scroll = usize::MAX; // Start at bottom (most recent messages)
        self.display_events.clear();
        self.invalidate_render_cache();
        self.event_parser = crate::events::EventParser::new();
        self.selected_event = None;
        self.pending_tool_calls.clear();
        self.failed_tool_calls.clear();

        if let Some(session) = self.current_session() {
            let branch_name = session.branch_name.clone();
            let worktree_path = session.worktree_path.clone();

            // Try to get Claude session ID, or auto-discover from Claude's files
            let mut claude_session_id = session.claude_session_id.clone()
                .or_else(|| self.claude_session_ids.get(&branch_name).cloned());

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
                    self.session_file_modified = std::fs::metadata(&session_file)
                        .and_then(|m| m.modified())
                        .ok();

                    let parsed = crate::app::session_parser::parse_session_file(&session_file);
                    self.display_events = parsed.events;
                    self.pending_tool_calls = parsed.pending_tools;
                    self.failed_tool_calls = parsed.failed_tools;
                    self.invalidate_render_cache();
                }
            }
        }

        // Load file tree for new session
        self.load_file_tree();

        // Auto-dump debug output on debug builds
        #[cfg(debug_assertions)]
        let _ = self.dump_debug_output();
    }

    /// Poll session file for changes and reload if modified
    pub fn poll_session_file(&mut self) -> bool {
        let Some(path) = &self.session_file_path else { return false };
        let Ok(metadata) = std::fs::metadata(path) else { return false };
        let Ok(modified) = metadata.modified() else { return false };

        if self.session_file_modified.map(|t| modified > t).unwrap_or(true) {
            self.load_session_output();
            true
        } else {
            false
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
    }

    pub fn refresh_sessions(&mut self) -> anyhow::Result<()> { self.load_sessions() }

    pub fn dump_debug_output(&self) -> anyhow::Result<()> {
        use std::io::Write;

        let debug_dir = self.current_session()
            .and_then(|s| s.worktree_path.as_ref())
            .map(|p| p.join(".azural"))
            .unwrap_or_else(crate::config::config_dir);
        std::fs::create_dir_all(&debug_dir)?;
        let debug_path = debug_dir.join("debug-output.txt");
        let mut file = std::fs::File::create(&debug_path)?;

        let rendered_lines = crate::tui::util::render_display_events(
            &self.display_events,
            120,
            &self.pending_tool_calls,
            &self.failed_tool_calls,
            self.animation_tick,
            &self.syntax_highlighter,
        );

        for line in rendered_lines.iter() {
            let text: String = line.spans.iter().map(|span| span.content.as_ref()).collect();
            writeln!(file, "{}", text)?;
        }

        Ok(())
    }
}
