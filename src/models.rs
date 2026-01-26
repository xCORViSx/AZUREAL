use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A project represents a git repository that can have multiple sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub path: PathBuf,
    pub system_prompt: Option<String>,
    pub main_branch: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Project {
    pub fn worktrees_dir(&self) -> PathBuf {
        self.path.join(".worktrees")
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
            SessionStatus::Waiting => "◐",
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub initial_prompt: String,
    pub worktree_name: String,
    pub worktree_path: PathBuf,
    pub branch_name: String,
    pub status: SessionStatus,
    pub project_id: i64,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Output from a session (terminal output)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionOutput {
    pub id: i64,
    pub session_id: String,
    pub output_type: OutputType,
    pub data: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputType {
    Stdout,
    Stderr,
    System,
    Json,
    Error,
}

impl OutputType {
    pub fn as_str(&self) -> &'static str {
        match self {
            OutputType::Stdout => "stdout",
            OutputType::Stderr => "stderr",
            OutputType::System => "system",
            OutputType::Json => "json",
            OutputType::Error => "error",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "stdout" => OutputType::Stdout,
            "stderr" => OutputType::Stderr,
            "system" => OutputType::System,
            "json" => OutputType::Json,
            "error" => OutputType::Error,
            _ => OutputType::Stdout,
        }
    }
}

/// A conversation message (for resuming sessions)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub id: i64,
    pub session_id: String,
    pub message_type: MessageType,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageType {
    User,
    Assistant,
}

impl MessageType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageType::User => "user",
            MessageType::Assistant => "assistant",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "user" => MessageType::User,
            "assistant" => MessageType::Assistant,
            _ => MessageType::User,
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
    pub timestamp: DateTime<Utc>,
}
