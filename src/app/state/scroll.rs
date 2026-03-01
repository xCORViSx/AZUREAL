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

#[cfg(test)]
mod tests {
    use super::*;

    // ========== HELPER ==========

    /// Build an App with session rendered_lines_cache of given length
    fn app_with_session_lines(line_count: usize, viewport: usize) -> App {
        let mut app = App::new();
        app.rendered_lines_cache = vec![ratatui::text::Line::default(); line_count];
        app.session_viewport_height = viewport;
        app
    }

    /// Build an App with viewer_lines_cache of given length
    fn app_with_viewer_lines(line_count: usize, viewport: usize) -> App {
        let mut app = App::new();
        app.viewer_lines_cache = vec![ratatui::text::Line::default(); line_count];
        app.viewer_viewport_height = viewport;
        app
    }

    // ========== session_natural_bottom ==========

    /// Natural bottom with more lines than viewport
    #[test]
    fn session_natural_bottom_normal() {
        let app = app_with_session_lines(100, 20);
        assert_eq!(app.session_natural_bottom(), 80);
    }

    /// Natural bottom with lines exactly equal to viewport
    #[test]
    fn session_natural_bottom_exact() {
        let app = app_with_session_lines(20, 20);
        assert_eq!(app.session_natural_bottom(), 0);
    }

    /// Natural bottom with fewer lines than viewport (saturating_sub)
    #[test]
    fn session_natural_bottom_fewer_lines() {
        let app = app_with_session_lines(5, 20);
        assert_eq!(app.session_natural_bottom(), 0);
    }

    /// Natural bottom with zero lines
    #[test]
    fn session_natural_bottom_zero_lines() {
        let app = app_with_session_lines(0, 20);
        assert_eq!(app.session_natural_bottom(), 0);
    }

    /// Natural bottom with zero viewport
    #[test]
    fn session_natural_bottom_zero_viewport() {
        let app = app_with_session_lines(100, 0);
        assert_eq!(app.session_natural_bottom(), 100);
    }

    /// Natural bottom with single line
    #[test]
    fn session_natural_bottom_single_line() {
        let app = app_with_session_lines(1, 20);
        assert_eq!(app.session_natural_bottom(), 0);
    }

    // ========== session_max_scroll ==========

    /// Max scroll with many lines
    #[test]
    fn session_max_scroll_normal() {
        let app = app_with_session_lines(100, 20);
        assert_eq!(app.session_max_scroll(), 99);
    }

    /// Max scroll with single line
    #[test]
    fn session_max_scroll_single() {
        let app = app_with_session_lines(1, 20);
        assert_eq!(app.session_max_scroll(), 0);
    }

    /// Max scroll with zero lines
    #[test]
    fn session_max_scroll_zero() {
        let app = app_with_session_lines(0, 20);
        assert_eq!(app.session_max_scroll(), 0);
    }

    // ========== clamp_session_scroll ==========

