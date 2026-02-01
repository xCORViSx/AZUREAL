use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A project represents a git repository (derived from current working directory)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    pub main_branch: String,
}

impl Project {
    /// Create a project from a git repo path
    pub fn from_path(path: PathBuf, main_branch: String) -> Self {
        let name = path.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed".to_string());
        Self { name, path, main_branch }
    }

    pub fn worktrees_dir(&self) -> PathBuf {
        self.path.join("worktrees")
    }
}

/// Session status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    Pending,
    Running,
    Waiting,
    Stopped,
    Completed,
    Failed,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionStatus::Pending => "pending",
            SessionStatus::Running => "running",
            SessionStatus::Waiting => "waiting",
            SessionStatus::Stopped => "stopped",
            SessionStatus::Completed => "completed",
            SessionStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "pending" => SessionStatus::Pending,
            "running" => SessionStatus::Running,
            "waiting" => SessionStatus::Waiting,
            "stopped" => SessionStatus::Stopped,
            "completed" => SessionStatus::Completed,
            "failed" => SessionStatus::Failed,
            _ => SessionStatus::Pending,
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            SessionStatus::Pending => "○",
            SessionStatus::Running => "●",
            SessionStatus::Waiting => "○",
            SessionStatus::Stopped => "◌",
            SessionStatus::Completed => "✓",
            SessionStatus::Failed => "✗",
        }
    }

    pub fn color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            SessionStatus::Pending => Color::Gray,
            SessionStatus::Running => Color::Green,
            SessionStatus::Waiting => Color::Yellow,
            SessionStatus::Stopped => Color::Gray,
            SessionStatus::Completed => Color::Cyan,
            SessionStatus::Failed => Color::Red,
        }
    }
}

/// A session represents a single Claude Code conversation in a worktree
/// Derived from git worktrees + Claude session files (stateless)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Branch name (e.g., "azural/tui-help-overlay")
    pub branch_name: String,
    /// Worktree path (None if archived - branch exists but no worktree)
    pub worktree_path: Option<PathBuf>,
    /// Claude CLI session ID for --resume (read from Claude's session file)
    pub claude_session_id: Option<String>,
    /// Whether this is an archived session (branch exists, no worktree)
    pub archived: bool,
}

impl Session {
    /// Display name (branch name without azural/ prefix)
    pub fn name(&self) -> &str {
        self.branch_name.strip_prefix("azural/").unwrap_or(&self.branch_name)
    }

    /// Session status (derived from runtime state, not stored)
    pub fn status(&self, running_sessions: &std::collections::HashSet<String>) -> SessionStatus {
        if self.archived {
            SessionStatus::Stopped
        } else if running_sessions.contains(&self.branch_name) {
            SessionStatus::Running
        } else if self.claude_session_id.is_some() {
            SessionStatus::Waiting
        } else {
            SessionStatus::Pending
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

impl OutputType {
    pub fn as_str(&self) -> &'static str {
        match self {
            OutputType::Stdout => "stdout",
            OutputType::Stderr => "stderr",
            OutputType::System => "system",
            OutputType::Json => "json",
            OutputType::Error => "error",
            OutputType::Hook => "hook",
        }
    }
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

/// Rebase state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RebaseState {
    /// No rebase in progress
    None,
    /// Rebase is in progress (may have conflicts)
    InProgress,
    /// Rebase is paused due to conflicts
    Conflicts,
    /// Rebase completed successfully
    Completed,
    /// Rebase was aborted
    Aborted,
}

impl RebaseState {
    pub fn as_str(&self) -> &'static str {
        match self {
            RebaseState::None => "none",
            RebaseState::InProgress => "in_progress",
            RebaseState::Conflicts => "conflicts",
            RebaseState::Completed => "completed",
            RebaseState::Aborted => "aborted",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            RebaseState::None => " ",
            RebaseState::InProgress => "↻",
            RebaseState::Conflicts => "⚠",
            RebaseState::Completed => "✓",
            RebaseState::Aborted => "✗",
        }
    }

    pub fn color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            RebaseState::None => Color::Gray,
            RebaseState::InProgress => Color::Yellow,
            RebaseState::Conflicts => Color::Red,
            RebaseState::Completed => Color::Green,
            RebaseState::Aborted => Color::Magenta,
        }
    }
}

/// Detailed rebase status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebaseStatus {
    /// Current rebase state
    pub state: RebaseState,
    /// Branch being rebased onto (if rebasing)
    pub onto_branch: Option<String>,
    /// Original branch name (head being rebased)
    pub head_name: Option<String>,
    /// Current step number in rebase (1-indexed)
    pub current_step: Option<usize>,
    /// Total number of steps in rebase
    pub total_steps: Option<usize>,
    /// Files with conflicts (if any)
    pub conflicted_files: Vec<String>,
    /// Current commit being applied (short hash)
    pub current_commit: Option<String>,
    /// Current commit message being applied
    pub current_commit_message: Option<String>,
}

impl Default for RebaseStatus {
    fn default() -> Self {
        Self {
            state: RebaseState::None,
            onto_branch: None,
            head_name: None,
            current_step: None,
            total_steps: None,
            conflicted_files: Vec::new(),
            current_commit: None,
            current_commit_message: None,
        }
    }
}

/// Result of a rebase operation
#[derive(Debug, Clone)]
pub enum RebaseResult {
    /// Rebase completed successfully
    Success,
    /// Rebase has conflicts that need resolution
    Conflicts(RebaseStatus),
    /// Rebase was aborted
    Aborted,
    /// Rebase failed with an error
    Failed(String),
    /// Nothing to rebase (already up to date)
    UpToDate,
}
