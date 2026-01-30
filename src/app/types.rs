//! App type definitions (enums, dialogs, menus)

use std::path::PathBuf;

use crate::models::SessionStatus;

/// Viewer pane display mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewerMode {
    #[default]
    Empty, // Nothing selected
    File, // Showing file from FileTree
    Diff, // Showing diff from Output
}

/// Entry in the file tree (file or directory)
#[derive(Debug, Clone)]
pub struct FileTreeEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
}

/// State for the branch selection dialog
pub struct BranchDialog {
    pub branches: Vec<String>,
    pub selected: usize,
    pub filter: String,
    pub filtered_indices: Vec<usize>,
}

impl BranchDialog {
    pub fn new(branches: Vec<String>) -> Self {
        let filtered_indices: Vec<usize> = (0..branches.len()).collect();
        Self { branches, selected: 0, filter: String::new(), filtered_indices }
    }

    pub fn apply_filter(&mut self) {
        let filter_lower = self.filter.to_lowercase();
        self.filtered_indices = self.branches.iter().enumerate()
            .filter(|(_, b)| b.to_lowercase().contains(&filter_lower))
            .map(|(i, _)| i)
            .collect();
        if self.selected >= self.filtered_indices.len() { self.selected = 0; }
    }

    pub fn selected_branch(&self) -> Option<&String> {
        self.filtered_indices.get(self.selected).and_then(|&idx| self.branches.get(idx))
    }

    pub fn select_next(&mut self) {
        if !self.filtered_indices.is_empty() && self.selected + 1 < self.filtered_indices.len() {
            self.selected += 1;
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected > 0 { self.selected -= 1; }
    }

    pub fn filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.apply_filter();
    }

    pub fn filter_backspace(&mut self) {
        self.filter.pop();
        self.apply_filter();
    }
}

/// Context menu for session actions
#[derive(Debug, Clone)]
pub struct ContextMenu {
    pub actions: Vec<SessionAction>,
    pub selected: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionAction {
    Start,
    Stop,
    Archive,
    Delete,
    ViewDiff,
    RebaseFromMain,
    OpenInEditor,
    CopyWorktreePath,
}

impl SessionAction {
    pub fn label(&self) -> &'static str {
        match self {
            SessionAction::Start => "Start/Resume Session",
            SessionAction::Stop => "Stop Session",
            SessionAction::Archive => "Archive Session",
            SessionAction::Delete => "Delete Session",
            SessionAction::ViewDiff => "View Diff",
            SessionAction::RebaseFromMain => "Rebase from Main",
            SessionAction::OpenInEditor => "Open in Editor",
            SessionAction::CopyWorktreePath => "Copy Worktree Path",
        }
    }

    pub fn key_hint(&self) -> &'static str {
        match self {
            SessionAction::Start => "Enter",
            SessionAction::Stop => "s",
            SessionAction::Archive => "a",
            SessionAction::Delete => "D",
            SessionAction::ViewDiff => "d",
            SessionAction::RebaseFromMain => "r",
            SessionAction::OpenInEditor => "e",
            SessionAction::CopyWorktreePath => "c",
        }
    }

    pub fn available_for_status(status: SessionStatus) -> Vec<SessionAction> {
        match status {
            SessionStatus::Pending | SessionStatus::Stopped | SessionStatus::Completed => vec![
                SessionAction::Start,
                SessionAction::ViewDiff,
                SessionAction::RebaseFromMain,
                SessionAction::OpenInEditor,
                SessionAction::CopyWorktreePath,
                SessionAction::Archive,
                SessionAction::Delete,
            ],
            SessionStatus::Running | SessionStatus::Waiting => vec![
                SessionAction::Stop,
                SessionAction::ViewDiff,
                SessionAction::OpenInEditor,
                SessionAction::CopyWorktreePath,
            ],
            SessionStatus::Failed => vec![
                SessionAction::Start,
                SessionAction::ViewDiff,
                SessionAction::RebaseFromMain,
                SessionAction::OpenInEditor,
                SessionAction::CopyWorktreePath,
                SessionAction::Archive,
                SessionAction::Delete,
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Output,
    Diff,
    Messages,
    Rebase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Worktrees,
    FileTree,
    Viewer,
    Output,
    Input,
    WorktreeCreation,
    BranchDialog,
}
