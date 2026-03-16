//! App type definitions (enums, dialogs, menus)

use std::path::PathBuf;

use ratatui::text::Line;

use crate::config::ProjectEntry;

/// Viewer pane display mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewerMode {
    #[default]
    Empty, // Nothing selected
    File, // Showing file from FileTree
    #[allow(dead_code)] // Used in draw_viewer match + tests
    Diff, // Showing diff from Session
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
    /// Per-branch worktree count (active + archived)
    pub worktree_counts: Vec<usize>,
    /// Branches already checked out in an active worktree
    pub checked_out: Vec<String>,
    /// 0 = "Create new" row, 1..=N = branch rows
    pub selected: usize,
    pub filter: String,
    pub filtered_indices: Vec<usize>,
}

impl BranchDialog {
    pub fn new(
        branches: Vec<String>,
        checked_out: Vec<String>,
        worktree_counts: Vec<usize>,
    ) -> Self {
        let filtered_indices: Vec<usize> = (0..branches.len()).collect();
        Self {
            branches,
            worktree_counts,
            checked_out,
            selected: 0,
            filter: String::new(),
            filtered_indices,
        }
    }

    /// True if "Create new" row is selected
    pub fn on_create_new(&self) -> bool {
        self.selected == 0
    }

    /// Total display rows: 1 ("Create new") + filtered branches
    pub fn display_len(&self) -> usize {
        1 + self.filtered_indices.len()
    }

    /// Worktree count for a branch index
    pub fn worktree_count(&self, branch_idx: usize) -> usize {
        self.worktree_counts.get(branch_idx).copied().unwrap_or(0)
    }

    /// True if the branch is already checked out in a worktree
    pub fn is_checked_out(&self, branch: &str) -> bool {
        let local_name = if branch.contains('/') {
            branch.split('/').skip(1).collect::<Vec<_>>().join("/")
        } else {
            branch.to_string()
        };
        self.checked_out
            .iter()
            .any(|co| co == branch || co == &local_name)
    }

    pub fn apply_filter(&mut self) {
        let filter_lower = self.filter.to_lowercase();
        self.filtered_indices = self
            .branches
            .iter()
            .enumerate()
            .filter(|(_, b)| b.to_lowercase().contains(&filter_lower))
            .map(|(i, _)| i)
            .collect();
        if self.selected >= self.display_len() {
            self.selected = 0;
        }
    }

    /// Get the selected branch (None if on "Create new" row)
    pub fn selected_branch(&self) -> Option<&String> {
        if self.selected == 0 {
            return None;
        }
        let branch_idx = self.selected - 1;
        self.filtered_indices
            .get(branch_idx)
            .and_then(|&idx| self.branches.get(idx))
    }

    pub fn select_next(&mut self) {
        if self.selected + 1 < self.display_len() {
            self.selected += 1;
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn filter_char(&mut self, c: char) {
        if is_git_safe_char(c) {
            self.filter.push(c);
            self.apply_filter();
        }
    }

    pub fn filter_backspace(&mut self) {
        self.filter.pop();
        self.apply_filter();
    }
}

/// Check if a character is valid in a git branch/worktree name
pub fn is_git_safe_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '+' | '@' | '/' | '!')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Worktrees,
    FileTree,
    Viewer,
    Session,
    Input,
    BranchDialog,
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
        Self {
            name: name.into(),
            command: command.into(),
            global,
        }
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
        Self {
            name: String::new(),
            command: String::new(),
            name_cursor: 0,
            command_cursor: 0,
            editing_name: true,
            editing_idx: None,
            field_mode: CommandFieldMode::Command,
            global: false,
        }
    }

    pub fn edit(idx: usize, cmd: &RunCommand) -> Self {
        Self {
            name: cmd.name.clone(),
            command: cmd.command.clone(),
            name_cursor: cmd.name.len(),
            command_cursor: cmd.command.len(),
            editing_name: true,
            editing_idx: Some(idx),
            field_mode: CommandFieldMode::Command,
            global: cmd.global,
        }
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
    pub fn new() -> Self {
        Self {
            selected: 0,
            confirm_delete: None,
        }
    }
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
        Self {
            name: name.into(),
            prompt: prompt.into(),
            global,
        }
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
    pub fn new() -> Self {
        Self {
            selected: 0,
            confirm_delete: None,
        }
    }
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
        Self {
            name: String::new(),
            prompt: String::new(),
            name_cursor: 0,
            prompt_cursor: 0,
            editing_name: true,
            editing_idx: None,
            global: false,
        }
    }

    pub fn edit(idx: usize, preset: &PresetPrompt) -> Self {
        Self {
            name: preset.name.clone(),
            prompt: preset.prompt.clone(),
            name_cursor: preset.name.chars().count(),
            prompt_cursor: preset.prompt.chars().count(),
            editing_name: true,
            editing_idx: Some(idx),
            global: preset.global,
        }
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
        Self {
            entries,
            selected: 0,
            mode: ProjectsPanelMode::Browse,
            input: String::new(),
            input_cursor: 0,
            error: None,
        }
    }

    pub fn select_next(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
            self.error = None;
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.error = None;
        }
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

    pub fn cursor_left(&mut self) {
        if self.input_cursor > 0 {
            self.input_cursor -= 1;
        }
    }
    pub fn cursor_right(&mut self) {
        if self.input_cursor < self.input.len() {
            self.input_cursor += 1;
        }
    }
    pub fn cursor_home(&mut self) {
        self.input_cursor = 0;
    }
    pub fn cursor_end(&mut self) {
        self.input_cursor = self.input.len();
    }
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
    pub receiver:
        Option<std::sync::mpsc::Receiver<Result<GeneratedCommitMessage, String>>>,
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

