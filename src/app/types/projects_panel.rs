//! Projects panel state and input handling

use crate::config::ProjectEntry;

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
            self.input_cursor = self.input.chars().count();
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

    fn input_byte_offset(&self, char_idx: usize) -> usize {
        self.input
            .char_indices()
            .nth(char_idx)
            .map(|(idx, _)| idx)
            .unwrap_or(self.input.len())
    }

    /// Insert a character at cursor position
    pub fn input_char(&mut self, c: char) {
        self.error = None;
        let byte_offset = self.input_byte_offset(self.input_cursor);
        self.input.insert(byte_offset, c);
        self.input_cursor += 1;
    }

    /// Insert pasted text at the cursor position. Project panel inputs are
    /// single-line fields, so line endings from copied shell output are ignored.
    pub fn input_text(&mut self, text: &str) {
        let normalized: String = text.chars().filter(|c| *c != '\r' && *c != '\n').collect();
        if normalized.is_empty() {
            return;
        }
        self.error = None;
        let byte_offset = self.input_byte_offset(self.input_cursor);
        self.input.insert_str(byte_offset, &normalized);
        self.input_cursor += normalized.chars().count();
    }

    /// Delete character before cursor
    pub fn input_backspace(&mut self) {
        if self.input_cursor > 0 {
            self.input_cursor -= 1;
            let byte_offset = self.input_byte_offset(self.input_cursor);
            self.input.remove(byte_offset);
        }
    }

    /// Delete character at cursor
    pub fn input_delete(&mut self) {
        if self.input_cursor < self.input.chars().count() {
            let byte_offset = self.input_byte_offset(self.input_cursor);
            self.input.remove(byte_offset);
        }
    }

    pub fn cursor_left(&mut self) {
        if self.input_cursor > 0 {
            self.input_cursor -= 1;
        }
    }
    pub fn cursor_right(&mut self) {
        if self.input_cursor < self.input.chars().count() {
            self.input_cursor += 1;
        }
    }
    pub fn cursor_home(&mut self) {
        self.input_cursor = 0;
    }
    pub fn cursor_end(&mut self) {
        self.input_cursor = self.input.chars().count();
    }
}
