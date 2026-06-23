//! Render cache invalidation and file tree refresh helpers.

use super::App;
use crate::events::DisplayEvent;

/// Cache invalidation and file tree refresh methods for application state.
impl App {
    /// Mark rendered lines cache as dirty after display events change.
    pub fn invalidate_render_cache(&mut self) {
        self.rendered_lines_dirty = true;
    }

    /// Replace session display events and force the renderer to rebuild from
    /// the first event instead of treating the new array as an append-only tail.
    pub(crate) fn replace_display_events_for_render(&mut self, events: Vec<DisplayEvent>) {
        self.display_events = events;
        self.invalidate_render_cache_from_start();
    }

    /// Reset render bookkeeping after `display_events` has been replaced or
    /// reordered, while keeping the old rendered lines visible until a fresh
    /// full render result arrives.
    pub(crate) fn invalidate_render_cache_from_start(&mut self) {
        self.rendered_lines_dirty = true;
        self.rendered_events_count = 0;
        self.rendered_content_line_count = 0;
        self.rendered_events_start = 0;
        self.render_in_flight = false;
        self.render_seq_applied = self.render_thread.current_seq();
        self.session_viewport_scroll = usize::MAX;
    }

    /// Mark sidebar cache as dirty after worktree or selection changes.
    pub fn invalidate_sidebar(&mut self) {
        // Sidebar replaced by worktree tab row; no cache remains to invalidate.
    }

    /// Mark file tree cache as dirty so the next draw rebuilds it.
    pub fn invalidate_file_tree(&mut self) {
        self.file_tree_dirty = true;
    }

    /// Rebuild file tree entries from disk while preserving expansion state.
    pub fn refresh_file_tree(&mut self) {
        let Some(wt) = self.current_worktree() else {
            return;
        };
        let Some(ref worktree_path) = wt.worktree_path else {
            return;
        };
        let wt_path = worktree_path.clone();
        self.file_tree_entries = crate::app::state::helpers::build_file_tree(
            &wt_path,
            &self.file_tree_expanded,
            &self.file_tree_hidden_dirs,
        );
        if self
            .file_tree_selected
            .map_or(true, |i| i >= self.file_tree_entries.len())
        {
            self.file_tree_selected = if self.file_tree_entries.is_empty() {
                None
            } else {
                Some(0)
            };
        }
        self.invalidate_file_tree();
    }
}

#[cfg(test)]
/// Tests for render cache replacement and invalidation helpers.
mod tests {
    use super::*;
    use ratatui::text::Line;

    /// Build a minimal assistant text event for render-cache replacement tests.
    fn assistant_text(text: &str) -> DisplayEvent {
        DisplayEvent::AssistantText {
            _uuid: String::new(),
            _message_id: String::new(),
            text: text.to_string(),
        }
    }

    /// Replacing display events resets incremental render counters but leaves
    /// the old line cache visible until the full replacement render completes.
    #[test]
    fn replace_display_events_for_render_resets_incremental_state() {
        let mut app = App::new();
        app.display_events = vec![assistant_text("old")];
        app.rendered_lines_cache = vec![Line::from("old rendered line")];
        app.rendered_lines_dirty = false;
        app.rendered_events_count = 12;
        app.rendered_content_line_count = 34;
        app.rendered_events_start = 5;
        app.render_in_flight = true;
        app.session_viewport_scroll = 7;

        app.replace_display_events_for_render(vec![assistant_text("new tail")]);

        assert!(app.rendered_lines_dirty);
        assert_eq!(app.rendered_events_count, 0);
        assert_eq!(app.rendered_content_line_count, 0);
        assert_eq!(app.rendered_events_start, 0);
        assert!(!app.render_in_flight);
        assert_eq!(app.session_viewport_scroll, usize::MAX);
        assert_eq!(app.rendered_lines_cache.len(), 1);
        assert!(matches!(
            &app.display_events[..],
            [DisplayEvent::AssistantText { text, .. }] if text == "new tail"
        ));
    }
}
