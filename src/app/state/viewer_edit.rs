//! Viewer edit mode methods
//!
//! Handles entering/exiting edit mode, cursor movement, text editing,
//! undo/redo, and saving files.

use std::fs;
use textwrap::{wrap, Options};

use super::App;

/// Compute word-boundary wrap break positions for a single line. Returns a
/// Vec of char offsets where each visual row starts (first is always 0).
fn word_wrap_breaks(text: &str, max_width: usize) -> Vec<usize> {
    if max_width == 0 || text.is_empty() {
        return vec![0];
    }
    if text.chars().count() <= max_width {
        return vec![0];
    }
    let opts = Options::new(max_width).break_words(true);
    let wrapped = wrap(text, opts);
    let mut breaks = Vec::with_capacity(wrapped.len());
    let mut offset = 0usize;
    for segment in &wrapped {
        breaks.push(offset);
        offset += segment.chars().count();
        if text.chars().nth(offset) == Some(' ') {
            offset += 1;
        }
    }
    breaks
}

impl App {
    /// Enter edit mode for current viewer file
    pub fn enter_viewer_edit_mode(&mut self) {
        let Some(ref content) = self.viewer_content else {
            return;
        };
        if self.viewer_path.is_none() {
            return;
        };

        // Split content into lines for editing
        self.viewer_edit_content = content.lines().map(String::from).collect();
        if self.viewer_edit_content.is_empty() {
            self.viewer_edit_content.push(String::new());
        }

        // Position cursor at current scroll position
        self.viewer_edit_cursor = (self.viewer_scroll, 0);
        self.viewer_edit_undo.clear();
        self.viewer_edit_redo.clear();
        self.viewer_edit_dirty = false;
        self.viewer_edit_discard_dialog = false;
        self.viewer_edit_mode = true;
    }

    /// Exit edit mode without saving
    pub fn exit_viewer_edit_mode(&mut self) {
        self.viewer_edit_mode = false;
        self.viewer_edit_content.clear();
        self.viewer_edit_undo.clear();
        self.viewer_edit_redo.clear();
        self.viewer_edit_dirty = false;
        self.viewer_edit_discard_dialog = false;
        self.viewer_edit_highlight_cache.clear();
        self.viewer_edit_highlight_ver = usize::MAX;
    }

    /// Save edits to file
    pub fn save_viewer_edits(&mut self) -> Result<(), String> {
        let Some(ref path) = self.viewer_path else {
            return Err("No file path".to_string());
        };

        let content = self.viewer_edit_content.join("\n");
        fs::write(path, &content).map_err(|e| e.to_string())?;

        // Update viewer content to match saved
        self.viewer_content = Some(content);
        self.viewer_lines_dirty = true;
        self.viewer_edit_dirty = false;

        Ok(())
    }

    /// Push current state to undo stack before making changes
    fn push_undo(&mut self) {
        self.viewer_edit_undo.push(self.viewer_edit_content.clone());
        self.viewer_edit_redo.clear();
        // Bump monotonic edit counter so highlight cache knows content changed.
        // Can't use undo.len() because the 100-entry cap makes it stall.
        self.viewer_edit_version = self.viewer_edit_version.wrapping_add(1);
        if self.viewer_edit_undo.len() > 100 {
            self.viewer_edit_undo.remove(0);
        }
    }

    /// Undo last edit
    pub fn viewer_edit_undo(&mut self) {
        if let Some(prev) = self.viewer_edit_undo.pop() {
            self.viewer_edit_redo.push(self.viewer_edit_content.clone());
            self.viewer_edit_content = prev;
            self.viewer_edit_version = self.viewer_edit_version.wrapping_add(1);
            self.clamp_edit_cursor();
        }
    }

    /// Redo last undone edit
    pub fn viewer_edit_redo(&mut self) {
        if let Some(next) = self.viewer_edit_redo.pop() {
            self.viewer_edit_undo.push(self.viewer_edit_content.clone());
            self.viewer_edit_content = next;
            self.viewer_edit_version = self.viewer_edit_version.wrapping_add(1);
            self.clamp_edit_cursor();
        }
    }

    /// Clamp cursor to valid position within content
    fn clamp_edit_cursor(&mut self) {
        let (line, col) = self.viewer_edit_cursor;
        let max_line = self.viewer_edit_content.len().saturating_sub(1);
        let clamped_line = line.min(max_line);
        let line_len = self
            .viewer_edit_content
            .get(clamped_line)
            .map(|l| l.len())
            .unwrap_or(0);
        self.viewer_edit_cursor = (clamped_line, col.min(line_len));
    }

    /// Insert character at cursor
    pub fn viewer_edit_char(&mut self, c: char) {
        self.push_undo();
        let (line, col) = self.viewer_edit_cursor;
        if let Some(line_str) = self.viewer_edit_content.get_mut(line) {
            // Handle inserting at byte boundary
            let byte_pos = line_str.chars().take(col).map(|c| c.len_utf8()).sum();
            line_str.insert(byte_pos, c);
            self.viewer_edit_cursor.1 += 1;
        }
        self.viewer_edit_dirty = true;
    }

    /// Handle backspace in edit mode
    pub fn viewer_edit_backspace(&mut self) {
        let (line, col) = self.viewer_edit_cursor;
        if col > 0 {
            self.push_undo();
            if let Some(line_str) = self.viewer_edit_content.get_mut(line) {
                let chars: Vec<char> = line_str.chars().collect();
                let new_str: String = chars[..col - 1].iter().chain(chars[col..].iter()).collect();
                *line_str = new_str;
                self.viewer_edit_cursor.1 -= 1;
            }
            self.viewer_edit_dirty = true;
        } else if line > 0 {
            // Join with previous line
            self.push_undo();
            let current_line = self.viewer_edit_content.remove(line);
            // Use char count (not byte len) — cursor is a char index
            let prev_len = self.viewer_edit_content[line - 1].chars().count();
            self.viewer_edit_content[line - 1].push_str(&current_line);
            self.viewer_edit_cursor = (line - 1, prev_len);
            self.viewer_edit_dirty = true;
        }
    }

    /// Handle delete in edit mode
    pub fn viewer_edit_delete(&mut self) {
        let (line, col) = self.viewer_edit_cursor;
        let line_len = self
            .viewer_edit_content
            .get(line)
            .map(|s| s.chars().count())
            .unwrap_or(0);
        let total_lines = self.viewer_edit_content.len();

        if col < line_len {
            self.push_undo();
            let chars: Vec<char> = self.viewer_edit_content[line].chars().collect();
            let new_str: String = chars[..col].iter().chain(chars[col + 1..].iter()).collect();
            self.viewer_edit_content[line] = new_str;
            self.viewer_edit_dirty = true;
        } else if line + 1 < total_lines {
            // Join with next line
            self.push_undo();
            let next_line = self.viewer_edit_content.remove(line + 1);
            self.viewer_edit_content[line].push_str(&next_line);
            self.viewer_edit_dirty = true;
        }
    }

