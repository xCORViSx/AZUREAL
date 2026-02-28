//! Session navigation and CRUD operations

use crate::git::Git;
use crate::models::Worktree;

use super::App;

impl App {
    pub fn select_next_session(&mut self) {
        if self.worktrees.is_empty() { return; }
        let next = match self.selected_worktree {
            Some(i) if i + 1 < self.worktrees.len() => i + 1,
            Some(_) => 0, // wrap to first
            None => 0,
        };
        self.save_current_terminal();
        self.selected_worktree = Some(next);
        self.load_session_output();
        self.invalidate_sidebar();
    }

    pub fn select_prev_session(&mut self) {
        if self.worktrees.is_empty() { return; }
        let prev = match self.selected_worktree {
            Some(0) => self.worktrees.len() - 1, // wrap to last
            Some(i) => i - 1,
            None => self.worktrees.len() - 1,
        };
        self.save_current_terminal();
        self.selected_worktree = Some(prev);
        self.load_session_output();
        self.invalidate_sidebar();
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

    /// Delete the current worktree AND its branch permanently
    pub fn delete_current_worktree(&mut self) -> anyhow::Result<()> {
        let wt = self.current_worktree().ok_or_else(|| anyhow::anyhow!("No worktree selected"))?;
        let project = self.project.clone().ok_or_else(|| anyhow::anyhow!("No project loaded"))?;
        if wt.branch_name == project.main_branch {
            anyhow::bail!("Cannot delete main branch");
        }
        let branch = wt.branch_name.clone();
        let name = wt.name().to_string();
        // Remove the worktree directory (if active, not archived)
        if let Some(ref wt_path) = wt.worktree_path {
            Git::remove_worktree(&project.path, wt_path)?;
        }
        // Delete the git branch
        let _ = Git::delete_branch(&project.path, &branch);
        // Clean up auto-rebase config
        self.auto_rebase_enabled.remove(&branch);
        crate::azufig::set_auto_rebase(&project.path, &branch, false);
        self.set_status(format!("Deleted: {}", name));
        let prev_idx = self.selected_worktree.unwrap_or(0);
        self.refresh_worktrees()?;
        // Clamp selection after removal
        self.selected_worktree = if self.worktrees.is_empty() {
            None
        } else {
            Some(prev_idx.min(self.worktrees.len() - 1))
        };
        self.load_session_output();
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

    /// Jump to first session
    pub fn select_first_session(&mut self) {
        if self.worktrees.is_empty() { return; }
        if self.selected_worktree != Some(0) {
            self.save_current_terminal();
            self.selected_worktree = Some(0);
            self.load_session_output();
            self.invalidate_sidebar();
        }
    }

    /// Jump to last session
    pub fn select_last_session(&mut self) {
        if self.worktrees.is_empty() { return; }
        let last = self.worktrees.len() - 1;
        if self.selected_worktree != Some(last) {
            self.save_current_terminal();
            self.selected_worktree = Some(last);
            self.load_session_output();
            self.invalidate_sidebar();
        }
    }

}
