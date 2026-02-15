//! App type definitions (enums, dialogs, menus)

use std::path::PathBuf;

use crate::config::ProjectEntry;

/// Viewer pane display mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewerMode {
    #[default]
    Empty, // Nothing selected
    File, // Showing file from FileTree
    Diff, // Showing diff from Output
    Image, // Showing image from FileTree (rendered via terminal graphics protocol)
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

/// Active file tree action requiring text input or confirmation
#[derive(Debug, Clone)]
pub enum FileTreeAction {
    /// Creating a new file (input = filename). Trailing '/' means create directory.
    Add(String),
    /// Renaming the selected entry (input = new name)
    Rename(String),
    /// Clipboard copy — source path stored, navigate to target dir and press Enter
    Copy(PathBuf),
    /// Clipboard move — source path stored, navigate to target dir and press Enter (dashed border)
    Move(PathBuf),
    /// Deleting the selected entry (awaiting 'y' confirmation)
    Delete,
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
    /// A worktree row — index into app.sessions
    Worktree(usize),
}

/// A saved run command — can be global (~/.azureal/) or project-local (.azureal/)
#[derive(Debug, Clone)]
pub struct RunCommand {
    pub name: String,
    pub command: String,
    /// true = saved globally (~/.azureal/), false = project-local (.azureal/)
    pub global: bool,
}

impl RunCommand {
    pub fn new(name: impl Into<String>, command: impl Into<String>, global: bool) -> Self {
        Self { name: name.into(), command: command.into(), global }
    }
}

/// Whether the second field in RunCommandDialog is a raw shell command or an AI prompt
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandFieldMode {
    /// User types a shell command directly
    Command,
    /// User types a natural-language prompt; Claude generates the command
    Prompt,
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
    /// Whether the second field is "Command" (raw shell) or "Prompt" (AI-generated)
    pub field_mode: CommandFieldMode,
    /// true = save globally (~/.azureal/), false = project-local (.azureal/)
    pub global: bool,
}

impl RunCommandDialog {
    pub fn new() -> Self {
        Self { name: String::new(), command: String::new(), name_cursor: 0, command_cursor: 0, editing_name: true, editing_idx: None, field_mode: CommandFieldMode::Command, global: false }
    }

    pub fn edit(idx: usize, cmd: &RunCommand) -> Self {
        Self { name: cmd.name.clone(), command: cmd.command.clone(), name_cursor: cmd.name.len(), command_cursor: cmd.command.len(), editing_name: true, editing_idx: Some(idx), field_mode: CommandFieldMode::Command, global: cmd.global }
    }
}

/// Picker for selecting from saved run commands
#[derive(Debug, Clone)]
pub struct RunCommandPicker {
    pub selected: usize,
    /// When Some(idx), a delete confirmation is pending for this run command index
    pub confirm_delete: Option<usize>,
}

impl RunCommandPicker {
    pub fn new() -> Self { Self { selected: 0, confirm_delete: None } }
}

/// A saved prompt template the user can quickly insert into the input box
#[derive(Debug, Clone)]
pub struct PresetPrompt {
    /// Short label shown in the picker list
    pub name: String,
    /// Full prompt text that populates the input box on selection
    pub prompt: String,
    /// true = saved globally (~/.azureal/), false = project-local (.azureal/)
    pub global: bool,
}

impl PresetPrompt {
    pub fn new(name: impl Into<String>, prompt: impl Into<String>, global: bool) -> Self {
        Self { name: name.into(), prompt: prompt.into(), global }
    }
}

/// Picker overlay for selecting from saved preset prompts (⌥P)
#[derive(Debug, Clone)]
pub struct PresetPromptPicker {
    pub selected: usize,
    /// When Some(idx), a delete confirmation is pending for this preset index
    pub confirm_delete: Option<usize>,
}

impl PresetPromptPicker {
    pub fn new() -> Self { Self { selected: 0, confirm_delete: None } }
}

/// Dialog for creating/editing a preset prompt (two fields: name + prompt text)
#[derive(Debug, Clone)]
pub struct PresetPromptDialog {
    pub name: String,
    pub prompt: String,
    pub name_cursor: usize,
    pub prompt_cursor: usize,
    /// true = name field focused, false = prompt field focused
    pub editing_name: bool,
    /// Some(i) = editing existing preset at index i, None = adding new
    pub editing_idx: Option<usize>,
    /// true = save globally (~/.azureal/), false = project-local (.azureal/)
    pub global: bool,
}

impl PresetPromptDialog {
    pub fn new() -> Self {
        Self { name: String::new(), prompt: String::new(), name_cursor: 0, prompt_cursor: 0, editing_name: true, editing_idx: None, global: false }
    }