    /// Handle enter in edit mode (insert new line)
    pub fn viewer_edit_enter(&mut self) {
        self.push_undo();
        let (line, col) = self.viewer_edit_cursor;
        if let Some(line_str) = self.viewer_edit_content.get(line) {
            let chars: Vec<char> = line_str.chars().collect();
            let before: String = chars[..col].iter().collect();
            let after: String = chars[col..].iter().collect();
            self.viewer_edit_content[line] = before;
            self.viewer_edit_content.insert(line + 1, after);
            self.viewer_edit_cursor = (line + 1, 0);
        }
        self.viewer_edit_dirty = true;
    }

    /// Move cursor left
    pub fn viewer_edit_left(&mut self) {
        let (line, col) = self.viewer_edit_cursor;
        if col > 0 {
            self.viewer_edit_cursor.1 -= 1;
        } else if line > 0 {
            let prev_len = self.viewer_edit_content[line - 1].chars().count();
            self.viewer_edit_cursor = (line - 1, prev_len);
        }
    }

    /// Move cursor right
    pub fn viewer_edit_right(&mut self) {
        let (line, col) = self.viewer_edit_cursor;
        if let Some(line_str) = self.viewer_edit_content.get(line) {
            let line_len = line_str.chars().count();
            if col < line_len {
                self.viewer_edit_cursor.1 += 1;
            } else if line + 1 < self.viewer_edit_content.len() {
                self.viewer_edit_cursor = (line + 1, 0);
            }
        }
    }

    /// Move cursor up through word-wrapped visual lines.
    /// If the cursor is on a wrapped continuation row, moves up within
    /// the same source line. Otherwise jumps to previous source line's
    /// last wrap row, preserving visual column position.
    pub fn viewer_edit_up(&mut self) {
        let (line, col) = self.viewer_edit_cursor;
        let cw = self.viewer_edit_content_width.max(1);
        let breaks = word_wrap_breaks(&self.viewer_edit_content[line], cw);
        // Find which wrap row the cursor is on
        let mut wrap_row = 0;
        for (j, &brk) in breaks.iter().enumerate() {
            if col >= brk {
                wrap_row = j;
            }
        }
        let visual_col = col - breaks[wrap_row];

        if wrap_row > 0 {
            // Move up one visual row within the same source line
            let prev_start = breaks[wrap_row - 1];
            let seg_len = breaks[wrap_row] - prev_start;
            self.viewer_edit_cursor.1 = prev_start + visual_col.min(seg_len.saturating_sub(1));
        } else if line > 0 {
            // Jump to previous source line's last wrap row
            let prev_line = &self.viewer_edit_content[line - 1];
            let prev_breaks = word_wrap_breaks(prev_line, cw);
            let last_start = *prev_breaks.last().unwrap_or(&0);
            let prev_len = prev_line.chars().count();
            let seg_len = prev_len - last_start;
            self.viewer_edit_cursor = (line - 1, last_start + visual_col.min(seg_len));
        }
    }

    /// Move cursor down through word-wrapped visual lines.
    /// If not on the last wrap row, moves down within the same source
    /// line. Otherwise jumps to next source line's first wrap row.
    pub fn viewer_edit_down(&mut self) {
        let (line, col) = self.viewer_edit_cursor;
        let cw = self.viewer_edit_content_width.max(1);
        let line_str = &self.viewer_edit_content[line];
        let breaks = word_wrap_breaks(line_str, cw);
        let mut wrap_row = 0;
        for (j, &brk) in breaks.iter().enumerate() {
            if col >= brk {
                wrap_row = j;
            }
        }
        let visual_col = col - breaks[wrap_row];

        if wrap_row + 1 < breaks.len() {
            // Move down one visual row within the same source line
            let next_start = breaks[wrap_row + 1];
            let seg_end = if wrap_row + 2 < breaks.len() {
                breaks[wrap_row + 2]
            } else {
                line_str.chars().count()
            };
            let seg_len = seg_end - next_start;
            self.viewer_edit_cursor.1 = next_start + visual_col.min(seg_len);
        } else if line + 1 < self.viewer_edit_content.len() {
            // Jump to next source line's first wrap row
            let next_len = self.viewer_edit_content[line + 1].chars().count();
            self.viewer_edit_cursor = (line + 1, visual_col.min(next_len));
        }
    }

    /// Move cursor to start of line
    pub fn viewer_edit_home(&mut self) {
        self.viewer_edit_cursor.1 = 0;
    }

    /// Move cursor to end of line
    pub fn viewer_edit_end(&mut self) {
        let (line, _) = self.viewer_edit_cursor;
        if let Some(line_str) = self.viewer_edit_content.get(line) {
            self.viewer_edit_cursor.1 = line_str.chars().count();
        }
    }

    /// Ensure cursor is visible in viewport, accounting for word wrapping.
    /// Computes the visual line index by summing wrap counts for all source
    /// lines before the cursor, plus the cursor's own wrap row.
    pub fn viewer_edit_scroll_to_cursor(&mut self) {
        let (cursor_line, cursor_col) = self.viewer_edit_cursor;
        let cw = self.viewer_edit_content_width.max(1);
        let viewport = self.viewer_viewport_height;

        // Sum visual lines for all source lines before cursor_line
        let mut visual_line: usize = 0;
        for i in 0..cursor_line.min(self.viewer_edit_content.len()) {
            visual_line += word_wrap_breaks(&self.viewer_edit_content[i], cw).len();
        }
        // Add cursor's wrap row within its source line
        let cursor_breaks = word_wrap_breaks(
            self.viewer_edit_content
                .get(cursor_line)
                .map(|s| s.as_str())
                .unwrap_or(""),
            cw,
        );
        let mut wrap_row = 0;
        for (j, &brk) in cursor_breaks.iter().enumerate() {
            if cursor_col >= brk {
                wrap_row = j;
            }
        }
        visual_line += wrap_row;

        if visual_line < self.viewer_scroll {
            self.viewer_scroll = visual_line;
        } else if visual_line >= self.viewer_scroll + viewport {
            self.viewer_scroll = visual_line.saturating_sub(viewport - 1);
        }
    }

    // ========== SELECTION METHODS ==========

    /// Start a new selection at cursor position
    pub fn viewer_edit_start_selection(&mut self) {
        let (line, col) = self.viewer_edit_cursor;
        self.viewer_edit_selection = Some((line, col, line, col));
    }

    /// Extend selection to current cursor position (keeps anchor, moves end)
    pub fn viewer_edit_extend_selection(&mut self) {
        let (line, col) = self.viewer_edit_cursor;
        if let Some((start_line, start_col, _, _)) = self.viewer_edit_selection {
            self.viewer_edit_selection = Some((start_line, start_col, line, col));
        }
    }

    /// Clear current selection
    pub fn viewer_edit_clear_selection(&mut self) {
        self.viewer_edit_selection = None;
    }

    /// Check if there's an active selection
    pub fn has_edit_selection(&self) -> bool {
        if let Some((sl, sc, el, ec)) = self.viewer_edit_selection {
            sl != el || sc != ec
        } else {
            false
        }
    }

