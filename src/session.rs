use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::path::PathBuf;
use uuid::Uuid;

use crate::db::Database;
use crate::git::Git;
use crate::models::{Project, Session, SessionStatus};

/// Manages session lifecycle
pub struct SessionManager<'a> {
    db: &'a Database,
}

impl<'a> SessionManager<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Create a new session with a worktree
    pub fn create_session(&self, project: &Project, prompt: &str) -> Result<Session> {
        // Validate project path exists and is a git repo
        if !project.path.exists() {
            bail!("Project path does not exist: {}", project.path.display());
        }

        if !Git::is_git_repo(&project.path) {
            bail!("Project path is not a git repository: {}", project.path.display());
        }

        // Generate session ID and name
        let session_id = Uuid::new_v4().to_string();
        let session_name = generate_session_name(prompt);
        let worktree_name = sanitize_for_branch(&session_name);
        let branch_name = format!("crystal/{}", worktree_name);

        // Calculate worktree path
        let worktree_path = project.worktrees_dir().join(&worktree_name);

        // Check if worktree already exists
        if worktree_path.exists() {
            bail!("Worktree already exists: {}", worktree_path.display());
        }

        // Create the worktree
        Git::create_worktree(&project.path, &worktree_path, &branch_name)
            .context("Failed to create git worktree")?;

        // Create session record
        let now = Utc::now();
        let session = Session {
            id: session_id,
            name: session_name,
            initial_prompt: prompt.to_string(),
            worktree_name,
            worktree_path,
            branch_name,
            status: SessionStatus::Pending,
            project_id: project.id,
            pid: None,
            exit_code: None,
            archived: false,
            created_at: now,
            updated_at: now,
        };

        self.db.create_session(&session)?;

        Ok(session)
    }

    /// Delete a session and its worktree
    pub fn delete_session(&self, session: &Session, project: &Project) -> Result<()> {
        // Remove worktree
        if session.worktree_path.exists() {
            Git::remove_worktree(&project.path, &session.worktree_path)
                .context("Failed to remove worktree")?;
        }

        // Delete from database
        self.db.delete_session(&session.id)?;

        Ok(())
    }

    /// Archive a session (keeps worktree but marks as archived)
    pub fn archive_session(&self, session_id: &str) -> Result<()> {
        self.db.archive_session(session_id)
    }

    /// Update session status
    pub fn update_status(&self, session_id: &str, status: SessionStatus) -> Result<()> {
        self.db.update_session_status(session_id, status)
    }

    /// Get session by ID
    pub fn get_session(&self, session_id: &str) -> Result<Option<Session>> {
        self.db.get_session(session_id)
    }

    /// List all active sessions
    pub fn list_sessions(&self) -> Result<Vec<Session>> {
        self.db.list_sessions()
    }

    /// List sessions for a project
    pub fn list_sessions_for_project(&self, project_id: i64) -> Result<Vec<Session>> {
        self.db.list_sessions_for_project(project_id)
    }
}

/// Generate a session name from the prompt
fn generate_session_name(prompt: &str) -> String {
    // Take first 30 chars of prompt, clean it up
    let name: String = prompt
        .chars()
        .take(40)
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-' || *c == '_')
        .collect();

    let name = name.trim();

    if name.is_empty() {
        format!("session-{}", &Uuid::new_v4().to_string()[..8])
    } else {
        // Truncate at word boundary if possible
        let name = if name.len() > 30 {
            if let Some(pos) = name[..30].rfind(' ') {
                &name[..pos]
            } else {
                &name[..30]
            }
        } else {
            name
        };
        name.to_string()
    }
}

/// Sanitize a string for use as a git branch name
fn sanitize_for_branch(s: &str) -> String {
    let sanitized: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();

    // Remove consecutive dashes and trim
    let mut result = String::new();
    let mut last_was_dash = false;

    for c in sanitized.chars() {
        if c == '-' {
            if !last_was_dash && !result.is_empty() {
                result.push(c);
                last_was_dash = true;
            }
        } else {
            result.push(c);
            last_was_dash = false;
        }
    }

    // Trim trailing dashes
    result.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_session_name() {
        assert_eq!(
            generate_session_name("Fix the login bug"),
            "Fix the login bug"
        );

        let long_prompt = "This is a very long prompt that should be truncated at a reasonable word boundary";
        let name = generate_session_name(long_prompt);
        assert!(name.len() <= 40);
    }

    #[test]
    fn test_sanitize_for_branch() {
        assert_eq!(sanitize_for_branch("Hello World"), "hello-world");
        assert_eq!(sanitize_for_branch("Fix bug #123"), "fix-bug-123");
        assert_eq!(sanitize_for_branch("  spaces  "), "spaces");
    }
}