/// Full-width table popup overlay (click a table in session pane to open).
/// Pre-rendered at popup width so columns aren't truncated.
#[derive(Debug, Clone)]
pub struct TablePopup {
    pub lines: Vec<Line<'static>>,
    pub scroll: usize,
    pub total_lines: usize,
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
    /// Git panel operation result (pull, push) — set result_message + refresh
    GitResult { message: String, is_error: bool },
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

/// Result from background worktree refresh (git + FS I/O done off main thread)
pub struct WorktreeRefreshResult {
    /// Main branch worktree (accessed via 'M' browse mode)
    pub main_worktree: Option<crate::models::Worktree>,
    /// Feature + archived worktrees (sidebar entries)
    pub worktrees: Vec<crate::models::Worktree>,
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
#[derive(Debug, Clone)]
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
    /// Worktree display name shown in the panel title (e.g. "Health: my-feature")
    pub worktree_name: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    // ══════════════════════════════════════════════════════════════════
    // ViewerMode enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn viewer_mode_default_is_empty() {
        assert_eq!(ViewerMode::default(), ViewerMode::Empty);
    }

    #[test]
    fn viewer_mode_clone_and_copy() {
        let m = ViewerMode::File;
        let cloned = m.clone();
        let copied = m;
        assert_eq!(m, cloned);
        assert_eq!(m, copied);
    }

    #[test]
    fn viewer_mode_all_variants_distinct() {
        let variants = [
            ViewerMode::Empty,
            ViewerMode::File,
            ViewerMode::Diff,
            ViewerMode::Image,
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn viewer_mode_debug_format() {
        assert_eq!(format!("{:?}", ViewerMode::Empty), "Empty");
        assert_eq!(format!("{:?}", ViewerMode::File), "File");
        assert_eq!(format!("{:?}", ViewerMode::Diff), "Diff");
        assert_eq!(format!("{:?}", ViewerMode::Image), "Image");
    }

    // ══════════════════════════════════════════════════════════════════
    // ViewMode enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn view_mode_session_eq() {
        assert_eq!(ViewMode::Session, ViewMode::Session);
    }

    #[test]
    fn view_mode_debug() {
        assert_eq!(format!("{:?}", ViewMode::Session), "Session");
    }

    // ══════════════════════════════════════════════════════════════════
    // Focus enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn focus_all_variants_distinct() {
        let variants = [
            Focus::Worktrees,
            Focus::FileTree,
            Focus::Viewer,
            Focus::Session,
            Focus::Input,
            Focus::BranchDialog,
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn focus_debug_format() {
        assert_eq!(format!("{:?}", Focus::Worktrees), "Worktrees");
        assert_eq!(format!("{:?}", Focus::Input), "Input");
        assert_eq!(format!("{:?}", Focus::BranchDialog), "BranchDialog");
    }

    // ══════════════════════════════════════════════════════════════════
    // CommandFieldMode enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn command_field_mode_variants_distinct() {
        assert_ne!(CommandFieldMode::Command, CommandFieldMode::Prompt);
    }

    #[test]
    fn command_field_mode_clone_copy() {
        let m = CommandFieldMode::Prompt;
        let cloned = m.clone();
        let copied = m;
        assert_eq!(m, cloned);
        assert_eq!(m, copied);
    }

    // ══════════════════════════════════════════════════════════════════
    // ProjectsPanelMode enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn projects_panel_mode_all_variants_distinct() {
        let variants = [
            ProjectsPanelMode::Browse,
            ProjectsPanelMode::AddPath,
            ProjectsPanelMode::Rename,
            ProjectsPanelMode::Init,
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // RustModuleStyle enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn rust_module_style_variants_distinct() {
        assert_ne!(RustModuleStyle::FileBased, RustModuleStyle::ModRs);
    }

    #[test]
    fn rust_module_style_clone_copy() {
        let s = RustModuleStyle::FileBased;
        let c = s;
        assert_eq!(s, c);
    }

    // ══════════════════════════════════════════════════════════════════
    // PythonModuleStyle enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn python_module_style_variants_distinct() {
        assert_ne!(PythonModuleStyle::Package, PythonModuleStyle::SingleFile);
    }

