//! Git Actions panel types — commits, changed files, overlays, and background operation types

use std::path::PathBuf;

/// A commit entry for the Git panel's commit log pane
#[derive(Debug, Clone)]
pub struct GitCommit {
    /// Short hash (7 chars)
    pub hash: String,
    /// Full hash (for `git show`)
    pub full_hash: String,
    /// First line of commit message
    pub subject: String,
    /// Whether this commit has been pushed to the remote
    pub is_pushed: bool,
}

/// A changed file entry in the Git Actions panel (from `git diff --stat main...HEAD`)
#[derive(Debug, Clone)]
pub struct GitChangedFile {
    /// Relative file path
    pub path: String,
    /// Git status indicator: M=Modified, A=Added, D=Deleted, R=Renamed
    pub status: char,
    /// Lines added in this file
    pub additions: usize,
    /// Lines deleted in this file
    pub deletions: usize,
    /// Whether the file is staged (git add) — unstaged files shown with strikethrough
    pub staged: bool,
}

/// Result from the one-shot commit-message generator worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedCommitMessage {
    /// The generated commit message text inserted into the editor.
    pub message: String,
    /// Human-readable model/helper label shown in the git status box.
    pub generator_label: String,
    /// Optional notice when generation succeeded only after backend fallback.
    pub fallback_notice: Option<String>,
}

/// Commit message overlay state — shown when pressing `c` in the Git panel.
/// Claude or Codex generates the message via a one-shot background CLI call,
/// and the user can edit before committing.
#[derive(Debug)]
pub struct GitCommitOverlay {
    /// The editable commit message text (may be multi-line)
    pub message: String,
    /// Cursor position as char index within message
    pub cursor: usize,
    /// True while the background generator is still producing the message
    pub generating: bool,
    /// Scroll offset for displaying long messages
    pub scroll: usize,
    /// Receiver for the generated message from the background thread.
    /// Ok(message + generator metadata), Err(error_if_both_backends_failed).
    pub receiver: Option<std::sync::mpsc::Receiver<Result<GeneratedCommitMessage, String>>>,
}

/// Conflict resolution overlay — shown when squash merge encounters conflicts.
/// Displays conflicted/auto-merged file lists and offers Claude-assisted resolution.
#[derive(Debug)]
pub struct GitConflictOverlay {
    /// Files with CONFLICT markers that need resolution
    pub conflicted_files: Vec<String>,
    /// Files that git auto-merged cleanly
    pub auto_merged_files: Vec<String>,
    /// Scroll offset for the file list display
    pub scroll: usize,
    /// Selected action: 0 = "Resolve with Claude", 1 = "Abort rebase"
    pub selected: usize,
    /// When true, auto-proceed with squash merge after conflict resolution.
    /// Set when the rebase was triggered by exec_squash_merge().
    pub continue_with_merge: bool,
}

/// Merge Conflict Resolution session state. Tracks an active RCR flow so the
/// session pane routes prompts to the correct working directory (feature branch
/// worktree), shows green borders, and displays the approval dialog after
/// Claude exits. RCR resolves rebase conflicts on the feature branch — Claude
/// runs in the worktree directory, not repo root.
#[derive(Debug, Clone)]
pub struct RcrSession {
    /// Feature branch name this RCR is resolving (e.g. "{prefix}/health")
    pub branch: String,
    /// Display name for the title (branch without "{prefix}/" prefix)
    pub display_name: String,
    /// Feature branch worktree path — Claude's working directory during RCR
    pub worktree_path: std::path::PathBuf,
    /// Repo root path (main worktree) — for session file cleanup
    pub repo_root: std::path::PathBuf,
    /// Slot ID (PID string) of the current RCR Claude process
    pub slot_id: String,
    /// Claude API session UUID (set when SessionId event arrives, used for --resume + cleanup)
    pub session_id: Option<String>,
    /// True when Claude has exited and we're awaiting user approval
    pub approval_pending: bool,
    /// When true, auto-proceed with squash merge after rebase RCR completes.
    /// Set when the rebase was triggered by exec_squash_merge(), not manual rebase.
    pub continue_with_merge: bool,
}

