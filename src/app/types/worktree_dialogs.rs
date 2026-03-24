//! Worktree dialog types — rename, delete, table popup, and refresh result

use ratatui::text::Line;

/// Full-width table popup overlay (click a table in session pane to open).
/// Pre-rendered at popup width so columns aren't truncated.
#[derive(Debug, Clone)]
pub struct TablePopup {
    pub lines: Vec<Line<'static>>,
    pub scroll: usize,
    pub total_lines: usize,
}

/// Rename worktree dialog — text input for new branch suffix.
/// The full branch name is `{prefix}/{input}`.
#[derive(Debug, Clone)]
pub struct RenameWorktreeDialog {
    /// Display name shown in title (strip_branch_prefix result)
    pub old_name: String,
    /// User-typed new name (suffix only, no prefix)
    pub input: String,
    /// Cursor byte offset within `input`
    pub cursor: usize,
}

/// Delete worktree confirmation dialog (⌘d). Two variants:
/// - Sole: only worktree on this branch — confirm delete worktree + branch
/// - Siblings: other worktrees exist on same branch — choose delete-all or archive-only
#[derive(Debug, Clone)]
pub enum DeleteWorktreeDialog {
    /// Sole worktree on branch — simple yes/no
    Sole {
        name: String,
        /// Yellow warnings shown before action keys (uncommitted changes, unmerged commits)
        warnings: Vec<String>,
    },
    /// Multiple worktrees on branch — choose (y)delete-all or (a)archive-only
    Siblings {
        branch: String,
        sibling_indices: Vec<usize>,
        count: usize,
        /// Yellow warnings shown before action keys (uncommitted changes, unmerged commits)
        warnings: Vec<String>,
    },
}

/// Result from background worktree refresh (git + FS I/O done off main thread)
pub struct WorktreeRefreshResult {
    /// Main branch worktree (accessed via 'M' browse mode)
    pub main_worktree: Option<crate::models::Worktree>,
    /// Feature + archived worktrees (sidebar entries)
    pub worktrees: Vec<crate::models::Worktree>,
}
