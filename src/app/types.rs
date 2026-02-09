//! App type definitions (enums, dialogs, menus)

use std::path::PathBuf;

use crate::config::ProjectEntry;
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

/// Maps sidebar visual rows to clickable actions for mouse click handling.
/// Built alongside sidebar_cache in draw_sidebar::build_sidebar_items().
#[derive(Debug, Clone)]
pub enum SidebarRowAction {
    /// A session/worktree row — index into app.sessions
    Session(usize),
    /// An expanded session file row — (session_idx, file_idx)
    SessionFile(usize, usize),
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

/// Which mode the Projects panel is in
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectsPanelMode {
    /// Browsing the project list
    Browse,
    /// Adding a new project by entering a path
    AddPath,
    /// Renaming a project's display name
    Rename,
    /// Initializing a new git repo at a path
    Init,
}

/// Full-screen Projects panel state (shown on startup without git repo, or via 'P')
#[derive(Debug)]
pub struct ProjectsPanel {
    pub entries: Vec<ProjectEntry>,
    pub selected: usize,
    pub mode: ProjectsPanelMode,
    /// Text input buffer for Add/Rename/Init modes
    pub input: String,
    pub input_cursor: usize,
    /// Transient error message (cleared on next action)
    pub error: Option<String>,
}

impl ProjectsPanel {
    pub fn new(entries: Vec<ProjectEntry>) -> Self {
        Self { entries, selected: 0, mode: ProjectsPanelMode::Browse, input: String::new(), input_cursor: 0, error: None }
    }

    pub fn select_next(&mut self) {
        if self.selected + 1 < self.entries.len() { self.selected += 1; }
    }

    pub fn select_prev(&mut self) {
        if self.selected > 0 { self.selected -= 1; }
    }

    /// Enter AddPath mode with an empty input
    pub fn start_add(&mut self) {
        self.mode = ProjectsPanelMode::AddPath;
        self.input.clear();
        self.input_cursor = 0;
        self.error = None;
    }

    /// Enter Rename mode pre-filled with current display name
    pub fn start_rename(&mut self) {
        if let Some(entry) = self.entries.get(self.selected) {
            self.mode = ProjectsPanelMode::Rename;
            self.input = entry.display_name.clone();
            self.input_cursor = self.input.len();
            self.error = None;
        }
    }

    /// Enter Init mode with an empty path (blank = cwd)
    pub fn start_init(&mut self) {
        self.mode = ProjectsPanelMode::Init;
        self.input.clear();
        self.input_cursor = 0;
        self.error = None;
    }

    /// Cancel input mode, return to Browse
    pub fn cancel_input(&mut self) {
        self.mode = ProjectsPanelMode::Browse;
        self.input.clear();
        self.input_cursor = 0;
        self.error = None;
    }

    /// Insert a character at cursor position
    pub fn input_char(&mut self, c: char) {
        self.error = None;
        self.input.insert(self.input_cursor, c);
        self.input_cursor += 1;
    }

    /// Delete character before cursor
    pub fn input_backspace(&mut self) {
        if self.input_cursor > 0 {
            self.input_cursor -= 1;
            self.input.remove(self.input_cursor);
        }
    }

    /// Delete character at cursor
    pub fn input_delete(&mut self) {
        if self.input_cursor < self.input.len() {
            self.input.remove(self.input_cursor);
        }
    }

    pub fn cursor_left(&mut self) { if self.input_cursor > 0 { self.input_cursor -= 1; } }
    pub fn cursor_right(&mut self) { if self.input_cursor < self.input.len() { self.input_cursor += 1; } }
    pub fn cursor_home(&mut self) { self.input_cursor = 0; }
    pub fn cursor_end(&mut self) { self.input_cursor = self.input.len(); }
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