    // ══════════════════════════════════════════════════════════════════
    // HealthTab enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn health_tab_variants_distinct() {
        assert_ne!(HealthTab::GodFiles, HealthTab::Documentation);
    }

    #[test]
    fn health_tab_clone_copy() {
        let t = HealthTab::GodFiles;
        let c = t;
        assert_eq!(t, c);
    }

    // ══════════════════════════════════════════════════════════════════
    // FileTreeEntry struct
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn file_tree_entry_clone() {
        let entry = FileTreeEntry {
            path: PathBuf::from("/src/main.rs"),
            name: "main.rs".to_string(),
            is_dir: false,
            depth: 1,
            is_hidden: false,
        };
        let cloned = entry.clone();
        assert_eq!(cloned.path, PathBuf::from("/src/main.rs"));
        assert_eq!(cloned.name, "main.rs");
        assert!(!cloned.is_dir);
        assert_eq!(cloned.depth, 1);
        assert!(!cloned.is_hidden);
    }

    #[test]
    fn file_tree_entry_hidden_dotfile() {
        let entry = FileTreeEntry {
            path: PathBuf::from("/.gitignore"),
            name: ".gitignore".to_string(),
            is_dir: false,
            depth: 0,
            is_hidden: true,
        };
        assert!(entry.is_hidden);
    }

    #[test]
    fn file_tree_entry_directory() {
        let entry = FileTreeEntry {
            path: PathBuf::from("/src"),
            name: "src".to_string(),
            is_dir: true,
            depth: 0,
            is_hidden: false,
        };
        assert!(entry.is_dir);
    }

    #[test]
    fn file_tree_entry_debug_format() {
        let entry = FileTreeEntry {
            path: PathBuf::from("/a"),
            name: "a".to_string(),
            is_dir: false,
            depth: 0,
            is_hidden: false,
        };
        let dbg = format!("{:?}", entry);
        assert!(dbg.contains("FileTreeEntry"));
        assert!(dbg.contains("\"a\""));
    }

    // ══════════════════════════════════════════════════════════════════
    // FileTreeAction enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn file_tree_action_add_clone() {
        let action = FileTreeAction::Add("test.rs".to_string());
        let cloned = action.clone();
        if let FileTreeAction::Add(name) = cloned {
            assert_eq!(name, "test.rs");
        } else {
            panic!("Expected Add variant");
        }
    }

    #[test]
    fn file_tree_action_copy_stores_path() {
        let action = FileTreeAction::Copy(PathBuf::from("/foo/bar.txt"));
        if let FileTreeAction::Copy(p) = action {
            assert_eq!(p, PathBuf::from("/foo/bar.txt"));
        } else {
            panic!("Expected Copy variant");
        }
    }

    #[test]
    fn file_tree_action_move_stores_path() {
        let action = FileTreeAction::Move(PathBuf::from("/baz"));
        if let FileTreeAction::Move(p) = action {
            assert_eq!(p, PathBuf::from("/baz"));
        } else {
            panic!("Expected Move variant");
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // BranchDialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn branch_dialog_new_empty() {
        let d = BranchDialog::new(vec![], vec![], vec![]);
        assert!(d.branches.is_empty());
        assert!(d.checked_out.is_empty());
        assert_eq!(d.selected, 0);
        assert!(d.filter.is_empty());
        assert!(d.filtered_indices.is_empty());
    }

    #[test]
    fn branch_dialog_new_populates_filtered_indices() {
        let d = BranchDialog::new(
            vec!["main".into(), "feat/a".into(), "feat/b".into()],
            vec![],
            vec![0, 0, 0],
        );
        assert_eq!(d.filtered_indices, vec![0, 1, 2]);
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn branch_dialog_is_checked_out_exact_match() {
        let d = BranchDialog::new(vec![], vec!["main".into(), "feat/a".into()], vec![]);
        assert!(d.is_checked_out("main"));
        assert!(d.is_checked_out("feat/a"));
        assert!(!d.is_checked_out("feat/b"));
    }

    #[test]
    fn branch_dialog_is_checked_out_remote_prefix_stripped() {
        // "origin/feat" -> local_name = "feat"
        let d = BranchDialog::new(vec![], vec!["feat".into()], vec![]);
        assert!(d.is_checked_out("origin/feat"));
    }

    #[test]
    fn branch_dialog_is_checked_out_multi_slash() {
        // "origin/azureal/health" -> local_name = "azureal/health"
        let d = BranchDialog::new(vec![], vec!["azureal/health".into()], vec![]);
        assert!(d.is_checked_out("origin/azureal/health"));
    }

    #[test]
    fn branch_dialog_is_checked_out_no_slash_no_match() {
        let d = BranchDialog::new(vec![], vec!["other".into()], vec![]);
        assert!(!d.is_checked_out("feat"));
    }

    #[test]
    fn branch_dialog_selected_branch_with_entries() {
        let mut d = BranchDialog::new(vec!["alpha".into(), "beta".into()], vec![], vec![0, 0]);
        // selected==0 is "[+] Create new" row, so move to first branch
        d.select_next();
        assert_eq!(d.selected_branch(), Some(&"alpha".to_string()));
    }

    #[test]
    fn branch_dialog_selected_branch_empty() {
        let d = BranchDialog::new(vec![], vec![], vec![]);
        assert_eq!(d.selected_branch(), None);
    }

    #[test]
    fn branch_dialog_select_next() {
        // display_len = 1 (Create new) + 3 branches = 4, max selected = 3
        let mut d = BranchDialog::new(
            vec!["a".into(), "b".into(), "c".into()],
            vec![],
            vec![0, 0, 0],
        );
        assert_eq!(d.selected, 0);
        d.select_next();
        assert_eq!(d.selected, 1);
        d.select_next();
        assert_eq!(d.selected, 2);
        d.select_next();
        assert_eq!(d.selected, 3);
        // At the end, should not overflow
        d.select_next();
        assert_eq!(d.selected, 3);
    }

    #[test]
    fn branch_dialog_select_prev() {
        let mut d = BranchDialog::new(vec!["a".into(), "b".into()], vec![], vec![0, 0]);
        d.select_next(); // now at 1
        d.select_prev();
        assert_eq!(d.selected, 0);
        // At 0, should not underflow
        d.select_prev();
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn branch_dialog_select_next_on_empty() {
        let mut d = BranchDialog::new(vec![], vec![], vec![]);
        d.select_next();
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn branch_dialog_filter_char_narrows_results() {
        let mut d = BranchDialog::new(
            vec![
                "main".into(),
                "feat/auth".into(),
                "feat/api".into(),
                "fix/bug".into(),
            ],
            vec![],
            vec![0, 0, 0, 0],
        );
        d.filter_char('f');
        assert_eq!(d.filtered_indices, vec![1, 2, 3]); // feat/auth, feat/api, fix/bug
        d.filter_char('e');
        assert_eq!(d.filtered_indices, vec![1, 2]); // feat/auth, feat/api
    }

    #[test]
    fn branch_dialog_filter_case_insensitive() {
        let mut d = BranchDialog::new(vec!["MAIN".into(), "Feature".into()], vec![], vec![0, 0]);
        d.filter_char('m');
        // "MAIN" contains "m" (case insensitive)
        assert!(d.filtered_indices.contains(&0));
    }

    #[test]
    fn branch_dialog_filter_backspace_widens_results() {
        let mut d = BranchDialog::new(vec!["main".into(), "feat/auth".into()], vec![], vec![0, 0]);
        d.filter_char('f');
        d.filter_char('e');
        assert_eq!(d.filtered_indices, vec![1]); // only feat/auth
        d.filter_backspace();
        // Now filter is just "f", both "feat/auth" and nothing else with f
        assert_eq!(d.filter, "f");
        assert_eq!(d.filtered_indices, vec![1]);
        d.filter_backspace();
        // Empty filter, all shown
        assert_eq!(d.filtered_indices, vec![0, 1]);
    }

    #[test]
    fn branch_dialog_filter_backspace_on_empty() {
        let mut d = BranchDialog::new(vec!["a".into()], vec![], vec![0]);
        d.filter_backspace(); // should not panic
        assert!(d.filter.is_empty());
        assert_eq!(d.filtered_indices, vec![0]);
    }

    #[test]
    fn branch_dialog_selected_resets_when_filter_shrinks_results() {
        let mut d = BranchDialog::new(
            vec!["aaa".into(), "bbb".into(), "ccc".into()],
            vec![],
            vec![0, 0, 0],
        );
        d.select_next();
        d.select_next();
        assert_eq!(d.selected, 2);
        // Now filter to only one result
        d.filter_char('a');
        assert_eq!(d.filtered_indices, vec![0]);
        // selected should have been clamped to 0
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn branch_dialog_selected_branch_after_filter() {
        let mut d = BranchDialog::new(
            vec!["main".into(), "feat/auth".into(), "feat/api".into()],
            vec![],
            vec![0, 0, 0],
        );
        d.filter_char('a');
        d.filter_char('p');
        d.filter_char('i');
        // Only "feat/api" matches
        assert_eq!(d.filtered_indices, vec![2]);
        // selected==0 is "[+] Create new", move to first filtered branch
        d.select_next();
        assert_eq!(d.selected_branch(), Some(&"feat/api".to_string()));
    }

    #[test]
    fn branch_dialog_unicode_filter_rejected_by_git_safe() {
        // Emoji chars are rejected by is_git_safe_char, so filter stays empty
        let mut d = BranchDialog::new(
            vec!["feat/unicorn-\u{1F984}".into(), "main".into()],
            vec![],
            vec![0, 0],
        );
        d.filter_char('\u{1F984}');
        // filter is still empty, all branches shown
        assert_eq!(d.filtered_indices, vec![0, 1]);
    }

    // ══════════════════════════════════════════════════════════════════
    // RunCommand
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn run_command_new_basic() {
        let cmd = RunCommand::new("build", "cargo build", false);
        assert_eq!(cmd.name, "build");
        assert_eq!(cmd.command, "cargo build");
        assert!(!cmd.global);
    }

    #[test]
    fn run_command_new_global() {
        let cmd = RunCommand::new("test", "cargo test", true);
        assert!(cmd.global);
    }

    #[test]
    fn run_command_new_from_string_types() {
        let name = String::from("deploy");
        let command = String::from("./deploy.sh");
        let cmd = RunCommand::new(name, command, false);
        assert_eq!(cmd.name, "deploy");
        assert_eq!(cmd.command, "./deploy.sh");
    }

    #[test]
    fn run_command_clone() {
        let cmd = RunCommand::new("x", "y", true);
        let cloned = cmd.clone();
        assert_eq!(cloned.name, "x");
        assert_eq!(cloned.command, "y");
        assert!(cloned.global);
    }

    #[test]
    fn run_command_empty_strings() {
        let cmd = RunCommand::new("", "", false);
        assert!(cmd.name.is_empty());
        assert!(cmd.command.is_empty());
    }

    #[test]
    fn run_command_special_chars() {
        let cmd = RunCommand::new(
            "build & test",
            "cargo build && cargo test 2>&1 | tee log",
            false,
        );
        assert_eq!(cmd.name, "build & test");
        assert_eq!(cmd.command, "cargo build && cargo test 2>&1 | tee log");
    }

    // ══════════════════════════════════════════════════════════════════
    // RunCommandDialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn run_command_dialog_new_defaults() {
        let d = RunCommandDialog::new();
        assert!(d.name.is_empty());
        assert!(d.command.is_empty());
        assert_eq!(d.name_cursor, 0);
        assert_eq!(d.command_cursor, 0);
        assert!(d.editing_name);
        assert!(d.editing_idx.is_none());
        assert_eq!(d.field_mode, CommandFieldMode::Command);
        assert!(!d.global);
    }

    #[test]
    fn run_command_dialog_edit_populates_fields() {
        let cmd = RunCommand::new("test", "cargo test --all", true);
        let d = RunCommandDialog::edit(3, &cmd);
        assert_eq!(d.name, "test");
        assert_eq!(d.command, "cargo test --all");
        assert_eq!(d.name_cursor, 4); // len of "test"
        assert_eq!(d.command_cursor, 16); // len of "cargo test --all"
        assert!(d.editing_name);
        assert_eq!(d.editing_idx, Some(3));
        assert_eq!(d.field_mode, CommandFieldMode::Command);
        assert!(d.global);
    }

    #[test]
    fn run_command_dialog_edit_index_zero() {
        let cmd = RunCommand::new("a", "b", false);
        let d = RunCommandDialog::edit(0, &cmd);
        assert_eq!(d.editing_idx, Some(0));
    }

    // ══════════════════════════════════════════════════════════════════
    // RunCommandPicker
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn run_command_picker_new() {
        let p = RunCommandPicker::new();
        assert_eq!(p.selected, 0);
        assert!(p.confirm_delete.is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    // PresetPrompt
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn preset_prompt_new_basic() {
        let p = PresetPrompt::new("review", "Review this PR for bugs", false);
        assert_eq!(p.name, "review");
        assert_eq!(p.prompt, "Review this PR for bugs");
        assert!(!p.global);
    }

    #[test]
    fn preset_prompt_new_global() {
        let p = PresetPrompt::new("explain", "Explain this code", true);
        assert!(p.global);
    }

    #[test]
    fn preset_prompt_clone() {
        let p = PresetPrompt::new("test", "prompt text", false);
        let cloned = p.clone();
        assert_eq!(cloned.name, "test");
        assert_eq!(cloned.prompt, "prompt text");
    }

    #[test]
    fn preset_prompt_unicode_content() {
        let p = PresetPrompt::new(
            "Japanese",
            "\u{65E5}\u{672C}\u{8A9E}\u{306E}\u{8AAC}\u{660E}",
            false,
        );
        assert_eq!(p.name, "Japanese");
        assert_eq!(p.prompt.chars().count(), 6);
    }

    // ══════════════════════════════════════════════════════════════════
    // PresetPromptPicker
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn preset_prompt_picker_new() {
        let p = PresetPromptPicker::new();
        assert_eq!(p.selected, 0);
        assert!(p.confirm_delete.is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    // PresetPromptDialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn preset_prompt_dialog_new_defaults() {
        let d = PresetPromptDialog::new();
        assert!(d.name.is_empty());
        assert!(d.prompt.is_empty());
        assert_eq!(d.name_cursor, 0);
        assert_eq!(d.prompt_cursor, 0);
        assert!(d.editing_name);
        assert!(d.editing_idx.is_none());
        assert!(!d.global);
    }

    #[test]
    fn preset_prompt_dialog_edit_populates() {
        let preset = PresetPrompt::new("summarize", "Summarize in 3 bullets", true);
        let d = PresetPromptDialog::edit(5, &preset);
        assert_eq!(d.name, "summarize");
        assert_eq!(d.prompt, "Summarize in 3 bullets");
        assert_eq!(d.name_cursor, 9); // char count of "summarize"
        assert_eq!(d.prompt_cursor, 22); // char count of "Summarize in 3 bullets"
        assert!(d.editing_name);
        assert_eq!(d.editing_idx, Some(5));
        assert!(d.global);
    }

    #[test]
    fn preset_prompt_dialog_edit_unicode_cursors() {
        // Unicode chars: cursor should be char count, not byte len
        let preset = PresetPrompt::new("\u{1F600}", "\u{1F4BB}\u{1F680}", false);
        let d = PresetPromptDialog::edit(0, &preset);
        assert_eq!(d.name_cursor, 1); // 1 emoji char
        assert_eq!(d.prompt_cursor, 2); // 2 emoji chars
    }

    // ══════════════════════════════════════════════════════════════════
    // ProjectsPanel
    // ══════════════════════════════════════════════════════════════════

    fn make_entries(count: usize) -> Vec<ProjectEntry> {
        (0..count)
            .map(|i| ProjectEntry {
                path: PathBuf::from(format!("/projects/proj{}", i)),
                display_name: format!("Project {}", i),
            })
            .collect()
    }

    #[test]
    fn projects_panel_new_defaults() {
        let p = ProjectsPanel::new(vec![]);
        assert!(p.entries.is_empty());
        assert_eq!(p.selected, 0);
        assert_eq!(p.mode, ProjectsPanelMode::Browse);
        assert!(p.input.is_empty());
        assert_eq!(p.input_cursor, 0);
        assert!(p.error.is_none());
    }

    #[test]
    fn projects_panel_new_with_entries() {
        let entries = make_entries(3);
        let p = ProjectsPanel::new(entries);
        assert_eq!(p.entries.len(), 3);
        assert_eq!(p.entries[0].display_name, "Project 0");
    }

    #[test]
    fn projects_panel_select_next() {
        let mut p = ProjectsPanel::new(make_entries(3));
        p.error = Some("stale error".into());
        p.select_next();
        assert_eq!(p.selected, 1);
        assert!(p.error.is_none()); // error cleared
        p.select_next();
        assert_eq!(p.selected, 2);
        p.select_next(); // at end, should not move
        assert_eq!(p.selected, 2);
    }

    #[test]
    fn projects_panel_select_prev() {
        let mut p = ProjectsPanel::new(make_entries(3));
        p.selected = 2;
        p.error = Some("err".into());
        p.select_prev();
        assert_eq!(p.selected, 1);
        assert!(p.error.is_none());
        p.select_prev();
        assert_eq!(p.selected, 0);
        p.select_prev(); // at start, should not move
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn projects_panel_select_next_empty() {
        let mut p = ProjectsPanel::new(vec![]);
        p.select_next(); // should not panic
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn projects_panel_start_add() {
        let mut p = ProjectsPanel::new(make_entries(1));
        p.input = "leftover".into();
        p.input_cursor = 5;
        p.error = Some("old error".into());
        p.start_add();
        assert_eq!(p.mode, ProjectsPanelMode::AddPath);
        assert!(p.input.is_empty());
        assert_eq!(p.input_cursor, 0);
        assert!(p.error.is_none());
    }

    #[test]
    fn projects_panel_start_rename() {
        let mut p = ProjectsPanel::new(make_entries(2));
        p.selected = 1;
        p.start_rename();
        assert_eq!(p.mode, ProjectsPanelMode::Rename);
        assert_eq!(p.input, "Project 1");
        assert_eq!(p.input_cursor, "Project 1".len());
        assert!(p.error.is_none());
    }

    #[test]
    fn projects_panel_start_rename_empty_list() {
        let mut p = ProjectsPanel::new(vec![]);
        p.start_rename(); // should not panic, mode stays Browse
        assert_eq!(p.mode, ProjectsPanelMode::Browse);
    }

    #[test]
    fn projects_panel_start_init() {
        let mut p = ProjectsPanel::new(make_entries(1));
        p.input = "stale".into();
        p.error = Some("x".into());
        p.start_init();
        assert_eq!(p.mode, ProjectsPanelMode::Init);
        assert!(p.input.is_empty());
        assert_eq!(p.input_cursor, 0);
        assert!(p.error.is_none());
    }

    #[test]
    fn projects_panel_cancel_input() {
        let mut p = ProjectsPanel::new(make_entries(1));
        p.mode = ProjectsPanelMode::AddPath;
        p.input = "/some/path".into();
        p.input_cursor = 10;
        p.error = Some("bad".into());
        p.cancel_input();
        assert_eq!(p.mode, ProjectsPanelMode::Browse);
        assert!(p.input.is_empty());
        assert_eq!(p.input_cursor, 0);
        assert!(p.error.is_none());
    }

    #[test]
    fn projects_panel_input_char() {
        let mut p = ProjectsPanel::new(vec![]);
        p.mode = ProjectsPanelMode::AddPath;
        p.error = Some("old".into());
        p.input_char('h');
        p.input_char('i');
        assert_eq!(p.input, "hi");
        assert_eq!(p.input_cursor, 2);
        assert!(p.error.is_none());
    }

    #[test]
    fn projects_panel_input_char_inserts_at_cursor() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "ac".into();
        p.input_cursor = 1; // between 'a' and 'c'
        p.input_char('b');
        assert_eq!(p.input, "abc");
        assert_eq!(p.input_cursor, 2);
    }

    #[test]
    fn projects_panel_input_backspace() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "abc".into();
        p.input_cursor = 3;
        p.input_backspace();
        assert_eq!(p.input, "ab");
        assert_eq!(p.input_cursor, 2);
    }

    #[test]
    fn projects_panel_input_backspace_at_start() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "abc".into();
        p.input_cursor = 0;
        p.input_backspace(); // should do nothing
        assert_eq!(p.input, "abc");
        assert_eq!(p.input_cursor, 0);
    }

    #[test]
    fn projects_panel_input_backspace_empty() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input_backspace(); // should not panic
        assert!(p.input.is_empty());
    }

    #[test]
    fn projects_panel_input_delete() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "abc".into();
        p.input_cursor = 1; // at 'b'
        p.input_delete();
        assert_eq!(p.input, "ac");
        assert_eq!(p.input_cursor, 1);
    }

    #[test]
    fn projects_panel_input_delete_at_end() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "abc".into();
        p.input_cursor = 3;
        p.input_delete(); // should do nothing
        assert_eq!(p.input, "abc");
    }

    #[test]
    fn projects_panel_cursor_left() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "abc".into();
        p.input_cursor = 2;
        p.cursor_left();
        assert_eq!(p.input_cursor, 1);
    }

    #[test]
    fn projects_panel_cursor_left_at_zero() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input_cursor = 0;
        p.cursor_left();
        assert_eq!(p.input_cursor, 0);
    }

    #[test]
    fn projects_panel_cursor_right() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "abc".into();
        p.input_cursor = 1;
        p.cursor_right();
        assert_eq!(p.input_cursor, 2);
    }

    #[test]
    fn projects_panel_cursor_right_at_end() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "abc".into();
        p.input_cursor = 3;
        p.cursor_right();
        assert_eq!(p.input_cursor, 3);
    }

    #[test]
    fn projects_panel_cursor_home() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "hello".into();
        p.input_cursor = 3;
        p.cursor_home();
        assert_eq!(p.input_cursor, 0);
    }

    #[test]
    fn projects_panel_cursor_end() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "hello".into();
        p.input_cursor = 0;
        p.cursor_end();
        assert_eq!(p.input_cursor, 5);
    }

    // ══════════════════════════════════════════════════════════════════
    // ViewerTab
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn viewer_tab_name_returns_title() {
        let tab = ViewerTab {
            path: Some(PathBuf::from("/foo/bar.rs")),
            content: Some("fn main() {}".into()),
            scroll: 0,
            mode: ViewerMode::File,
            title: "bar.rs".to_string(),
        };
        assert_eq!(tab.name(), "bar.rs");
    }

    #[test]
    fn viewer_tab_name_empty_title() {
        let tab = ViewerTab {
            path: None,
            content: None,
            scroll: 0,
            mode: ViewerMode::Empty,
            title: String::new(),
        };
        assert_eq!(tab.name(), "");
    }

    #[test]
    fn viewer_tab_clone() {
        let tab = ViewerTab {
            path: Some(PathBuf::from("/x")),
            content: Some("content".into()),
            scroll: 42,
            mode: ViewerMode::Diff,
            title: "diff".into(),
        };
        let cloned = tab.clone();
        assert_eq!(cloned.path, Some(PathBuf::from("/x")));
        assert_eq!(cloned.content, Some("content".into()));
        assert_eq!(cloned.scroll, 42);
        assert_eq!(cloned.mode, ViewerMode::Diff);
        assert_eq!(cloned.title, "diff");
    }

    // ══════════════════════════════════════════════════════════════════
    // GitCommit
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn git_commit_clone() {
        let c = GitCommit {
            hash: "abc1234".into(),
            full_hash: "abc1234567890abcdef1234567890abcdef123456".into(),
            subject: "feat: add health panel".into(),
            is_pushed: false,
        };
        let cloned = c.clone();
        assert_eq!(cloned.hash, "abc1234");
        assert_eq!(cloned.subject, "feat: add health panel");
        assert!(!cloned.is_pushed);
    }

    #[test]
    fn git_commit_debug() {
        let c = GitCommit {
            hash: "a".into(),
            full_hash: "a".into(),
            subject: "s".into(),
            is_pushed: true,
        };
        let dbg = format!("{:?}", c);
        assert!(dbg.contains("GitCommit"));
        assert!(dbg.contains("is_pushed: true"));
    }

    // ══════════════════════════════════════════════════════════════════
    // GitChangedFile
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn git_changed_file_clone() {
        let f = GitChangedFile {
            path: "src/main.rs".into(),
            status: 'M',
            additions: 10,
            deletions: 3,
            staged: false,
        };
        let cloned = f.clone();
        assert_eq!(cloned.path, "src/main.rs");
        assert_eq!(cloned.status, 'M');
        assert_eq!(cloned.additions, 10);
        assert_eq!(cloned.deletions, 3);
    }

    #[test]
    fn git_changed_file_added_status() {
        let f = GitChangedFile {
            path: "new_file.rs".into(),
            status: 'A',
            additions: 50,
            deletions: 0,
            staged: false,
        };
        assert_eq!(f.status, 'A');
        assert_eq!(f.deletions, 0);
    }

    #[test]
    fn git_changed_file_deleted_status() {
        let f = GitChangedFile {
            path: "old_file.rs".into(),
            status: 'D',
            additions: 0,
            deletions: 100,
            staged: false,
        };
        assert_eq!(f.status, 'D');
        assert_eq!(f.additions, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    // GodFileEntry
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn god_file_entry_clone() {
        let e = GodFileEntry {
            path: PathBuf::from("/src/big.rs"),
            rel_path: "src/big.rs".into(),
            line_count: 2500,
            checked: true,
        };
        let cloned = e.clone();
        assert_eq!(cloned.path, PathBuf::from("/src/big.rs"));
        assert_eq!(cloned.rel_path, "src/big.rs");
        assert_eq!(cloned.line_count, 2500);
        assert!(cloned.checked);
    }

    // ══════════════════════════════════════════════════════════════════
    // DocEntry
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn doc_entry_clone() {
        let e = DocEntry {
            path: PathBuf::from("/src/lib.rs"),
            rel_path: "src/lib.rs".into(),
            total_items: 20,
            documented_items: 15,
            coverage_pct: 75.0,
            checked: false,
        };
        let cloned = e.clone();
        assert_eq!(cloned.total_items, 20);
        assert_eq!(cloned.documented_items, 15);
        assert!((cloned.coverage_pct - 75.0).abs() < f32::EPSILON);
        assert!(!cloned.checked);
    }

    #[test]
    fn doc_entry_zero_coverage() {
        let e = DocEntry {
            path: PathBuf::from("/a.rs"),
            rel_path: "a.rs".into(),
            total_items: 10,
            documented_items: 0,
            coverage_pct: 0.0,
            checked: false,
        };
        assert_eq!(e.documented_items, 0);
        assert!((e.coverage_pct).abs() < f32::EPSILON);
    }

    #[test]
    fn doc_entry_full_coverage() {
        let e = DocEntry {
            path: PathBuf::from("/b.rs"),
            rel_path: "b.rs".into(),
            total_items: 5,
            documented_items: 5,
            coverage_pct: 100.0,
            checked: true,
        };
        assert_eq!(e.total_items, e.documented_items);
        assert!((e.coverage_pct - 100.0).abs() < f32::EPSILON);
    }

    // ══════════════════════════════════════════════════════════════════
    // GitConflictOverlay
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn git_conflict_overlay_fields() {
        let o = GitConflictOverlay {
            conflicted_files: vec!["src/main.rs".into()],
            auto_merged_files: vec!["Cargo.toml".into()],
            scroll: 0,
            selected: 0,
            continue_with_merge: true,
        };
        assert_eq!(o.conflicted_files.len(), 1);
        assert_eq!(o.auto_merged_files.len(), 1);
        assert!(o.continue_with_merge);
    }

    // ══════════════════════════════════════════════════════════════════
    // PostMergeDialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn post_merge_dialog_fields() {
        let d = PostMergeDialog {
            branch: "azureal/health".into(),
            display_name: "health".into(),
            worktree_path: PathBuf::from("/repo/worktrees/health"),
            selected: 0,
        };
        assert_eq!(d.branch, "azureal/health");
        assert_eq!(d.display_name, "health");
        assert_eq!(d.selected, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    // RcrSession
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn rcr_session_fields() {
        let s = RcrSession {
            branch: "azureal/feat".into(),
            display_name: "feat".into(),
            worktree_path: PathBuf::from("/repo/worktrees/feat"),
            repo_root: PathBuf::from("/repo"),
            slot_id: "12345".into(),
            session_id: None,
            approval_pending: false,
            continue_with_merge: true,
        };
        assert_eq!(s.branch, "azureal/feat");
        assert!(s.session_id.is_none());
        assert!(!s.approval_pending);
        assert!(s.continue_with_merge);
    }

    // ══════════════════════════════════════════════════════════════════
    // AutoResolveOverlay
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn auto_resolve_overlay_fields() {
        let o = AutoResolveOverlay {
            files: vec![("AGENTS.md".into(), true), ("CHANGELOG.md".into(), false)],
            selected: 0,
            adding: false,
            input_buffer: String::new(),
            input_cursor: 0,
        };
        assert_eq!(o.files.len(), 2);
        assert!(o.files[0].1);
        assert!(!o.files[1].1);
        assert!(!o.adding);
    }

    // ══════════════════════════════════════════════════════════════════
    // ModuleStyleDialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn module_style_dialog_fields() {
        let d = ModuleStyleDialog {
            has_rust: true,
            has_python: false,
            rust_style: RustModuleStyle::FileBased,
            python_style: PythonModuleStyle::Package,
            selected: 0,
        };
        assert!(d.has_rust);
        assert!(!d.has_python);
        assert_eq!(d.rust_style, RustModuleStyle::FileBased);
        assert_eq!(d.python_style, PythonModuleStyle::Package);
    }

    // ══════════════════════════════════════════════════════════════════
    // HealthPanel
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn health_panel_fields() {
        let p = HealthPanel {
            worktree_name: "my-feature".into(),
            tab: HealthTab::GodFiles,
            god_files: vec![],
            god_selected: 0,
            god_scroll: 0,
            doc_entries: vec![],
            doc_selected: 0,
            doc_scroll: 0,
            doc_score: 0.0,
            module_style_dialog: None,
        };
        assert_eq!(p.worktree_name, "my-feature");
        assert_eq!(p.tab, HealthTab::GodFiles);
        assert!(p.god_files.is_empty());
        assert!(p.module_style_dialog.is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    // GitActionsPanel — field construction (no methods to test, but
    // verifying all fields initialize correctly)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn git_actions_panel_construction() {
        let p = GitActionsPanel {
            worktree_name: "feat-api".into(),
            worktree_path: PathBuf::from("/repo/worktrees/feat-api"),
            repo_root: PathBuf::from("/repo"),
            main_branch: "main".into(),
            is_on_main: false,
            changed_files: vec![],
            selected_file: 0,
            file_scroll: 0,
            focused_pane: 0,
            selected_action: 0,
            result_message: None,
            commit_overlay: None,
            conflict_overlay: None,
            commits: vec![],
            selected_commit: 0,
            commit_scroll: 0,
            viewer_diff: None,
            viewer_diff_title: None,
            commits_behind_main: 0,
            commits_ahead_main: 0,
            commits_behind_remote: 0,
            commits_ahead_remote: 0,
            auto_resolve_files: vec![],
            auto_resolve_overlay: None,
            squash_merge_receiver: None,
            discard_confirm: None,
        };
        assert_eq!(p.worktree_name, "feat-api");
        assert!(!p.is_on_main);
        assert_eq!(p.focused_pane, 0);
        assert!(p.result_message.is_none());
        assert!(p.commit_overlay.is_none());
        assert!(p.conflict_overlay.is_none());
        assert!(p.auto_resolve_overlay.is_none());
    }
}
