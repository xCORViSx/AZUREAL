//! File tree navigation and viewer operations

use std::path::Path;
use crate::app::types::ViewerMode;

use super::helpers::build_file_tree;
use super::App;

/// Recursively copy a directory and all contents
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

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

        self.file_tree_entries = build_file_tree(worktree_path, &self.file_tree_expanded, &self.file_tree_hidden_dirs);
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

    /// Check if a file extension indicates an image format
    fn is_image_extension(path: &std::path::Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
            Some("png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico")
        )
    }

    /// Load selected file into viewer (from FileTree selection)
    pub fn load_file_into_viewer(&mut self) {
        let Some(idx) = self.file_tree_selected else { return };
        let Some(entry) = self.file_tree_entries.get(idx) else { return };
        if entry.is_dir { return; }
        let path = entry.path.clone();
        self.load_file_by_path(&path);
    }

    /// Load a file by path into the viewer pane. Handles both image files
    /// (decoded via terminal graphics protocol) and text files (syntax-highlighted).
    /// Called directly or via deferred action after a loading indicator renders.
    pub fn load_file_by_path(&mut self, path: &std::path::Path) {
        if self.viewer_edit_mode {
            self.exit_viewer_edit_mode();
        }
        let path = path.to_path_buf();

        // Image files get decoded and rendered via terminal graphics protocol
        if Self::is_image_extension(&path) {
            match std::fs::read(&path) {
                Ok(bytes) => match image::load_from_memory(&bytes) {
                    Ok(dyn_img) => {
                        // Lazy-init the picker (detects terminal graphics capabilities once)
                        if self.image_picker.is_none() {
                            self.image_picker = ratatui_image::picker::Picker::from_query_stdio().ok();
                        }
                        if let Some(ref picker) = self.image_picker {
                            self.viewer_image_state = Some(picker.new_resize_protocol(dyn_img));
                            self.viewer_content = None;
                            self.viewer_path = Some(path);
                            self.viewer_mode = ViewerMode::Image;
                            self.viewer_scroll = 0;
                            self.viewer_lines_cache.clear();
                            self.viewer_lines_dirty = false;
                            return;
                        }
                        self.viewer_content = Some("Error: terminal does not support image rendering".into());
                        self.viewer_path = Some(path);
                        self.viewer_mode = ViewerMode::File;
                        self.viewer_scroll = 0;
                        self.viewer_lines_dirty = true;
                    }
                    Err(e) => {
                        self.viewer_content = Some(format!("Error decoding image: {}", e));
                        self.viewer_path = Some(path);
                        self.viewer_mode = ViewerMode::File;
                        self.viewer_scroll = 0;
                        self.viewer_lines_dirty = true;
                    }
                },
                Err(e) => {
                    self.viewer_content = Some(format!("Error reading file: {}", e));
                    self.viewer_path = Some(path);
                    self.viewer_mode = ViewerMode::File;
                    self.viewer_scroll = 0;
                    self.viewer_lines_dirty = true;
                }
            }
            return;
        }

        // Text files — read as string and syntax-highlight
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                self.viewer_content = Some(content);
                self.viewer_path = Some(path);
                self.viewer_mode = ViewerMode::File;
                self.viewer_scroll = 0;
                self.viewer_lines_dirty = true;
                self.viewer_image_state = None;
            }
            Err(e) => {
                self.viewer_content = Some(format!("Error reading file: {}", e));
                self.viewer_path = Some(path);
                self.viewer_mode = ViewerMode::File;
                self.viewer_scroll = 0;
                self.viewer_lines_dirty = true;
                self.viewer_image_state = None;
            }
        }
    }

    /// Execute a file add action. Name ending with '/' creates a directory.
    /// Created in the selected entry's parent (if file) or inside it (if dir).
    pub fn file_tree_exec_add(&mut self, name: &str) {
        let Some(idx) = self.file_tree_selected else { return };
        let Some(entry) = self.file_tree_entries.get(idx) else { return };
        // Add inside directory if selected, or alongside file in its parent
        let parent = if entry.is_dir { entry.path.clone() } else {
            entry.path.parent().unwrap_or(&entry.path).to_path_buf()
        };
        let target = parent.join(name.trim_end_matches('/'));
        if name.ends_with('/') {
            if let Err(e) = std::fs::create_dir_all(&target) {
                self.set_status(format!("mkdir failed: {}", e)); return;
            }
        } else {
            // Create parent dirs if needed, then empty file
            if let Some(p) = target.parent() { let _ = std::fs::create_dir_all(p); }
            if let Err(e) = std::fs::File::create(&target) {
                self.set_status(format!("create failed: {}", e)); return;
            }
        }
        self.set_status(format!("Created {}", target.display()));
        self.file_tree_refresh_after_action(&target);
    }

    /// Execute a file rename action
    pub fn file_tree_exec_rename(&mut self, new_name: &str) {
        let Some(idx) = self.file_tree_selected else { return };
        let Some(entry) = self.file_tree_entries.get(idx) else { return };
        let Some(parent) = entry.path.parent() else { return };
        let target = parent.join(new_name);
        if target.exists() {
            self.set_status(format!("Already exists: {}", target.display())); return;
        }
        if let Err(e) = std::fs::rename(&entry.path, &target) {
            self.set_status(format!("Rename failed: {}", e)); return;
        }
        self.set_status(format!("Renamed → {}", new_name));
        self.file_tree_refresh_after_action(&target);
    }

    /// Execute a file delete action
    pub fn file_tree_exec_delete(&mut self) {
        let Some(idx) = self.file_tree_selected else { return };
        let Some(entry) = self.file_tree_entries.get(idx) else { return };
        let path = entry.path.clone();
        let is_dir = entry.is_dir;
        let result = if is_dir {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if let Err(e) = result {
            self.set_status(format!("Delete failed: {}", e)); return;
        }
        self.set_status(format!("Deleted {}", path.file_name().unwrap_or_default().to_string_lossy()));
        // Select previous entry after deletion
        let select_path = if idx > 0 {
            self.file_tree_entries.get(idx - 1).map(|e| e.path.clone())
        } else { None };
        self.file_tree_refresh_after_action(&select_path.unwrap_or(path));
    }

    /// Execute a clipboard-style copy: copy source into target directory
    pub fn file_tree_exec_copy_to(&mut self, src: &std::path::Path, target_dir: &std::path::Path) {
        let Some(name) = src.file_name() else { return };
        let target = target_dir.join(name);
        if target.exists() {
            self.set_status(format!("Already exists: {}", target.display())); return;
        }
        let is_dir = src.is_dir();
        let result = if is_dir { copy_dir_recursive(src, &target) }
            else { std::fs::copy(src, &target).map(|_| ()) };
        if let Err(e) = result {
            self.set_status(format!("Copy failed: {}", e)); return;
        }
        self.set_status(format!("Copied → {}", target_dir.display()));
        // Expand target dir so the pasted file is visible in the tree
        self.file_tree_expanded.insert(target_dir.to_path_buf());
        self.file_tree_refresh_after_action(&target);
    }

    /// Execute a clipboard-style move: move source into target directory
    pub fn file_tree_exec_move_to(&mut self, src: &std::path::Path, target_dir: &std::path::Path) {
        let Some(name) = src.file_name() else { return };
        let target = target_dir.join(name);
        if target.exists() {
            self.set_status(format!("Already exists: {}", target.display())); return;
        }
        if let Err(e) = std::fs::rename(src, &target) {
            self.set_status(format!("Move failed: {}", e)); return;
        }
        self.set_status(format!("Moved → {}", target_dir.display()));
        // Expand target dir so the moved file is visible in the tree
        self.file_tree_expanded.insert(target_dir.to_path_buf());
        self.file_tree_refresh_after_action(&target);
    }

    /// Rebuild file tree after a file action, selecting the target path
    fn file_tree_refresh_after_action(&mut self, select_path: &std::path::Path) {
        let Some(session) = self.current_session() else { return };
        let Some(ref worktree_path) = session.worktree_path else { return };
        let select_target = select_path.to_path_buf();
        self.file_tree_entries = build_file_tree(worktree_path, &self.file_tree_expanded, &self.file_tree_hidden_dirs);
        self.file_tree_selected = self.file_tree_entries
            .iter().position(|e| e.path == select_target)
            .or_else(|| if self.file_tree_entries.is_empty() { None } else { Some(0) });
        self.invalidate_file_tree();
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
