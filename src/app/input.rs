//! Input handling methods for App
//!
//! `input_cursor` is a CHAR INDEX (not byte offset). String methods like
//! `insert()` and `remove()` need byte offsets, so we convert via
//! `char_to_byte()` before calling them. This prevents panics on multi-byte
//! characters like `ç` (⌥+c on macOS sends unicode).

use super::App;

impl App {
    /// Convert char index to byte offset in self.input
    fn char_to_byte(&self, char_idx: usize) -> usize {
        self.input.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(self.input.len())
    }

    /// Handle input character
    pub fn input_char(&mut self, c: char) {
        let byte_pos = self.char_to_byte(self.input_cursor);
        self.input.insert(byte_pos, c);
        self.input_cursor += 1;
    }

    /// Handle backspace
    pub fn input_backspace(&mut self) {
        if self.input_cursor > 0 {
            self.input_cursor -= 1;
            let byte_pos = self.char_to_byte(self.input_cursor);
            self.input.remove(byte_pos);
        }
    }

    /// Handle delete
    pub fn input_delete(&mut self) {
        let char_count = self.input.chars().count();
        if self.input_cursor < char_count {
            let byte_pos = self.char_to_byte(self.input_cursor);
            self.input.remove(byte_pos);
        }
    }

    /// Move cursor left
    pub fn input_left(&mut self) {
        self.input_cursor = self.input_cursor.saturating_sub(1);
    }

    /// Move cursor right
    pub fn input_right(&mut self) {
        if self.input_cursor < self.input.chars().count() { self.input_cursor += 1; }
    }

    /// Move cursor to start
    pub fn input_home(&mut self) {
        self.input_cursor = 0;
    }

