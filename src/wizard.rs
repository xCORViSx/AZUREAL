//! Creation wizard for new resources (Projects, Branches, Worktrees, Sessions)
//!
//! Provides a tabbed dialog for creating different resource types.

use crate::models::Project;

/// Main wizard container with tabs for different creation types
#[derive(Debug, Clone)]
pub struct CreationWizard {
    /// Currently active tab
    pub active_tab: WizardTab,
    /// Worktree creation wizard state
    pub worktree: WorktreeWizard,
    /// Session creation wizard state
    pub session: SessionWizard,
}

/// Tab selection for creation wizard
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardTab {
    Project,
    Branch,
    Worktree,
    Session,
}

impl WizardTab {
    /// Get all tabs in order
    pub fn all() -> &'static [WizardTab] {
        &[WizardTab::Project, WizardTab::Branch, WizardTab::Worktree, WizardTab::Session]
    }

    /// Get display name for tab
    pub fn name(&self) -> &'static str {
        match self {
            WizardTab::Project => "Project",
            WizardTab::Branch => "Branch",
            WizardTab::Worktree => "Worktree",
            WizardTab::Session => "Session",
        }
    }

    /// Cycle to next tab
    pub fn next(&self) -> WizardTab {
        match self {
            WizardTab::Project => WizardTab::Branch,
            WizardTab::Branch => WizardTab::Worktree,
            WizardTab::Worktree => WizardTab::Session,
            WizardTab::Session => WizardTab::Project,
        }
    }

    /// Cycle to previous tab
    pub fn prev(&self) -> WizardTab {
        match self {
            WizardTab::Project => WizardTab::Session,
            WizardTab::Branch => WizardTab::Project,
            WizardTab::Worktree => WizardTab::Branch,
            WizardTab::Session => WizardTab::Worktree,
        }
    }
}

