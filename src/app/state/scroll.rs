//! Scroll operations for output, diff, and viewer panes

use super::App;

impl App {
    /// Scroll output down, returns true if position changed
    pub fn scroll_output_down(&mut self, lines: usize, _viewport_height: usize) -> bool {
        let old = self.output_scroll;
        self.output_scroll = self.output_scroll.saturating_add(lines);
        self.output_scroll != old
    }

    /// Scroll output up, returns true if position changed
    pub fn scroll_output_up(&mut self, lines: usize) -> bool {
        let old = self.output_scroll;
        self.output_scroll = self.output_scroll.saturating_sub(lines);
        self.output_scroll != old
    }

    pub fn scroll_output_to_bottom(&mut self, _viewport_height: usize) {
        self.output_scroll = usize::MAX;
    }

    /// Scroll diff down, returns true if position changed
    pub fn scroll_diff_down(&mut self, lines: usize, viewport_height: usize) -> bool {
        let old = self.diff_scroll;
        if let Some(ref diff) = self.diff_text {
            let total_lines = diff.lines().count();
            let max_scroll = total_lines.saturating_sub(viewport_height);
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

    pub fn scroll_diff_to_bottom(&mut self, viewport_height: usize) {
        if let Some(ref diff) = self.diff_text {
            self.diff_scroll = diff.lines().count().saturating_sub(viewport_height);
        }
    }

    /// Scroll viewer down, returns true if position changed
    pub fn scroll_viewer_down(&mut self, lines: usize, viewport_height: usize) -> bool {
        let old = self.viewer_scroll;
        if let Some(ref content) = self.viewer_content {
            let total_lines = content.lines().count();
            let max_scroll = total_lines.saturating_sub(viewport_height);
            self.viewer_scroll = self.viewer_scroll.saturating_add(lines).min(max_scroll);
        }
        self.viewer_scroll != old
    }

    /// Scroll viewer up, returns true if position changed
    pub fn scroll_viewer_up(&mut self, lines: usize) -> bool {
        let old = self.viewer_scroll;
        self.viewer_scroll = self.viewer_scroll.saturating_sub(lines);
        self.viewer_scroll != old
    }
}
