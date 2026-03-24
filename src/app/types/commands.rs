//! Run command and preset prompt types (user-defined saved items with picker/dialog pattern)

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
