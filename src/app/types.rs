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
    pub is_hidden: bool,
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

/// A saved run command
#[derive(Debug, Clone)]
pub struct RunCommand {
    pub name: String,
    pub command: String,
}

impl RunCommand {
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self { name: name.into(), command: command.into() }
    }
}

/// Dialog for creating/editing run commands
#[derive(Debug, Clone)]
pub struct RunCommandDialog {
    pub name: String,
    pub command: String,
    pub name_cursor: usize,
    pub command_cursor: usize,
    pub editing_name: bool,
    pub editing_idx: Option<usize>,
}

impl RunCommandDialog {
    pub fn new() -> Self {
        Self { name: String::new(), command: String::new(), name_cursor: 0, command_cursor: 0, editing_name: true, editing_idx: None }
    }

    pub fn edit(idx: usize, cmd: &RunCommand) -> Self {
        Self { name: cmd.name.clone(), command: cmd.command.clone(), name_cursor: cmd.name.len(), command_cursor: cmd.command.len(), editing_name: true, editing_idx: Some(idx) }
    }
}

/// Picker for selecting from saved run commands
#[derive(Debug, Clone)]
pub struct RunCommandPicker {
    pub selected: usize,
}

impl RunCommandPicker {
    pub fn new() -> Self { Self { selected: 0 } }
}

/// A viewer tab holding file state
#[derive(Debug, Clone)]
pub struct ViewerTab {
    pub path: Option<PathBuf>,
    pub content: Option<String>,
    pub scroll: usize,
    pub mode: ViewerMode,
    pub title: String,
}

impl ViewerTab {
    pub fn name(&self) -> &str {
        &self.title
    }
}
