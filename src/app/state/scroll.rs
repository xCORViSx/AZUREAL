//! Scroll operations for output, diff, and viewer panes

use super::App;

impl App {
    /// Natural bottom position: last line at bottom of viewport
    fn output_natural_bottom(&self) -> usize {
        self.rendered_lines_cache.len().saturating_sub(self.output_viewport_height)
    }

    /// Max scroll position: allows scrolling so last line can be at top (vim-style)
    fn output_max_scroll(&self) -> usize {
        self.rendered_lines_cache.len().saturating_sub(1)
    }

    /// Clamp output_scroll to valid range, resolving usize::MAX sentinel to natural bottom
    pub fn clamp_output_scroll(&mut self) {
        if self.output_scroll == usize::MAX {
            // Sentinel: scroll to natural bottom (last line at bottom of viewport)
            self.output_scroll = self.output_natural_bottom();
        } else {
            // Manual scroll: allow up to vim-style max
            self.output_scroll = self.output_scroll.min(self.output_max_scroll());
        }
    }

    /// Scroll output down, returns true if position changed
    pub fn scroll_output_down(&mut self, lines: usize) -> bool {
        if self.output_scroll == usize::MAX {
            self.output_scroll = self.output_natural_bottom();
        }
        let old = self.output_scroll;
        self.output_scroll = self.output_scroll.saturating_add(lines).min(self.output_max_scroll());
        self.output_scroll != old
    }

    /// Scroll output up, returns true if position changed
    pub fn scroll_output_up(&mut self, lines: usize) -> bool {
        if self.output_scroll == usize::MAX {
            self.output_scroll = self.output_natural_bottom();
        }
        let old = self.output_scroll;
        self.output_scroll = self.output_scroll.saturating_sub(lines);
        self.output_scroll != old
    }

    pub fn scroll_output_to_bottom(&mut self) {
        self.output_scroll = usize::MAX;
    }

    /// Scroll diff down, returns true if position changed
    pub fn scroll_diff_down(&mut self, lines: usize) -> bool {
        let old = self.diff_scroll;
        if let Some(ref diff) = self.diff_text {
            let total_lines = diff.lines().count();
            let max_scroll = total_lines.saturating_sub(1);
            self.diff_scroll = self.diff_scroll.saturating_add(lines).min(max_scroll);
        }
        self.diff_scroll != old
    }

    /// Scroll diff up, returns true if position changed
    pub fn scroll_diff_up(&mut self, lines: usize) -> bool {
        let old = self.diff_scroll;
        self.diff_scroll = self.diff_scroll.saturating_sub(lines);
        self.diff_scroll != old
    }

    pub fn scroll_diff_to_bottom(&mut self) {
        if let Some(ref diff) = self.diff_text {
            self.diff_scroll = diff.lines().count().saturating_sub(self.output_viewport_height);
        }
    }

    /// Natural bottom position: last line at bottom of viewport
    fn viewer_natural_bottom(&self) -> usize {
        self.viewer_lines_cache.len().saturating_sub(self.viewer_viewport_height)
    }

    /// Max scroll position: allows scrolling so last line can be at top (vim-style)
    fn viewer_max_scroll(&self) -> usize {
        self.viewer_lines_cache.len().saturating_sub(1)
    }

    /// Clamp viewer_scroll to valid range, resolving usize::MAX sentinel to natural bottom
    pub fn clamp_viewer_scroll(&mut self) {
        if self.viewer_scroll == usize::MAX {
            // Sentinel: scroll to natural bottom (last line at bottom of viewport)
            self.viewer_scroll = self.viewer_natural_bottom();
        } else {
            // Manual scroll: allow up to vim-style max
            self.viewer_scroll = self.viewer_scroll.min(self.viewer_max_scroll());
        }
    }

    /// Scroll viewer down, returns true if position changed
    pub fn scroll_viewer_down(&mut self, lines: usize) -> bool {
        if self.viewer_scroll == usize::MAX {
            self.viewer_scroll = self.viewer_natural_bottom();
        }
        let old = self.viewer_scroll;
        self.viewer_scroll = self.viewer_scroll.saturating_add(lines).min(self.viewer_max_scroll());
        self.viewer_scroll != old
    }

    /// Scroll viewer up, returns true if position changed
    pub fn scroll_viewer_up(&mut self, lines: usize) -> bool {
        if self.viewer_scroll == usize::MAX {
            self.viewer_scroll = self.viewer_natural_bottom();
        }
        let old = self.viewer_scroll;
        self.viewer_scroll = self.viewer_scroll.saturating_sub(lines);
        self.viewer_scroll != old
    }

    /// Scroll viewer to bottom
    pub fn scroll_viewer_to_bottom(&mut self) {
        self.viewer_scroll = usize::MAX;
    }

    /// Jump to the next message bubble in convo pane
    /// If include_assistant is true, jumps to both UserMessage and AssistantText bubbles
    /// Otherwise, only jumps to UserMessage bubbles
    pub fn jump_to_next_bubble(&mut self, include_assistant: bool) {
        if self.output_scroll == usize::MAX {
            self.output_scroll = self.output_natural_bottom();
        }
        let current = self.output_scroll;
        // Find next bubble position after current scroll
        for &(line_idx, is_user) in &self.message_bubble_positions {
            if line_idx > current && (include_assistant || is_user) {
                self.output_scroll = line_idx.min(self.output_max_scroll());
                return;
            }
        }
        // No more bubbles, scroll to bottom
        self.output_scroll = self.output_max_scroll();
    }

    /// Jump to the previous message bubble in convo pane
    /// If include_assistant is true, jumps to both UserMessage and AssistantText bubbles
    /// Otherwise, only jumps to UserMessage bubbles
    pub fn jump_to_prev_bubble(&mut self, include_assistant: bool) {
        if self.output_scroll == usize::MAX {
            self.output_scroll = self.output_natural_bottom();
        }
        let current = self.output_scroll;
        // Find previous bubble position before current scroll (iterate in reverse)
        for &(line_idx, is_user) in self.message_bubble_positions.iter().rev() {
            if line_idx < current && (include_assistant || is_user) {
                self.output_scroll = line_idx;
                return;
            }
        }
        // No previous bubbles, scroll to top
        self.output_scroll = 0;
    }
}
