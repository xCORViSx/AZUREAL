//! GitHub Issues panel types — issue data, panel state, and creation session

/// A GitHub issue fetched from `gh issue list`
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct GhIssue {
    pub number: u32,
    pub title: String,
    pub labels: Vec<String>,
    pub state: String,
    pub author: String,
    pub created_at: String,
    pub url: String,
}

/// State for the Issues panel modal overlay (Shift+I)
pub struct IssuesPanel {
    /// Fetched issues from `gh issue list`
    pub issues: Vec<GhIssue>,
    /// Navigation cursor
    pub selected: usize,
    /// Scroll offset
    pub scroll: usize,
    /// Filter text (/ to activate, case-insensitive)
    pub filter: String,
    /// Whether the filter bar is active
    pub filter_active: bool,
    /// Cursor position in filter text
    pub filter_cursor: usize,
    /// Filtered indices into `issues` vec
    pub filtered_indices: Vec<usize>,
    /// Error message from `gh` CLI (shown instead of list)
    pub error: Option<String>,
    /// Receiver for background gh issue fetch
    pub fetch_receiver: Option<std::sync::mpsc::Receiver<Result<Vec<GhIssue>, String>>>,
    /// True while the initial fetch is in-flight
    pub loading: bool,
}

/// Active issue creation session — mirrors RcrSession pattern
pub struct IssueSession {
    /// Slot ID (PID string) of the agent process — empty until first prompt spawns agent
    pub slot_id: String,
    /// Claude API session UUID (set when SessionId event arrives)
    pub session_id: Option<String>,
    /// True when agent exited and we're awaiting user approval
    pub approval_pending: bool,
    /// Worktree path where the agent runs
    pub worktree_path: std::path::PathBuf,
    /// True when the agent found a duplicate and we should block further prompts
    #[allow(dead_code)]
    pub duplicate_detected: bool,
    /// Cached issues JSON from the panel (preserved after panel closes for prompt injection)
    pub cached_issues_json: String,
    /// SQLite store session ID created for multi-turn context (deleted on accept/abort)
    pub store_session_id: Option<i64>,
    /// Previous current_session_id saved before issue session (restored on accept/abort)
    pub saved_session_id: Option<i64>,
}

/// Parsed issue data extracted from `<azureal-issue>` tags
#[derive(Debug, Clone)]
pub struct ParsedIssue {
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
}
