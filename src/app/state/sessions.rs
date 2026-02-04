//! Session navigation and CRUD operations

use crate::git::Git;
use crate::models::Session;

use super::helpers::{generate_session_name, sanitize_for_branch};
use super::App;

impl App {
    pub fn select_next_session(&mut self) {
        if let Some(idx) = self.selected_session {
            if idx + 1 < self.sessions.len() {
                self.save_current_terminal(); // Save BEFORE changing selection
                self.selected_session = Some(idx + 1);
                self.load_session_output();
                self.invalidate_sidebar();
            }
        } else if !self.sessions.is_empty() {
            self.save_current_terminal();
            self.selected_session = Some(0);
            self.load_session_output();
            self.invalidate_sidebar();
        }
    }

    pub fn select_prev_session(&mut self) {
        if let Some(idx) = self.selected_session {
            if idx > 0 {
                self.save_current_terminal(); // Save BEFORE changing selection
                self.selected_session = Some(idx - 1);
                self.load_session_output();
                self.invalidate_sidebar();
            }
        }
    }

    /// Create a new git worktree with a Claude session (name auto-generated from prompt)
    pub fn create_new_worktree(&mut self, prompt: String) -> anyhow::Result<Session> {
        let name = generate_session_name(&prompt);
        let worktree_name = sanitize_for_branch(&name);
        self.create_new_worktree_with_name(worktree_name, prompt)
    }

    /// Create a new git worktree with a custom name
    pub fn create_new_worktree_with_name(&mut self, worktree_name: String, _prompt: String) -> anyhow::Result<Session> {
        let Some(project) = self.project.clone() else {
            anyhow::bail!("No project loaded")
        };

        let branch_name = format!("azureal/{}", worktree_name);
        let worktree_path = project.worktrees_dir().join(&worktree_name);

        if worktree_path.exists() {
            anyhow::bail!("Worktree already exists: {}", worktree_path.display());
        }

        // Create git worktree
        Git::create_worktree(&project.path, &worktree_path, &branch_name)?;

        let worktree = Session {
            branch_name: branch_name.clone(),
            worktree_path: Some(worktree_path),
            claude_session_id: None,
            archived: false,
        };

        self.refresh_sessions()?;

        // Select the new worktree
        if let Some(idx) = self.sessions.iter().position(|s| s.branch_name == branch_name) {
            self.save_current_terminal(); // Save before switching
            self.selected_session = Some(idx);
            self.load_session_output();
        }

        Ok(worktree)
    }

    pub fn archive_current_session(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_session() {
            if let Some(ref wt_path) = session.worktree_path {
                if let Some(project) = &self.project {
                    Git::remove_worktree(&project.path, wt_path)?;
                }
            }
            self.set_status("Session archived");
            self.refresh_sessions()?;
        }
        Ok(())
    }

    pub fn rebase_current_session(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_session() {
            if let Some(ref wt_path) = session.worktree_path {
                if let Some(project) = self.current_project() {
                    Git::rebase_onto_main(wt_path, &project.main_branch)?;
                    self.set_status("Rebased successfully");
                    return Ok(());
                }
            }
        }
        anyhow::bail!("No active session with worktree")
    }

    /// Expand a session's file dropdown and load its session files
    pub fn expand_session(&mut self, branch_name: &str) {
        self.sessions_expanded.insert(branch_name.to_string());
        self.load_session_files(branch_name);
        self.invalidate_sidebar();
    }

    /// Collapse a session's file dropdown
    pub fn collapse_session(&mut self, branch_name: &str) {
        self.sessions_expanded.remove(branch_name);
        self.invalidate_sidebar();
    }

    /// Toggle session expansion state
    pub fn toggle_session_expanded(&mut self, branch_name: &str) {
        if self.sessions_expanded.contains(branch_name) {
            self.collapse_session(branch_name);
        } else {
            self.expand_session(branch_name);
        }
    }

    /// Load and cache session files for a branch from Claude's project directory
    pub fn load_session_files(&mut self, branch_name: &str) {
        let Some(session) = self.sessions.iter().find(|s| s.branch_name == branch_name) else { return };
        let Some(ref wt_path) = session.worktree_path else { return };
        let files = crate::config::list_claude_sessions(wt_path);
        self.session_files.insert(branch_name.to_string(), files);
        // Initialize selection to 0 (latest) if not set
        self.session_selected_file_idx.entry(branch_name.to_string()).or_insert(0);
        self.invalidate_sidebar();
    }

    /// Select a specific session file by index
    pub fn select_session_file(&mut self, branch_name: &str, idx: usize) {
        if let Some(files) = self.session_files.get(branch_name) {
            if idx < files.len() {
                self.session_selected_file_idx.insert(branch_name.to_string(), idx);
                // Load the selected session file
                self.load_session_output();
                self.invalidate_sidebar();
            }
        }
    }

    /// Navigate to next file in expanded dropdown (loads immediately)
    pub fn session_file_next(&mut self) {
        let Some(session) = self.current_session() else { return };
        let branch = session.branch_name.clone();
        let Some(files) = self.session_files.get(&branch) else { return };
        if files.is_empty() { return; }
        let current = *self.session_selected_file_idx.get(&branch).unwrap_or(&0);
        if current + 1 < files.len() {
            self.session_selected_file_idx.insert(branch, current + 1);
            self.load_session_output();
            self.invalidate_sidebar();
        }
    }

    /// Navigate to previous file in expanded dropdown (loads immediately)
    pub fn session_file_prev(&mut self) {
        let Some(session) = self.current_session() else { return };
        let branch = session.branch_name.clone();
        let current = *self.session_selected_file_idx.get(&branch).unwrap_or(&0);
        if current > 0 {
            self.session_selected_file_idx.insert(branch, current - 1);
            self.load_session_output();
            self.invalidate_sidebar();
        }
    }

    /// Check if current session is expanded
    pub fn is_current_session_expanded(&self) -> bool {
        self.current_session()
            .map(|s| self.sessions_expanded.contains(&s.branch_name))
            .unwrap_or(false)
    }
}
