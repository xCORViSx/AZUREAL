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
            while idx > 0 && !self.session_creation_input.is_char_boundary(idx) { idx -= 1; }
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
            while idx > 0 && !self.session_creation_input.is_char_boundary(idx) { idx -= 1; }
            self.session_creation_cursor = idx;
        }
    }

    /// Move cursor right in session creation
    pub fn session_creation_right(&mut self) {
        if self.session_creation_cursor < self.session_creation_input.len() {
            let mut idx = self.session_creation_cursor + 1;
            while idx < self.session_creation_input.len() && !self.session_creation_input.is_char_boundary(idx) { idx += 1; }
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
}
