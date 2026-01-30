//! Input handling methods for App

use super::App;

impl App {
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
        if self.input_cursor < self.input.len() { self.input_cursor += 1; }
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
        if self.input_cursor == 0 { return; }
        let chars: Vec<char> = self.input.chars().collect();
        let mut pos = self.input_cursor.saturating_sub(1);
        while pos > 0 && chars[pos].is_whitespace() { pos -= 1; }
        while pos > 0 && !chars[pos - 1].is_whitespace() { pos -= 1; }
        self.input_cursor = pos;
    }

    /// Move cursor to next word boundary
    pub fn input_word_right(&mut self) {
        let chars: Vec<char> = self.input.chars().collect();
        if self.input_cursor >= chars.len() { return; }
        let mut pos = self.input_cursor;
        while pos < chars.len() && !chars[pos].is_whitespace() { pos += 1; }
        while pos < chars.len() && chars[pos].is_whitespace() { pos += 1; }
        self.input_cursor = pos;
    }

    /// Delete word before cursor
    pub fn input_delete_word(&mut self) {
        if self.input_cursor == 0 { return; }
        let chars: Vec<char> = self.input.chars().collect();
        let mut pos = self.input_cursor.saturating_sub(1);
        while pos > 0 && chars[pos].is_whitespace() { pos -= 1; }
        while pos > 0 && !chars[pos - 1].is_whitespace() { pos -= 1; }
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

    // Worktree creation input methods

    /// Handle character input for worktree creation
    pub fn worktree_creation_char(&mut self, c: char) {
        self.worktree_creation_input.insert(self.worktree_creation_cursor, c);
        self.worktree_creation_cursor += c.len_utf8();
    }

    /// Handle backspace for worktree creation
    pub fn worktree_creation_backspace(&mut self) {
        if self.worktree_creation_cursor > 0 {
            let mut idx = self.worktree_creation_cursor - 1;
            while idx > 0 && !self.worktree_creation_input.is_char_boundary(idx) { idx -= 1; }
            self.worktree_creation_input.remove(idx);
            self.worktree_creation_cursor = idx;
        }
    }

    /// Handle delete for worktree creation
    pub fn worktree_creation_delete(&mut self) {
        if self.worktree_creation_cursor < self.worktree_creation_input.len() {
            self.worktree_creation_input.remove(self.worktree_creation_cursor);
        }
    }

    /// Move cursor left in worktree creation
    pub fn worktree_creation_left(&mut self) {
        if self.worktree_creation_cursor > 0 {
            let mut idx = self.worktree_creation_cursor - 1;
            while idx > 0 && !self.worktree_creation_input.is_char_boundary(idx) { idx -= 1; }
            self.worktree_creation_cursor = idx;
        }
    }

    /// Move cursor right in worktree creation
    pub fn worktree_creation_right(&mut self) {
        if self.worktree_creation_cursor < self.worktree_creation_input.len() {
            let mut idx = self.worktree_creation_cursor + 1;
            while idx < self.worktree_creation_input.len() && !self.worktree_creation_input.is_char_boundary(idx) { idx += 1; }
            self.worktree_creation_cursor = idx;
        }
    }

    /// Move cursor to start of worktree creation input
    pub fn worktree_creation_home(&mut self) {
        self.worktree_creation_cursor = 0;
    }

    /// Move cursor to end of worktree creation input
    pub fn worktree_creation_end(&mut self) {
        self.worktree_creation_cursor = self.worktree_creation_input.len();
    }

    /// Clear worktree creation input
    pub fn clear_worktree_creation_input(&mut self) {
        self.worktree_creation_input.clear();
        self.worktree_creation_cursor = 0;
    }
}
