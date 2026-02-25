use serde::{Deserialize, Serialize};
use crate::tui::util::AZURE;
use std::path::PathBuf;

/// A project represents a git repository (derived from current working directory)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    pub main_branch: String,
}

impl Project {
    /// Create a project from a git repo path.
    /// Uses display_name if provided, otherwise falls back to folder name.
    pub fn from_path(path: PathBuf, main_branch: String) -> Self {
        let name = crate::config::project_display_name(&path)
            .unwrap_or_else(|| path.file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "unnamed".to_string()));
        Self { name, path, main_branch }
    }

    pub fn worktrees_dir(&self) -> PathBuf {
        self.path.join("worktrees")
    }
}

/// Session status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorktreeStatus {
    Pending,
    Running,
    Waiting,
    Stopped,
    Completed,
    Failed,
}

impl WorktreeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorktreeStatus::Pending => "pending",
            WorktreeStatus::Running => "running",
            WorktreeStatus::Waiting => "waiting",
            WorktreeStatus::Stopped => "stopped",
            WorktreeStatus::Completed => "completed",
            WorktreeStatus::Failed => "failed",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            WorktreeStatus::Pending => "○",
            WorktreeStatus::Running => "●",
            WorktreeStatus::Waiting => "○",
            WorktreeStatus::Stopped => "◌",
            WorktreeStatus::Completed => "✓",
            WorktreeStatus::Failed => "✗",
        }
    }

    pub fn color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            WorktreeStatus::Pending => Color::Gray,
            WorktreeStatus::Running => Color::Green,
            WorktreeStatus::Waiting => Color::Yellow,
            WorktreeStatus::Stopped => Color::Gray,
            WorktreeStatus::Completed => AZURE,
            WorktreeStatus::Failed => Color::Red,
        }
    }
}

/// A worktree represents a git worktree paired with an optional Claude session.
/// Derived from git worktrees + Claude session files (stateless).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    /// Branch name (e.g., "azureal/tui-help-overlay")
    pub branch_name: String,
    /// Worktree path (None if archived - branch exists but no worktree)
    pub worktree_path: Option<PathBuf>,
    /// Claude CLI session ID for --resume (read from Claude's session file)
    pub claude_session_id: Option<String>,
    /// Whether this is an archived worktree (branch exists, no worktree dir)
    pub archived: bool,
}

impl Worktree {
    /// Display name (branch name without azureal/ prefix)
    pub fn name(&self) -> &str {
        self.branch_name.strip_prefix("azureal/").unwrap_or(&self.branch_name)
    }

    /// Worktree status (derived from runtime state, not stored).
    /// `is_running` = whether any Claude process is active on this branch.
    pub fn status(&self, is_running: bool) -> WorktreeStatus {
        if self.archived {
            WorktreeStatus::Stopped
        } else if is_running {
            WorktreeStatus::Running
        } else if self.claude_session_id.is_some() {
            WorktreeStatus::Waiting
        } else {
            WorktreeStatus::Pending
        }
    }
}

/// Output type for Claude events
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputType {
    Stdout,
    Stderr,
    System,
    Json,
    Error,
    Hook,
}

/// Git diff information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffInfo {
    pub session_id: String,
    pub diff_text: String,
    pub files_changed: Vec<String>,
    pub additions: i32,
    pub deletions: i32,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Result of a rebase operation
#[derive(Debug, Clone)]
pub enum RebaseResult {
    /// Rebase was aborted
    Aborted,
    /// Rebase failed with an error (message for future display)
    Failed(#[allow(dead_code)] String),
}