/// Post-merge dialog shown after a successful squash merge. Asks the user
/// whether to keep (rebase), archive (remove worktree, keep branch), or
/// delete (remove worktree + delete branch) the feature worktree.
#[derive(Debug)]
pub struct PostMergeDialog {
    /// Branch name being merged (e.g. "{prefix}/health")
    pub branch: String,
    /// Display name for the dialog (without "{prefix}/" prefix)
    pub display_name: String,
    /// Worktree path on disk (needed for archive/delete)
    pub worktree_path: std::path::PathBuf,
    /// Currently selected option: 0=Keep, 1=Archive, 2=Delete
    pub selected: usize,
}

/// State for the Git Actions panel (Shift+G — full-app layout).
/// Actions are context-aware: main branch gets pull+commit+push,
/// feature branches get squash-merge+rebase+commit+push.
#[derive(Debug)]
pub struct GitActionsPanel {
    /// Current worktree name (branch) shown in the title
    pub worktree_name: String,
    /// Worktree path on disk (for running git commands without reborrowing App)
    pub worktree_path: std::path::PathBuf,
    /// Repo root path (main worktree, always on main branch — for squash-merge)
    pub repo_root: std::path::PathBuf,
    /// Main branch name (for diff base)
    pub main_branch: String,
    /// Whether the panel was opened on the main/master branch (changes available actions)
    pub is_on_main: bool,
    /// Changed files from git diff --stat against main
    pub changed_files: Vec<GitChangedFile>,
    /// Selected file index in the file list
    pub selected_file: usize,
    /// Scroll offset for the file list
    pub file_scroll: usize,
    /// Which pane has focus: 0=Actions, 1=Files, 2=Commits. Tab cycles.
    pub focused_pane: u8,
    /// Selected action index when focused_pane==0
    pub selected_action: usize,
    /// Transient status/result message from last git operation: (message, is_error)
    pub result_message: Option<(String, bool)>,
    /// Commit message overlay — shown when `c` is pressed in the actions list.
    /// Claude generates a conventional commit message from `git diff --staged`.
    pub commit_overlay: Option<GitCommitOverlay>,
    /// Conflict resolution overlay — shown when rebase hits conflicts.
    /// Offers Claude-assisted resolution or rebase abort.
    pub conflict_overlay: Option<GitConflictOverlay>,
    /// Recent commits from `git log` (displayed in the commits pane)
    pub commits: Vec<GitCommit>,
    /// Selected commit index in the commits pane
    pub selected_commit: usize,
    /// Scroll offset for the commits pane
    pub commit_scroll: usize,
    /// Diff text shown in the viewer pane (file diff or commit diff)
    pub viewer_diff: Option<String>,
    /// Title for the viewer pane diff (e.g. "diff: path" or "commit: abc1234")
    pub viewer_diff_title: Option<String>,
    /// How many commits on main are not yet in this branch
    pub commits_behind_main: usize,
    /// How many commits this branch has that main doesn't
    pub commits_ahead_main: usize,
    /// How many commits the remote tracking branch has that we don't
    pub commits_behind_remote: usize,
    /// How many local commits not yet pushed to remote
    pub commits_ahead_remote: usize,
    /// Files that are auto-resolved during rebase via union merge (cached from azufig)
    pub auto_resolve_files: Vec<String>,
    /// Auto-resolve settings overlay (opened with `s` in actions pane)
    pub auto_resolve_overlay: Option<AutoResolveOverlay>,
    /// Receiver for squash merge progress from a background thread
    pub squash_merge_receiver: Option<std::sync::mpsc::Receiver<SquashMergeProgress>>,
    /// When Some, a discard confirmation is pending for the file at this index
    pub discard_confirm: Option<usize>,
    /// Cached file stats — recomputed when changed_files or staged flags mutate,
    /// avoids three `.iter()` passes per frame in draw_git_sidebar.
    pub cached_staged_count: usize,
    pub cached_total_add: usize,
    pub cached_total_del: usize,
}

