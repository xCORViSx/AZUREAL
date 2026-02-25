//! Session navigation and CRUD operations

use crate::git::Git;
use crate::models::Worktree;

use super::App;

impl App {
    /// Whether a session at the given index passes the current sidebar filter.
    /// Matches against: project name, worktree display name, session file custom names, and UUIDs.
    /// `names` is pre-loaded session names to avoid repeated disk reads.
    fn session_matches_filter_with_names(&self, idx: usize, filter: &str, names: &std::collections::HashMap<String, String>) -> bool {
        // Project name matches → all sessions visible
        if let Some(ref project) = self.project {
            if project.name.to_lowercase().contains(filter) { return true; }
        }
        let Some(session) = self.worktrees.get(idx) else { return false };
        // Match on worktree/branch display name
        if session.name().to_lowercase().contains(filter) { return true; }
        // Match on session file UUIDs and custom names
        if let Some(files) = self.session_files.get(&session.branch_name) {
            for (session_id, _, _) in files {
                if session_id.to_lowercase().contains(filter) { return true; }
                if let Some(name) = names.get(session_id.as_str()) {
                    if name.to_lowercase().contains(filter) { return true; }
                }
            }
        }
        false
    }

    pub fn select_next_session(&mut self) {
        let start = self.selected_worktree.map(|i| i + 1).unwrap_or(0);
        if self.sidebar_filter.is_empty() {
            // No filter — simple index bump
            if start < self.worktrees.len() {
                self.save_current_terminal();
                self.selected_worktree = Some(start);
                self.load_session_output();
                self.invalidate_sidebar();
            }
        } else {
            let filter = self.sidebar_filter.to_lowercase();
            let names = self.load_all_session_names();
            if let Some(next) = (start..self.worktrees.len()).find(|&i| self.session_matches_filter_with_names(i, &filter, &names)) {
                if self.selected_worktree != Some(next) {
                    self.save_current_terminal();
                    self.selected_worktree = Some(next);
                    self.load_session_output();
                    self.invalidate_sidebar();
                }
            }
        }
    }

    /// If the current selection doesn't match the filter, snap to the first matching session.
    /// Called after each keystroke in the sidebar filter input.
    pub fn snap_selection_to_filter(&mut self) {
        if self.sidebar_filter.is_empty() { return; }
        let filter = self.sidebar_filter.to_lowercase();
        let names = self.load_all_session_names();
        // Current selection already matches — keep it
        if let Some(idx) = self.selected_worktree {
            if self.session_matches_filter_with_names(idx, &filter, &names) { return; }
        }
        // Find first matching session
        if let Some(first) = (0..self.worktrees.len()).find(|&i| self.session_matches_filter_with_names(i, &filter, &names)) {
            self.save_current_terminal();
            self.selected_worktree = Some(first);
            self.load_session_output();
        }
    }

    pub fn select_prev_session(&mut self) {
        let Some(current) = self.selected_worktree else { return };
        if current == 0 { return; }
        if self.sidebar_filter.is_empty() {
            self.save_current_terminal();
            self.selected_worktree = Some(current - 1);
            self.load_session_output();
            self.invalidate_sidebar();
        } else {
            let filter = self.sidebar_filter.to_lowercase();
            let names = self.load_all_session_names();
            if let Some(prev) = (0..current).rev().find(|&i| self.session_matches_filter_with_names(i, &filter, &names)) {
                self.save_current_terminal();
                self.selected_worktree = Some(prev);
                self.load_session_output();
                self.invalidate_sidebar();
            }
        }
    }

