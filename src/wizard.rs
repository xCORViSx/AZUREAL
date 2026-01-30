use crate::models::Project;

/// Worktree creation wizard state
#[derive(Debug, Clone)]
pub struct SessionCreationWizard {
    /// Current step in the wizard
    pub step: WizardStep,
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
    pub focused_field: WizardField,
    /// Validation errors
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    /// Select project (shown only if multiple projects)
    SelectProject,
    /// Enter worktree name and prompt
    EnterDetails,
    /// Confirm worktree creation
    Confirm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardField {
    Name,
    Prompt,
}

impl SessionCreationWizard {
    /// Create a new wizard
    pub fn new(projects: &[Project]) -> Self {
        let step = if projects.len() > 1 {
            WizardStep::SelectProject
        } else {
            WizardStep::EnterDetails
        };

        Self {
            step,
            worktree_name: String::new(),
            name_cursor: 0,
            prompt: String::new(),
            prompt_cursor: 0,
            selected_project_idx: if projects.len() == 1 { Some(0) } else { None },
            focused_field: WizardField::Name,
            errors: Vec::new(),
        }
    }

    /// Create a wizard for single-project stateless mode
    pub fn new_single_project(project: Option<&Project>) -> Self {
        Self {
            step: WizardStep::EnterDetails,
            worktree_name: String::new(),
            name_cursor: 0,
            prompt: String::new(),
            prompt_cursor: 0,
            selected_project_idx: if project.is_some() { Some(0) } else { None },
            focused_field: WizardField::Name,
            errors: Vec::new(),
        }
    }

    /// Toggle between name and prompt fields
    pub fn toggle_field(&mut self) {
        if self.step == WizardStep::EnterDetails {
            self.focused_field = match self.focused_field {
                WizardField::Name => WizardField::Prompt,
                WizardField::Prompt => WizardField::Name,
            };
        }
    }

    /// Move to the next step
    pub fn next_step(&mut self) -> bool {
        self.errors.clear();

        match self.step {
            WizardStep::SelectProject => {
                if self.selected_project_idx.is_none() {
                    self.errors.push("Please select a project".to_string());
                    return false;
                }
                self.step = WizardStep::EnterDetails;
                true
            }
            WizardStep::EnterDetails => {
                if self.worktree_name.trim().is_empty() {
                    self.errors.push("Worktree name cannot be empty".to_string());
                    return false;
                }
                if self.prompt.trim().is_empty() {
                    self.errors.push("Prompt cannot be empty".to_string());
                    return false;
                }
                self.step = WizardStep::Confirm;
                true
            }
            WizardStep::Confirm => {
                // Final step - ready to create
                true
            }
        }
    }

    /// Move to the previous step
    pub fn prev_step(&mut self) {
        self.errors.clear();

        match self.step {
            WizardStep::SelectProject => {
                // Already at first step
            }
            WizardStep::EnterDetails => {
                if self.selected_project_idx.is_none() {
                    // Only go back if we have project selection
                    return;
                }
                self.step = WizardStep::SelectProject;
            }
            WizardStep::Confirm => {
                self.step = WizardStep::EnterDetails;
            }
        }
    }

    /// Handle character input
    pub fn input_char(&mut self, c: char) {
        if self.step == WizardStep::EnterDetails {
            match self.focused_field {
                WizardField::Name => {
                    // Only allow valid branch name characters
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        self.worktree_name.insert(self.name_cursor, c);
                        self.name_cursor += 1;
                    }
                }
                WizardField::Prompt => {
                    self.prompt.insert(self.prompt_cursor, c);
                    self.prompt_cursor += 1;
                }
            }
        }
    }

    /// Handle backspace
    pub fn input_backspace(&mut self) {
        if self.step == WizardStep::EnterDetails {
            match self.focused_field {
                WizardField::Name => {
                    if self.name_cursor > 0 {
                        self.name_cursor -= 1;
                        self.worktree_name.remove(self.name_cursor);
                    }
                }
                WizardField::Prompt => {
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
        if self.step == WizardStep::EnterDetails {
            match self.focused_field {
                WizardField::Name => {
                    if self.name_cursor < self.worktree_name.len() {
                        self.worktree_name.remove(self.name_cursor);
                    }
                }
                WizardField::Prompt => {
                    if self.prompt_cursor < self.prompt.len() {
                        self.prompt.remove(self.prompt_cursor);
                    }
                }
            }
        }
    }

    /// Move cursor left
    pub fn cursor_left(&mut self) {
        if self.step == WizardStep::EnterDetails {
            match self.focused_field {
                WizardField::Name => self.name_cursor = self.name_cursor.saturating_sub(1),
                WizardField::Prompt => self.prompt_cursor = self.prompt_cursor.saturating_sub(1),
            }
        }
    }

    /// Move cursor right
    pub fn cursor_right(&mut self) {
        if self.step == WizardStep::EnterDetails {
            match self.focused_field {
                WizardField::Name => {
                    if self.name_cursor < self.worktree_name.len() {
                        self.name_cursor += 1;
                    }
                }
                WizardField::Prompt => {
                    if self.prompt_cursor < self.prompt.len() {
                        self.prompt_cursor += 1;
                    }
                }
            }
        }
    }

    /// Move cursor to start
    pub fn cursor_home(&mut self) {
        if self.step == WizardStep::EnterDetails {
            match self.focused_field {
                WizardField::Name => self.name_cursor = 0,
                WizardField::Prompt => self.prompt_cursor = 0,
            }
        }
    }

    /// Move cursor to end
    pub fn cursor_end(&mut self) {
        if self.step == WizardStep::EnterDetails {
            match self.focused_field {
                WizardField::Name => self.name_cursor = self.worktree_name.len(),
                WizardField::Prompt => self.prompt_cursor = self.prompt.len(),
            }
        }
    }

    /// Select next project
    pub fn select_next_project(&mut self, max_projects: usize) {
        if self.step == WizardStep::SelectProject {
            self.selected_project_idx = Some(
                self.selected_project_idx
                    .map(|idx| (idx + 1).min(max_projects - 1))
                    .unwrap_or(0)
            );
        }
    }

    /// Select previous project
    pub fn select_prev_project(&mut self) {
        if self.step == WizardStep::SelectProject {
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
            WizardStep::SelectProject => "Select Project",
            WizardStep::EnterDetails => "Enter Details",
            WizardStep::Confirm => "Confirm",
        }
    }

    /// Get help text for the current step
    pub fn help_text(&self) -> &'static str {
        match self.step {
            WizardStep::SelectProject => "j/k:select  Enter:next  Esc:cancel",
            WizardStep::EnterDetails => "Tab:switch field  Enter:next  Esc:back",
            WizardStep::Confirm => "Enter:create  Esc:back  q:cancel",
        }
    }

    /// Get step number (1-based) and total steps
    pub fn step_progress(&self) -> (usize, usize) {
        let total = if self.selected_project_idx.is_none() { 3 } else { 2 };
        let current = match self.step {
            WizardStep::SelectProject => 1,
            WizardStep::EnterDetails => if self.selected_project_idx.is_none() { 2 } else { 1 },
            WizardStep::Confirm => if self.selected_project_idx.is_none() { 3 } else { 2 },
        };
        (current, total)
    }
}

/// Sanitize a string for use as a branch name
fn sanitize_for_branch(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .to_lowercase()
}