impl GitActionsPanel {
    /// Recompute cached file stats from changed_files.
    /// Call after any mutation to changed_files or staged flags.
    pub fn recompute_file_stats(&mut self) {
        self.cached_staged_count = self.changed_files.iter().filter(|f| f.staged).count();
        self.cached_total_add = self.changed_files.iter().map(|f| f.additions).sum();
        self.cached_total_del = self.changed_files.iter().map(|f| f.deletions).sum();
    }
}

/// Progress update from the squash merge background thread
#[derive(Debug)]
pub struct SquashMergeProgress {
    /// Current phase message (e.g. "Rebasing onto main...")
    pub phase: String,
    /// Final outcome — None while phases are still running
    pub outcome: Option<SquashMergeOutcome>,
}

/// Final result of a background squash merge
#[derive(Debug)]
pub enum SquashMergeOutcome {
    /// Merge succeeded — ready to show post-merge dialog
    Success {
        status_msg: String,
        branch: String,
        display_name: String,
        worktree_path: PathBuf,
    },
    /// Rebase or merge hit conflicts — show conflict overlay
    Conflict {
        conflicted: Vec<String>,
        auto_merged: Vec<String>,
    },
    /// Operation failed with an error message
    Failed(String),
}

/// Auto-resolve settings overlay — manage which files are auto-resolved during
/// rebase via union merge (keeps both sides' changes, no conflict markers).
#[derive(Debug)]
pub struct AutoResolveOverlay {
    /// (filename, enabled) — files in the auto-resolve list
    pub files: Vec<(String, bool)>,
    /// Selected row index
    pub selected: usize,
    /// True when user is typing a new filename to add
    pub adding: bool,
    /// Input buffer for the new filename being typed
    pub input_buffer: String,
    /// Cursor position within input_buffer
    pub input_cursor: usize,
}

/// Progress update from a background worktree/git operation.
/// Used for archive, unarchive, create, delete, pull, push, rebase — any
/// blocking operation that needs a progress dialog.
#[derive(Debug)]
pub struct BackgroundOpProgress {
    /// Phase message shown in the loading dialog (e.g. "Archiving worktree...")
    pub phase: String,
    /// Final outcome — None while the operation is still running
    pub outcome: Option<BackgroundOpOutcome>,
}

/// Result payload for an RCR accept/abort operation that finished in the
/// background. The UI restores the normal session pane and optionally opens
/// the post-merge dialog.
#[derive(Debug)]
pub struct RcrCompletion {
    pub status_msg: String,
    pub post_merge_dialog: Option<PostMergeDialog>,
}

/// Final result of a background worktree/git operation.
/// Each variant carries enough data for the event loop to do post-processing
/// (refresh worktrees, select branch, clean up state, etc.)
#[derive(Debug)]
pub enum BackgroundOpOutcome {
    /// Worktree archived — refresh and maintain current selection
    Archived,
    /// Worktree unarchived — refresh and select the named branch
    Unarchived {
        branch: String,
        display_name: String,
    },
    /// Worktree created — refresh and select the named branch
    Created { branch: String },
    /// Worktree deleted — refresh and clamp selection (state cleanup done before spawn)
    Deleted {
        display_name: String,
        prev_idx: usize,
    },
    /// Worktree renamed — refresh and re-select the branch
    Renamed { new_branch: String },
    /// Git panel operation result (pull, push) — set result_message + refresh
    GitResult { message: String, is_error: bool },
    /// RCR accept/abort finished — restore the normal session pane
    RcrFinished(RcrCompletion),
    /// Operation failed — show error in status bar
    Failed(String),
}

/// Result of a background rebase, sent via mpsc to the event loop.
/// Separate from `BackgroundOpOutcome` because rebase has conflict handling
/// that needs different post-processing (conflict overlay, not just status).
#[derive(Debug)]
pub enum BackgroundRebaseOutcome {
    /// Rebase succeeded + push result
    Rebased(String),
    /// Already up to date with main
    UpToDate,
    /// Rebase hit conflicts — show conflict overlay
    Conflict {
        conflicted: Vec<String>,
        auto_merged: Vec<String>,
    },
    /// Rebase failed with error
    Failed(String),
}
