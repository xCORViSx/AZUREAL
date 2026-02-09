//! File tree navigation and viewer operations

use crate::app::types::ViewerMode;

use super::helpers::build_file_tree;
use super::App;

impl App {
    /// Toggle expand/collapse of a directory in the file tree
    pub fn toggle_file_tree_dir(&mut self) {
        let Some(idx) = self.file_tree_selected else { return };
        let Some(entry) = self.file_tree_entries.get(idx) else { return };
        if !entry.is_dir { return; }

        // Remember the selected path before rebuilding
        let selected_path = entry.path.clone();

        if self.file_tree_expanded.contains(&selected_path) {
            self.file_tree_expanded.remove(&selected_path);
        } else {
            self.file_tree_expanded.insert(selected_path.clone());
        }

        // Rebuild tree and restore selection to same path
        let Some(session) = self.current_session() else { return };
        let Some(ref worktree_path) = session.worktree_path else { return };

        self.file_tree_entries = build_file_tree(worktree_path, &self.file_tree_expanded);
        self.file_tree_selected = self.file_tree_entries
            .iter()
            .position(|e| e.path == selected_path)
            .or(Some(0));
        self.invalidate_file_tree();
    }

    /// Select next file tree entry
    pub fn file_tree_next(&mut self) {
        if let Some(idx) = self.file_tree_selected {
            if idx + 1 < self.file_tree_entries.len() {
                self.file_tree_selected = Some(idx + 1);
                self.invalidate_file_tree();
            }
        } else if !self.file_tree_entries.is_empty() {
            self.file_tree_selected = Some(0);
            self.invalidate_file_tree();
        }
    }

    /// Select previous file tree entry
    pub fn file_tree_prev(&mut self) {
        if let Some(idx) = self.file_tree_selected {
            if idx > 0 {
                self.file_tree_selected = Some(idx - 1);
                self.invalidate_file_tree();
            }
        }
    }

    /// Jump to first sibling in the same parent folder as the current selection.
    /// "Siblings" = entries at the same depth whose parent path matches.
    pub fn file_tree_first_sibling(&mut self) {
        let Some(idx) = self.file_tree_selected else { return };
        let Some(entry) = self.file_tree_entries.get(idx) else { return };
        let depth = entry.depth;
        let parent = entry.path.parent().map(|p| p.to_path_buf());
        // Walk backwards to find first entry with same parent at same depth
        let first = (0..=idx).rev()
            .take_while(|&i| {
                let e = &self.file_tree_entries[i];
                e.depth >= depth
            })
            .filter(|&i| {
                let e = &self.file_tree_entries[i];
                e.depth == depth && e.path.parent().map(|p| p.to_path_buf()) == parent
            })
            .last()
            .unwrap_or(idx);
        if first != idx {
            self.file_tree_selected = Some(first);
            self.invalidate_file_tree();
        }
    }

    /// Jump to last sibling in the same parent folder as the current selection.
    pub fn file_tree_last_sibling(&mut self) {
        let Some(idx) = self.file_tree_selected else { return };
        let Some(entry) = self.file_tree_entries.get(idx) else { return };
        let depth = entry.depth;
        let parent = entry.path.parent().map(|p| p.to_path_buf());
        // Walk forward to find last entry with same parent at same depth
        let last = (idx..self.file_tree_entries.len())
            .take_while(|&i| {
                // Stop when we hit an entry at a shallower depth (left the folder)
                i == idx || self.file_tree_entries[i].depth >= depth
            })
            .filter(|&i| {
                let e = &self.file_tree_entries[i];
                e.depth == depth && e.path.parent().map(|p| p.to_path_buf()) == parent
            })
            .last()
            .unwrap_or(idx);
        if last != idx {
            self.file_tree_selected = Some(last);
            self.invalidate_file_tree();
        }
    }

    /// Load selected file into viewer
    pub fn load_file_into_viewer(&mut self) {
        let Some(idx) = self.file_tree_selected else { return };
        let Some(entry) = self.file_tree_entries.get(idx) else { return };
        if entry.is_dir { return; }

        match std::fs::read_to_string(&entry.path) {
            Ok(content) => {
                self.viewer_content = Some(content);
                self.viewer_path = Some(entry.path.clone());
                self.viewer_mode = ViewerMode::File;
                self.viewer_scroll = 0;
                self.viewer_lines_dirty = true;
            }
            Err(e) => {
                self.viewer_content = Some(format!("Error reading file: {}", e));
                self.viewer_path = Some(entry.path.clone());
                self.viewer_mode = ViewerMode::File;
                self.viewer_scroll = 0;
                self.viewer_lines_dirty = true;
            }
        }
    }

    /// Clear viewer content
    pub fn clear_viewer(&mut self) {
        self.viewer_content = None;
        self.viewer_path = None;
        self.viewer_mode = ViewerMode::Empty;
        self.viewer_scroll = 0;
        self.viewer_lines_dirty = true;
    }
}
