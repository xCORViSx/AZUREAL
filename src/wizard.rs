use crate::models::Project;

/// Session creation wizard state
#[derive(Debug, Clone)]
pub struct SessionCreationWizard {
    /// Current step in the wizard
    pub step: WizardStep,
    /// User's prompt input
    pub prompt: String,
    /// Cursor position in prompt
    pub prompt_cursor: usize,
    /// Selected project index (if multiple projects)
    pub selected_project_idx: Option<usize>,
    /// Generated session name preview
    pub session_name_preview: String,
    /// Validation errors
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    /// Select project (shown only if multiple projects)
    SelectProject,
    /// Enter prompt for the session
    EnterPrompt,
    /// Confirm session creation
    Confirm,
}

impl SessionCreationWizard {
    /// Create a new wizard
    pub fn new(projects: &[Project]) -> Self {
        let step = if projects.len() > 1 {
            WizardStep::SelectProject
        } else {
            WizardStep::EnterPrompt
        };

        Self {
            step,
            prompt: String::new(),
            prompt_cursor: 0,
            selected_project_idx: if projects.len() == 1 { Some(0) } else { None },
            session_name_preview: String::new(),
            errors: Vec::new(),
        }
    }

    /// Create a wizard for single-project stateless mode
    pub fn new_single_project(project: Option<&Project>) -> Self {
        Self {
            step: WizardStep::EnterPrompt,
            prompt: String::new(),
            prompt_cursor: 0,
            selected_project_idx: if project.is_some() { Some(0) } else { None },
            session_name_preview: String::new(),
            errors: Vec::new(),
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
                self.step = WizardStep::EnterPrompt;
                true
            }
            WizardStep::EnterPrompt => {
                if self.prompt.trim().is_empty() {
                    self.errors.push("Prompt cannot be empty".to_string());
                    return false;
                }
                if self.prompt.trim().len() < 3 {
                    self.errors.push("Prompt must be at least 3 characters".to_string());
                    return false;
                }
                self.update_session_name_preview();
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
            WizardStep::EnterPrompt => {
                if self.selected_project_idx.is_none() {
                    // Only go back if we have project selection
                    return;
                }
                self.step = WizardStep::SelectProject;
            }
            WizardStep::Confirm => {
                self.step = WizardStep::EnterPrompt;
            }
        }
    }

    /// Handle character input
    pub fn input_char(&mut self, c: char) {
        if self.step == WizardStep::EnterPrompt {
            self.prompt.insert(self.prompt_cursor, c);
            self.prompt_cursor += 1;
            self.update_session_name_preview();
        }
    }

    /// Handle backspace
    pub fn input_backspace(&mut self) {
        if self.step == WizardStep::EnterPrompt && self.prompt_cursor > 0 {
            self.prompt_cursor -= 1;
            self.prompt.remove(self.prompt_cursor);
            self.update_session_name_preview();
        }
    }

    /// Handle delete
    pub fn input_delete(&mut self) {
        if self.step == WizardStep::EnterPrompt && self.prompt_cursor < self.prompt.len() {
            self.prompt.remove(self.prompt_cursor);
            self.update_session_name_preview();
        }
    }

    /// Move cursor left
    pub fn cursor_left(&mut self) {
        if self.step == WizardStep::EnterPrompt {
            self.prompt_cursor = self.prompt_cursor.saturating_sub(1);
        }
    }

    /// Move cursor right
    pub fn cursor_right(&mut self) {
        if self.step == WizardStep::EnterPrompt && self.prompt_cursor < self.prompt.len() {
            self.prompt_cursor += 1;
        }
    }

    /// Move cursor to start
    pub fn cursor_home(&mut self) {
        if self.step == WizardStep::EnterPrompt {
            self.prompt_cursor = 0;
        }
    }

    /// Move cursor to end
    pub fn cursor_end(&mut self) {
        if self.step == WizardStep::EnterPrompt {
            self.prompt_cursor = self.prompt.len();
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

    /// Update the session name preview
    fn update_session_name_preview(&mut self) {
        self.session_name_preview = generate_session_name(&self.prompt);
    }

    /// Get the current step title
    pub fn step_title(&self) -> &'static str {
        match self.step {
            WizardStep::SelectProject => "Select Project",
            WizardStep::EnterPrompt => "Enter Prompt",
            WizardStep::Confirm => "Confirm",
        }
    }

    /// Get help text for the current step
    pub fn help_text(&self) -> &'static str {
        match self.step {
            WizardStep::SelectProject => "j/k:select  Enter:next  Esc:cancel",
            WizardStep::EnterPrompt => "Type your prompt  Enter:next  Backspace:delete  Esc:back",
            WizardStep::Confirm => "Enter:create  Esc:back  q:cancel",
        }
    }

    /// Get step number (1-based) and total steps
    pub fn step_progress(&self) -> (usize, usize) {
        let total = if self.selected_project_idx.is_none() { 3 } else { 2 };
        let current = match self.step {
            WizardStep::SelectProject => 1,
            WizardStep::EnterPrompt => if self.selected_project_idx.is_none() { 2 } else { 1 },
            WizardStep::Confirm => if self.selected_project_idx.is_none() { 3 } else { 2 },
        };
        (current, total)
    }
}

/// Generate a session name from the prompt (duplicated from session.rs for preview)
fn generate_session_name(prompt: &str) -> String {
    use uuid::Uuid;

    let name: String = prompt
        .chars()
        .take(40)
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-' || *c == '_')
        .collect();

    let name = name.trim();

    if name.is_empty() {
        format!("session-{}", &Uuid::new_v4().to_string()[..8])
    } else {
        let name = if name.len() > 30 {
            if let Some(pos) = name[..30].rfind(' ') {
                &name[..pos]
            } else {
                &name[..30]
            }
        } else {
            name
        };
        name.to_string()
    }
}