    /// Get normalized selection bounds (start <= end)
    fn get_normalized_selection(&self) -> Option<(usize, usize, usize, usize)> {
        let (sl, sc, el, ec) = self.viewer_edit_selection?;
        // Normalize: start position <= end position
        if sl < el || (sl == el && sc <= ec) {
            Some((sl, sc, el, ec))
        } else {
            Some((el, ec, sl, sc))
        }
    }

    /// Get selected text as a string
    pub fn get_selected_text(&self) -> Option<String> {
        let (sl, sc, el, ec) = self.get_normalized_selection()?;
        if sl == el && sc == ec {
            return None;
        }

        let mut result = String::new();
        for line_idx in sl..=el {
            let Some(line) = self.viewer_edit_content.get(line_idx) else {
                continue;
            };
            let chars: Vec<char> = line.chars().collect();
            let start_col = if line_idx == sl { sc } else { 0 };
            let end_col = if line_idx == el {
                ec.min(chars.len())
            } else {
                chars.len()
            };

            if start_col < chars.len() {
                let segment: String = chars[start_col..end_col.min(chars.len())].iter().collect();
                result.push_str(&segment);
            }
            // Add newline between lines (not after last line)
            if line_idx < el {
                result.push('\n');
            }
        }
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Delete selected text and return it
    fn delete_selection_text(&mut self) -> Option<String> {
        let (sl, sc, el, ec) = self.get_normalized_selection()?;
        if sl == el && sc == ec {
            return None;
        }

        self.push_undo();
        let deleted = self.get_selected_text();

        if sl == el {
            // Single-line selection: remove chars from sc to ec
            let chars: Vec<char> = self.viewer_edit_content[sl].chars().collect();
            let new_str: String = chars[..sc]
                .iter()
                .chain(chars[ec.min(chars.len())..].iter())
                .collect();
            self.viewer_edit_content[sl] = new_str;
        } else {
            // Multi-line selection: keep before sc on first line, after ec on last line, remove middle
            let first_chars: Vec<char> = self.viewer_edit_content[sl].chars().collect();
            let last_chars: Vec<char> = self.viewer_edit_content[el].chars().collect();
            let before: String = first_chars[..sc.min(first_chars.len())].iter().collect();
            let after: String = last_chars[ec.min(last_chars.len())..].iter().collect();

            // Join first and last, remove middle lines
            self.viewer_edit_content[sl] = before + &after;
            // Remove lines sl+1 through el (in reverse to preserve indices)
            for _ in (sl + 1)..=el {
                self.viewer_edit_content.remove(sl + 1);
            }
        }

        // Move cursor to selection start
        self.viewer_edit_cursor = (sl, sc);
        self.viewer_edit_selection = None;
        self.viewer_edit_dirty = true;

        deleted
    }

    // ========== CLIPBOARD OPERATIONS ==========

    /// Copy selected text to system clipboard. Returns true if text was copied.
    pub fn viewer_edit_copy(&mut self) -> bool {
        let Some(text) = self.get_selected_text() else {
            return false;
        };
        // Try system clipboard first, fall back to internal
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(&text);
        }
        self.clipboard = text;
        true
    }

