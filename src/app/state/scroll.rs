//! Scroll operations for output, diff, and viewer panes

use super::App;

impl App {
    pub fn scroll_output_down(&mut self, lines: usize, _viewport_height: usize) {
        self.output_scroll = self.output_scroll.saturating_add(lines);
    }

    pub fn scroll_output_up(&mut self, lines: usize) {
        self.output_scroll = self.output_scroll.saturating_sub(lines);
    }

    pub fn scroll_output_to_bottom(&mut self, _viewport_height: usize) {
        self.output_scroll = usize::MAX;
    }

    pub fn scroll_diff_down(&mut self, lines: usize, viewport_height: usize) {
        if let Some(ref diff) = self.diff_text {
            let total_lines = diff.lines().count();
            let max_scroll = total_lines.saturating_sub(viewport_height);
            self.diff_scroll = self.diff_scroll.saturating_add(lines).min(max_scroll);
        }
    }

    pub fn scroll_diff_up(&mut self, lines: usize) {
        self.diff_scroll = self.diff_scroll.saturating_sub(lines);
    }

    pub fn scroll_diff_to_bottom(&mut self, viewport_height: usize) {
        if let Some(ref diff) = self.diff_text {
            self.diff_scroll = diff.lines().count().saturating_sub(viewport_height);
        }
    }

    /// Scroll viewer down
    pub fn scroll_viewer_down(&mut self, lines: usize, viewport_height: usize) {
        if let Some(ref content) = self.viewer_content {
            let total_lines = content.lines().count();
            let max_scroll = total_lines.saturating_sub(viewport_height);
            self.viewer_scroll = self.viewer_scroll.saturating_add(lines).min(max_scroll);
        }
    }

    /// Scroll viewer up
    pub fn scroll_viewer_up(&mut self, lines: usize) {
        self.viewer_scroll = self.viewer_scroll.saturating_sub(lines);
    }
}