/// Worktree creation wizard state
#[derive(Debug, Clone)]
pub struct WorktreeWizard {
    /// Current step in the wizard
    pub step: WorktreeStep,
    /// Worktree name (user-editable)
    pub worktree_name: String,
    /// Cursor position in worktree name
    pub name_cursor: usize,
    /// User's prompt input
    pub prompt: String,
    /// Cursor position in prompt
    pub prompt_cursor: usize,
    /// Selected project index (if multiple projects)
    pub selected_project_idx: Option<usize>,
    /// Which field is focused in EnterDetails step
    pub focused_field: WorktreeField,
    /// Validation errors
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeStep {
    /// Select project (shown only if multiple projects)
    SelectProject,
    /// Enter worktree name and prompt
    EnterDetails,
    /// Confirm worktree creation
    Confirm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeField {
    Name,
    Prompt,
}

/// Session creation wizard state (creates named session in existing worktree)
#[derive(Debug, Clone)]
pub struct SessionWizard {
    /// Current step
    pub step: SessionStep,
    /// Selected worktree index
    pub selected_worktree_idx: usize,
    /// Custom session name
    pub session_name: String,
    /// Cursor position in session name
    pub name_cursor: usize,
    /// Initial prompt for session
    pub prompt: String,
    /// Cursor position in prompt
    pub prompt_cursor: usize,
    /// Which field is focused
    pub focused_field: SessionField,
    /// Validation errors
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStep {
    /// Select which worktree to create session in
    SelectWorktree,
    /// Enter session name and prompt
    EnterDetails,
    /// Confirm session creation
    Confirm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionField {
    Name,
    Prompt,
}

impl CreationWizard {
    /// Create a new wizard starting on Worktree tab
    pub fn new(projects: &[Project]) -> Self {
        Self {
            active_tab: WizardTab::Worktree,
            worktree: WorktreeWizard::new(projects),
            session: SessionWizard::new(),
        }
    }

    /// Create a wizard for single-project stateless mode
    pub fn new_single_project(project: Option<&Project>) -> Self {
        Self {
            active_tab: WizardTab::Worktree,
            worktree: WorktreeWizard::new_single_project(project),
            session: SessionWizard::new(),
        }
    }

    /// Cycle to next tab
    pub fn next_tab(&mut self) {
        self.active_tab = self.active_tab.next();
    }

    /// Cycle to previous tab
    pub fn prev_tab(&mut self) {
        self.active_tab = self.active_tab.prev();
    }

    /// Get help text for current tab
    pub fn help_text(&self) -> &'static str {
        match self.active_tab {
            WizardTab::Project => "⌥Tab:type  (Coming soon)",
            WizardTab::Branch => "⌥Tab:type  (Coming soon)",
            WizardTab::Worktree => self.worktree.help_text(),
            WizardTab::Session => self.session.help_text(),
        }
    }

    /// Check if current tab is implemented
    pub fn is_implemented(&self) -> bool {
        matches!(self.active_tab, WizardTab::Worktree | WizardTab::Session)
    }
}

impl WorktreeWizard {
    /// Create a new worktree wizard
    pub fn new(projects: &[Project]) -> Self {
        let step = if projects.len() > 1 {
            WorktreeStep::SelectProject
        } else {
            WorktreeStep::EnterDetails
        };

        Self {
            step,
            worktree_name: String::new(),
            name_cursor: 0,
            prompt: String::new(),
            prompt_cursor: 0,
            selected_project_idx: if projects.len() == 1 { Some(0) } else { None },
            focused_field: WorktreeField::Name,
            errors: Vec::new(),
        }
    }

    /// Create a wizard for single-project stateless mode
    pub fn new_single_project(project: Option<&Project>) -> Self {
        Self {
            step: WorktreeStep::EnterDetails,
            worktree_name: String::new(),
            name_cursor: 0,
            prompt: String::new(),
            prompt_cursor: 0,
            selected_project_idx: if project.is_some() { Some(0) } else { None },
            focused_field: WorktreeField::Name,
            errors: Vec::new(),
        }
    }

    /// Toggle between name and prompt fields
    pub fn toggle_field(&mut self) {
        if self.step == WorktreeStep::EnterDetails {
            self.focused_field = match self.focused_field {
                WorktreeField::Name => WorktreeField::Prompt,
                WorktreeField::Prompt => WorktreeField::Name,
            };
        }
    }

    /// Move to the next step
    pub fn next_step(&mut self) -> bool {
        self.errors.clear();

        match self.step {
            WorktreeStep::SelectProject => {
                if self.selected_project_idx.is_none() {
                    self.errors.push("Please select a project".to_string());
                    return false;
                }
                self.step = WorktreeStep::EnterDetails;
                true
            }
            WorktreeStep::EnterDetails => {
                if self.worktree_name.trim().is_empty() {
                    self.errors.push("Worktree name cannot be empty".to_string());
                    return false;
                }
                if self.prompt.trim().is_empty() {
                    self.errors.push("Prompt cannot be empty".to_string());
                    return false;
                }
                self.step = WorktreeStep::Confirm;
                true
            }
            WorktreeStep::Confirm => true,
        }
    }

    /// Move to the previous step
    pub fn prev_step(&mut self) {
        self.errors.clear();

        match self.step {
            WorktreeStep::SelectProject => {}
            WorktreeStep::EnterDetails => {
                if self.selected_project_idx.is_none() { return; }
                self.step = WorktreeStep::SelectProject;
            }
            WorktreeStep::Confirm => {
                self.step = WorktreeStep::EnterDetails;
            }
        }
    }

    /// Handle character input
    pub fn input_char(&mut self, c: char) {
        if self.step == WorktreeStep::EnterDetails {
            match self.focused_field {
                WorktreeField::Name => {
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        self.worktree_name.insert(self.name_cursor, c);
                        self.name_cursor += 1;
                    }
                }
                WorktreeField::Prompt => {
                    self.prompt.insert(self.prompt_cursor, c);
                    self.prompt_cursor += 1;
                }
            }
        }
    }

    /// Handle backspace
    pub fn input_backspace(&mut self) {
        if self.step == WorktreeStep::EnterDetails {
            match self.focused_field {
                WorktreeField::Name => {
                    if self.name_cursor > 0 {
                        self.name_cursor -= 1;
                        self.worktree_name.remove(self.name_cursor);
                    }
                }
                WorktreeField::Prompt => {
                    if self.prompt_cursor > 0 {
                        self.prompt_cursor -= 1;
                        self.prompt.remove(self.prompt_cursor);
                    }
                }
            }
        }
    }

    /// Handle delete
    pub fn input_delete(&mut self) {
        if self.step == WorktreeStep::EnterDetails {
            match self.focused_field {
                WorktreeField::Name => {
                    if self.name_cursor < self.worktree_name.len() {
                        self.worktree_name.remove(self.name_cursor);
                    }
                }
                WorktreeField::Prompt => {
                    if self.prompt_cursor < self.prompt.len() {
                        self.prompt.remove(self.prompt_cursor);
                    }
                }
            }
        }
    }

    /// Move cursor left
    pub fn cursor_left(&mut self) {
        if self.step == WorktreeStep::EnterDetails {
            match self.focused_field {
                WorktreeField::Name => self.name_cursor = self.name_cursor.saturating_sub(1),
                WorktreeField::Prompt => self.prompt_cursor = self.prompt_cursor.saturating_sub(1),
            }
        }
    }

    /// Move cursor right
    pub fn cursor_right(&mut self) {
        if self.step == WorktreeStep::EnterDetails {
            match self.focused_field {
                WorktreeField::Name => {
                    if self.name_cursor < self.worktree_name.len() {
                        self.name_cursor += 1;
                    }
                }
                WorktreeField::Prompt => {
                    if self.prompt_cursor < self.prompt.len() {
                        self.prompt_cursor += 1;
                    }
                }
            }
        }
    }

    /// Move cursor to start
    pub fn cursor_home(&mut self) {
        if self.step == WorktreeStep::EnterDetails {
            match self.focused_field {
                WorktreeField::Name => self.name_cursor = 0,
                WorktreeField::Prompt => self.prompt_cursor = 0,
            }
        }
    }

    /// Move cursor to end
    pub fn cursor_end(&mut self) {
        if self.step == WorktreeStep::EnterDetails {
            match self.focused_field {
                WorktreeField::Name => self.name_cursor = self.worktree_name.len(),
                WorktreeField::Prompt => self.prompt_cursor = self.prompt.len(),
            }
        }
    }

    /// Select next project
    pub fn select_next_project(&mut self, max_projects: usize) {
        if self.step == WorktreeStep::SelectProject {
            self.selected_project_idx = Some(
                self.selected_project_idx
                    .map(|idx| (idx + 1).min(max_projects - 1))
                    .unwrap_or(0)
            );
        }
    }

    /// Select previous project
    pub fn select_prev_project(&mut self) {
        if self.step == WorktreeStep::SelectProject {
            if let Some(idx) = self.selected_project_idx {
                if idx > 0 {
                    self.selected_project_idx = Some(idx - 1);
                }
            } else {
                self.selected_project_idx = Some(0);
            }
        }
    }

    /// Get the final worktree name (sanitized for branch name)
    pub fn final_worktree_name(&self) -> String {
        sanitize_for_branch(&self.worktree_name)
    }

    /// Get the current step title
    pub fn step_title(&self) -> &'static str {
        match self.step {
            WorktreeStep::SelectProject => "Select Project",
            WorktreeStep::EnterDetails => "Enter Details",
            WorktreeStep::Confirm => "Confirm",
        }
    }

    /// Get help text for the current step
    pub fn help_text(&self) -> &'static str {
        match self.step {
            WorktreeStep::SelectProject => "⌥Tab:type  j/k:select  Enter:next  Esc:cancel",
            WorktreeStep::EnterDetails => "⌥Tab:type  Tab:field  Enter:next  Esc:back",
            WorktreeStep::Confirm => "⌥Tab:type  Enter:create  Esc:back  q:cancel",
        }
    }

