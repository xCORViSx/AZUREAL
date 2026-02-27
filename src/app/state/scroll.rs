//! Scroll operations for session, diff, and viewer panes

use super::App;

impl App {
    /// Natural bottom position: last line at bottom of viewport
    pub(crate) fn session_natural_bottom(&self) -> usize {
        self.rendered_lines_cache.len().saturating_sub(self.session_viewport_height)
    }

    /// Max scroll position: allows scrolling so last line can be at top (vim-style)
    pub(crate) fn session_max_scroll(&self) -> usize {
        self.rendered_lines_cache.len().saturating_sub(1)
    }

    /// Clamp session_scroll to valid range, resolving usize::MAX sentinel to natural bottom
    pub fn clamp_session_scroll(&mut self) {
        if self.session_scroll == usize::MAX {
            // Sentinel: scroll to natural bottom (last line at bottom of viewport)
            self.session_scroll = self.session_natural_bottom();
        } else {
            // Manual scroll: allow up to vim-style max
            self.session_scroll = self.session_scroll.min(self.session_max_scroll());
        }
    }

    /// Scroll output down, returns true if position changed.
    /// If scrolling reaches the natural bottom, re-engage follow-bottom sentinel
    /// so new content auto-scrolls without needing ⌥↓.
    pub fn scroll_session_down(&mut self, lines: usize) -> bool {
        if self.session_scroll == usize::MAX {
            self.session_scroll = self.session_natural_bottom();
        }
        let old = self.session_scroll;
        self.session_scroll = self.session_scroll.saturating_add(lines).min(self.session_max_scroll());
        // Re-engage auto-follow when user scrolls to (or past) the natural bottom
        if self.session_scroll >= self.session_natural_bottom() {
            self.session_scroll = usize::MAX;
        }
        self.session_scroll != old
    }

    /// Scroll output up, returns true if position changed
    pub fn scroll_session_up(&mut self, lines: usize) -> bool {
        if self.session_scroll == usize::MAX {
            self.session_scroll = self.session_natural_bottom();
        }
        let old = self.session_scroll;
        self.session_scroll = self.session_scroll.saturating_sub(lines);
        // If we hit the top and early events were deferred, trigger full render
        if self.session_scroll == 0 && self.rendered_events_start > 0 {
            self.rendered_lines_dirty = true;
        }
        self.session_scroll != old
    }

    pub fn scroll_session_to_bottom(&mut self) {
        self.session_scroll = usize::MAX;
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

    /// Jump to the next message bubble in session pane.
    /// If include_assistant is true, jumps to ALL bubbles (user + assistant).
    /// Otherwise, only jumps to UserMessage bubbles (user prompts).
    /// Positions viewport so the bubble header sits 2 lines from top (showing the spacer).
    pub fn jump_to_next_bubble(&mut self, include_assistant: bool) {
        if self.session_scroll == usize::MAX {
            self.session_scroll = self.session_natural_bottom();
        }
        let current = self.session_scroll;
        // Bubble positions store the header line index. We display at line_idx - 2
        // (showing the empty spacer lines above). Find next bubble whose display
        // position (line_idx - 2) is strictly after current scroll.
        for &(line_idx, is_user) in &self.message_bubble_positions {
            let display_pos = line_idx.saturating_sub(2);
            if display_pos > current && (include_assistant || is_user) {
                self.session_scroll = display_pos.min(self.session_max_scroll());
                return;
            }
        }
        // No more bubbles — re-engage auto-follow sentinel
        self.session_scroll = usize::MAX;
    }

    /// Jump to the previous message bubble in session pane.
    /// If include_assistant is true, jumps to ALL bubbles (user + assistant).
    /// Otherwise, only jumps to UserMessage bubbles (user prompts).
    /// Positions viewport so the bubble header sits 2 lines from top (showing the spacer).
    pub fn jump_to_prev_bubble(&mut self, include_assistant: bool) {
        if self.session_scroll == usize::MAX {
            self.session_scroll = self.session_natural_bottom();
        }
        let current = self.session_scroll;
        // Find previous bubble whose display position is strictly before current scroll
        for &(line_idx, is_user) in self.message_bubble_positions.iter().rev() {
            let display_pos = line_idx.saturating_sub(2);
            if display_pos < current && (include_assistant || is_user) {
                self.session_scroll = display_pos;
                return;
            }
        }
        // No previous bubbles, scroll to top
        self.session_scroll = 0;
        // If early events were deferred (not yet rendered), trigger a full render
        // so the user can continue navigating upward through the entire conversation.
        // Without this, rendered_lines_dirty stays false and submit_render_request
        // never re-checks the deferred expansion condition.
        if self.rendered_events_start > 0 {
            self.rendered_lines_dirty = true;
        }
    }
}
