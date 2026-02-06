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

    /// Clear input and reset selection
    pub fn clear_input(&mut self) {
        self.input.clear();
        self.input_cursor = 0;
        self.input_selection = None;
        self.prompt_history_idx = None;
        self.prompt_history_temp = None;
    }

    /// Collect prompt history from display_events UserMessage entries (most recent last)
    fn collect_prompt_history(&self) -> Vec<String> {
        self.display_events.iter().filter_map(|ev| {
            if let crate::events::DisplayEvent::UserMessage { content, .. } = ev {
                let trimmed = content.trim();
                if !trimmed.is_empty() { Some(trimmed.to_string()) } else { None }
            } else { None }
        }).collect()
    }

    /// Navigate to previous prompt in history (↑)
    pub fn prompt_history_prev(&mut self) {
        let history = self.collect_prompt_history();
        if history.is_empty() { return; }
        match self.prompt_history_idx {
            None => {
                // First press: save current input, jump to most recent history entry
                self.prompt_history_temp = Some(self.input.clone());
                let idx = history.len() - 1;
                self.prompt_history_idx = Some(idx);
                self.input = history[idx].clone();
                self.input_cursor = self.input.chars().count();
                self.input_selection = None;
            }
            Some(idx) if idx > 0 => {
                // Move further back in history
                let new_idx = idx - 1;
                self.prompt_history_idx = Some(new_idx);
                self.input = history[new_idx].clone();
                self.input_cursor = self.input.chars().count();
                self.input_selection = None;
            }
            _ => {} // already at oldest entry
        }
    }

    /// Navigate to next prompt in history (↓)
    pub fn prompt_history_next(&mut self) {
        let history = self.collect_prompt_history();
        match self.prompt_history_idx {
            Some(idx) if idx + 1 < history.len() => {
                // Move forward in history
                let new_idx = idx + 1;
                self.prompt_history_idx = Some(new_idx);
                self.input = history[new_idx].clone();
                self.input_cursor = self.input.chars().count();
                self.input_selection = None;
            }
            Some(_) => {
                // Past the newest entry — restore saved input
                self.prompt_history_idx = None;
                self.input = self.prompt_history_temp.take().unwrap_or_default();
                self.input_cursor = self.input.chars().count();
                self.input_selection = None;
            }
            None => {} // not browsing history
        }
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

    // ========== PROMPT INPUT SELECTION METHODS ==========

    /// Start a new selection at cursor position
    pub fn input_start_selection(&mut self) {
        self.input_selection = Some((self.input_cursor, self.input_cursor));
    }

    /// Extend selection to current cursor position
    pub fn input_extend_selection(&mut self) {
        if let Some((start, _)) = self.input_selection {
            self.input_selection = Some((start, self.input_cursor));
        }
    }

    /// Clear selection
    pub fn input_clear_selection(&mut self) {
        self.input_selection = None;
    }

    /// Check if there's an active selection
    pub fn has_input_selection(&self) -> bool {
        self.input_selection.map(|(s, e)| s != e).unwrap_or(false)
    }

    /// Get normalized selection (start <= end)
    fn get_normalized_input_selection(&self) -> Option<(usize, usize)> {
        let (s, e) = self.input_selection?;
        if s <= e { Some((s, e)) } else { Some((e, s)) }
    }

    /// Get selected text
    pub fn get_input_selected_text(&self) -> Option<String> {
        let (start, end) = self.get_normalized_input_selection()?;
        if start == end { return None; }
        let chars: Vec<char> = self.input.chars().collect();
        Some(chars[start..end.min(chars.len())].iter().collect())
    }

    /// Delete selected text and return it
    fn delete_input_selection(&mut self) -> Option<String> {
        let (start, end) = self.get_normalized_input_selection()?;
        if start == end { return None; }
        let deleted = self.get_input_selected_text();
        let chars: Vec<char> = self.input.chars().collect();
        self.input = chars[..start].iter().chain(chars[end..].iter()).collect();
        self.input_cursor = start;
        self.input_selection = None;
        deleted
    }

    // ========== PROMPT INPUT CLIPBOARD OPERATIONS ==========

    /// Copy selected text to system clipboard. Returns true if copied successfully.
    pub fn input_copy(&mut self) -> bool {
        let Some(text) = self.get_input_selected_text() else { return false };
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if cb.set_text(&text).is_ok() {
                self.clipboard = text;
                return true;
            }
        }
        // Fallback to internal clipboard only
        self.clipboard = text;
        true
    }

    /// Cut selected text to system clipboard
    pub fn input_cut(&mut self) {
        let Some(text) = self.delete_input_selection() else { return };
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text(&text);
        }
        self.clipboard = text;
    }

    /// Paste from system clipboard
    pub fn input_paste(&mut self) {
        let paste_text = arboard::Clipboard::new()
            .ok()
            .and_then(|mut cb| cb.get_text().ok())
            .unwrap_or_else(|| self.clipboard.clone());

        if paste_text.is_empty() { return; }

        // Delete selection first if any
        if self.has_input_selection() {
            self.delete_input_selection();
        }

        // Insert paste text at cursor (single line only - strip newlines)
        let paste_single = paste_text.lines().collect::<Vec<_>>().join(" ");
        let chars: Vec<char> = self.input.chars().collect();
        let before: String = chars[..self.input_cursor.min(chars.len())].iter().collect();
        let after: String = chars[self.input_cursor.min(chars.len())..].iter().collect();
        self.input = before + &paste_single + &after;
        self.input_cursor += paste_single.chars().count();
    }

    /// Delete selected text without copying
    pub fn input_delete_selection(&mut self) {
        self.delete_input_selection();
    }

    /// Select all text
    pub fn input_select_all(&mut self) {
        let len = self.input.chars().count();
        self.input_selection = Some((0, len));
        self.input_cursor = len;
    }

    // ========== SELECTION-AWARE MOVEMENT ==========

    /// Move left with optional selection extension
    pub fn input_left_select(&mut self, extend: bool) {
        if extend {
            if self.input_selection.is_none() { self.input_start_selection(); }
            self.input_left();
            self.input_extend_selection();
        } else {
            self.input_clear_selection();
            self.input_left();
        }
    }

    /// Move right with optional selection extension
    pub fn input_right_select(&mut self, extend: bool) {
        if extend {
            if self.input_selection.is_none() { self.input_start_selection(); }
            self.input_right();
            self.input_extend_selection();
        } else {
            self.input_clear_selection();
            self.input_right();
        }
    }
}
