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

        // Discard any in-flight background scan — this manual rebuild takes priority
        self.file_tree_receiver = None;

        // Remember the selected path before rebuilding
        let selected_path = entry.path.clone();

        if self.file_tree_expanded.contains(&selected_path) {
            self.file_tree_expanded.remove(&selected_path);
        } else {
            self.file_tree_expanded.insert(selected_path.clone());
        }

        // Rebuild tree and restore selection to same path
        let Some(session) = self.current_worktree() else { return };
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
        let Some(session) = self.current_worktree() else { return };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::FileTreeEntry;
    use std::path::PathBuf;
    use std::fs;
    use tempfile::TempDir;

    /// Build a minimal set of file tree entries for testing navigation
    fn make_entries() -> Vec<FileTreeEntry> {
        vec![
            FileTreeEntry { path: PathBuf::from("/root/src"), name: "src".into(), is_dir: true, depth: 0, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/root/src/main.rs"), name: "main.rs".into(), is_dir: false, depth: 1, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/root/src/lib.rs"), name: "lib.rs".into(), is_dir: false, depth: 1, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/root/docs"), name: "docs".into(), is_dir: true, depth: 0, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/root/README.md"), name: "README.md".into(), is_dir: false, depth: 0, is_hidden: false },
        ]
    }

    // ── is_image_extension ──

    #[test]
    fn test_is_image_png() {
        assert!(App::is_image_extension(std::path::Path::new("photo.png")));
    }

    #[test]
    fn test_is_image_jpg() {
        assert!(App::is_image_extension(std::path::Path::new("photo.jpg")));
    }

    #[test]
    fn test_is_image_jpeg() {
        assert!(App::is_image_extension(std::path::Path::new("photo.jpeg")));
    }

    #[test]
    fn test_is_image_gif() {
        assert!(App::is_image_extension(std::path::Path::new("anim.gif")));
    }

    #[test]
    fn test_is_image_bmp() {
        assert!(App::is_image_extension(std::path::Path::new("icon.bmp")));
    }

    #[test]
    fn test_is_image_webp() {
        assert!(App::is_image_extension(std::path::Path::new("hero.webp")));
    }

    #[test]
    fn test_is_image_ico() {
        assert!(App::is_image_extension(std::path::Path::new("favicon.ico")));
    }

    #[test]
    fn test_is_image_case_insensitive() {
        assert!(App::is_image_extension(std::path::Path::new("PHOTO.PNG")));
        assert!(App::is_image_extension(std::path::Path::new("Photo.Jpg")));
        assert!(App::is_image_extension(std::path::Path::new("ICON.GIF")));
    }

    #[test]
    fn test_is_not_image_rs() {
        assert!(!App::is_image_extension(std::path::Path::new("main.rs")));
    }

    #[test]
    fn test_is_not_image_txt() {
        assert!(!App::is_image_extension(std::path::Path::new("readme.txt")));
    }

    #[test]
    fn test_is_not_image_svg() {
        assert!(!App::is_image_extension(std::path::Path::new("logo.svg")));
    }

    #[test]
    fn test_is_not_image_no_extension() {
        assert!(!App::is_image_extension(std::path::Path::new("Makefile")));
    }

    #[test]
    fn test_is_not_image_pdf() {
        assert!(!App::is_image_extension(std::path::Path::new("doc.pdf")));
    }

    #[test]
    fn test_is_not_image_mp4() {
        assert!(!App::is_image_extension(std::path::Path::new("video.mp4")));
    }

    #[test]
    fn test_is_not_image_empty_path() {
        assert!(!App::is_image_extension(std::path::Path::new("")));
    }

    // ── copy_dir_recursive ──

    #[test]
    fn test_copy_dir_recursive_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src_dir");
        fs::create_dir(&src).unwrap();
        let dst = tmp.path().join("dst_dir");
        copy_dir_recursive(&src, &dst).unwrap();
        assert!(dst.exists());
        assert!(dst.is_dir());
    }

    #[test]
    fn test_copy_dir_recursive_with_files() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("a.txt"), "aaa").unwrap();
        fs::write(src.join("b.txt"), "bbb").unwrap();
        let dst = tmp.path().join("dst");
        copy_dir_recursive(&src, &dst).unwrap();
        assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "aaa");
        assert_eq!(fs::read_to_string(dst.join("b.txt")).unwrap(), "bbb");
    }

    #[test]
    fn test_copy_dir_recursive_nested() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("top.txt"), "top").unwrap();
        fs::write(src.join("sub/deep.txt"), "deep").unwrap();
        let dst = tmp.path().join("dst");
        copy_dir_recursive(&src, &dst).unwrap();
        assert_eq!(fs::read_to_string(dst.join("top.txt")).unwrap(), "top");
        assert_eq!(fs::read_to_string(dst.join("sub/deep.txt")).unwrap(), "deep");
    }

    #[test]
    fn test_copy_dir_recursive_preserves_content() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir(&src).unwrap();
        let large_content = "x".repeat(10000);
        fs::write(src.join("big.txt"), &large_content).unwrap();
        let dst = tmp.path().join("dst");
        copy_dir_recursive(&src, &dst).unwrap();
        assert_eq!(fs::read_to_string(dst.join("big.txt")).unwrap(), large_content);
    }

    // ── file_tree_next ──

    #[test]
    fn test_file_tree_next_from_first() {
        let mut app = App::new();
        app.file_tree_entries = make_entries();
        app.file_tree_selected = Some(0);
        app.file_tree_next();
        assert_eq!(app.file_tree_selected, Some(1));
    }

    #[test]
    fn test_file_tree_next_from_middle() {
        let mut app = App::new();
        app.file_tree_entries = make_entries();
        app.file_tree_selected = Some(2);
        app.file_tree_next();
        assert_eq!(app.file_tree_selected, Some(3));
    }

    #[test]
    fn test_file_tree_next_at_end_stays() {
        let mut app = App::new();
        app.file_tree_entries = make_entries();
        let last = app.file_tree_entries.len() - 1;
        app.file_tree_selected = Some(last);
        app.file_tree_next();
        assert_eq!(app.file_tree_selected, Some(last));
    }

    #[test]
    fn test_file_tree_next_from_none_selects_first() {
        let mut app = App::new();
        app.file_tree_entries = make_entries();
        app.file_tree_selected = None;
        app.file_tree_next();
        assert_eq!(app.file_tree_selected, Some(0));
    }

    #[test]
    fn test_file_tree_next_empty_tree_from_none() {
        let mut app = App::new();
        app.file_tree_entries = Vec::new();
        app.file_tree_selected = None;
        app.file_tree_next();
        assert_eq!(app.file_tree_selected, None);
    }

    // ── file_tree_prev ──

    #[test]
    fn test_file_tree_prev_from_last() {
        let mut app = App::new();
        app.file_tree_entries = make_entries();
        app.file_tree_selected = Some(4);
        app.file_tree_prev();
        assert_eq!(app.file_tree_selected, Some(3));
    }

    #[test]
    fn test_file_tree_prev_from_middle() {
        let mut app = App::new();
        app.file_tree_entries = make_entries();
        app.file_tree_selected = Some(2);
        app.file_tree_prev();
        assert_eq!(app.file_tree_selected, Some(1));
    }

    #[test]
    fn test_file_tree_prev_at_start_stays() {
        let mut app = App::new();
        app.file_tree_entries = make_entries();
        app.file_tree_selected = Some(0);
        app.file_tree_prev();
        assert_eq!(app.file_tree_selected, Some(0));
    }

    #[test]
    fn test_file_tree_prev_from_none_does_nothing() {
        let mut app = App::new();
        app.file_tree_entries = make_entries();
        app.file_tree_selected = None;
        app.file_tree_prev();
        assert_eq!(app.file_tree_selected, None);
    }

    // ── file_tree_first_sibling ──

    #[test]
    fn test_first_sibling_from_last_root_entry() {
        let mut app = App::new();
        app.file_tree_entries = vec![
            FileTreeEntry { path: PathBuf::from("/r/a"), name: "a".into(), is_dir: false, depth: 0, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/r/b"), name: "b".into(), is_dir: false, depth: 0, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/r/c"), name: "c".into(), is_dir: false, depth: 0, is_hidden: false },
        ];
        app.file_tree_selected = Some(2); // "c"
        app.file_tree_first_sibling();
        assert_eq!(app.file_tree_selected, Some(0)); // "a"
    }

    #[test]
    fn test_first_sibling_already_at_first() {
        let mut app = App::new();
        app.file_tree_entries = vec![
            FileTreeEntry { path: PathBuf::from("/r/a"), name: "a".into(), is_dir: false, depth: 0, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/r/b"), name: "b".into(), is_dir: false, depth: 0, is_hidden: false },
        ];
        app.file_tree_selected = Some(0);
        app.file_tree_first_sibling();
        assert_eq!(app.file_tree_selected, Some(0)); // stays
    }

    #[test]
    fn test_first_sibling_none_selected() {
        let mut app = App::new();
        app.file_tree_entries = make_entries();
        app.file_tree_selected = None;
        app.file_tree_first_sibling();
        assert_eq!(app.file_tree_selected, None);
    }

    // ── file_tree_last_sibling ──

    #[test]
    fn test_last_sibling_from_first_root_entry() {
        let mut app = App::new();
        app.file_tree_entries = vec![
            FileTreeEntry { path: PathBuf::from("/r/a"), name: "a".into(), is_dir: false, depth: 0, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/r/b"), name: "b".into(), is_dir: false, depth: 0, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/r/c"), name: "c".into(), is_dir: false, depth: 0, is_hidden: false },
        ];
        app.file_tree_selected = Some(0); // "a"
        app.file_tree_last_sibling();
        assert_eq!(app.file_tree_selected, Some(2)); // "c"
    }

    #[test]
    fn test_last_sibling_already_at_last() {
        let mut app = App::new();
        app.file_tree_entries = vec![
            FileTreeEntry { path: PathBuf::from("/r/a"), name: "a".into(), is_dir: false, depth: 0, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/r/b"), name: "b".into(), is_dir: false, depth: 0, is_hidden: false },
        ];
        app.file_tree_selected = Some(1);
        app.file_tree_last_sibling();
        assert_eq!(app.file_tree_selected, Some(1)); // stays
    }

    #[test]
    fn test_last_sibling_none_selected() {
        let mut app = App::new();
        app.file_tree_entries = make_entries();
        app.file_tree_selected = None;
        app.file_tree_last_sibling();
        assert_eq!(app.file_tree_selected, None);
    }

    // ── file_tree_first/last sibling with nested entries ──

    #[test]
    fn test_first_sibling_nested_children() {
        let mut app = App::new();
        app.file_tree_entries = vec![
            FileTreeEntry { path: PathBuf::from("/r/src"), name: "src".into(), is_dir: true, depth: 0, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/r/src/a.rs"), name: "a.rs".into(), is_dir: false, depth: 1, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/r/src/b.rs"), name: "b.rs".into(), is_dir: false, depth: 1, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/r/src/c.rs"), name: "c.rs".into(), is_dir: false, depth: 1, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/r/docs"), name: "docs".into(), is_dir: true, depth: 0, is_hidden: false },
        ];
        app.file_tree_selected = Some(3); // c.rs
        app.file_tree_first_sibling();
        assert_eq!(app.file_tree_selected, Some(1)); // a.rs
    }

    #[test]
    fn test_last_sibling_nested_children() {
        let mut app = App::new();
        app.file_tree_entries = vec![
            FileTreeEntry { path: PathBuf::from("/r/src"), name: "src".into(), is_dir: true, depth: 0, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/r/src/a.rs"), name: "a.rs".into(), is_dir: false, depth: 1, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/r/src/b.rs"), name: "b.rs".into(), is_dir: false, depth: 1, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/r/src/c.rs"), name: "c.rs".into(), is_dir: false, depth: 1, is_hidden: false },
            FileTreeEntry { path: PathBuf::from("/r/docs"), name: "docs".into(), is_dir: true, depth: 0, is_hidden: false },
        ];
        app.file_tree_selected = Some(1); // a.rs
        app.file_tree_last_sibling();
        assert_eq!(app.file_tree_selected, Some(3)); // c.rs
    }

    // ── clear_viewer ──

    #[test]
    fn test_clear_viewer_resets_content() {
        let mut app = App::new();
        app.viewer_content = Some("hello".into());
        app.viewer_path = Some(PathBuf::from("/test.rs"));
        app.viewer_mode = ViewerMode::File;
        app.viewer_scroll = 42;
        app.clear_viewer();
        assert!(app.viewer_content.is_none());
        assert!(app.viewer_path.is_none());
        assert_eq!(app.viewer_mode, ViewerMode::Empty);
        assert_eq!(app.viewer_scroll, 0);
        assert!(app.viewer_lines_dirty);
    }

    #[test]
    fn test_clear_viewer_on_already_empty() {
        let mut app = App::new();
        app.clear_viewer();
        assert!(app.viewer_content.is_none());
        assert!(app.viewer_path.is_none());
        assert_eq!(app.viewer_mode, ViewerMode::Empty);
    }

    // ── load_file_into_viewer ──

    #[test]
    fn test_load_file_into_viewer_no_selection() {
        let mut app = App::new();
        app.file_tree_entries = make_entries();
        app.file_tree_selected = None;
        app.load_file_into_viewer();
        // Should do nothing — viewer remains empty
        assert!(app.viewer_content.is_none());
    }

    #[test]
    fn test_load_file_into_viewer_dir_selected() {
        let mut app = App::new();
        app.file_tree_entries = make_entries();
        app.file_tree_selected = Some(0); // "src" is a dir
        app.load_file_into_viewer();
        // Should do nothing — can't view a directory
        assert!(app.viewer_content.is_none());
    }

    // ── load_file_by_path ──

    #[test]
    fn test_load_file_by_path_text_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("hello.rs");
        fs::write(&file, "fn main() {}").unwrap();
        let mut app = App::new();
        app.load_file_by_path(&file);
        assert_eq!(app.viewer_content.as_deref(), Some("fn main() {}"));
        assert_eq!(app.viewer_path.as_deref(), Some(file.as_path()));
        assert_eq!(app.viewer_mode, ViewerMode::File);
        assert_eq!(app.viewer_scroll, 0);
        assert!(app.viewer_lines_dirty);
    }

    #[test]
    fn test_load_file_by_path_nonexistent() {
        let mut app = App::new();
        app.load_file_by_path(std::path::Path::new("/nonexistent/file.rs"));
        assert!(app.viewer_content.as_ref().unwrap().contains("Error reading file"));
        assert_eq!(app.viewer_mode, ViewerMode::File);
    }

    #[test]
    fn test_load_file_by_path_empty_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("empty.rs");
        fs::write(&file, "").unwrap();
        let mut app = App::new();
        app.load_file_by_path(&file);
        assert_eq!(app.viewer_content.as_deref(), Some(""));
    }

    #[test]
    fn test_load_file_by_path_resets_scroll() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("code.rs");
        fs::write(&file, "content").unwrap();
        let mut app = App::new();
        app.viewer_scroll = 999;
        app.load_file_by_path(&file);
        assert_eq!(app.viewer_scroll, 0);
    }

    #[test]
    fn test_load_file_by_path_clears_image_state() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("code.rs");
        fs::write(&file, "text content").unwrap();
        let mut app = App::new();
        app.load_file_by_path(&file);
        assert!(app.viewer_image_state.is_none());
    }

    // ── file_tree_exec_add (filesystem operations) ──

    #[test]
    fn test_file_tree_exec_add_creates_file() {
        let tmp = TempDir::new().unwrap();
        let dir_path = tmp.path().to_path_buf();
        let mut app = App::new();
        app.file_tree_entries = vec![
            FileTreeEntry { path: dir_path.clone(), name: "root".into(), is_dir: true, depth: 0, is_hidden: false },
        ];
        app.file_tree_selected = Some(0);
        // Set up worktree so refresh works (it needs current_worktree)
        app.file_tree_exec_add("newfile.txt");
        assert!(dir_path.join("newfile.txt").exists());
    }

    #[test]
    fn test_file_tree_exec_add_creates_dir_with_trailing_slash() {
        let tmp = TempDir::new().unwrap();
        let dir_path = tmp.path().to_path_buf();
        let mut app = App::new();
        app.file_tree_entries = vec![
            FileTreeEntry { path: dir_path.clone(), name: "root".into(), is_dir: true, depth: 0, is_hidden: false },
        ];
        app.file_tree_selected = Some(0);
        app.file_tree_exec_add("newdir/");
        assert!(dir_path.join("newdir").is_dir());
    }

    #[test]
    fn test_file_tree_exec_add_no_selection() {
        let mut app = App::new();
        app.file_tree_selected = None;
        app.file_tree_exec_add("test.txt"); // should not crash
    }

    // ── file_tree_exec_rename ──

    #[test]
    fn test_file_tree_exec_rename_renames_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("old.txt");
        fs::write(&file, "content").unwrap();
        let mut app = App::new();
        app.file_tree_entries = vec![
            FileTreeEntry { path: file.clone(), name: "old.txt".into(), is_dir: false, depth: 0, is_hidden: false },
        ];
        app.file_tree_selected = Some(0);
        app.file_tree_exec_rename("new.txt");
        assert!(!file.exists());
        assert!(tmp.path().join("new.txt").exists());
    }

    #[test]
    fn test_file_tree_exec_rename_existing_target() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("old.txt");
        fs::write(&file, "old").unwrap();
        fs::write(tmp.path().join("new.txt"), "new").unwrap();
        let mut app = App::new();
        app.file_tree_entries = vec![
            FileTreeEntry { path: file.clone(), name: "old.txt".into(), is_dir: false, depth: 0, is_hidden: false },
        ];
        app.file_tree_selected = Some(0);
        app.file_tree_exec_rename("new.txt");
        // Should set status about already existing
        assert!(app.status_message.as_ref().unwrap().contains("Already exists"));
        // Original should still exist
        assert!(file.exists());
    }

    #[test]
    fn test_file_tree_exec_rename_no_selection() {
        let mut app = App::new();
        app.file_tree_selected = None;
        app.file_tree_exec_rename("whatever"); // should not crash
    }

    // ── file_tree_exec_delete ──

    #[test]
    fn test_file_tree_exec_delete_removes_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doomed.txt");
        fs::write(&file, "bye").unwrap();
        let mut app = App::new();
        app.file_tree_entries = vec![
            FileTreeEntry { path: file.clone(), name: "doomed.txt".into(), is_dir: false, depth: 0, is_hidden: false },
        ];
        app.file_tree_selected = Some(0);
        app.file_tree_exec_delete();
        assert!(!file.exists());
    }

    #[test]
    fn test_file_tree_exec_delete_removes_dir() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("bye_dir");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("inside.txt"), "").unwrap();
        let mut app = App::new();
        app.file_tree_entries = vec![
            FileTreeEntry { path: dir.clone(), name: "bye_dir".into(), is_dir: true, depth: 0, is_hidden: false },
        ];
        app.file_tree_selected = Some(0);
        app.file_tree_exec_delete();
        assert!(!dir.exists());
    }

    #[test]
    fn test_file_tree_exec_delete_no_selection() {
        let mut app = App::new();
        app.file_tree_selected = None;
        app.file_tree_exec_delete(); // should not crash
    }

    // ── file_tree_exec_copy_to ──

    #[test]
    fn test_file_tree_exec_copy_to_file() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("source.txt");
        fs::write(&src, "data").unwrap();
        let target_dir = tmp.path().join("dest");
        fs::create_dir(&target_dir).unwrap();
        let mut app = App::new();
        app.file_tree_exec_copy_to(&src, &target_dir);
        assert!(target_dir.join("source.txt").exists());
        assert_eq!(fs::read_to_string(target_dir.join("source.txt")).unwrap(), "data");
        // Source should still exist (it's a copy)
        assert!(src.exists());
    }

    #[test]
    fn test_file_tree_exec_copy_to_existing_target() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("file.txt");
        fs::write(&src, "original").unwrap();
        let target_dir = tmp.path().join("dest");
        fs::create_dir(&target_dir).unwrap();
        fs::write(target_dir.join("file.txt"), "existing").unwrap();
        let mut app = App::new();
        app.file_tree_exec_copy_to(&src, &target_dir);
        assert!(app.status_message.as_ref().unwrap().contains("Already exists"));
    }

    #[test]
    fn test_file_tree_exec_copy_to_expands_target_dir() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("file.txt");
        fs::write(&src, "data").unwrap();
        let target_dir = tmp.path().join("dest");
        fs::create_dir(&target_dir).unwrap();
        let mut app = App::new();
        app.file_tree_exec_copy_to(&src, &target_dir);
        assert!(app.file_tree_expanded.contains(&target_dir));
    }

    // ── file_tree_exec_move_to ──

    #[test]
    fn test_file_tree_exec_move_to_file() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("moving.txt");
        fs::write(&src, "data").unwrap();
        let target_dir = tmp.path().join("dest");
        fs::create_dir(&target_dir).unwrap();
        let mut app = App::new();
        app.file_tree_exec_move_to(&src, &target_dir);
        assert!(!src.exists()); // source removed
        assert!(target_dir.join("moving.txt").exists()); // target created
    }

    #[test]
    fn test_file_tree_exec_move_to_existing_target() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("file.txt");
        fs::write(&src, "source").unwrap();
        let target_dir = tmp.path().join("dest");
        fs::create_dir(&target_dir).unwrap();
        fs::write(target_dir.join("file.txt"), "existing").unwrap();
        let mut app = App::new();
        app.file_tree_exec_move_to(&src, &target_dir);
        assert!(app.status_message.as_ref().unwrap().contains("Already exists"));
        assert!(src.exists()); // source should still exist
    }

    #[test]
    fn test_file_tree_exec_move_to_expands_target_dir() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("file.txt");
        fs::write(&src, "data").unwrap();
        let target_dir = tmp.path().join("dest");
        fs::create_dir(&target_dir).unwrap();
        let mut app = App::new();
        app.file_tree_exec_move_to(&src, &target_dir);
        assert!(app.file_tree_expanded.contains(&target_dir));
    }

    // ── Navigation edge cases ──

    #[test]
    fn test_file_tree_next_single_entry() {
        let mut app = App::new();
        app.file_tree_entries = vec![
            FileTreeEntry { path: PathBuf::from("/only"), name: "only".into(), is_dir: false, depth: 0, is_hidden: false },
        ];
        app.file_tree_selected = Some(0);
        app.file_tree_next();
        assert_eq!(app.file_tree_selected, Some(0)); // can't go past
    }

    #[test]
    fn test_file_tree_prev_single_entry() {
        let mut app = App::new();
        app.file_tree_entries = vec![
            FileTreeEntry { path: PathBuf::from("/only"), name: "only".into(), is_dir: false, depth: 0, is_hidden: false },
        ];
        app.file_tree_selected = Some(0);
        app.file_tree_prev();
        assert_eq!(app.file_tree_selected, Some(0)); // can't go before
    }

    #[test]
    fn test_toggle_file_tree_dir_no_selection() {
        let mut app = App::new();
        app.file_tree_entries = make_entries();
        app.file_tree_selected = None;
        app.toggle_file_tree_dir(); // should not crash
    }

    #[test]
    fn test_toggle_file_tree_dir_on_file() {
        let mut app = App::new();
        app.file_tree_entries = make_entries();
        app.file_tree_selected = Some(4); // README.md is a file
        app.toggle_file_tree_dir(); // should do nothing since it's not a dir
        // No crash, expanded set unchanged
    }
}
