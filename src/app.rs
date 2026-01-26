use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use crate::claude::ClaudeEvent;
use crate::db::Database;
use crate::git::Git;
use crate::models::{Project, RebaseStatus, Session, SessionStatus};
use crate::session::SessionManager;
use crate::syntax::DiffHighlighter;
use crate::wizard::SessionCreationWizard;

/// Application state
pub struct App {
    /// Database connection
    pub db: Database,
    /// All projects
    pub projects: Vec<Project>,
    /// Currently selected project index
    pub selected_project: usize,
    /// Sessions for current project
    pub sessions: Vec<Session>,
    /// Currently selected session index
    pub selected_session: Option<usize>,
    /// Output lines for current session
    pub output_lines: VecDeque<String>,
    /// Maximum output lines to keep
    pub max_output_lines: usize,
    /// Buffer for incomplete lines (streaming chunks)
    pub output_buffer: String,
    /// Current input text
    pub input: String,
    /// Input cursor position
    pub input_cursor: usize,
    /// Session creation prompt (multi-line)
    pub session_creation_input: String,
    /// Session creation cursor position (linear position in string)
    pub session_creation_cursor: usize,
    /// Current view mode
    pub view_mode: ViewMode,
    /// Current focus
    pub focus: Focus,
    /// Whether the app should quit
    pub should_quit: bool,
    /// Status message to display
    pub status_message: Option<String>,
    /// Active Claude process receiver
    pub claude_receiver: Option<Receiver<ClaudeEvent>>,
    /// Currently running session ID (for sending input)
    pub running_session_id: Option<String>,
    /// Current diff text (if viewing diff)
    pub diff_text: Option<String>,
    /// Scroll offset for output
    pub output_scroll: usize,
    /// Scroll offset for diff
    pub diff_scroll: usize,
    /// Syntax highlighter for diff view
    pub diff_highlighter: DiffHighlighter,
    /// Whether to show help overlay
    pub show_help: bool,
    /// Branch selection dialog state
    pub branch_dialog: Option<BranchDialog>,
    /// Current rebase status (if any)
    pub rebase_status: Option<RebaseStatus>,
    /// Selected conflict file index (for conflict resolution)
    pub selected_conflict: Option<usize>,
    /// Context menu state
    pub context_menu: Option<ContextMenu>,
    /// Session creation wizard (if active)
    pub creation_wizard: Option<SessionCreationWizard>,
}

/// State for the branch selection dialog
pub struct BranchDialog {
    /// Available branches to select from
    pub branches: Vec<String>,
    /// Currently selected index
    pub selected: usize,
    /// Filter/search text
    pub filter: String,
    /// Filtered branch indices
    pub filtered_indices: Vec<usize>,
}

/// Context menu for session actions
#[derive(Debug, Clone)]
pub struct ContextMenu {
    /// Available actions
    pub actions: Vec<SessionAction>,
    /// Selected action index
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Output,
    Diff,
    Messages,
    Rebase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sessions,
    Output,
    Input,
    SessionCreation,
    BranchDialog,
}

impl BranchDialog {
    pub fn new(branches: Vec<String>) -> Self {
        let filtered_indices: Vec<usize> = (0..branches.len()).collect();
        Self {
            branches,
            selected: 0,
            filter: String::new(),
            filtered_indices,
        }
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

        // Reset selection if current selection is out of bounds
        if self.selected >= self.filtered_indices.len() {
            self.selected = 0;
        }
    }

    pub fn selected_branch(&self) -> Option<&String> {
        self.filtered_indices
            .get(self.selected)
            .and_then(|&idx| self.branches.get(idx))
    }