    /// Clamp resolves usize::MAX sentinel to natural bottom
    #[test]
    fn clamp_session_sentinel() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = usize::MAX;
        app.clamp_session_scroll();
        assert_eq!(app.session_scroll, 80);
    }

    /// Clamp manual scroll within range stays unchanged
    #[test]
    fn clamp_session_within_range() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 50;
        app.clamp_session_scroll();
        assert_eq!(app.session_scroll, 50);
    }

    /// Clamp manual scroll beyond max gets clamped
    #[test]
    fn clamp_session_beyond_max() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 200;
        app.clamp_session_scroll();
        assert_eq!(app.session_scroll, 99);
    }

    /// Clamp at zero stays zero
    #[test]
    fn clamp_session_at_zero() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 0;
        app.clamp_session_scroll();
        assert_eq!(app.session_scroll, 0);
    }

    /// Clamp with zero lines, sentinel resolves to 0
    #[test]
    fn clamp_session_empty_sentinel() {
        let mut app = app_with_session_lines(0, 20);
        app.session_scroll = usize::MAX;
        app.clamp_session_scroll();
        assert_eq!(app.session_scroll, 0);
    }

    // ========== scroll_session_down ==========

    /// Scroll down by 1 from position 0
    #[test]
    fn session_down_one() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 0;
        let changed = app.scroll_session_down(1);
        assert!(changed);
        assert_eq!(app.session_scroll, 1);
    }

    /// Scroll down from sentinel first resolves to natural bottom, then re-engages
    #[test]
    fn session_down_from_sentinel() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = usize::MAX;
        let changed = app.scroll_session_down(1);
        // old was stored AFTER resolving sentinel to natural_bottom (80).
        // 80 + 1 = 81, min(99) = 81. 81 >= natural_bottom(80) => re-engage sentinel.
        // Final = usize::MAX. old was 80. usize::MAX != 80 => changed = true.
        assert_eq!(app.session_scroll, usize::MAX);
        assert!(changed);
    }

    /// Scroll down past max gets clamped to max
    #[test]
    fn session_down_past_max() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 95;
        app.scroll_session_down(100);
        // Goes to max_scroll (99), which >= natural_bottom (80), re-engages sentinel
        assert_eq!(app.session_scroll, usize::MAX);
    }

    /// Scroll down by 0 does not change
    #[test]
    fn session_down_zero() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 50;
        let changed = app.scroll_session_down(0);
        assert!(!changed);
        assert_eq!(app.session_scroll, 50);
    }

    /// Scroll down reaching natural bottom re-engages sentinel
    #[test]
    fn session_down_reengage_sentinel() {
        let mut app = app_with_session_lines(100, 20);
        // natural_bottom = 80
        app.session_scroll = 79;
        app.scroll_session_down(1);
        // 79 + 1 = 80 >= natural_bottom, re-engage
        assert_eq!(app.session_scroll, usize::MAX);
    }

    /// Scroll down returns true when position changed
    #[test]
    fn session_down_returns_true() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 10;
        assert!(app.scroll_session_down(5));
    }

    /// Scroll down by large amount with small content
    #[test]
    fn session_down_large_scroll_small_content() {
        let mut app = app_with_session_lines(5, 20);
        app.session_scroll = 0;
        app.scroll_session_down(1000);
        // max_scroll = 4, natural_bottom = 0, so 4 >= 0 => sentinel
        assert_eq!(app.session_scroll, usize::MAX);
    }

    // ========== scroll_session_up ==========

    /// Scroll up by 1 from position 50
    #[test]
    fn session_up_one() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 50;
        let changed = app.scroll_session_up(1);
        assert!(changed);
        assert_eq!(app.session_scroll, 49);
    }

    /// Scroll up from sentinel resolves to natural bottom first
    #[test]
    fn session_up_from_sentinel() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = usize::MAX;
        let changed = app.scroll_session_up(1);
        assert!(changed);
        // natural_bottom = 80, up by 1 = 79
        assert_eq!(app.session_scroll, 79);
    }

    /// Scroll up past 0 saturates at 0
    #[test]
    fn session_up_past_zero() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 3;
        app.scroll_session_up(100);
        assert_eq!(app.session_scroll, 0);
    }

    /// Scroll up by 0 does not change
    #[test]
    fn session_up_zero() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 50;
        let changed = app.scroll_session_up(0);
        assert!(!changed);
        assert_eq!(app.session_scroll, 50);
    }

    /// Scroll up returns false when already at 0
    #[test]
    fn session_up_already_at_zero() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 0;
        let changed = app.scroll_session_up(5);
        assert!(!changed);
    }

    /// Scroll up to 0 triggers rendered_lines_dirty if deferred events
    #[test]
    fn session_up_to_zero_dirty_flag() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 5;
        app.rendered_events_start = 10; // simulate deferred events
        app.rendered_lines_dirty = false;
        app.scroll_session_up(100);
        assert_eq!(app.session_scroll, 0);
        assert!(app.rendered_lines_dirty);
    }

    /// Scroll up to 0 does NOT set dirty if no deferred events
    #[test]
    fn session_up_to_zero_no_dirty_without_deferred() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 5;
        app.rendered_events_start = 0;
        app.rendered_lines_dirty = false;
        app.scroll_session_up(100);
        assert_eq!(app.session_scroll, 0);
        assert!(!app.rendered_lines_dirty);
    }

    // ========== scroll_session_to_bottom ==========

    /// To bottom sets sentinel
    #[test]
    fn session_to_bottom() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 0;
        app.scroll_session_to_bottom();
        assert_eq!(app.session_scroll, usize::MAX);
    }

    /// To bottom from any position
    #[test]
    fn session_to_bottom_from_mid() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 42;
        app.scroll_session_to_bottom();
        assert_eq!(app.session_scroll, usize::MAX);
    }

    // ========== viewer_natural_bottom ==========

    /// Viewer natural bottom with more lines than viewport
    #[test]
    fn viewer_natural_bottom_normal() {
        let app = app_with_viewer_lines(100, 20);
        assert_eq!(app.viewer_natural_bottom(), 80);
    }

    /// Viewer natural bottom with fewer lines than viewport
    #[test]
    fn viewer_natural_bottom_fewer_lines() {
        let app = app_with_viewer_lines(5, 20);
        assert_eq!(app.viewer_natural_bottom(), 0);
    }

    /// Viewer natural bottom with zero lines
    #[test]
    fn viewer_natural_bottom_zero() {
        let app = app_with_viewer_lines(0, 20);
        assert_eq!(app.viewer_natural_bottom(), 0);
    }

    // ========== viewer_max_scroll ==========

    /// Viewer max scroll with many lines
    #[test]
    fn viewer_max_scroll_normal() {
        let app = app_with_viewer_lines(100, 20);
        assert_eq!(app.viewer_max_scroll(), 99);
    }

    /// Viewer max scroll with zero lines
    #[test]
    fn viewer_max_scroll_zero() {
        let app = app_with_viewer_lines(0, 20);
        assert_eq!(app.viewer_max_scroll(), 0);
    }

    // ========== clamp_viewer_scroll ==========

    /// Clamp viewer sentinel resolves to natural bottom
    #[test]
    fn clamp_viewer_sentinel() {
        let mut app = app_with_viewer_lines(100, 20);
        app.viewer_scroll = usize::MAX;
        app.clamp_viewer_scroll();
        assert_eq!(app.viewer_scroll, 80);
    }

    /// Clamp viewer within range unchanged
    #[test]
    fn clamp_viewer_within_range() {
        let mut app = app_with_viewer_lines(100, 20);
        app.viewer_scroll = 50;
        app.clamp_viewer_scroll();
        assert_eq!(app.viewer_scroll, 50);
    }

    /// Clamp viewer beyond max gets clamped
    #[test]
    fn clamp_viewer_beyond_max() {
        let mut app = app_with_viewer_lines(100, 20);
        app.viewer_scroll = 200;
        app.clamp_viewer_scroll();
        assert_eq!(app.viewer_scroll, 99);
    }

    /// Clamp viewer at zero
    #[test]
    fn clamp_viewer_at_zero() {
        let mut app = app_with_viewer_lines(100, 20);
        app.viewer_scroll = 0;
        app.clamp_viewer_scroll();
        assert_eq!(app.viewer_scroll, 0);
    }

    // ========== scroll_viewer_down ==========

    /// Viewer scroll down from 0
    #[test]
    fn viewer_down_one() {
        let mut app = app_with_viewer_lines(100, 20);
        app.viewer_scroll = 0;
        let changed = app.scroll_viewer_down(1);
        assert!(changed);
        assert_eq!(app.viewer_scroll, 1);
    }

    /// Viewer scroll down from sentinel resolves first
    #[test]
    fn viewer_down_from_sentinel() {
        let mut app = app_with_viewer_lines(100, 20);
        app.viewer_scroll = usize::MAX;
        let changed = app.scroll_viewer_down(1);
        // natural_bottom = 80, +1 = 81, min(99) = 81
        assert!(changed);
        assert_eq!(app.viewer_scroll, 81);
    }

    /// Viewer scroll down past max clamped
    #[test]
    fn viewer_down_past_max() {
        let mut app = app_with_viewer_lines(100, 20);
        app.viewer_scroll = 95;
        app.scroll_viewer_down(100);
        assert_eq!(app.viewer_scroll, 99);
    }

    /// Viewer scroll down by 0
    #[test]
    fn viewer_down_zero() {
        let mut app = app_with_viewer_lines(100, 20);
        app.viewer_scroll = 50;
        let changed = app.scroll_viewer_down(0);
        assert!(!changed);
    }

    /// Viewer scroll down at max returns false
    #[test]
    fn viewer_down_at_max_no_change() {
        let mut app = app_with_viewer_lines(100, 20);
        app.viewer_scroll = 99;
        let changed = app.scroll_viewer_down(5);
        assert!(!changed);
    }

    // ========== scroll_viewer_up ==========

    /// Viewer scroll up from 50
    #[test]
    fn viewer_up_one() {
        let mut app = app_with_viewer_lines(100, 20);
        app.viewer_scroll = 50;
        let changed = app.scroll_viewer_up(1);
        assert!(changed);
        assert_eq!(app.viewer_scroll, 49);
    }

    /// Viewer scroll up from sentinel
    #[test]
    fn viewer_up_from_sentinel() {
        let mut app = app_with_viewer_lines(100, 20);
        app.viewer_scroll = usize::MAX;
        let changed = app.scroll_viewer_up(1);
        assert!(changed);
        assert_eq!(app.viewer_scroll, 79);
    }

    /// Viewer scroll up past zero saturates
    #[test]
    fn viewer_up_past_zero() {
        let mut app = app_with_viewer_lines(100, 20);
        app.viewer_scroll = 3;
        app.scroll_viewer_up(100);
        assert_eq!(app.viewer_scroll, 0);
    }

    /// Viewer scroll up by 0
    #[test]
    fn viewer_up_zero() {
        let mut app = app_with_viewer_lines(100, 20);
        app.viewer_scroll = 50;
        let changed = app.scroll_viewer_up(0);
        assert!(!changed);
    }

    /// Viewer scroll up already at 0
    #[test]
    fn viewer_up_at_zero() {
        let mut app = app_with_viewer_lines(100, 20);
        app.viewer_scroll = 0;
        let changed = app.scroll_viewer_up(5);
        assert!(!changed);
    }

    // ========== scroll_viewer_to_bottom ==========

    /// Viewer to bottom sets sentinel
    #[test]
    fn viewer_to_bottom() {
        let mut app = app_with_viewer_lines(100, 20);
        app.viewer_scroll = 0;
        app.scroll_viewer_to_bottom();
        assert_eq!(app.viewer_scroll, usize::MAX);
    }

    // ========== jump_to_next_bubble ==========

    /// Next bubble with user-only filter
    #[test]
    fn next_bubble_user_only() {
        let mut app = app_with_session_lines(200, 20);
        app.message_bubble_positions = vec![
            (10, true),   // user at line 10
            (30, false),  // assistant at line 30
            (50, true),   // user at line 50
        ];
        app.session_scroll = 0;
        app.jump_to_next_bubble(false); // user only
        // First user bubble at line 10, display_pos = 10-2 = 8
        assert_eq!(app.session_scroll, 8);
    }

    /// Next bubble including assistant
    #[test]
    fn next_bubble_include_assistant() {
        let mut app = app_with_session_lines(200, 20);
        app.message_bubble_positions = vec![
            (10, true),
            (30, false),
            (50, true),
        ];
        app.session_scroll = 9; // past first bubble
        app.jump_to_next_bubble(true);
        // Next bubble is assistant at line 30, display = 28
        assert_eq!(app.session_scroll, 28);
    }

    /// Next bubble when no more bubbles re-engages sentinel
    #[test]
    fn next_bubble_no_more() {
        let mut app = app_with_session_lines(200, 20);
        app.message_bubble_positions = vec![(10, true)];
        app.session_scroll = 50; // past all bubbles
        app.jump_to_next_bubble(true);
        assert_eq!(app.session_scroll, usize::MAX);
    }

    /// Next bubble from sentinel resolves first
    #[test]
    fn next_bubble_from_sentinel() {
        let mut app = app_with_session_lines(200, 20);
        app.message_bubble_positions = vec![(10, true)];
        app.session_scroll = usize::MAX;
        app.jump_to_next_bubble(true);
        // natural_bottom = 180, no bubble after 180, so sentinel
        assert_eq!(app.session_scroll, usize::MAX);
    }

    /// Next bubble with empty bubble list re-engages sentinel
    #[test]
    fn next_bubble_empty_list() {
        let mut app = app_with_session_lines(200, 20);
        app.message_bubble_positions = vec![];
        app.session_scroll = 0;
        app.jump_to_next_bubble(true);
        assert_eq!(app.session_scroll, usize::MAX);
    }

    /// Next bubble skips assistant when user-only mode
    #[test]
    fn next_bubble_skip_assistant() {
        let mut app = app_with_session_lines(200, 20);
        app.message_bubble_positions = vec![
            (10, false),  // assistant
            (50, true),   // user
        ];
        app.session_scroll = 0;
        app.jump_to_next_bubble(false);
        // Should skip assistant at 10, jump to user at 50 => display = 48
        assert_eq!(app.session_scroll, 48);
    }

    /// Next bubble clamped to session_max_scroll
    #[test]
    fn next_bubble_clamp_to_max() {
        let mut app = app_with_session_lines(50, 20);
        // Bubble display position would be 198, but max_scroll is 49
        app.message_bubble_positions = vec![(200, true)];
        app.session_scroll = 0;
        app.jump_to_next_bubble(true);
        assert!(app.session_scroll <= app.session_max_scroll());
    }

    /// Next bubble with bubble at line 0 (saturating_sub gives display 0)
    #[test]
    fn next_bubble_at_line_zero() {
        let mut app = app_with_session_lines(200, 20);
        app.message_bubble_positions = vec![(1, true)]; // display = max(1-2, 0) = 0
        app.session_scroll = 0;
        app.jump_to_next_bubble(true);
        // display_pos = 0, not > 0 (current), so no bubble found
        assert_eq!(app.session_scroll, usize::MAX);
    }

    // ========== jump_to_prev_bubble ==========

    /// Prev bubble from end
    #[test]
    fn prev_bubble_from_end() {
        let mut app = app_with_session_lines(200, 20);
        app.message_bubble_positions = vec![
            (10, true),
            (50, true),
            (90, true),
        ];
        app.session_scroll = 100;
        app.jump_to_prev_bubble(true);
        // Previous bubble at line 90, display = 88
        assert_eq!(app.session_scroll, 88);
    }

    /// Prev bubble user-only
    #[test]
    fn prev_bubble_user_only() {
        let mut app = app_with_session_lines(200, 20);
        app.message_bubble_positions = vec![
            (10, true),
            (50, false),  // assistant
            (90, true),
        ];
        app.session_scroll = 100;
        app.jump_to_prev_bubble(false);
        // Skip assistant at 50, go to user at 90, display = 88
        assert_eq!(app.session_scroll, 88);
    }

    /// Prev bubble when no previous exists, goes to 0
    #[test]
    fn prev_bubble_none_goes_to_zero() {
        let mut app = app_with_session_lines(200, 20);
        app.message_bubble_positions = vec![(50, true)];
        app.session_scroll = 0;
        app.jump_to_prev_bubble(true);
        assert_eq!(app.session_scroll, 0);
    }

    /// Prev bubble from sentinel resolves first
    #[test]
    fn prev_bubble_from_sentinel() {
        let mut app = app_with_session_lines(200, 20);
        app.message_bubble_positions = vec![(10, true), (50, true)];
        app.session_scroll = usize::MAX;
        app.jump_to_prev_bubble(true);
        // natural_bottom = 180, prev bubble before 180 => 50, display = 48
        assert_eq!(app.session_scroll, 48);
    }

    /// Prev bubble with empty list goes to 0
    #[test]
    fn prev_bubble_empty_list() {
        let mut app = app_with_session_lines(200, 20);
        app.message_bubble_positions = vec![];
        app.session_scroll = 50;
        app.jump_to_prev_bubble(true);
        assert_eq!(app.session_scroll, 0);
    }

    /// Prev bubble triggers dirty flag when deferred events exist
    #[test]
    fn prev_bubble_triggers_dirty() {
        let mut app = app_with_session_lines(200, 20);
        app.message_bubble_positions = vec![];
        app.session_scroll = 50;
        app.rendered_events_start = 10;
        app.rendered_lines_dirty = false;
        app.jump_to_prev_bubble(true);
        assert_eq!(app.session_scroll, 0);
        assert!(app.rendered_lines_dirty);
    }

    /// Prev bubble does NOT trigger dirty without deferred events
    #[test]
    fn prev_bubble_no_dirty_without_deferred() {
        let mut app = app_with_session_lines(200, 20);
        app.message_bubble_positions = vec![];
        app.session_scroll = 50;
        app.rendered_events_start = 0;
        app.rendered_lines_dirty = false;
        app.jump_to_prev_bubble(true);
        assert_eq!(app.session_scroll, 0);
        assert!(!app.rendered_lines_dirty);
    }

    // ========== Comprehensive / combined scenarios ==========

    /// Sequential down then up returns to original position
    #[test]
    fn session_down_up_roundtrip() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 30;
        app.scroll_session_down(10);
        assert_eq!(app.session_scroll, 40);
        app.scroll_session_up(10);
        assert_eq!(app.session_scroll, 30);
    }

    /// Viewer down then up roundtrip
    #[test]
    fn viewer_down_up_roundtrip() {
        let mut app = app_with_viewer_lines(100, 20);
        app.viewer_scroll = 30;
        app.scroll_viewer_down(10);
        assert_eq!(app.viewer_scroll, 40);
        app.scroll_viewer_up(10);
        assert_eq!(app.viewer_scroll, 30);
    }

    /// Scroll down multiple times accumulates correctly
    #[test]
    fn session_down_accumulates() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 0;
        app.scroll_session_down(5);
        app.scroll_session_down(3);
        app.scroll_session_down(2);
        assert_eq!(app.session_scroll, 10);
    }

    /// Scroll up multiple times accumulates correctly
    #[test]
    fn session_up_accumulates() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 50;
        app.scroll_session_up(10);
        app.scroll_session_up(5);
        assert_eq!(app.session_scroll, 35);
    }

    /// Viewer with exactly one line: down doesn't move, up doesn't move
    #[test]
    fn viewer_single_line_no_movement() {
        let mut app = app_with_viewer_lines(1, 20);
        app.viewer_scroll = 0;
        let down_changed = app.scroll_viewer_down(1);
        assert!(!down_changed);
        let up_changed = app.scroll_viewer_up(1);
        assert!(!up_changed);
    }

    /// Session with exactly viewport lines: natural bottom is 0
    #[test]
    fn session_exact_viewport_bottom_zero() {
        let app = app_with_session_lines(20, 20);
        assert_eq!(app.session_natural_bottom(), 0);
        assert_eq!(app.session_max_scroll(), 19);
    }

    /// Page-sized scroll down
    #[test]
    fn session_page_down() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 0;
        app.scroll_session_down(20); // page-size scroll
        assert_eq!(app.session_scroll, 20);
    }

    /// Page-sized scroll up
    #[test]
    fn session_page_up() {
        let mut app = app_with_session_lines(100, 20);
        app.session_scroll = 50;
        app.scroll_session_up(20);
        assert_eq!(app.session_scroll, 30);
    }

    /// Bubble navigation forward through all bubbles
    #[test]
    fn bubble_walk_forward() {
        let mut app = app_with_session_lines(200, 20);
        app.message_bubble_positions = vec![
            (10, true),
            (30, true),
            (60, true),
        ];
        app.session_scroll = 0;

        app.jump_to_next_bubble(true);
        assert_eq!(app.session_scroll, 8);

        app.jump_to_next_bubble(true);
        assert_eq!(app.session_scroll, 28);

        app.jump_to_next_bubble(true);
        assert_eq!(app.session_scroll, 58);

        app.jump_to_next_bubble(true);
        // Past all bubbles => sentinel
        assert_eq!(app.session_scroll, usize::MAX);
    }

    /// Bubble navigation backward through all bubbles
    #[test]
    fn bubble_walk_backward() {
        let mut app = app_with_session_lines(200, 20);
        app.message_bubble_positions = vec![
            (10, true),
            (30, true),
            (60, true),
        ];
        app.session_scroll = 100;

        app.jump_to_prev_bubble(true);
        assert_eq!(app.session_scroll, 58);

        app.jump_to_prev_bubble(true);
        assert_eq!(app.session_scroll, 28);

        app.jump_to_prev_bubble(true);
        assert_eq!(app.session_scroll, 8);

        app.jump_to_prev_bubble(true);
        assert_eq!(app.session_scroll, 0);
    }
}
