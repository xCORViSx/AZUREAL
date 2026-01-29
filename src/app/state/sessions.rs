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
            }
        } else if !self.sessions.is_empty() {
            self.save_current_terminal();
            self.selected_session = Some(0);
            self.load_session_output();
        }
    }

    pub fn select_prev_session(&mut self) {
        if let Some(idx) = self.selected_session {
            if idx > 0 {
                self.save_current_terminal(); // Save BEFORE changing selection
                self.selected_session = Some(idx - 1);
                self.load_session_output();
            }
        }
    }

    pub fn create_new_session(&mut self, prompt: String) -> anyhow::Result<Session> {
        let Some(project) = self.project.clone() else {
            anyhow::bail!("No project loaded")
        };

        // Generate session name from prompt
        let name = generate_session_name(&prompt);
        let worktree_name = sanitize_for_branch(&name);
        let branch_name = format!("azural/{}", worktree_name);
        let worktree_path = project.worktrees_dir().join(&worktree_name);

        if worktree_path.exists() {
            anyhow::bail!("Worktree already exists: {}", worktree_path.display());
        }

        // Create git worktree
        Git::create_worktree(&project.path, &worktree_path, &branch_name)?;

        let session = Session {
            branch_name: branch_name.clone(),
            worktree_path: Some(worktree_path),
            claude_session_id: None,
            archived: false,
        };

        self.refresh_sessions()?;

        // Select the new session
        if let Some(idx) = self.sessions.iter().position(|s| s.branch_name == branch_name) {
            self.save_current_terminal(); // Save before switching
            self.selected_session = Some(idx);
            self.load_session_output();
        }

        Ok(session)
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
}