    /// Move cursor to end
    pub fn input_end(&mut self) {
        self.input_cursor = self.input.chars().count();
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

#[cfg(test)]
mod tests {
    use crate::app::App;

    // ── char_to_byte (tested indirectly via input methods) ──

    #[test]
    fn test_input_char_ascii() {
        let mut app = App::new();
        app.input_char('a');
        assert_eq!(app.input, "a");
        assert_eq!(app.input_cursor, 1);
    }

    #[test]
    fn test_input_char_multiple() {
        let mut app = App::new();
        app.input_char('h');
        app.input_char('i');
        assert_eq!(app.input, "hi");
        assert_eq!(app.input_cursor, 2);
    }

    #[test]
    fn test_input_char_unicode() {
        let mut app = App::new();
        app.input_char('ç');
        assert_eq!(app.input, "ç");
        assert_eq!(app.input_cursor, 1);
    }

    #[test]
    fn test_input_char_emoji() {
        let mut app = App::new();
        app.input_char('🚀');
        assert_eq!(app.input, "🚀");
        assert_eq!(app.input_cursor, 1);
    }

    #[test]
    fn test_input_char_at_middle() {
        let mut app = App::new();
        app.input = "ac".to_string();
        app.input_cursor = 1;
        app.input_char('b');
        assert_eq!(app.input, "abc");
        assert_eq!(app.input_cursor, 2);
    }

    #[test]
    fn test_input_char_at_start() {
        let mut app = App::new();
        app.input = "bc".to_string();
        app.input_cursor = 0;
        app.input_char('a');
        assert_eq!(app.input, "abc");
        assert_eq!(app.input_cursor, 1);
    }

    // ── input_backspace ──

    #[test]
    fn test_backspace_empty() {
        let mut app = App::new();
        app.input_backspace();
        assert_eq!(app.input, "");
        assert_eq!(app.input_cursor, 0);
    }

    #[test]
    fn test_backspace_single_char() {
        let mut app = App::new();
        app.input = "a".to_string();
        app.input_cursor = 1;
        app.input_backspace();
        assert_eq!(app.input, "");
        assert_eq!(app.input_cursor, 0);
    }

    #[test]
    fn test_backspace_at_cursor_zero() {
        let mut app = App::new();
        app.input = "abc".to_string();
        app.input_cursor = 0;
        app.input_backspace();
        assert_eq!(app.input, "abc");
    }

    #[test]
    fn test_backspace_middle() {
        let mut app = App::new();
        app.input = "abc".to_string();
        app.input_cursor = 2;
        app.input_backspace();
        assert_eq!(app.input, "ac");
        assert_eq!(app.input_cursor, 1);
    }

    #[test]
    fn test_backspace_unicode() {
        let mut app = App::new();
        app.input = "aç".to_string();
        app.input_cursor = 2;
        app.input_backspace();
        assert_eq!(app.input, "a");
        assert_eq!(app.input_cursor, 1);
    }

    // ── input_delete ──

    #[test]
    fn test_delete_at_end() {
        let mut app = App::new();
        app.input = "abc".to_string();
        app.input_cursor = 3;
        app.input_delete();
        assert_eq!(app.input, "abc");
    }

    #[test]
    fn test_delete_at_start() {
        let mut app = App::new();
        app.input = "abc".to_string();
        app.input_cursor = 0;
        app.input_delete();
        assert_eq!(app.input, "bc");
        assert_eq!(app.input_cursor, 0);
    }

    #[test]
    fn test_delete_middle() {
        let mut app = App::new();
        app.input = "abc".to_string();
        app.input_cursor = 1;
        app.input_delete();
        assert_eq!(app.input, "ac");
    }

    #[test]
    fn test_delete_empty() {
        let mut app = App::new();
        app.input_delete();
        assert_eq!(app.input, "");
    }

    // ── input_left / input_right ──

    #[test]
    fn test_left_from_middle() {
        let mut app = App::new();
        app.input = "abc".to_string();
        app.input_cursor = 2;
        app.input_left();
        assert_eq!(app.input_cursor, 1);
    }

    #[test]
    fn test_left_from_zero() {
        let mut app = App::new();
        app.input = "abc".to_string();
        app.input_cursor = 0;
        app.input_left();
        assert_eq!(app.input_cursor, 0);
    }

    #[test]
    fn test_right_from_middle() {
        let mut app = App::new();
        app.input = "abc".to_string();
        app.input_cursor = 1;
        app.input_right();
        assert_eq!(app.input_cursor, 2);
    }

    #[test]
    fn test_right_at_end() {
        let mut app = App::new();
        app.input = "abc".to_string();
        app.input_cursor = 3;
        app.input_right();
        assert_eq!(app.input_cursor, 3);
    }

    #[test]
    fn test_right_empty() {
        let mut app = App::new();
        app.input_right();
        assert_eq!(app.input_cursor, 0);
    }

    // ── input_home / input_end ──

    #[test]
    fn test_home() {
        let mut app = App::new();
        app.input = "abc".to_string();
        app.input_cursor = 2;
        app.input_home();
        assert_eq!(app.input_cursor, 0);
    }

    #[test]
    fn test_home_already_at_start() {
        let mut app = App::new();
        app.input = "abc".to_string();
        app.input_cursor = 0;
        app.input_home();
        assert_eq!(app.input_cursor, 0);
    }

    #[test]
    fn test_end() {
        let mut app = App::new();
        app.input = "abc".to_string();
        app.input_cursor = 0;
        app.input_end();
        assert_eq!(app.input_cursor, 3);
    }

    #[test]
    fn test_end_already_at_end() {
        let mut app = App::new();
        app.input = "abc".to_string();
        app.input_cursor = 3;
        app.input_end();
        assert_eq!(app.input_cursor, 3);
    }

    #[test]
    fn test_end_unicode() {
        let mut app = App::new();
        app.input = "aç🚀".to_string();
        app.input_end();
        assert_eq!(app.input_cursor, 3); // 3 chars
    }

    // ── input_word_left ──

    #[test]
    fn test_word_left_single_word() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.input_cursor = 5;
        app.input_word_left();
        assert_eq!(app.input_cursor, 0);
    }

    #[test]
    fn test_word_left_two_words() {
        let mut app = App::new();
        app.input = "hello world".to_string();
        app.input_cursor = 11;
        app.input_word_left();
        assert_eq!(app.input_cursor, 6);
    }

    #[test]
    fn test_word_left_at_zero() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.input_cursor = 0;
        app.input_word_left();
        assert_eq!(app.input_cursor, 0);
    }

    #[test]
    fn test_word_left_multiple_spaces() {
        let mut app = App::new();
        app.input = "hello   world".to_string();
        app.input_cursor = 13;
        app.input_word_left();
        assert_eq!(app.input_cursor, 8);
    }

    // ── input_word_right ──