    pub fn select_next(&mut self) {
        if !self.filtered_indices.is_empty() && self.selected + 1 < self.filtered_indices.len() {
            self.selected += 1;
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
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

impl App {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            projects: Vec::new(),
            selected_project: 0,
            sessions: Vec::new(),
            selected_session: None,
            output_lines: VecDeque::with_capacity(10000),
            max_output_lines: 10000,
            output_buffer: String::new(),
            input: String::new(),
            input_cursor: 0,
            session_creation_input: String::new(),
            session_creation_cursor: 0,
            view_mode: ViewMode::Output,
            focus: Focus::Sessions,
            should_quit: false,
            status_message: None,
            claude_receiver: None,
            running_session_id: None,
            diff_text: None,
            output_scroll: 0,
            diff_scroll: 0,
            diff_highlighter: DiffHighlighter::new(),
            show_help: false,
            branch_dialog: None,
            rebase_status: None,
            selected_conflict: None,
            context_menu: None,
            creation_wizard: None,
        }
    }

    /// Load initial data
    pub fn load(&mut self) -> anyhow::Result<()> {
        self.projects = self.db.list_projects()?;

        if !self.projects.is_empty() {
            self.load_sessions_for_project()?;
        }

        Ok(())
    }

    /// Load sessions for the currently selected project by scanning git worktrees
    pub fn load_sessions_for_project(&mut self) -> anyhow::Result<()> {
        if let Some(project) = self.projects.get(self.selected_project) {
            // Scan git worktrees directly instead of database
            let worktrees = Git::list_worktrees_detailed(&project.path)?;

            self.sessions = worktrees
                .into_iter()
                .filter(|wt| !wt.is_main) // Skip main worktree
                .map(|wt| {
                    let name = wt.path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    Session {
                        id: name.clone(), // Use worktree name as stable ID
                        name: name.clone(),
                        initial_prompt: String::new(), // Not stored in git
                        worktree_name: name,
                        worktree_path: wt.path,
                        branch_name: wt.branch.unwrap_or_default(),
                        status: SessionStatus::Pending,
                        project_id: project.id,
                        pid: None,
                        exit_code: None,
                        archived: false,
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                    }
                })
                .collect();

            self.selected_session = if self.sessions.is_empty() {
                None
            } else {
                Some(0)
            };
        }
        Ok(())
    }

    /// Get the currently selected project
    pub fn current_project(&self) -> Option<&Project> {
        self.projects.get(self.selected_project)
    }

    /// Get the currently selected session
    pub fn current_session(&self) -> Option<&Session> {
        self.selected_session
            .and_then(|idx| self.sessions.get(idx))
    }

    /// Select next session
    pub fn select_next_session(&mut self) {
        if let Some(idx) = self.selected_session {
            if idx + 1 < self.sessions.len() {
                self.selected_session = Some(idx + 1);
                self.load_session_output();
            }
        } else if !self.sessions.is_empty() {
            self.selected_session = Some(0);
            self.load_session_output();
        }
    }

    /// Select previous session
    pub fn select_prev_session(&mut self) {
        if let Some(idx) = self.selected_session {
            if idx > 0 {
                self.selected_session = Some(idx - 1);
                self.load_session_output();
            }
        }
    }

    /// Select next project
    pub fn select_next_project(&mut self) {
        if self.selected_project + 1 < self.projects.len() {
            self.selected_project += 1;
            let _ = self.load_sessions_for_project();
            self.load_session_output();
        }
    }

    /// Select previous project
    pub fn select_prev_project(&mut self) {
        if self.selected_project > 0 {
            self.selected_project -= 1;
            let _ = self.load_sessions_for_project();
            self.load_session_output();
        }
    }

    /// Load output for the current session
    pub fn load_session_output(&mut self) {
        self.output_lines.clear();
        self.output_buffer.clear();
        self.output_scroll = 0;

        if let Some(session) = self.current_session() {
            if let Ok(outputs) = self.db.get_session_outputs(&session.id) {
                for output in outputs {
                    // Process stored output chunks
                    self.process_output_chunk(&output.data);
                }
            }
        }
    }

    /// Process an output chunk (may contain partial lines)
    fn process_output_chunk(&mut self, chunk: &str) {
        // Strip ANSI escape sequences for cleaner display
        let cleaned = strip_ansi_escapes(chunk);

        for ch in cleaned.chars() {
            match ch {
                '\n' => {
                    // Complete line - add to output_lines
                    let line = self.output_buffer.clone();
                    self.output_lines.push_back(line);
                    self.output_buffer.clear();

                    if self.output_lines.len() > self.max_output_lines {
                        self.output_lines.pop_front();
                    }
                }
                '\r' => {
                    // Carriage return - overwrite current line buffer
                    // This handles progress indicators that use \r to update in place
                    self.output_buffer.clear();
                }
                _ => {
                    // Regular character - append to buffer
                    self.output_buffer.push(ch);
                }
            }
        }
    }

    /// Add output chunk (streaming mode)
    pub fn add_output(&mut self, chunk: String) {
        self.process_output_chunk(&chunk);
        // Auto-scroll to bottom - use usize::MAX as sentinel for auto-scroll
        // The actual scroll position will be clamped in the UI layer
        self.output_scroll = usize::MAX;
    }

    /// Scroll output down
    pub fn scroll_output_down(&mut self, lines: usize, viewport_height: usize) {
        let max_scroll = self.output_lines.len().saturating_sub(viewport_height);
        self.output_scroll = self.output_scroll.saturating_add(lines).min(max_scroll);
    }

    /// Scroll output up
    pub fn scroll_output_up(&mut self, lines: usize) {
        self.output_scroll = self.output_scroll.saturating_sub(lines);
    }

    /// Scroll to bottom of output
    pub fn scroll_output_to_bottom(&mut self, viewport_height: usize) {
        self.output_scroll = self.output_lines.len().saturating_sub(viewport_height);
    }

    /// Scroll diff down
    pub fn scroll_diff_down(&mut self, lines: usize, viewport_height: usize) {
        if let Some(ref diff) = self.diff_text {
            let total_lines = diff.lines().count();
            let max_scroll = total_lines.saturating_sub(viewport_height);
            self.diff_scroll = self.diff_scroll.saturating_add(lines).min(max_scroll);
        }
    }

    /// Scroll diff up
    pub fn scroll_diff_up(&mut self, lines: usize) {
        self.diff_scroll = self.diff_scroll.saturating_sub(lines);
    }

    /// Scroll to bottom of diff
    pub fn scroll_diff_to_bottom(&mut self, viewport_height: usize) {
        if let Some(ref diff) = self.diff_text {
            let total_lines = diff.lines().count();
            self.diff_scroll = total_lines.saturating_sub(viewport_height);
        }
    }

    /// Set status message
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }

    /// Clear status message
    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    /// Handle input character
    pub fn input_char(&mut self, c: char) {
        self.input.insert(self.input_cursor, c);
        self.input_cursor += 1;
    }

    /// Handle backspace
    pub fn input_backspace(&mut self) {
        if self.input_cursor > 0 {
            self.input_cursor -= 1;
            self.input.remove(self.input_cursor);
        }
    }

    /// Handle delete
    pub fn input_delete(&mut self) {
        if self.input_cursor < self.input.len() {
            self.input.remove(self.input_cursor);
        }
    }

    /// Move cursor left
    pub fn input_left(&mut self) {
        self.input_cursor = self.input_cursor.saturating_sub(1);
    }

    /// Move cursor right
    pub fn input_right(&mut self) {
        if self.input_cursor < self.input.len() {
            self.input_cursor += 1;
        }
    }

    /// Move cursor to start
    pub fn input_home(&mut self) {
        self.input_cursor = 0;
    }

    /// Move cursor to end
    pub fn input_end(&mut self) {
        self.input_cursor = self.input.len();
    }

    /// Move cursor to previous word boundary
    pub fn input_word_left(&mut self) {
        if self.input_cursor == 0 {
            return;
        }

        let chars: Vec<char> = self.input.chars().collect();
        let mut pos = self.input_cursor.saturating_sub(1);

        // Skip whitespace
        while pos > 0 && chars[pos].is_whitespace() {
            pos -= 1;
        }

        // Skip non-whitespace to find word boundary
        while pos > 0 && !chars[pos - 1].is_whitespace() {
            pos -= 1;
        }

        self.input_cursor = pos;
    }

    /// Move cursor to next word boundary
    pub fn input_word_right(&mut self) {
        let chars: Vec<char> = self.input.chars().collect();
        if self.input_cursor >= chars.len() {
            return;
        }

        let mut pos = self.input_cursor;

        // Skip non-whitespace
        while pos < chars.len() && !chars[pos].is_whitespace() {
            pos += 1;
        }

        // Skip whitespace
        while pos < chars.len() && chars[pos].is_whitespace() {
            pos += 1;
        }

        self.input_cursor = pos;
    }

    /// Delete word before cursor
    pub fn input_delete_word(&mut self) {
        if self.input_cursor == 0 {
            return;
        }

        let chars: Vec<char> = self.input.chars().collect();
        let mut pos = self.input_cursor.saturating_sub(1);

        // Skip whitespace
        while pos > 0 && chars[pos].is_whitespace() {
            pos -= 1;
        }

        // Skip non-whitespace to find word boundary
        while pos > 0 && !chars[pos - 1].is_whitespace() {
            pos -= 1;
        }

        // Remove from pos to cursor
        let before: String = chars[..pos].iter().collect();
        let after: String = chars[self.input_cursor..].iter().collect();
        self.input = format!("{}{}", before, after);
        self.input_cursor = pos;
    }

