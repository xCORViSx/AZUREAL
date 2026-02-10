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
    if max_width == 0 || text.is_empty() { return vec![0]; }
    if text.chars().count() <= max_width { return vec![0]; }
    let opts = Options::new(max_width).break_words(true);
    let wrapped = wrap(text, opts);
    let mut breaks = Vec::with_capacity(wrapped.len());
    let mut offset = 0usize;
    for segment in &wrapped {
        breaks.push(offset);
        offset += segment.chars().count();
        if text.chars().nth(offset) == Some(' ') { offset += 1; }
    }
    breaks
}

impl App {
    /// Enter edit mode for current viewer file
    pub fn enter_viewer_edit_mode(&mut self) {
        let Some(ref content) = self.viewer_content else { return };
        if self.viewer_path.is_none() { return };

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
        let line_len = self.viewer_edit_content.get(clamped_line).map(|l| l.len()).unwrap_or(0);
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
                let new_str: String = chars[..col-1].iter().chain(chars[col..].iter()).collect();
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
        let line_len = self.viewer_edit_content.get(line).map(|s| s.chars().count()).unwrap_or(0);
        let total_lines = self.viewer_edit_content.len();

        if col < line_len {
            self.push_undo();
            let chars: Vec<char> = self.viewer_edit_content[line].chars().collect();
            let new_str: String = chars[..col].iter().chain(chars[col+1..].iter()).collect();
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
            if col >= brk { wrap_row = j; }
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
            if col >= brk { wrap_row = j; }
        }
        let visual_col = col - breaks[wrap_row];

        if wrap_row + 1 < breaks.len() {
            // Move down one visual row within the same source line
            let next_start = breaks[wrap_row + 1];
            let seg_end = if wrap_row + 2 < breaks.len() { breaks[wrap_row + 2] } else { line_str.chars().count() };
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
            self.viewer_edit_content.get(cursor_line).map(|s| s.as_str()).unwrap_or(""), cw
        );
        let mut wrap_row = 0;
        for (j, &brk) in cursor_breaks.iter().enumerate() {
            if cursor_col >= brk { wrap_row = j; }
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
        if sl == el && sc == ec { return None; }

        let mut result = String::new();
        for line_idx in sl..=el {
            let Some(line) = self.viewer_edit_content.get(line_idx) else { continue };
            let chars: Vec<char> = line.chars().collect();
            let start_col = if line_idx == sl { sc } else { 0 };
            let end_col = if line_idx == el { ec.min(chars.len()) } else { chars.len() };

            if start_col < chars.len() {
                let segment: String = chars[start_col..end_col.min(chars.len())].iter().collect();
                result.push_str(&segment);
            }
            // Add newline between lines (not after last line)
            if line_idx < el {
                result.push('\n');
            }
        }
        if result.is_empty() { None } else { Some(result) }
    }

    /// Delete selected text and return it
    fn delete_selection_text(&mut self) -> Option<String> {
        let (sl, sc, el, ec) = self.get_normalized_selection()?;
        if sl == el && sc == ec { return None; }

        self.push_undo();
        let deleted = self.get_selected_text();

        if sl == el {
            // Single-line selection: remove chars from sc to ec
            let chars: Vec<char> = self.viewer_edit_content[sl].chars().collect();
            let new_str: String = chars[..sc].iter().chain(chars[ec.min(chars.len())..].iter()).collect();
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
        let Some(text) = self.get_selected_text() else { return false };
        // Try system clipboard first, fall back to internal
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(&text);
        }
        self.clipboard = text;
        true
    }

    /// Cut selected text to system clipboard
    pub fn viewer_edit_cut(&mut self) {
        let Some(text) = self.delete_selection_text() else { return };
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

        if paste_text.is_empty() { return; }

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
            for (i, paste_line) in paste_lines.iter().enumerate().skip(1).take(paste_lines.len() - 2) {
                self.viewer_edit_content.insert(line + i, paste_line.to_string());
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
        if self.viewer_edit_content.is_empty() { return; }
        let last_line = self.viewer_edit_content.len() - 1;
        let last_col = self.viewer_edit_content[last_line].chars().count();
        self.viewer_edit_selection = Some((0, 0, last_line, last_col));
        self.viewer_edit_cursor = (last_line, last_col);
    }
}