    #[test]
    fn test_word_right_single_word() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.input_cursor = 0;
        app.input_word_right();
        assert_eq!(app.input_cursor, 5);
    }

    #[test]
    fn test_word_right_two_words() {
        let mut app = App::new();
        app.input = "hello world".to_string();
        app.input_cursor = 0;
        app.input_word_right();
        assert_eq!(app.input_cursor, 6);
    }

    #[test]
    fn test_word_right_at_end() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.input_cursor = 5;
        app.input_word_right();
        assert_eq!(app.input_cursor, 5);
    }

    // ── input_delete_word ──

    #[test]
    fn test_delete_word_single_word() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.input_cursor = 5;
        app.input_delete_word();
        assert_eq!(app.input, "");
        assert_eq!(app.input_cursor, 0);
    }

    #[test]
    fn test_delete_word_last_word() {
        let mut app = App::new();
        app.input = "hello world".to_string();
        app.input_cursor = 11;
        app.input_delete_word();
        assert_eq!(app.input, "hello ");
        assert_eq!(app.input_cursor, 6);
    }

    #[test]
    fn test_delete_word_at_zero() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.input_cursor = 0;
        app.input_delete_word();
        assert_eq!(app.input, "hello");
    }

    #[test]
    fn test_delete_word_middle() {
        let mut app = App::new();
        app.input = "aaa bbb ccc".to_string();
        app.input_cursor = 7; // end of "bbb"
        app.input_delete_word();
        assert_eq!(app.input, "aaa  ccc");
        assert_eq!(app.input_cursor, 4);
    }

    // ── clear_input ──

    #[test]
    fn test_clear_input() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.input_cursor = 3;
        app.input_selection = Some((1, 3));
        app.prompt_history_idx = Some(2);
        app.prompt_history_temp = Some("temp".to_string());
        app.clear_input();
        assert_eq!(app.input, "");
        assert_eq!(app.input_cursor, 0);
        assert!(app.input_selection.is_none());
        assert!(app.prompt_history_idx.is_none());
        assert!(app.prompt_history_temp.is_none());
    }

    #[test]
    fn test_clear_input_already_empty() {
        let mut app = App::new();
        app.clear_input();
        assert_eq!(app.input, "");
        assert_eq!(app.input_cursor, 0);
    }

    // ── worktree_creation input ──

    #[test]
    fn test_worktree_creation_char() {
        let mut app = App::new();
        app.worktree_creation_char('a');
        assert_eq!(app.worktree_creation_input, "a");
        assert_eq!(app.worktree_creation_cursor, 1);
    }

    #[test]
    fn test_worktree_creation_multiple_chars() {
        let mut app = App::new();
        app.worktree_creation_char('a');
        app.worktree_creation_char('b');
        app.worktree_creation_char('c');
        assert_eq!(app.worktree_creation_input, "abc");
        assert_eq!(app.worktree_creation_cursor, 3);
    }

    #[test]
    fn test_worktree_creation_backspace() {
        let mut app = App::new();
        app.worktree_creation_input = "abc".to_string();
        app.worktree_creation_cursor = 3;
        app.worktree_creation_backspace();
        assert_eq!(app.worktree_creation_input, "ab");
        assert_eq!(app.worktree_creation_cursor, 2);
    }

    #[test]
    fn test_worktree_creation_backspace_empty() {
        let mut app = App::new();
        app.worktree_creation_backspace();
        assert_eq!(app.worktree_creation_input, "");
        assert_eq!(app.worktree_creation_cursor, 0);
    }

    #[test]
    fn test_worktree_creation_delete() {
        let mut app = App::new();
        app.worktree_creation_input = "abc".to_string();
        app.worktree_creation_cursor = 0;
        app.worktree_creation_delete();
        assert_eq!(app.worktree_creation_input, "bc");
    }

    #[test]
    fn test_worktree_creation_delete_at_end() {
        let mut app = App::new();
        app.worktree_creation_input = "abc".to_string();
        app.worktree_creation_cursor = 3;
        app.worktree_creation_delete();
        assert_eq!(app.worktree_creation_input, "abc");
    }

    #[test]
    fn test_worktree_creation_left() {
        let mut app = App::new();
        app.worktree_creation_input = "abc".to_string();
        app.worktree_creation_cursor = 2;
        app.worktree_creation_left();
        assert_eq!(app.worktree_creation_cursor, 1);
    }

    #[test]
    fn test_worktree_creation_left_at_zero() {
        let mut app = App::new();
        app.worktree_creation_input = "abc".to_string();
        app.worktree_creation_cursor = 0;
        app.worktree_creation_left();
        assert_eq!(app.worktree_creation_cursor, 0);
    }

    #[test]
    fn test_worktree_creation_right() {
        let mut app = App::new();
        app.worktree_creation_input = "abc".to_string();
        app.worktree_creation_cursor = 1;
        app.worktree_creation_right();
        assert_eq!(app.worktree_creation_cursor, 2);
    }

    #[test]
    fn test_worktree_creation_right_at_end() {
        let mut app = App::new();
        app.worktree_creation_input = "abc".to_string();
        app.worktree_creation_cursor = 3;
        app.worktree_creation_right();
        assert_eq!(app.worktree_creation_cursor, 3);
    }

    #[test]
    fn test_worktree_creation_home() {
        let mut app = App::new();
        app.worktree_creation_input = "abc".to_string();
        app.worktree_creation_cursor = 3;
        app.worktree_creation_home();
        assert_eq!(app.worktree_creation_cursor, 0);
    }

    #[test]
    fn test_worktree_creation_end() {
        let mut app = App::new();
        app.worktree_creation_input = "abc".to_string();
        app.worktree_creation_cursor = 0;
        app.worktree_creation_end();
        assert_eq!(app.worktree_creation_cursor, 3);
    }

    #[test]
    fn test_clear_worktree_creation_input() {
        let mut app = App::new();
        app.worktree_creation_input = "test".to_string();
        app.worktree_creation_cursor = 4;
        app.clear_worktree_creation_input();
        assert_eq!(app.worktree_creation_input, "");
        assert_eq!(app.worktree_creation_cursor, 0);
    }

    // ── selection methods ──

    #[test]
    fn test_start_selection() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.input_cursor = 2;
        app.input_start_selection();
        assert_eq!(app.input_selection, Some((2, 2)));
    }

    #[test]
    fn test_extend_selection() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.input_cursor = 2;
        app.input_selection = Some((1, 2));
        app.input_cursor = 4;
        app.input_extend_selection();
        assert_eq!(app.input_selection, Some((1, 4)));
    }

    #[test]
    fn test_extend_selection_no_active() {
        let mut app = App::new();
        app.input_cursor = 3;
        app.input_extend_selection();
        assert!(app.input_selection.is_none());
    }

    #[test]
    fn test_clear_selection() {
        let mut app = App::new();
        app.input_selection = Some((0, 5));
        app.input_clear_selection();
        assert!(app.input_selection.is_none());
    }

    #[test]
    fn test_has_selection_true() {
        let mut app = App::new();
        app.input_selection = Some((1, 3));
        assert!(app.has_input_selection());
    }

    #[test]
    fn test_has_selection_false_same_pos() {
        let mut app = App::new();
        app.input_selection = Some((2, 2));
        assert!(!app.has_input_selection());
    }

    #[test]
    fn test_has_selection_false_none() {
        let app = App::new();
        assert!(!app.has_input_selection());
    }

    #[test]
    fn test_get_selected_text() {
        let mut app = App::new();
        app.input = "hello world".to_string();
        app.input_selection = Some((0, 5));
        assert_eq!(app.get_input_selected_text(), Some("hello".to_string()));
    }

    #[test]
    fn test_get_selected_text_reversed() {
        let mut app = App::new();
        app.input = "hello world".to_string();
        app.input_selection = Some((5, 0));
        assert_eq!(app.get_input_selected_text(), Some("hello".to_string()));
    }

    #[test]
    fn test_get_selected_text_same_pos() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.input_selection = Some((2, 2));
        assert!(app.get_input_selected_text().is_none());
    }

    #[test]
    fn test_get_selected_text_none_selection() {
        let mut app = App::new();
        app.input = "hello".to_string();
        assert!(app.get_input_selected_text().is_none());
    }

    // ── select_all ──

    #[test]
    fn test_select_all() {
        let mut app = App::new();
        app.input = "hello world".to_string();
        app.input_cursor = 3;
        app.input_select_all();
        assert_eq!(app.input_selection, Some((0, 11)));
        assert_eq!(app.input_cursor, 11);
    }

    #[test]
    fn test_select_all_empty() {
        let mut app = App::new();
        app.input_select_all();
        assert_eq!(app.input_selection, Some((0, 0)));
        assert_eq!(app.input_cursor, 0);
    }

    // ── input_delete_selection ──

    #[test]
    fn test_delete_selection() {
        let mut app = App::new();
        app.input = "hello world".to_string();
        app.input_selection = Some((5, 11));
        app.input_delete_selection();
        assert_eq!(app.input, "hello");
        assert_eq!(app.input_cursor, 5);
        assert!(app.input_selection.is_none());
    }

    #[test]
    fn test_delete_selection_no_selection() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.input_delete_selection();
        assert_eq!(app.input, "hello");
    }

    // ── input_left_select / input_right_select ──

    #[test]
    fn test_left_select_extend() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.input_cursor = 3;
        app.input_left_select(true);
        assert_eq!(app.input_cursor, 2);
        assert!(app.input_selection.is_some());
    }

    #[test]
    fn test_left_select_no_extend() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.input_cursor = 3;
        app.input_selection = Some((1, 3));
        app.input_left_select(false);
        assert_eq!(app.input_cursor, 2);
        assert!(app.input_selection.is_none());
    }

    #[test]
    fn test_right_select_extend() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.input_cursor = 2;
        app.input_right_select(true);
        assert_eq!(app.input_cursor, 3);
        assert!(app.input_selection.is_some());
    }

    #[test]
    fn test_right_select_no_extend() {
        let mut app = App::new();
        app.input = "hello".to_string();
        app.input_cursor = 2;
        app.input_selection = Some((1, 3));
        app.input_right_select(false);
        assert_eq!(app.input_cursor, 3);
        assert!(app.input_selection.is_none());
    }

    // ── prompt history ──

    #[test]
    fn test_prompt_history_prev_no_history() {
        let mut app = App::new();
        app.prompt_history_prev();
        assert!(app.prompt_history_idx.is_none());
    }

    #[test]
    fn test_prompt_history_next_no_history() {
        let mut app = App::new();
        app.prompt_history_next();
        assert!(app.prompt_history_idx.is_none());
    }

    #[test]
    fn test_prompt_history_prev_with_history() {
        let mut app = App::new();
        app.display_events.push(crate::events::DisplayEvent::UserMessage {
            _uuid: "u1".to_string(),
            content: "first prompt".to_string(),
        });
        app.display_events.push(crate::events::DisplayEvent::UserMessage {
            _uuid: "u2".to_string(),
            content: "second prompt".to_string(),
        });
        app.input = "current".to_string();
        app.prompt_history_prev();
        assert_eq!(app.input, "second prompt");
        assert_eq!(app.prompt_history_idx, Some(1));
        assert_eq!(app.prompt_history_temp, Some("current".to_string()));
    }

    #[test]
    fn test_prompt_history_prev_twice() {
        let mut app = App::new();
        app.display_events.push(crate::events::DisplayEvent::UserMessage {
            _uuid: "u1".to_string(),
            content: "first".to_string(),
        });
        app.display_events.push(crate::events::DisplayEvent::UserMessage {
            _uuid: "u2".to_string(),
            content: "second".to_string(),
        });
        app.prompt_history_prev();
        app.prompt_history_prev();
        assert_eq!(app.input, "first");
        assert_eq!(app.prompt_history_idx, Some(0));
    }

    #[test]
    fn test_prompt_history_next_restores() {
        let mut app = App::new();
        app.display_events.push(crate::events::DisplayEvent::UserMessage {
            _uuid: "u1".to_string(),
            content: "prompt".to_string(),
        });
        app.input = "typing...".to_string();
        app.prompt_history_prev();
        assert_eq!(app.input, "prompt");
        app.prompt_history_next();
        assert_eq!(app.input, "typing...");
        assert!(app.prompt_history_idx.is_none());
    }

    #[test]
    fn test_prompt_history_prev_at_oldest_stays() {
        let mut app = App::new();
        app.display_events.push(crate::events::DisplayEvent::UserMessage {
            _uuid: "u1".to_string(),
            content: "only".to_string(),
        });
        app.prompt_history_prev();
        app.prompt_history_prev(); // already at oldest
        assert_eq!(app.input, "only");
        assert_eq!(app.prompt_history_idx, Some(0));
    }

    // ── collect_prompt_history ──

    #[test]
    fn test_collect_prompt_history_empty() {
        let app = App::new();
        assert!(app.collect_prompt_history().is_empty());
    }

    #[test]
    fn test_collect_prompt_history_filters_non_user() {
        let mut app = App::new();
        app.display_events.push(crate::events::DisplayEvent::AssistantText {
            _uuid: "u".to_string(),
            _message_id: "m".to_string(),
            text: "response".to_string(),
        });
        assert!(app.collect_prompt_history().is_empty());
    }

    #[test]
    fn test_collect_prompt_history_filters_empty_content() {
        let mut app = App::new();
        app.display_events.push(crate::events::DisplayEvent::UserMessage {
            _uuid: "u".to_string(),
            content: "   ".to_string(),
        });
        assert!(app.collect_prompt_history().is_empty());
    }

    #[test]
    fn test_collect_prompt_history_trims() {
        let mut app = App::new();
        app.display_events.push(crate::events::DisplayEvent::UserMessage {
            _uuid: "u".to_string(),
            content: "  hello  ".to_string(),
        });
        let h = app.collect_prompt_history();
        assert_eq!(h, vec!["hello"]);
    }
}