    /// Clear input
    pub fn clear_input(&mut self) {
        self.input.clear();
        self.input_cursor = 0;
    }

    /// Add a project by path
    pub fn add_project(&mut self, path: PathBuf) -> anyhow::Result<()> {
        let project = self.db.get_or_create_project(&path)?;
        self.projects.push(project);
        self.selected_project = self.projects.len() - 1;
        self.load_sessions_for_project()?;
        Ok(())
    }

    /// Refresh session list
    pub fn refresh_sessions(&mut self) -> anyhow::Result<()> {
        self.load_sessions_for_project()
    }

    /// Open the branch selection dialog
    pub fn open_branch_dialog(&mut self, branches: Vec<String>) {
        if branches.is_empty() {
            self.set_status("No available branches to checkout");
            return;
        }
        self.branch_dialog = Some(BranchDialog::new(branches));
        self.focus = Focus::BranchDialog;
    }

    /// Close the branch selection dialog
    pub fn close_branch_dialog(&mut self) {
        self.branch_dialog = None;
        self.focus = Focus::Sessions;
    }

    /// Update session status in the list
    pub fn update_session_status(&mut self, session_id: &str, status: SessionStatus) {
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == session_id) {
            session.status = status;
        }
    }

    /// Handle Claude process started event
    pub fn handle_claude_started(&mut self, pid: u32) {
        if let Some(session) = self.current_session() {
            let session_id = session.id.clone();
            let _ = self.db.update_session_pid(&session_id, Some(pid));
            let _ = self.db.update_session_status(&session_id, SessionStatus::Running);
            self.update_session_status(&session_id, SessionStatus::Running);
        }
        self.set_status(format!("Claude started (PID: {})", pid));
    }

    /// Handle Claude process exited event
    pub fn handle_claude_exited(&mut self, code: Option<i32>) -> bool {
        if let Some(session) = self.current_session() {
            let session_id = session.id.clone();
            let status = if code == Some(0) {
                SessionStatus::Completed
            } else {
                SessionStatus::Failed
            };
            let _ = self.db.update_session_status(&session_id, status);
            self.update_session_status(&session_id, status);
        }
        self.set_status(format!("Claude exited with code: {:?}", code));
        true // Signal to clear receiver
    }

    /// Handle Claude output event
    pub fn handle_claude_output(&mut self, output_type: crate::models::OutputType, data: String) {
        // Save to database first
        if let Some(session) = self.current_session() {
            let session_id = session.id.clone();
            let _ = self.db.add_session_output(&session_id, output_type, &data);
        }
        self.add_output(data);
    }

    /// Handle Claude error event
    pub fn handle_claude_error(&mut self, error: String) {
        self.add_output(format!("Error: {}", error));
        self.set_status(format!("Error: {}", error));
    }

    /// Create a new session with the given prompt
    pub fn create_new_session(&mut self, prompt: String) -> anyhow::Result<crate::models::Session> {
        if let Some(project) = self.current_project().cloned() {
            let session = SessionManager::new(&self.db).create_session(&project, &prompt)?;
            self.refresh_sessions()?;
            self.selected_session = Some(0);
            self.load_session_output();
            Ok(session)
        } else {
            anyhow::bail!("No project selected")
        }
    }

    /// Archive the current session
    pub fn archive_current_session(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_session() {
            let session_id = session.id.clone();
            SessionManager::new(&self.db).archive_session(&session_id)?;
            self.set_status("Session archived");
            self.refresh_sessions()?;
        }
        Ok(())
    }

    /// Get diff for current session
    pub fn load_diff(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_session() {
            if let Some(project) = self.current_project() {
                let diff = Git::get_diff(&session.worktree_path, &project.main_branch)?;
                self.diff_text = Some(diff.diff_text);
                self.view_mode = ViewMode::Diff;
                self.focus = Focus::Output;
                Ok(())
            } else {
                anyhow::bail!("No project selected")
            }
        } else {
            anyhow::bail!("No session selected")
        }
    }

    /// Rebase current session onto main
    pub fn rebase_current_session(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_session() {
            if let Some(project) = self.current_project() {
                Git::rebase_onto_main(&session.worktree_path, &project.main_branch)?;
                self.set_status("Rebased successfully");
                Ok(())
            } else {
                anyhow::bail!("No project selected")
            }
        } else {
            anyhow::bail!("No session selected")
        }
    }

    /// Cycle focus forward
    pub fn focus_next(&mut self) {
        self.focus = match self.focus {
            Focus::Sessions => Focus::Output,
            Focus::Output => Focus::Input,
            Focus::Input => Focus::Sessions,
            Focus::SessionCreation => Focus::SessionCreation, // Don't cycle out of modal
            Focus::BranchDialog => Focus::BranchDialog, // Don't cycle when dialog is open
        };
    }

    /// Cycle focus backward
    pub fn focus_prev(&mut self) {
        self.focus = match self.focus {
            Focus::Sessions => Focus::Input,
            Focus::Output => Focus::Sessions,
            Focus::Input => Focus::Output,
            Focus::SessionCreation => Focus::SessionCreation, // Don't cycle out of modal
            Focus::BranchDialog => Focus::BranchDialog, // Don't cycle when dialog is open
        };
    }

    /// Toggle help overlay
    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    // Session creation input methods

    /// Handle character input for session creation
    pub fn session_creation_char(&mut self, c: char) {
        self.session_creation_input.insert(self.session_creation_cursor, c);
        self.session_creation_cursor += c.len_utf8();
    }

    /// Handle backspace for session creation
    pub fn session_creation_backspace(&mut self) {
        if self.session_creation_cursor > 0 {
            let mut idx = self.session_creation_cursor - 1;
            while idx > 0 && !self.session_creation_input.is_char_boundary(idx) {
                idx -= 1;
            }
            self.session_creation_input.remove(idx);
            self.session_creation_cursor = idx;
        }
    }

    /// Handle delete for session creation
    pub fn session_creation_delete(&mut self) {
        if self.session_creation_cursor < self.session_creation_input.len() {
            self.session_creation_input.remove(self.session_creation_cursor);
        }
    }

    /// Move cursor left in session creation
    pub fn session_creation_left(&mut self) {
        if self.session_creation_cursor > 0 {
            let mut idx = self.session_creation_cursor - 1;
            while idx > 0 && !self.session_creation_input.is_char_boundary(idx) {
                idx -= 1;
            }
            self.session_creation_cursor = idx;
        }
    }

    /// Move cursor right in session creation
    pub fn session_creation_right(&mut self) {
        if self.session_creation_cursor < self.session_creation_input.len() {
            let mut idx = self.session_creation_cursor + 1;
            while idx < self.session_creation_input.len()
                && !self.session_creation_input.is_char_boundary(idx)
            {
                idx += 1;
            }
            self.session_creation_cursor = idx;
        }
    }

    /// Move cursor to start of session creation input
    pub fn session_creation_home(&mut self) {
        self.session_creation_cursor = 0;
    }

    /// Move cursor to end of session creation input
    pub fn session_creation_end(&mut self) {
        self.session_creation_cursor = self.session_creation_input.len();
    }

    /// Clear session creation input
    pub fn clear_session_creation_input(&mut self) {
        self.session_creation_input.clear();
        self.session_creation_cursor = 0;
    }

    /// Enter session creation mode
    pub fn enter_session_creation_mode(&mut self) {
        self.focus = Focus::SessionCreation;
        self.clear_session_creation_input();
        self.set_status("Enter prompt for new session (Ctrl+Enter to submit, Esc to cancel)");
    }

    /// Exit session creation mode
    pub fn exit_session_creation_mode(&mut self) {
        self.focus = Focus::Sessions;
        self.clear_session_creation_input();
        self.clear_status();
    }

    /// Set rebase status and switch to rebase view
    pub fn set_rebase_status(&mut self, status: RebaseStatus) {
        self.rebase_status = Some(status);
        self.selected_conflict = if self.rebase_status.as_ref().map_or(false, |s| !s.conflicted_files.is_empty()) {
            Some(0)
        } else {
            None
        };
        self.view_mode = ViewMode::Rebase;
        self.focus = Focus::Output;
    }

    /// Clear rebase status
    pub fn clear_rebase_status(&mut self) {
        self.rebase_status = None;
        self.selected_conflict = None;
        if self.view_mode == ViewMode::Rebase {
            self.view_mode = ViewMode::Output;
        }
    }

    /// Select next conflict file
    pub fn select_next_conflict(&mut self) {
        if let Some(ref status) = self.rebase_status {
            if let Some(idx) = self.selected_conflict {
                if idx + 1 < status.conflicted_files.len() {
                    self.selected_conflict = Some(idx + 1);
                }
            }
        }
    }

    /// Select previous conflict file
    pub fn select_prev_conflict(&mut self) {
        if let Some(idx) = self.selected_conflict {
            if idx > 0 {
                self.selected_conflict = Some(idx - 1);
            }
        }
    }

    /// Get the currently selected conflict file path
    pub fn current_conflict_file(&self) -> Option<&str> {
        self.rebase_status.as_ref().and_then(|status| {
            self.selected_conflict.and_then(|idx| {
                status.conflicted_files.get(idx).map(|s| s.as_str())
            })
        })
    }

    /// Open context menu for current session
    pub fn open_context_menu(&mut self) {
        if let Some(session) = self.current_session() {
            let actions = SessionAction::available_for_status(session.status);
            if !actions.is_empty() {
                self.context_menu = Some(ContextMenu {
                    actions,
                    selected: 0,
                });
            }
        }
    }

    /// Close context menu
    pub fn close_context_menu(&mut self) {
        self.context_menu = None;
    }

    /// Select next action in context menu
    pub fn context_menu_next(&mut self) {
        if let Some(ref mut menu) = self.context_menu {
            if menu.selected + 1 < menu.actions.len() {
                menu.selected += 1;
            }
        }
    }

    /// Select previous action in context menu
    pub fn context_menu_prev(&mut self) {
        if let Some(ref mut menu) = self.context_menu {
            if menu.selected > 0 {
                menu.selected -= 1;
            }
        }
    }

    /// Get currently selected action
    pub fn selected_action(&self) -> Option<SessionAction> {
        self.context_menu.as_ref().map(|menu| menu.actions[menu.selected].clone())
    }

    /// Start the session creation wizard
    pub fn start_wizard(&mut self) {
        self.creation_wizard = Some(SessionCreationWizard::new(&self.projects));
        self.focus = Focus::Input; // Reuse Input focus for wizard
    }

    /// Cancel the wizard
    pub fn cancel_wizard(&mut self) {
        self.creation_wizard = None;
        self.focus = Focus::Sessions;
    }

    /// Check if wizard is active
    pub fn is_wizard_active(&self) -> bool {
        self.creation_wizard.is_some()
    }
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

    /// Get available actions based on session status
    pub fn available_for_status(status: SessionStatus) -> Vec<SessionAction> {
        match status {
            SessionStatus::Pending | SessionStatus::Stopped | SessionStatus::Completed => {
                vec![
                    SessionAction::Start,
                    SessionAction::ViewDiff,
                    SessionAction::RebaseFromMain,
                    SessionAction::OpenInEditor,
                    SessionAction::CopyWorktreePath,
                    SessionAction::Archive,
                    SessionAction::Delete,
                ]
            }
            SessionStatus::Running | SessionStatus::Waiting => {
                vec![
                    SessionAction::Stop,
                    SessionAction::ViewDiff,
                    SessionAction::OpenInEditor,
                    SessionAction::CopyWorktreePath,
                ]
            }
            SessionStatus::Failed => {
                vec![
                    SessionAction::Start,
                    SessionAction::ViewDiff,
                    SessionAction::RebaseFromMain,
                    SessionAction::OpenInEditor,
                    SessionAction::CopyWorktreePath,
                    SessionAction::Archive,
                    SessionAction::Delete,
                ]
            }
        }
    }
}

/// Strip ANSI escape sequences from text
fn strip_ansi_escapes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // ESC character - start of escape sequence
            match chars.peek() {
                Some(&'[') => {
                    // CSI sequence: \x1b[...letter
                    chars.next();
                    while let Some(&next_ch) = chars.peek() {
                        chars.next();
                        if next_ch.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(&']') => {
                    // OSC sequence: \x1b]...(\x07 or \x1b\\)
                    // Used by Claude Code for progress notifications like \x1b]9;4;0;...\x07
                    chars.next();
                    while let Some(&next_ch) = chars.peek() {
                        if next_ch == '\x07' {
                            chars.next();
                            break;
                        }
                        if next_ch == '\x1b' {
                            chars.next();
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                        chars.next();
                    }
                }
                _ => {
                    // Other escape sequences
                    chars.next();
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}