    /// Get step number (1-based) and total steps
    pub fn step_progress(&self) -> (usize, usize) {
        let total = if self.selected_project_idx.is_none() { 3 } else { 2 };
        let current = match self.step {
            WorktreeStep::SelectProject => 1,
            WorktreeStep::EnterDetails => if self.selected_project_idx.is_none() { 2 } else { 1 },
            WorktreeStep::Confirm => if self.selected_project_idx.is_none() { 3 } else { 2 },
        };
        (current, total)
    }
}

impl SessionWizard {
    /// Create a new session wizard
    pub fn new() -> Self {
        Self {
            step: SessionStep::SelectWorktree,
            selected_worktree_idx: 0,
            session_name: String::new(),
            name_cursor: 0,
            prompt: String::new(),
            prompt_cursor: 0,
            focused_field: SessionField::Name,
            errors: Vec::new(),
        }
    }

    /// Toggle between name and prompt fields
    pub fn toggle_field(&mut self) {
        if self.step == SessionStep::EnterDetails {
            self.focused_field = match self.focused_field {
                SessionField::Name => SessionField::Prompt,
                SessionField::Prompt => SessionField::Name,
            };
        }
    }

    /// Move to the next step
    pub fn next_step(&mut self) -> bool {
        self.errors.clear();

        match self.step {
            SessionStep::SelectWorktree => {
                self.step = SessionStep::EnterDetails;
                true
            }
            SessionStep::EnterDetails => {
                if self.session_name.trim().is_empty() {
                    self.errors.push("Session name cannot be empty".to_string());
                    return false;
                }
                if self.prompt.trim().is_empty() {
                    self.errors.push("Prompt cannot be empty".to_string());
                    return false;
                }
                self.step = SessionStep::Confirm;
                true
            }
            SessionStep::Confirm => true,
        }
    }

    /// Move to the previous step
    pub fn prev_step(&mut self) {
        self.errors.clear();

        match self.step {
            SessionStep::SelectWorktree => {}
            SessionStep::EnterDetails => {
                self.step = SessionStep::SelectWorktree;
            }
            SessionStep::Confirm => {
                self.step = SessionStep::EnterDetails;
            }
        }
    }