    pub fn edit(idx: usize, preset: &PresetPrompt) -> Self {
        Self { name: preset.name.clone(), prompt: preset.prompt.clone(), name_cursor: preset.name.chars().count(), prompt_cursor: preset.prompt.chars().count(), editing_name: true, editing_idx: Some(idx), global: preset.global }
    }
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
        if self.selected + 1 < self.entries.len() { self.selected += 1; self.error = None; }
    }

    pub fn select_prev(&mut self) {
        if self.selected > 0 { self.selected -= 1; self.error = None; }
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
}

/// State for the Git Actions panel (Shift+G overlay in Worktrees pane).
/// Shows git operations (rebase, merge, fetch, etc.) and changed files list.
#[derive(Debug)]
pub struct GitActionsPanel {
    /// Current worktree name (branch) shown in the title
    pub worktree_name: String,
    /// Worktree path on disk (for running git commands without reborrowing App)
    pub worktree_path: std::path::PathBuf,
    /// Main branch name (for rebase/merge/diff base)
    pub main_branch: String,
    /// Changed files from git diff --stat against main
    pub changed_files: Vec<GitChangedFile>,
    /// Selected file index in the file list
    pub selected_file: usize,
    /// Scroll offset for the file list
    pub file_scroll: usize,
    /// true = action list focused, false = file list focused. Tab toggles.
    pub actions_focused: bool,
    /// Selected action index when actions_focused is true
    pub selected_action: usize,
    /// Transient status/result message from last git operation: (message, is_error)
    pub result_message: Option<(String, bool)>,
}

/// A source file detected as a "god file" (>1k LOC) — candidate for modularization
#[derive(Debug, Clone)]
pub struct GodFileEntry {
    /// Absolute path to the file
    pub path: PathBuf,
    /// Path relative to project root (for display)
    pub rel_path: String,
    /// Total line count in the file
    pub line_count: usize,
    /// Whether the user checked this file for modularization
    pub checked: bool,
}

/// Rust module organization: file-based root (modern) vs mod.rs (legacy)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustModuleStyle {
    /// Modern: `modulename.rs` as root alongside `modulename/` directory
    FileBased,
    /// Legacy: `modulename/mod.rs` as root inside the directory
    ModRs,
}

/// Python module organization: package with __init__.py vs single-file modules
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PythonModuleStyle {
    /// Directory package with `__init__.py` re-exporting public names
    Package,
    /// Standalone `.py` files with explicit imports (no __init__.py)
    SingleFile,
}

/// Pre-modularize dialog: lets user pick module style for Rust/Python files
/// before spawning GFM sessions. Only shown when checked files include .rs/.py.
#[derive(Debug)]
pub struct ModuleStyleDialog {
    /// Whether any checked files are .rs
    pub has_rust: bool,
    /// Whether any checked files are .py
    pub has_python: bool,
    /// Currently selected Rust module style
    pub rust_style: RustModuleStyle,
    /// Currently selected Python module style
    pub python_style: PythonModuleStyle,
    /// Cursor row: 0 = first visible language, 1 = second (if both present)
    pub selected: usize,
}

/// Which tab is active in the Worktree Health panel
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HealthTab {
    /// God Files — source files exceeding 1000 LOC
    GodFiles,
    /// Documentation — measures doc-comment coverage across source files
    Documentation,
}

/// State for the Worktree Health panel — tabbed modal overlay housing
/// multiple health-check systems (god files, documentation coverage, etc.)
#[derive(Debug)]
pub struct HealthPanel {
    /// Which tab is currently active/visible
    pub tab: HealthTab,
    // ── God Files tab ──
    /// All source files exceeding the LOC threshold
    pub god_files: Vec<GodFileEntry>,
    /// Navigation cursor in god files list
    pub god_selected: usize,
    /// Scroll offset for god files list
    pub god_scroll: usize,
    // ── Documentation tab ──
    /// All source files with documentation coverage metrics
    pub doc_entries: Vec<DocEntry>,
    /// Navigation cursor in doc entries list
    pub doc_selected: usize,
    /// Scroll offset for doc entries list
    pub doc_scroll: usize,
    /// Overall documentation score 0.0–100.0 across all scanned files
    pub doc_score: f32,
    /// When Some, the module style selector is shown before modularizing.
    /// Set when Enter/m pressed and checked files include .rs or .py.
    pub module_style_dialog: Option<ModuleStyleDialog>,
}

/// A source file with documentation coverage metrics — how many documentable
/// items (fns, structs, enums, traits, consts, etc.) have doc comments
#[derive(Debug, Clone)]
pub struct DocEntry {
    /// Absolute path to the file
    pub path: PathBuf,
    /// Path relative to project root (for display)
    pub rel_path: String,
    /// Total documentable items found (fns, structs, enums, traits, consts, types, impls)
    pub total_items: usize,
    /// How many of those items have a preceding /// or //! doc comment
    pub documented_items: usize,
    /// Per-file coverage percentage 0.0–100.0
    pub coverage_pct: f32,
    /// Whether this entry is checked for batch doc-health session spawning
    pub checked: bool,
}
