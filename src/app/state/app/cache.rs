//! Render cache invalidation and file tree refresh helpers.

use super::App;

/// Cache invalidation and file tree refresh methods for application state.
impl App {
    /// Mark rendered lines cache as dirty after display events change.
    pub fn invalidate_render_cache(&mut self) {
        self.rendered_lines_dirty = true;
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