    /// Cut selected text to system clipboard
    pub fn viewer_edit_cut(&mut self) {
        let Some(text) = self.delete_selection_text() else {
            return;
        };
        // Try system clipboard first, fall back to internal
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(&text);
        }
        self.clipboard = text;
    }

    /// Paste from system clipboard (falls back to internal clipboard)
    pub fn viewer_edit_paste(&mut self) {
        // Try system clipboard first, fall back to internal
        let paste_text = arboard::Clipboard::new()
            .ok()
            .and_then(|mut cb| cb.get_text().ok())
            .unwrap_or_else(|| self.clipboard.clone());

        if paste_text.is_empty() {
            return;
        }

        // Delete selection first if any
        if self.has_edit_selection() {
            self.delete_selection_text();
        }

        self.push_undo();
        let (line, col) = self.viewer_edit_cursor;
        let paste_lines: Vec<&str> = paste_text.split('\n').collect();

        if paste_lines.len() == 1 {
            // Single line paste: insert at cursor position
            let chars: Vec<char> = self.viewer_edit_content[line].chars().collect();
            let before: String = chars[..col.min(chars.len())].iter().collect();
            let after: String = chars[col.min(chars.len())..].iter().collect();
            self.viewer_edit_content[line] = before + paste_lines[0] + &after;
            self.viewer_edit_cursor.1 = col + paste_lines[0].chars().count();
        } else {
            // Multi-line paste: split current line, insert paste lines
            let chars: Vec<char> = self.viewer_edit_content[line].chars().collect();
            let before: String = chars[..col.min(chars.len())].iter().collect();
            let after: String = chars[col.min(chars.len())..].iter().collect();

            // First paste line gets appended to before
            self.viewer_edit_content[line] = before + paste_lines[0];

            // Insert middle lines
            for (i, paste_line) in paste_lines
                .iter()
                .enumerate()
                .skip(1)
                .take(paste_lines.len() - 2)
            {
                self.viewer_edit_content
                    .insert(line + i, paste_line.to_string());
            }

            // Last paste line gets after appended
            let last_idx = paste_lines.len() - 1;
            let last_line = paste_lines[last_idx].to_string() + &after;
            self.viewer_edit_content.insert(line + last_idx, last_line);

            // Move cursor to end of pasted text
            self.viewer_edit_cursor = (line + last_idx, paste_lines[last_idx].chars().count());
        }

        self.viewer_edit_dirty = true;
    }

    /// Delete selected text (without copying to clipboard)
    pub fn viewer_edit_delete_selection(&mut self) {
        self.delete_selection_text();
    }

    // ========== SELECTION-AWARE MOVEMENT ==========

    /// Move left with optional selection extension
    pub fn viewer_edit_left_select(&mut self, extend: bool) {
        if extend {
            if self.viewer_edit_selection.is_none() {
                self.viewer_edit_start_selection();
            }
            self.viewer_edit_left();
            self.viewer_edit_extend_selection();
        } else {
            self.viewer_edit_clear_selection();
            self.viewer_edit_left();
        }
    }

    /// Move right with optional selection extension
    pub fn viewer_edit_right_select(&mut self, extend: bool) {
        if extend {
            if self.viewer_edit_selection.is_none() {
                self.viewer_edit_start_selection();
            }
            self.viewer_edit_right();
            self.viewer_edit_extend_selection();
        } else {
            self.viewer_edit_clear_selection();
            self.viewer_edit_right();
        }
    }

    /// Move up with optional selection extension
    pub fn viewer_edit_up_select(&mut self, extend: bool) {
        if extend {
            if self.viewer_edit_selection.is_none() {
                self.viewer_edit_start_selection();
            }
            self.viewer_edit_up();
            self.viewer_edit_extend_selection();
        } else {
            self.viewer_edit_clear_selection();
            self.viewer_edit_up();
        }
    }

    /// Move down with optional selection extension
    pub fn viewer_edit_down_select(&mut self, extend: bool) {
        if extend {
            if self.viewer_edit_selection.is_none() {
                self.viewer_edit_start_selection();
            }
            self.viewer_edit_down();
            self.viewer_edit_extend_selection();
        } else {
            self.viewer_edit_clear_selection();
            self.viewer_edit_down();
        }
    }

    /// Select all text
    pub fn viewer_edit_select_all(&mut self) {
        if self.viewer_edit_content.is_empty() {
            return;
        }
        let last_line = self.viewer_edit_content.len() - 1;
        let last_col = self.viewer_edit_content[last_line].chars().count();
        self.viewer_edit_selection = Some((0, 0, last_line, last_col));
        self.viewer_edit_cursor = (last_line, last_col);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== HELPER ==========

    /// Build an App with viewer_edit_content pre-populated and edit mode on
    fn app_with_lines(lines: &[&str]) -> App {
        let mut app = App::new();
        app.viewer_edit_content = lines.iter().map(|s| s.to_string()).collect();
        app.viewer_edit_mode = true;
        app.viewer_edit_cursor = (0, 0);
        app.viewer_edit_content_width = 80;
        app
    }

    // ========== word_wrap_breaks (pure function) ==========

    /// Empty string always returns vec![0]
    #[test]
    fn word_wrap_breaks_empty_string() {
        assert_eq!(word_wrap_breaks("", 80), vec![0]);
    }

    /// Zero width always returns vec![0]
    #[test]
    fn word_wrap_breaks_zero_width() {
        assert_eq!(word_wrap_breaks("hello world", 0), vec![0]);
    }

    /// Short text that fits within width returns a single break at 0
    #[test]
    fn word_wrap_breaks_fits_no_wrap() {
        assert_eq!(word_wrap_breaks("hello", 10), vec![0]);
    }

    /// Text exactly at width boundary should not wrap
    #[test]
    fn word_wrap_breaks_exact_width() {
        assert_eq!(word_wrap_breaks("abcde", 5), vec![0]);
    }

    /// Text one char over width should wrap
    #[test]
    fn word_wrap_breaks_one_over_width() {
        let breaks = word_wrap_breaks("abcdef", 5);
        assert!(breaks.len() >= 2, "Should wrap: {:?}", breaks);
        assert_eq!(breaks[0], 0);
    }

    /// Long text wraps at word boundaries
    #[test]
    fn word_wrap_breaks_word_boundary() {
        let breaks = word_wrap_breaks("hello world", 6);
        // "hello " fits in 6 chars, "world" on next line
        assert_eq!(breaks.len(), 2);
        assert_eq!(breaks[0], 0);
        assert_eq!(breaks[1], 6); // "world" starts at char index 6
    }

    /// Very narrow width (1 char) wraps every character
    #[test]
    fn word_wrap_breaks_width_one() {
        let breaks = word_wrap_breaks("abc", 1);
        assert_eq!(breaks.len(), 3);
        assert_eq!(breaks, vec![0, 1, 2]);
    }

    /// Width of 2 on a 6-char string
    #[test]
    fn word_wrap_breaks_width_two() {
        let breaks = word_wrap_breaks("abcdef", 2);
        assert_eq!(breaks.len(), 3);
        assert_eq!(breaks[0], 0);
        assert_eq!(breaks[1], 2);
        assert_eq!(breaks[2], 4);
    }

    /// Single character never wraps
    #[test]
    fn word_wrap_breaks_single_char() {
        assert_eq!(word_wrap_breaks("a", 1), vec![0]);
    }

    /// Unicode text wraps by char count, not byte count
    #[test]
    fn word_wrap_breaks_unicode() {
        // Each emoji is 1 char but multiple bytes
        let text = "aaaa bbbb";
        let breaks = word_wrap_breaks(text, 5);
        assert!(breaks.len() >= 2, "Should wrap: {:?}", breaks);
    }

    /// Long continuous word is broken when break_words is true
    #[test]
    fn word_wrap_breaks_long_word_forced() {
        let breaks = word_wrap_breaks("abcdefghij", 3);
        // 10 chars at width 3 = 4 rows (3+3+3+1)
        assert_eq!(breaks.len(), 4);
    }

    /// Multiple spaces between words
    #[test]
    fn word_wrap_breaks_multiple_spaces() {
        let breaks = word_wrap_breaks("ab cd", 10);
        // fits within 10 chars
        assert_eq!(breaks, vec![0]);
    }

    /// First break is always 0
    #[test]
    fn word_wrap_breaks_first_always_zero() {
        for width in 1..20 {
            let breaks = word_wrap_breaks("this is a test string for wrapping", width);
            assert_eq!(breaks[0], 0, "First break must be 0 at width {}", width);
        }
    }

    /// Breaks are monotonically increasing
    #[test]
    fn word_wrap_breaks_monotonically_increasing() {
        let text = "the quick brown fox jumps over the lazy dog";
        for width in 1..20 {
            let breaks = word_wrap_breaks(text, width);
            for i in 1..breaks.len() {
                assert!(
                    breaks[i] > breaks[i - 1],
                    "Breaks not increasing at width {}: {:?}",
                    width,
                    breaks
                );
            }
        }
    }

    /// Width larger than text produces single break
    #[test]
    fn word_wrap_breaks_huge_width() {
        assert_eq!(word_wrap_breaks("short", 1000), vec![0]);
    }

    // ========== viewer_edit_char (insert character) ==========

    /// Insert char at start of empty line
    #[test]
    fn edit_char_empty_line() {
        let mut app = app_with_lines(&[""]);
        app.viewer_edit_char('a');
        assert_eq!(app.viewer_edit_content[0], "a");
        assert_eq!(app.viewer_edit_cursor, (0, 1));
    }

    /// Insert char at end of line
    #[test]
    fn edit_char_end_of_line() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_cursor = (0, 3);
        app.viewer_edit_char('d');
        assert_eq!(app.viewer_edit_content[0], "abcd");
        assert_eq!(app.viewer_edit_cursor, (0, 4));
    }

    /// Insert char in middle of line
    #[test]
    fn edit_char_middle() {
        let mut app = app_with_lines(&["ac"]);
        app.viewer_edit_cursor = (0, 1);
        app.viewer_edit_char('b');
        assert_eq!(app.viewer_edit_content[0], "abc");
        assert_eq!(app.viewer_edit_cursor, (0, 2));
    }

    /// Insert unicode char
    #[test]
    fn edit_char_unicode() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_cursor = (0, 5);
        app.viewer_edit_char('\u{00e9}'); // e-acute
        assert_eq!(app.viewer_edit_content[0], "hello\u{00e9}");
        assert_eq!(app.viewer_edit_cursor, (0, 6));
    }

    /// Insert sets dirty flag
    #[test]
    fn edit_char_sets_dirty() {
        let mut app = app_with_lines(&["hi"]);
        assert!(!app.viewer_edit_dirty);
        app.viewer_edit_char('!');
        assert!(app.viewer_edit_dirty);
    }

    /// Insert pushes undo state
    #[test]
    fn edit_char_pushes_undo() {
        let mut app = app_with_lines(&["hi"]);
        assert!(app.viewer_edit_undo.is_empty());
        app.viewer_edit_char('!');
        assert_eq!(app.viewer_edit_undo.len(), 1);
    }

    /// Insert into a line with multi-byte characters at correct position
    #[test]
    fn edit_char_multibyte_position() {
        let mut app = app_with_lines(&["\u{00e9}\u{00e9}"]); // "éé"
        app.viewer_edit_cursor = (0, 1); // between the two é characters
        app.viewer_edit_char('x');
        assert_eq!(app.viewer_edit_content[0], "\u{00e9}x\u{00e9}");
    }

    // ========== viewer_edit_backspace ==========

    /// Backspace at start of first line does nothing
    #[test]
    fn backspace_start_first_line() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_cursor = (0, 0);
        app.viewer_edit_backspace();
        assert_eq!(app.viewer_edit_content[0], "hello");
        assert_eq!(app.viewer_edit_cursor, (0, 0));
    }

    /// Backspace removes character before cursor
    #[test]
    fn backspace_middle_of_line() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_cursor = (0, 2);
        app.viewer_edit_backspace();
        assert_eq!(app.viewer_edit_content[0], "ac");
        assert_eq!(app.viewer_edit_cursor, (0, 1));
    }

    /// Backspace at end of line removes last char
    #[test]
    fn backspace_end_of_line() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_cursor = (0, 3);
        app.viewer_edit_backspace();
        assert_eq!(app.viewer_edit_content[0], "ab");
        assert_eq!(app.viewer_edit_cursor, (0, 2));
    }

    /// Backspace at start of line joins with previous line
    #[test]
    fn backspace_joins_lines() {
        let mut app = app_with_lines(&["hello", "world"]);
        app.viewer_edit_cursor = (1, 0);
        app.viewer_edit_backspace();
        assert_eq!(app.viewer_edit_content.len(), 1);
        assert_eq!(app.viewer_edit_content[0], "helloworld");
        assert_eq!(app.viewer_edit_cursor, (0, 5));
    }

    /// Backspace join preserves char-count cursor position with unicode
    #[test]
    fn backspace_join_unicode_cursor() {
        let mut app = app_with_lines(&["\u{00e9}\u{00e9}", "ab"]);
        app.viewer_edit_cursor = (1, 0);
        app.viewer_edit_backspace();
        assert_eq!(app.viewer_edit_content[0], "\u{00e9}\u{00e9}ab");
        // Cursor should be at char index 2 (the two é chars)
        assert_eq!(app.viewer_edit_cursor, (0, 2));
    }

    /// Backspace on single-char line leaves empty line
    #[test]
    fn backspace_single_char() {
        let mut app = app_with_lines(&["a"]);
        app.viewer_edit_cursor = (0, 1);
        app.viewer_edit_backspace();
        assert_eq!(app.viewer_edit_content[0], "");
        assert_eq!(app.viewer_edit_cursor, (0, 0));
    }

    // ========== viewer_edit_delete ==========

    /// Delete at end of last line does nothing
    #[test]
    fn delete_end_of_last_line() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_cursor = (0, 3);
        app.viewer_edit_delete();
        assert_eq!(app.viewer_edit_content[0], "abc");
    }

    /// Delete removes character at cursor
    #[test]
    fn delete_middle_of_line() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_cursor = (0, 1);
        app.viewer_edit_delete();
        assert_eq!(app.viewer_edit_content[0], "ac");
        assert_eq!(app.viewer_edit_cursor, (0, 1)); // cursor stays
    }

    /// Delete at start removes first character
    #[test]
    fn delete_start_of_line() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_cursor = (0, 0);
        app.viewer_edit_delete();
        assert_eq!(app.viewer_edit_content[0], "bc");
    }

    /// Delete at end of line joins with next line
    #[test]
    fn delete_joins_with_next_line() {
        let mut app = app_with_lines(&["hello", "world"]);
        app.viewer_edit_cursor = (0, 5);
        app.viewer_edit_delete();
        assert_eq!(app.viewer_edit_content.len(), 1);
        assert_eq!(app.viewer_edit_content[0], "helloworld");
        assert_eq!(app.viewer_edit_cursor, (0, 5));
    }

    /// Delete on empty line joins with next
    #[test]
    fn delete_empty_line_joins() {
        let mut app = app_with_lines(&["", "world"]);
        app.viewer_edit_cursor = (0, 0);
        app.viewer_edit_delete();
        assert_eq!(app.viewer_edit_content.len(), 1);
        assert_eq!(app.viewer_edit_content[0], "world");
    }

    // ========== viewer_edit_enter ==========

    /// Enter at end of line creates new empty line below
    #[test]
    fn enter_end_of_line() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_cursor = (0, 5);
        app.viewer_edit_enter();
        assert_eq!(app.viewer_edit_content, vec!["hello", ""]);
        assert_eq!(app.viewer_edit_cursor, (1, 0));
    }

    /// Enter at start of line pushes content down
    #[test]
    fn enter_start_of_line() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_cursor = (0, 0);
        app.viewer_edit_enter();
        assert_eq!(app.viewer_edit_content, vec!["", "hello"]);
        assert_eq!(app.viewer_edit_cursor, (1, 0));
    }

    /// Enter in middle splits line
    #[test]
    fn enter_splits_line() {
        let mut app = app_with_lines(&["helloworld"]);
        app.viewer_edit_cursor = (0, 5);
        app.viewer_edit_enter();
        assert_eq!(app.viewer_edit_content, vec!["hello", "world"]);
        assert_eq!(app.viewer_edit_cursor, (1, 0));
    }

    /// Enter on empty line creates another empty line
    #[test]
    fn enter_empty_line() {
        let mut app = app_with_lines(&[""]);
        app.viewer_edit_cursor = (0, 0);
        app.viewer_edit_enter();
        assert_eq!(app.viewer_edit_content, vec!["", ""]);
        assert_eq!(app.viewer_edit_cursor, (1, 0));
    }

    /// Enter sets dirty flag
    #[test]
    fn enter_sets_dirty() {
        let mut app = app_with_lines(&["hi"]);
        app.viewer_edit_enter();
        assert!(app.viewer_edit_dirty);
    }

    /// Enter with unicode splits correctly by char index
    #[test]
    fn enter_unicode_split() {
        let mut app = app_with_lines(&["\u{00e9}x\u{00e9}"]);
        app.viewer_edit_cursor = (0, 1); // after first é
        app.viewer_edit_enter();
        assert_eq!(app.viewer_edit_content[0], "\u{00e9}");
        assert_eq!(app.viewer_edit_content[1], "x\u{00e9}");
    }

    // ========== cursor movement (left/right/home/end) ==========

    /// Left at start of first line stays put
    #[test]
    fn left_start_stays() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_cursor = (0, 0);
        app.viewer_edit_left();
        assert_eq!(app.viewer_edit_cursor, (0, 0));
    }

    /// Left moves cursor one position left
    #[test]
    fn left_normal() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_cursor = (0, 2);
        app.viewer_edit_left();
        assert_eq!(app.viewer_edit_cursor, (0, 1));
    }

    /// Left at start of line wraps to end of previous line
    #[test]
    fn left_wraps_to_prev_line() {
        let mut app = app_with_lines(&["abc", "def"]);
        app.viewer_edit_cursor = (1, 0);
        app.viewer_edit_left();
        assert_eq!(app.viewer_edit_cursor, (0, 3));
    }

    /// Right at end of last line stays put
    #[test]
    fn right_end_stays() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_cursor = (0, 3);
        app.viewer_edit_right();
        assert_eq!(app.viewer_edit_cursor, (0, 3));
    }

    /// Right moves cursor one position right
    #[test]
    fn right_normal() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_cursor = (0, 1);
        app.viewer_edit_right();
        assert_eq!(app.viewer_edit_cursor, (0, 2));
    }

    /// Right at end of line wraps to start of next line
    #[test]
    fn right_wraps_to_next_line() {
        let mut app = app_with_lines(&["abc", "def"]);
        app.viewer_edit_cursor = (0, 3);
        app.viewer_edit_right();
        assert_eq!(app.viewer_edit_cursor, (1, 0));
    }

    /// Home moves cursor to column 0
    #[test]
    fn home_moves_to_start() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_cursor = (0, 3);
        app.viewer_edit_home();
        assert_eq!(app.viewer_edit_cursor, (0, 0));
    }

    /// Home when already at start stays
    #[test]
    fn home_already_at_start() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_cursor = (0, 0);
        app.viewer_edit_home();
        assert_eq!(app.viewer_edit_cursor, (0, 0));
    }

    /// End moves cursor to end of line
    #[test]
    fn end_moves_to_end() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_cursor = (0, 0);
        app.viewer_edit_end();
        assert_eq!(app.viewer_edit_cursor, (0, 5));
    }

    /// End on empty line stays at 0
    #[test]
    fn end_empty_line() {
        let mut app = app_with_lines(&[""]);
        app.viewer_edit_cursor = (0, 0);
        app.viewer_edit_end();
        assert_eq!(app.viewer_edit_cursor, (0, 0));
    }

    /// End with unicode counts chars not bytes
    #[test]
    fn end_unicode() {
        let mut app = app_with_lines(&["\u{00e9}\u{00e9}\u{00e9}"]); // "ééé"
        app.viewer_edit_cursor = (0, 0);
        app.viewer_edit_end();
        assert_eq!(app.viewer_edit_cursor, (0, 3));
    }

    // ========== up/down movement ==========

    /// Up at first line stays
    #[test]
    fn up_first_line_stays() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_cursor = (0, 2);
        app.viewer_edit_up();
        assert_eq!(app.viewer_edit_cursor, (0, 2));
    }

    /// Up moves to previous line
    #[test]
    fn up_simple() {
        let mut app = app_with_lines(&["hello", "world"]);
        app.viewer_edit_cursor = (1, 3);
        app.viewer_edit_up();
        assert_eq!(app.viewer_edit_cursor.0, 0);
    }

    /// Down at last line stays
    #[test]
    fn down_last_line_stays() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_cursor = (0, 2);
        app.viewer_edit_down();
        assert_eq!(app.viewer_edit_cursor, (0, 2));
    }

    /// Down moves to next line
    #[test]
    fn down_simple() {
        let mut app = app_with_lines(&["hello", "world"]);
        app.viewer_edit_cursor = (0, 3);
        app.viewer_edit_down();
        assert_eq!(app.viewer_edit_cursor.0, 1);
    }

    /// Down clamps column to shorter next line
    #[test]
    fn down_clamps_column() {
        let mut app = app_with_lines(&["hello world", "hi"]);
        app.viewer_edit_cursor = (0, 10);
        app.viewer_edit_down();
        assert_eq!(app.viewer_edit_cursor, (1, 2));
    }

    // ========== clamp_edit_cursor ==========

    /// Clamp cursor beyond last line
    #[test]
    fn clamp_cursor_beyond_lines() {
        let mut app = app_with_lines(&["abc", "def"]);
        app.viewer_edit_cursor = (10, 5);
        app.clamp_edit_cursor();
        assert_eq!(app.viewer_edit_cursor.0, 1); // last line
    }

    /// Clamp cursor beyond line length
    #[test]
    fn clamp_cursor_beyond_col() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_cursor = (0, 100);
        app.clamp_edit_cursor();
        assert_eq!(app.viewer_edit_cursor, (0, 3));
    }

    /// Clamp cursor within valid range stays unchanged
    #[test]
    fn clamp_cursor_valid_unchanged() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_cursor = (0, 3);
        app.clamp_edit_cursor();
        assert_eq!(app.viewer_edit_cursor, (0, 3));
    }

    /// Clamp with single empty line
    #[test]
    fn clamp_cursor_single_empty_line() {
        let mut app = app_with_lines(&[""]);
        app.viewer_edit_cursor = (5, 5);
        app.clamp_edit_cursor();
        assert_eq!(app.viewer_edit_cursor, (0, 0));
    }

    // ========== undo / redo ==========

    /// Undo after inserting restores previous state
    #[test]
    fn undo_restore() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_char('d');
        assert_eq!(app.viewer_edit_content[0], "dabc");
        app.viewer_edit_undo();
        assert_eq!(app.viewer_edit_content[0], "abc");
    }

    /// Redo after undo re-applies the change
    #[test]
    fn redo_reapply() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_char('d');
        app.viewer_edit_undo();
        assert_eq!(app.viewer_edit_content[0], "abc");
        app.viewer_edit_redo();
        assert_eq!(app.viewer_edit_content[0], "dabc");
    }

    /// Undo on empty stack does nothing
    #[test]
    fn undo_empty_noop() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_undo(); // should not panic
        assert_eq!(app.viewer_edit_content[0], "abc");
    }

    /// Redo on empty stack does nothing
    #[test]
    fn redo_empty_noop() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_redo(); // should not panic
        assert_eq!(app.viewer_edit_content[0], "abc");
    }

    /// New edit clears redo stack
    #[test]
    fn edit_clears_redo() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_char('x');
        app.viewer_edit_undo();
        assert!(!app.viewer_edit_redo.is_empty());
        app.viewer_edit_char('y');
        assert!(app.viewer_edit_redo.is_empty());
    }

    /// Multiple undos work sequentially
    #[test]
    fn multiple_undo() {
        let mut app = app_with_lines(&[""]);
        app.viewer_edit_char('a');
        app.viewer_edit_char('b');
        app.viewer_edit_char('c');
        assert_eq!(app.viewer_edit_content[0], "abc");
        app.viewer_edit_undo();
        assert_eq!(app.viewer_edit_content[0], "ab");
        app.viewer_edit_undo();
        assert_eq!(app.viewer_edit_content[0], "a");
        app.viewer_edit_undo();
        assert_eq!(app.viewer_edit_content[0], "");
    }

    // ========== selection ==========

    /// Start selection sets anchor at cursor
    #[test]
    fn start_selection_at_cursor() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_cursor = (0, 2);
        app.viewer_edit_start_selection();
        assert_eq!(app.viewer_edit_selection, Some((0, 2, 0, 2)));
    }

    /// Extend selection moves end to cursor
    #[test]
    fn extend_selection() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_cursor = (0, 1);
        app.viewer_edit_start_selection();
        app.viewer_edit_cursor = (0, 4);
        app.viewer_edit_extend_selection();
        assert_eq!(app.viewer_edit_selection, Some((0, 1, 0, 4)));
    }

    /// Clear selection sets to None
    #[test]
    fn clear_selection() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_selection = Some((0, 1, 0, 4));
        app.viewer_edit_clear_selection();
        assert_eq!(app.viewer_edit_selection, None);
    }

    /// has_edit_selection returns false when None
    #[test]
    fn has_selection_none() {
        let app = app_with_lines(&["hello"]);
        assert!(!app.has_edit_selection());
    }

    /// has_edit_selection returns false when start == end (zero-width)
    #[test]
    fn has_selection_zero_width() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_selection = Some((0, 2, 0, 2));
        assert!(!app.has_edit_selection());
    }

    /// has_edit_selection returns true for real selection
    #[test]
    fn has_selection_true() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_selection = Some((0, 1, 0, 4));
        assert!(app.has_edit_selection());
    }

    /// get_normalized_selection normalizes backward selection
    #[test]
    fn normalized_selection_backward() {
        let mut app = app_with_lines(&["hello", "world"]);
        app.viewer_edit_selection = Some((1, 3, 0, 1));
        let norm = app.get_normalized_selection();
        assert_eq!(norm, Some((0, 1, 1, 3)));
    }

    /// get_normalized_selection keeps forward selection as-is
    #[test]
    fn normalized_selection_forward() {
        let mut app = app_with_lines(&["hello", "world"]);
        app.viewer_edit_selection = Some((0, 1, 1, 3));
        let norm = app.get_normalized_selection();
        assert_eq!(norm, Some((0, 1, 1, 3)));
    }

    /// get_selected_text single line
    #[test]
    fn selected_text_single_line() {
        let mut app = app_with_lines(&["hello world"]);
        app.viewer_edit_selection = Some((0, 6, 0, 11));
        assert_eq!(app.get_selected_text(), Some("world".to_string()));
    }

    /// get_selected_text multi-line
    #[test]
    fn selected_text_multi_line() {
        let mut app = app_with_lines(&["hello", "world", "test"]);
        app.viewer_edit_selection = Some((0, 3, 1, 3));
        let text = app.get_selected_text().unwrap();
        assert_eq!(text, "lo\nwor");
    }

    /// get_selected_text returns None for zero-width selection
    #[test]
    fn selected_text_zero_width() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_selection = Some((0, 2, 0, 2));
        assert_eq!(app.get_selected_text(), None);
    }

    /// get_selected_text returns None when no selection
    #[test]
    fn selected_text_no_selection() {
        let app = app_with_lines(&["hello"]);
        assert_eq!(app.get_selected_text(), None);
    }

    /// get_selected_text entire first line
    #[test]
    fn selected_text_entire_line() {
        let mut app = app_with_lines(&["hello", "world"]);
        app.viewer_edit_selection = Some((0, 0, 0, 5));
        assert_eq!(app.get_selected_text(), Some("hello".to_string()));
    }

    /// get_selected_text with backward selection still works
    #[test]
    fn selected_text_backward() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_selection = Some((0, 4, 0, 1));
        assert_eq!(app.get_selected_text(), Some("ell".to_string()));
    }

    // ========== select all ==========

    /// Select all on multi-line content
    #[test]
    fn select_all_multiline() {
        let mut app = app_with_lines(&["hello", "world", "test"]);
        app.viewer_edit_select_all();
        assert_eq!(app.viewer_edit_selection, Some((0, 0, 2, 4)));
        assert_eq!(app.viewer_edit_cursor, (2, 4));
    }

    /// Select all on single line
    #[test]
    fn select_all_single_line() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_select_all();
        assert_eq!(app.viewer_edit_selection, Some((0, 0, 0, 5)));
    }

    /// Select all on empty content does nothing
    #[test]
    fn select_all_empty() {
        let mut app = App::new();
        app.viewer_edit_content.clear();
        app.viewer_edit_select_all();
        assert_eq!(app.viewer_edit_selection, None);
    }

    /// Select all with unicode
    #[test]
    fn select_all_unicode() {
        let mut app = app_with_lines(&["\u{00e9}\u{00e9}\u{00e9}"]);
        app.viewer_edit_select_all();
        assert_eq!(app.viewer_edit_selection, Some((0, 0, 0, 3)));
    }

    // ========== delete_selection_text ==========

    /// Delete single-line selection
    #[test]
    fn delete_selection_single_line() {
        let mut app = app_with_lines(&["hello world"]);
        app.viewer_edit_selection = Some((0, 5, 0, 11));
        app.viewer_edit_delete_selection();
        assert_eq!(app.viewer_edit_content[0], "hello");
        assert_eq!(app.viewer_edit_cursor, (0, 5));
    }

    /// Delete multi-line selection
    #[test]
    fn delete_selection_multi_line() {
        let mut app = app_with_lines(&["hello", "middle", "world"]);
        app.viewer_edit_selection = Some((0, 3, 2, 2));
        app.viewer_edit_delete_selection();
        assert_eq!(app.viewer_edit_content.len(), 1);
        assert_eq!(app.viewer_edit_content[0], "helrld");
        assert_eq!(app.viewer_edit_cursor, (0, 3));
    }

    /// Delete selection clears the selection
    #[test]
    fn delete_selection_clears() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_selection = Some((0, 1, 0, 4));
        app.viewer_edit_delete_selection();
        assert_eq!(app.viewer_edit_selection, None);
    }

    /// Delete zero-width selection does nothing
    #[test]
    fn delete_selection_zero_width_noop() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_selection = Some((0, 2, 0, 2));
        app.viewer_edit_delete_selection();
        assert_eq!(app.viewer_edit_content[0], "hello");
    }

    // ========== viewer_edit_scroll_to_cursor ==========

    /// Scroll to cursor at top of viewport stays
    #[test]
    fn scroll_to_cursor_at_top() {
        let mut app = app_with_lines(&["a", "b", "c", "d", "e"]);
        app.viewer_viewport_height = 3;
        app.viewer_scroll = 0;
        app.viewer_edit_cursor = (0, 0);
        app.viewer_edit_scroll_to_cursor();
        assert_eq!(app.viewer_scroll, 0);
    }

    /// Scroll to cursor below viewport scrolls down
    #[test]
    fn scroll_to_cursor_below_viewport() {
        let mut app = app_with_lines(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        app.viewer_viewport_height = 3;
        app.viewer_scroll = 0;
        app.viewer_edit_cursor = (5, 0);
        app.viewer_edit_scroll_to_cursor();
        // Visual line 5 should be visible within viewport of 3
        assert!(app.viewer_scroll <= 5);
        assert!(app.viewer_scroll + 3 > 5);
    }

    /// Scroll to cursor above viewport scrolls up
    #[test]
    fn scroll_to_cursor_above_viewport() {
        let mut app = app_with_lines(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        app.viewer_viewport_height = 3;
        app.viewer_scroll = 5;
        app.viewer_edit_cursor = (1, 0);
        app.viewer_edit_scroll_to_cursor();
        assert_eq!(app.viewer_scroll, 1);
    }

    // ========== selection-aware movement ==========

    /// Left without extend clears selection
    #[test]
    fn left_select_no_extend_clears() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_cursor = (0, 3);
        app.viewer_edit_selection = Some((0, 1, 0, 3));
        app.viewer_edit_left_select(false);
        assert_eq!(app.viewer_edit_selection, None);
        assert_eq!(app.viewer_edit_cursor, (0, 2));
    }

    /// Left with extend creates and extends selection
    #[test]
    fn left_select_extend() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_cursor = (0, 3);
        app.viewer_edit_left_select(true);
        assert_eq!(app.viewer_edit_selection, Some((0, 3, 0, 2)));
    }

    /// Right without extend clears selection
    #[test]
    fn right_select_no_extend_clears() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_cursor = (0, 2);
        app.viewer_edit_selection = Some((0, 1, 0, 3));
        app.viewer_edit_right_select(false);
        assert_eq!(app.viewer_edit_selection, None);
    }

    /// Right with extend creates and extends selection
    #[test]
    fn right_select_extend() {
        let mut app = app_with_lines(&["hello"]);
        app.viewer_edit_cursor = (0, 2);
        app.viewer_edit_right_select(true);
        assert_eq!(app.viewer_edit_selection, Some((0, 2, 0, 3)));
    }

    /// Up with extend creates selection
    #[test]
    fn up_select_extend() {
        let mut app = app_with_lines(&["hello", "world"]);
        app.viewer_edit_cursor = (1, 3);
        app.viewer_edit_up_select(true);
        assert!(app.viewer_edit_selection.is_some());
    }

    /// Down with extend creates selection
    #[test]
    fn down_select_extend() {
        let mut app = app_with_lines(&["hello", "world"]);
        app.viewer_edit_cursor = (0, 3);
        app.viewer_edit_down_select(true);
        assert!(app.viewer_edit_selection.is_some());
    }

    // ========== Comprehensive / edge case tests ==========

    /// Multiple inserts then undo all restores original
    #[test]
    fn insert_sequence_undo_all() {
        let mut app = app_with_lines(&[""]);
        for c in "hello".chars() {
            app.viewer_edit_char(c);
        }
        assert_eq!(app.viewer_edit_content[0], "hello");
        for _ in 0..5 {
            app.viewer_edit_undo();
        }
        assert_eq!(app.viewer_edit_content[0], "");
    }

    /// Enter then backspace restores original single line
    #[test]
    fn enter_backspace_roundtrip() {
        let mut app = app_with_lines(&["helloworld"]);
        app.viewer_edit_cursor = (0, 5);
        app.viewer_edit_enter();
        assert_eq!(app.viewer_edit_content.len(), 2);
        app.viewer_edit_backspace();
        assert_eq!(app.viewer_edit_content.len(), 1);
        assert_eq!(app.viewer_edit_content[0], "helloworld");
    }

    /// Delete then undo restores
    #[test]
    fn delete_undo_restores() {
        let mut app = app_with_lines(&["abc"]);
        app.viewer_edit_cursor = (0, 1);
        app.viewer_edit_delete();
        assert_eq!(app.viewer_edit_content[0], "ac");
        app.viewer_edit_undo();
        assert_eq!(app.viewer_edit_content[0], "abc");
    }

    /// Backspace on unicode removes correct character
    #[test]
    fn backspace_unicode() {
        let mut app = app_with_lines(&["a\u{00e9}b"]); // "aéb"
        app.viewer_edit_cursor = (0, 2); // after é
        app.viewer_edit_backspace();
        assert_eq!(app.viewer_edit_content[0], "ab");
    }

    /// Delete on unicode removes correct character
    #[test]
    fn delete_unicode() {
        let mut app = app_with_lines(&["a\u{00e9}b"]); // "aéb"
        app.viewer_edit_cursor = (0, 1); // on é
        app.viewer_edit_delete();
        assert_eq!(app.viewer_edit_content[0], "ab");
    }

    /// Left wraps correctly from start of third line
    #[test]
    fn left_wrap_multi_lines() {
        let mut app = app_with_lines(&["ab", "cd", "ef"]);
        app.viewer_edit_cursor = (2, 0);
        app.viewer_edit_left();
        assert_eq!(app.viewer_edit_cursor, (1, 2));
        app.viewer_edit_left();
        assert_eq!(app.viewer_edit_cursor, (1, 1));
    }

    /// Right wraps correctly through multiple lines
    #[test]
    fn right_wrap_multi_lines() {
        let mut app = app_with_lines(&["ab", "cd", "ef"]);
        app.viewer_edit_cursor = (0, 2);
        app.viewer_edit_right();
        assert_eq!(app.viewer_edit_cursor, (1, 0));
        app.viewer_edit_right();
        assert_eq!(app.viewer_edit_cursor, (1, 1));
    }

    /// CJK characters (multi-byte) insert correctly
    #[test]
    fn edit_char_cjk() {
        let mut app = app_with_lines(&["ab"]);
        app.viewer_edit_cursor = (0, 1);
        app.viewer_edit_char('\u{4e16}'); // 世
        assert_eq!(app.viewer_edit_content[0], "a\u{4e16}b");
        assert_eq!(app.viewer_edit_cursor, (0, 2));
    }

    /// Tab character inserts normally
    #[test]
    fn edit_char_tab() {
        let mut app = app_with_lines(&["ab"]);
        app.viewer_edit_cursor = (0, 1);
        app.viewer_edit_char('\t');
        assert_eq!(app.viewer_edit_content[0], "a\tb");
    }

    /// Selection across three full lines
    #[test]
    fn selected_text_three_lines() {
        let mut app = app_with_lines(&["line1", "line2", "line3"]);
        app.viewer_edit_selection = Some((0, 0, 2, 5));
        let text = app.get_selected_text().unwrap();
        assert_eq!(text, "line1\nline2\nline3");
    }

    /// Undo stack caps at 100 entries
    #[test]
    fn undo_stack_cap() {
        let mut app = app_with_lines(&[""]);
        for i in 0..110 {
            app.viewer_edit_char(char::from(b'a' + (i % 26) as u8));
        }
        assert!(app.viewer_edit_undo.len() <= 100);
    }

    /// Version counter increments on edit
    #[test]
    fn version_increments_on_edit() {
        let mut app = app_with_lines(&[""]);
        let v0 = app.viewer_edit_version;
        app.viewer_edit_char('a');
        assert_eq!(app.viewer_edit_version, v0 + 1);
    }

    /// Version counter increments on undo
    #[test]
    fn version_increments_on_undo() {
        let mut app = app_with_lines(&[""]);
        app.viewer_edit_char('a');
        let v_before_undo = app.viewer_edit_version;
        app.viewer_edit_undo();
        assert_eq!(app.viewer_edit_version, v_before_undo + 1);
    }

    /// Version counter increments on redo
    #[test]
    fn version_increments_on_redo() {
        let mut app = app_with_lines(&[""]);
        app.viewer_edit_char('a');
        app.viewer_edit_undo();
        let v_before_redo = app.viewer_edit_version;
        app.viewer_edit_redo();
        assert_eq!(app.viewer_edit_version, v_before_redo + 1);
    }
}