    /// Create a new git worktree with a custom name
    pub fn create_new_worktree_with_name(&mut self, worktree_name: String, _prompt: String) -> anyhow::Result<Worktree> {
        let Some(project) = self.project.clone() else {
            anyhow::bail!("No project loaded")
        };

        let branch_name = format!("{}/{}", crate::models::BRANCH_PREFIX, worktree_name);
        let worktree_path = project.worktrees_dir().join(&worktree_name);

        if worktree_path.exists() {
            anyhow::bail!("Worktree already exists: {}", worktree_path.display());
        }

        // Create git worktree
        Git::create_worktree(&project.path, &worktree_path, &branch_name)?;

        let worktree = Worktree {
            branch_name: branch_name.clone(),
            worktree_path: Some(worktree_path),
            claude_session_id: None,
            archived: false,
        };

        self.refresh_worktrees()?;

        // Select the new worktree
        if let Some(idx) = self.worktrees.iter().position(|s| s.branch_name == branch_name) {
            self.save_current_terminal(); // Save before switching
            self.selected_worktree = Some(idx);
            self.load_session_output();
        }

        Ok(worktree)
    }

    pub fn archive_current_worktree(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_worktree() {
            // Never allow archiving the main branch — it would destroy the primary worktree
            if let Some(project) = &self.project {
                if session.branch_name == project.main_branch {
                    self.set_status("Cannot archive main branch");
                    return Ok(());
                }
            }
            if let Some(ref wt_path) = session.worktree_path {
                if let Some(project) = &self.project {
                    Git::remove_worktree(&project.path, wt_path)?;
                }
            }
            self.set_status("Session archived");
            self.refresh_worktrees()?;
        }
        Ok(())
    }

    /// Restore an archived worktree by recreating its git worktree from the preserved branch
    pub fn unarchive_current_worktree(&mut self) -> anyhow::Result<()> {
        let session = self.current_worktree().ok_or_else(|| anyhow::anyhow!("No worktree selected"))?;
        if !session.archived {
            anyhow::bail!("Worktree is not archived");
        }
        let branch = session.branch_name.clone();
        let worktree_name = session.name().to_string();
        let project = self.project.clone().ok_or_else(|| anyhow::anyhow!("No project loaded"))?;
        let worktree_path = project.worktrees_dir().join(&worktree_name);
        // Recreate worktree from the existing branch
        Git::create_worktree_from_branch(&project.path, &worktree_path, &branch)?;
        self.set_status(format!("Unarchived: {}", worktree_name));
        self.refresh_worktrees()?;
        // Re-select the worktree (index may have shifted after refresh)
        if let Some(idx) = self.worktrees.iter().position(|s| s.branch_name == branch) {
            self.selected_worktree = Some(idx);
            self.load_session_output();
        }
        Ok(())
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

    /// Jump to first session (respects sidebar filter)
    pub fn select_first_session(&mut self) {
        if self.worktrees.is_empty() { return; }
        if self.sidebar_filter.is_empty() {
            if self.selected_worktree != Some(0) {
                self.save_current_terminal();
                self.selected_worktree = Some(0);
                self.load_session_output();
                self.invalidate_sidebar();
            }
        } else {
            let filter = self.sidebar_filter.to_lowercase();
            let names = self.load_all_session_names();
            if let Some(first) = (0..self.worktrees.len()).find(|&i| self.session_matches_filter_with_names(i, &filter, &names)) {
                if self.selected_worktree != Some(first) {
                    self.save_current_terminal();
                    self.selected_worktree = Some(first);
                    self.load_session_output();
                    self.invalidate_sidebar();
                }
            }
        }
    }

    /// Jump to last session (respects sidebar filter)
    pub fn select_last_session(&mut self) {
        if self.worktrees.is_empty() { return; }
        if self.sidebar_filter.is_empty() {
            let last = self.worktrees.len() - 1;
            if self.selected_worktree != Some(last) {
                self.save_current_terminal();
                self.selected_worktree = Some(last);
                self.load_session_output();
                self.invalidate_sidebar();
            }
        } else {
            let filter = self.sidebar_filter.to_lowercase();
            let names = self.load_all_session_names();
            if let Some(last) = (0..self.worktrees.len()).rev().find(|&i| self.session_matches_filter_with_names(i, &filter, &names)) {
                if self.selected_worktree != Some(last) {
                    self.save_current_terminal();
                    self.selected_worktree = Some(last);
                    self.load_session_output();
                    self.invalidate_sidebar();
                }
            }
        }
    }

}