    /// Handle character input
    pub fn input_char(&mut self, c: char) {
        if self.step == SessionStep::EnterDetails {
            match self.focused_field {
                SessionField::Name => {
                    // Allow more characters for session names (spaces, etc.)
                    if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                        self.session_name.insert(self.name_cursor, c);
                        self.name_cursor += 1;
                    }
                }
                SessionField::Prompt => {
                    self.prompt.insert(self.prompt_cursor, c);
                    self.prompt_cursor += 1;
                }
            }
        }
    }

    /// Handle backspace
    pub fn input_backspace(&mut self) {
        if self.step == SessionStep::EnterDetails {
            match self.focused_field {
                SessionField::Name => {
                    if self.name_cursor > 0 {
                        self.name_cursor -= 1;
                        self.session_name.remove(self.name_cursor);
                    }
                }
                SessionField::Prompt => {
                    if self.prompt_cursor > 0 {
                        self.prompt_cursor -= 1;
                        self.prompt.remove(self.prompt_cursor);
                    }
                }
            }
        }
    }

    /// Handle delete
    pub fn input_delete(&mut self) {
        if self.step == SessionStep::EnterDetails {
            match self.focused_field {
                SessionField::Name => {
                    if self.name_cursor < self.session_name.len() {
                        self.session_name.remove(self.name_cursor);
                    }
                }
                SessionField::Prompt => {
                    if self.prompt_cursor < self.prompt.len() {
                        self.prompt.remove(self.prompt_cursor);
                    }
                }
            }
        }
    }

    /// Move cursor left
    pub fn cursor_left(&mut self) {
        if self.step == SessionStep::EnterDetails {
            match self.focused_field {
                SessionField::Name => self.name_cursor = self.name_cursor.saturating_sub(1),
                SessionField::Prompt => self.prompt_cursor = self.prompt_cursor.saturating_sub(1),
            }
        }
    }

    /// Move cursor right
    pub fn cursor_right(&mut self) {
        if self.step == SessionStep::EnterDetails {
            match self.focused_field {
                SessionField::Name => {
                    if self.name_cursor < self.session_name.len() {
                        self.name_cursor += 1;
                    }
                }
                SessionField::Prompt => {
                    if self.prompt_cursor < self.prompt.len() {
                        self.prompt_cursor += 1;
                    }
                }
            }
        }
    }

    /// Move cursor to start
    pub fn cursor_home(&mut self) {
        if self.step == SessionStep::EnterDetails {
            match self.focused_field {
                SessionField::Name => self.name_cursor = 0,
                SessionField::Prompt => self.prompt_cursor = 0,
            }
        }
    }

    /// Move cursor to end
    pub fn cursor_end(&mut self) {
        if self.step == SessionStep::EnterDetails {
            match self.focused_field {
                SessionField::Name => self.name_cursor = self.session_name.len(),
                SessionField::Prompt => self.prompt_cursor = self.prompt.len(),
            }
        }
    }

    /// Select next worktree
    pub fn select_next(&mut self, max: usize) {
        if self.step == SessionStep::SelectWorktree && max > 0 {
            self.selected_worktree_idx = (self.selected_worktree_idx + 1).min(max - 1);
        }
    }

    /// Select previous worktree
    pub fn select_prev(&mut self) {
        if self.step == SessionStep::SelectWorktree {
            self.selected_worktree_idx = self.selected_worktree_idx.saturating_sub(1);
        }
    }

    /// Get the current step title
    pub fn step_title(&self) -> &'static str {
        match self.step {
            SessionStep::SelectWorktree => "Select Worktree",
            SessionStep::EnterDetails => "Enter Details",
            SessionStep::Confirm => "Confirm",
        }
    }

    /// Get help text for the current step
    pub fn help_text(&self) -> &'static str {
        match self.step {
            SessionStep::SelectWorktree => "⌥Tab:type  j/k:select  Enter:next  Esc:cancel",
            SessionStep::EnterDetails => "⌥Tab:type  Tab:field  Enter:next  Esc:back",
            SessionStep::Confirm => "⌥Tab:type  Enter:create  Esc:back  q:cancel",
        }
    }

    /// Get step number (1-based) and total steps
    pub fn step_progress(&self) -> (usize, usize) {
        let current = match self.step {
            SessionStep::SelectWorktree => 1,
            SessionStep::EnterDetails => 2,
            SessionStep::Confirm => 3,
        };
        (current, 3)
    }
}

impl Default for SessionWizard {
    fn default() -> Self {
        Self::new()
    }
}

/// Sanitize a string for use as a branch name
pub fn sanitize_for_branch(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .to_lowercase()
}
