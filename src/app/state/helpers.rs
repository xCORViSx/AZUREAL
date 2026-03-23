//! Helper functions for session naming and file tree building

use std::collections::HashSet;
use std::path::PathBuf;

use super::FileTreeEntry;

/// Build file tree entries for a directory (respects expanded state)
pub fn build_file_tree(
    root: &PathBuf,
    expanded: &HashSet<PathBuf>,
    hidden_dirs: &HashSet<String>,
) -> Vec<FileTreeEntry> {
    let mut entries = Vec::new();
    build_file_tree_recursive(root, expanded, &mut entries, 0, false, hidden_dirs);
    entries
}

/// Recursively build file tree entries
fn build_file_tree_recursive(
    dir: &PathBuf,
    expanded: &HashSet<PathBuf>,
    entries: &mut Vec<FileTreeEntry>,
    depth: usize,
    parent_hidden: bool,
    hidden_dirs: &HashSet<String>,
) {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };

    let mut items: Vec<_> = read_dir
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            // Hide entries configured in Filetree Options overlay
            if hidden_dirs.contains(&name) {
                return false;
            }
            true
        })
        .collect();

    // Sort: directories first, then hidden last within each category, then alphabetically
    items.sort_by(|a, b| {
        let a_is_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let b_is_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let a_name = a.file_name().to_string_lossy().to_string();
        let b_name = b.file_name().to_string_lossy().to_string();
        let a_hidden = a_name.starts_with('.');
        let b_hidden = b_name.starts_with('.');

        match (a_is_dir, b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => match (a_hidden, b_hidden) {
                (false, true) => std::cmp::Ordering::Less,
                (true, false) => std::cmp::Ordering::Greater,
                _ => a.file_name().cmp(&b.file_name()),
            },
        }
    });

    for item in items {
        let path = item.path();
        let name = item.file_name().to_string_lossy().to_string();
        let is_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
        // Item is hidden if it starts with '.' OR if parent was hidden
        let is_hidden = parent_hidden || name.starts_with('.');

        entries.push(FileTreeEntry {
            path: path.clone(),
            name,
            is_dir,
            depth,
            is_hidden,
        });

        // Recurse into expanded directories, passing hidden state to children
        if is_dir && expanded.contains(&path) {
            build_file_tree_recursive(&path, expanded, entries, depth + 1, is_hidden, hidden_dirs);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Create a temp dir with a known file layout for testing
    fn make_test_tree() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // dirs
        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join("docs")).unwrap();
        fs::create_dir(root.join(".git")).unwrap();
        // files at root
        fs::write(root.join("README.md"), "hello").unwrap();
        fs::write(root.join("Cargo.toml"), "[package]").unwrap();
        // files in src
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("src/lib.rs"), "// lib").unwrap();
        // file in docs
        fs::write(root.join("docs/guide.md"), "# Guide").unwrap();
        // hidden file at root
        fs::write(root.join(".gitignore"), "target/").unwrap();
        // hidden dir contents
        fs::write(root.join(".git/HEAD"), "ref: refs/heads/main").unwrap();
        tmp
    }

    // ── build_file_tree: basic structure ──

    #[test]
    fn test_build_file_tree_returns_entries() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let expanded = HashSet::new();
        let hidden = HashSet::new();
        let entries = build_file_tree(&root, &expanded, &hidden);
        assert!(!entries.is_empty());
    }

    #[test]
    fn test_build_file_tree_top_level_only_when_collapsed() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let expanded = HashSet::new();
        let hidden = HashSet::new();
        let entries = build_file_tree(&root, &expanded, &hidden);
        // All entries should be depth 0
        for e in &entries {
            assert_eq!(e.depth, 0, "entry {} should be depth 0", e.name);
        }
    }

    #[test]
    fn test_build_file_tree_dirs_before_files() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let expanded = HashSet::new();
        let hidden = HashSet::new();
        let entries = build_file_tree(&root, &expanded, &hidden);
        let first_file_idx = entries.iter().position(|e| !e.is_dir);
        let last_dir_idx = entries.iter().rposition(|e| e.is_dir);
        if let (Some(ff), Some(ld)) = (first_file_idx, last_dir_idx) {
            assert!(ld < ff, "dirs should sort before files");
        }
    }

    #[test]
    fn test_build_file_tree_hidden_files_after_non_hidden() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let expanded = HashSet::new();
        let hidden = HashSet::new();
        let entries = build_file_tree(&root, &expanded, &hidden);
        // Among dirs: non-hidden dirs come before hidden dirs
        let dirs: Vec<_> = entries.iter().filter(|e| e.is_dir).collect();
        let first_hidden_dir = dirs.iter().position(|e| e.name.starts_with('.'));
        let last_non_hidden_dir = dirs.iter().rposition(|e| !e.name.starts_with('.'));
        if let (Some(fh), Some(ln)) = (first_hidden_dir, last_non_hidden_dir) {
            assert!(ln < fh, "non-hidden dirs should sort before hidden dirs");
        }
    }

    #[test]
    fn test_build_file_tree_expansion_adds_children() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let mut expanded = HashSet::new();
        expanded.insert(root.join("src"));
        let hidden = HashSet::new();
        let entries = build_file_tree(&root, &expanded, &hidden);
        let children: Vec<_> = entries.iter().filter(|e| e.depth == 1).collect();
        assert!(!children.is_empty(), "expanding src should reveal children");
    }

    #[test]
    fn test_build_file_tree_expanded_children_depth() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let mut expanded = HashSet::new();
        expanded.insert(root.join("src"));
        let hidden = HashSet::new();
        let entries = build_file_tree(&root, &expanded, &hidden);
        for e in &entries {
            if e.path.starts_with(root.join("src")) && e.path != root.join("src") {
                assert_eq!(e.depth, 1, "children of src should be depth 1");
            }
        }
    }

    #[test]
    fn test_build_file_tree_non_expanded_dir_no_children() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let expanded = HashSet::new();
        let hidden = HashSet::new();
        let entries = build_file_tree(&root, &expanded, &hidden);
        let has_src_child = entries
            .iter()
            .any(|e| e.path.starts_with(root.join("src")) && e.path != root.join("src"));
        assert!(!has_src_child, "collapsed src should not show children");
    }

    #[test]
    fn test_build_file_tree_skips_target_dir() {
        let tmp = make_test_tree();
        let root = tmp.path();
        fs::create_dir(root.join("target")).unwrap();
        fs::write(root.join("target/debug"), "bin").unwrap();
        let hidden: HashSet<String> = ["target".into()].into_iter().collect();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &hidden);
        assert!(
            !entries.iter().any(|e| e.name == "target"),
            "target dir should be filtered out when in hidden_dirs"
        );
    }

    #[test]
    fn test_build_file_tree_skips_node_modules() {
        let tmp = make_test_tree();
        let root = tmp.path();
        fs::create_dir(root.join("node_modules")).unwrap();
        let hidden: HashSet<String> = ["node_modules".into()].into_iter().collect();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &hidden);
        assert!(!entries.iter().any(|e| e.name == "node_modules"));
    }

    #[test]
    fn test_build_file_tree_hidden_dirs_filter() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let mut hidden = HashSet::new();
        hidden.insert("docs".to_string());
        let entries = build_file_tree(&root, &HashSet::new(), &hidden);
        assert!(
            !entries.iter().any(|e| e.name == "docs"),
            "docs should be hidden"
        );
    }

    #[test]
    fn test_build_file_tree_hidden_dirs_filter_multiple() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let mut hidden = HashSet::new();
        hidden.insert("docs".to_string());
        hidden.insert("src".to_string());
        let entries = build_file_tree(&root, &HashSet::new(), &hidden);
        assert!(!entries.iter().any(|e| e.name == "docs" || e.name == "src"));
    }

    #[test]
    fn test_build_file_tree_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let entries = build_file_tree(&tmp.path().to_path_buf(), &HashSet::new(), &HashSet::new());
        assert!(entries.is_empty());
    }

    #[test]
    fn test_build_file_tree_is_dir_flag() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let entries = build_file_tree(&root, &HashSet::new(), &HashSet::new());
        for e in &entries {
            if e.name == "src" || e.name == "docs" || e.name == ".git" {
                assert!(e.is_dir, "{} should be marked as dir", e.name);
            }
            if e.name == "README.md" || e.name == "Cargo.toml" || e.name == ".gitignore" {
                assert!(!e.is_dir, "{} should not be marked as dir", e.name);
            }
        }
    }

    #[test]
    fn test_build_file_tree_is_hidden_dot_prefix() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let entries = build_file_tree(&root, &HashSet::new(), &HashSet::new());
        for e in &entries {
            if e.name.starts_with('.') {
                assert!(e.is_hidden, "{} should be marked as hidden", e.name);
            }
        }
    }

    #[test]
    fn test_build_file_tree_non_dot_not_hidden_at_root() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let entries = build_file_tree(&root, &HashSet::new(), &HashSet::new());
        for e in &entries {
            if !e.name.starts_with('.') && e.depth == 0 {
                assert!(!e.is_hidden, "{} should not be marked hidden", e.name);
            }
        }
    }

    #[test]
    fn test_build_file_tree_children_of_hidden_dir_are_hidden() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let mut expanded = HashSet::new();
        expanded.insert(root.join(".git"));
        let entries = build_file_tree(&root, &expanded, &HashSet::new());
        for e in &entries {
            if e.path.starts_with(root.join(".git")) && e.path != root.join(".git") {
                assert!(e.is_hidden, "children of .git should inherit hidden");
            }
        }
    }

    #[test]
    fn test_build_file_tree_path_is_absolute() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let entries = build_file_tree(&root, &HashSet::new(), &HashSet::new());
        for e in &entries {
            assert!(
                e.path.is_absolute(),
                "path {} should be absolute",
                e.path.display()
            );
        }
    }

    #[test]
    fn test_build_file_tree_name_matches_filename() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let entries = build_file_tree(&root, &HashSet::new(), &HashSet::new());
        for e in &entries {
            let expected = e.path.file_name().unwrap().to_string_lossy().to_string();
            assert_eq!(e.name, expected);
        }
    }

    #[test]
    fn test_build_file_tree_alphabetical_within_category() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("zebra.txt"), "z").unwrap();
        fs::write(root.join("apple.txt"), "a").unwrap();
        fs::write(root.join("mango.txt"), "m").unwrap();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &HashSet::new());
        let names: Vec<_> = entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["apple.txt", "mango.txt", "zebra.txt"]);
    }

    #[test]
    fn test_build_file_tree_dirs_alphabetical() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("zzz")).unwrap();
        fs::create_dir(root.join("aaa")).unwrap();
        fs::create_dir(root.join("mmm")).unwrap();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &HashSet::new());
        let names: Vec<_> = entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["aaa", "mmm", "zzz"]);
    }

    #[test]
    fn test_build_file_tree_nested_expansion() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("a/b/deep.txt"), "deep").unwrap();
        let mut expanded = HashSet::new();
        expanded.insert(root.join("a"));
        expanded.insert(root.join("a/b"));
        let entries = build_file_tree(&root.to_path_buf(), &expanded, &HashSet::new());
        let deep = entries.iter().find(|e| e.name == "deep.txt");
        assert!(deep.is_some(), "deep nested file should appear");
        assert_eq!(deep.unwrap().depth, 2);
    }

    #[test]
    fn test_build_file_tree_partial_expansion() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("a/b/deep.txt"), "deep").unwrap();
        let mut expanded = HashSet::new();
        expanded.insert(root.join("a"));
        // "a/b" NOT expanded
        let entries = build_file_tree(&root.to_path_buf(), &expanded, &HashSet::new());
        let deep = entries.iter().find(|e| e.name == "deep.txt");
        assert!(
            deep.is_none(),
            "deep file should not appear if parent not expanded"
        );
    }

    #[test]
    fn test_build_file_tree_nonexistent_root() {
        let entries = build_file_tree(
            &PathBuf::from("/nonexistent/path/that/does/not/exist"),
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(entries.is_empty());
    }

    #[test]
    fn test_build_file_tree_hidden_dir_does_not_affect_sibling() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("visible")).unwrap();
        fs::create_dir(root.join(".hidden")).unwrap();
        let mut expanded = HashSet::new();
        expanded.insert(root.join("visible"));
        expanded.insert(root.join(".hidden"));
        fs::write(root.join("visible/a.txt"), "a").unwrap();
        fs::write(root.join(".hidden/b.txt"), "b").unwrap();
        let entries = build_file_tree(&root.to_path_buf(), &expanded, &HashSet::new());
        let a = entries.iter().find(|e| e.name == "a.txt").unwrap();
        let b = entries.iter().find(|e| e.name == "b.txt").unwrap();
        assert!(!a.is_hidden, "child of visible dir should not be hidden");
        assert!(b.is_hidden, "child of .hidden dir should be hidden");
    }

    #[test]
    fn test_build_file_tree_mixed_dirs_and_files_sorting() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("zdir")).unwrap();
        fs::write(root.join("afile.txt"), "").unwrap();
        fs::create_dir(root.join("adir")).unwrap();
        fs::write(root.join("zfile.txt"), "").unwrap();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &HashSet::new());
        let names: Vec<_> = entries.iter().map(|e| e.name.clone()).collect();
        // Dirs first (alphabetical), then files (alphabetical)
        assert_eq!(names, vec!["adir", "zdir", "afile.txt", "zfile.txt"]);
    }

    #[test]
    fn test_build_file_tree_hidden_files_sort_after_non_hidden_files() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join(".hidden"), "").unwrap();
        fs::write(root.join("visible"), "").unwrap();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &HashSet::new());
        let names: Vec<_> = entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names[0], "visible");
        assert_eq!(names[1], ".hidden");
    }

    #[test]
    fn test_build_file_tree_hidden_dirs_sort_after_non_hidden_dirs() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join(".hdir")).unwrap();
        fs::create_dir(root.join("vdir")).unwrap();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &HashSet::new());
        let names: Vec<_> = entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names[0], "vdir");
        assert_eq!(names[1], ".hdir");
    }

    #[test]
    fn test_build_file_tree_entry_count_no_expansion() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let entries = build_file_tree(&root, &HashSet::new(), &HashSet::new());
        // Should have: dirs (src, docs, .git) + files (README.md, Cargo.toml, .gitignore) = 6
        assert_eq!(entries.len(), 6);
    }

    #[test]
    fn test_build_file_tree_expand_src_adds_2() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let collapsed = build_file_tree(&root, &HashSet::new(), &HashSet::new());
        let mut expanded = HashSet::new();
        expanded.insert(root.join("src"));
        let with_src = build_file_tree(&root, &expanded, &HashSet::new());
        // src has main.rs and lib.rs
        assert_eq!(with_src.len(), collapsed.len() + 2);
    }

    #[test]
    fn test_build_file_tree_expand_all_dirs() {
        let tmp = make_test_tree();
        let root = tmp.path().to_path_buf();
        let mut expanded = HashSet::new();
        expanded.insert(root.join("src"));
        expanded.insert(root.join("docs"));
        expanded.insert(root.join(".git"));
        let entries = build_file_tree(&root, &expanded, &HashSet::new());
        // root: 3 dirs + 3 files = 6
        // src: main.rs, lib.rs = 2
        // docs: guide.md = 1
        // .git: HEAD = 1
        assert_eq!(entries.len(), 10);
    }

    #[test]
    fn test_build_file_tree_hidden_dir_name_exact_match() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("target")).unwrap(); // should be filtered when in hidden_dirs
        fs::create_dir(root.join("target2")).unwrap(); // should NOT be filtered
        let hidden: HashSet<String> = ["target".to_string()].into_iter().collect();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &hidden);
        assert!(entries.iter().any(|e| e.name == "target2"));
        assert!(!entries.iter().any(|e| e.name == "target"));
    }

    #[test]
    fn test_file_tree_entry_fields() {
        let entry = FileTreeEntry {
            path: PathBuf::from("/test/file.rs"),
            name: "file.rs".to_string(),
            is_dir: false,
            depth: 2,
            is_hidden: false,
        };
        assert_eq!(entry.path, PathBuf::from("/test/file.rs"));
        assert_eq!(entry.name, "file.rs");
        assert!(!entry.is_dir);
        assert_eq!(entry.depth, 2);
        assert!(!entry.is_hidden);
    }

    #[test]
    fn test_file_tree_entry_clone() {
        let entry = FileTreeEntry {
            path: PathBuf::from("/a/b"),
            name: "b".to_string(),
            is_dir: true,
            depth: 1,
            is_hidden: true,
        };
        let cloned = entry.clone();
        assert_eq!(entry.path, cloned.path);
        assert_eq!(entry.name, cloned.name);
        assert_eq!(entry.is_dir, cloned.is_dir);
        assert_eq!(entry.depth, cloned.depth);
        assert_eq!(entry.is_hidden, cloned.is_hidden);
    }

    #[test]
    fn test_file_tree_entry_debug() {
        let entry = FileTreeEntry {
            path: PathBuf::from("/x"),
            name: "x".to_string(),
            is_dir: false,
            depth: 0,
            is_hidden: false,
        };
        let dbg = format!("{:?}", entry);
        assert!(dbg.contains("FileTreeEntry"));
    }

    #[test]
    fn test_build_file_tree_symlinks_not_crash() {
        // Ensure the function doesn't crash on dirs with special files
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("normal.txt"), "ok").unwrap();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &HashSet::new());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "normal.txt");
    }

    #[test]
    fn test_build_file_tree_deeply_nested() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("a/b/c/d")).unwrap();
        fs::write(root.join("a/b/c/d/leaf.txt"), "leaf").unwrap();
        let mut expanded = HashSet::new();
        expanded.insert(root.join("a"));
        expanded.insert(root.join("a/b"));
        expanded.insert(root.join("a/b/c"));
        expanded.insert(root.join("a/b/c/d"));
        let entries = build_file_tree(&root.to_path_buf(), &expanded, &HashSet::new());
        let leaf = entries.iter().find(|e| e.name == "leaf.txt").unwrap();
        assert_eq!(leaf.depth, 4);
    }

    #[test]
    fn test_build_file_tree_expanding_nonexistent_dir_is_noop() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("file.txt"), "").unwrap();
        let mut expanded = HashSet::new();
        expanded.insert(root.join("nonexistent"));
        let entries = build_file_tree(&root.to_path_buf(), &expanded, &HashSet::new());
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_build_file_tree_unicode_filenames() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("日本語.txt"), "content").unwrap();
        fs::write(root.join("emoji_🎉.txt"), "party").unwrap();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &HashSet::new());
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_build_file_tree_hidden_files_via_custom_config() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("keep.txt"), "").unwrap();
        fs::write(root.join("remove_me.log"), "").unwrap();
        let mut hidden = HashSet::new();
        hidden.insert("remove_me.log".to_string());
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &hidden);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "keep.txt");
    }

    #[test]
    fn test_build_file_tree_empty_expanded_set() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("dir")).unwrap();
        fs::write(root.join("dir/child.txt"), "").unwrap();
        let expanded: HashSet<PathBuf> = HashSet::new();
        let entries = build_file_tree(&root.to_path_buf(), &expanded, &HashSet::new());
        // Only the dir itself should appear, not the child
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "dir");
    }

    #[test]
    fn test_build_file_tree_empty_hidden_set() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("a.txt"), "").unwrap();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &HashSet::new());
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_build_file_tree_only_hidden_files() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join(".a"), "").unwrap();
        fs::write(root.join(".b"), "").unwrap();
        fs::write(root.join(".c"), "").unwrap();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &HashSet::new());
        assert_eq!(entries.len(), 3);
        for e in &entries {
            assert!(e.is_hidden);
        }
    }

    #[test]
    fn test_build_file_tree_only_dirs_no_files() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("alpha")).unwrap();
        fs::create_dir(root.join("beta")).unwrap();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &HashSet::new());
        assert_eq!(entries.len(), 2);
        for e in &entries {
            assert!(e.is_dir);
        }
    }

    #[test]
    fn test_build_file_tree_only_files_no_dirs() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("x.txt"), "").unwrap();
        fs::write(root.join("y.rs"), "").unwrap();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &HashSet::new());
        for e in &entries {
            assert!(!e.is_dir);
        }
    }

    #[test]
    fn test_build_file_tree_many_files_sorted() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        for c in 'a'..='z' {
            fs::write(root.join(format!("{}.txt", c)), "").unwrap();
        }
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &HashSet::new());
        assert_eq!(entries.len(), 26);
        let names: Vec<_> = entries.iter().map(|e| e.name.clone()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn test_build_file_tree_multiple_hidden_dirs_in_hidden_set() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("hide_a")).unwrap();
        fs::create_dir(root.join("hide_b")).unwrap();
        fs::create_dir(root.join("keep")).unwrap();
        let mut hidden = HashSet::new();
        hidden.insert("hide_a".to_string());
        hidden.insert("hide_b".to_string());
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &hidden);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "keep");
    }

    #[test]
    fn test_build_file_tree_target_filter_applies_to_files_too() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // The filter removes ANY entry named "target" — file or dir
        fs::write(root.join("target"), "not a directory").unwrap();
        fs::write(root.join("other.txt"), "").unwrap();
        let hidden: HashSet<String> = ["target".into()].into_iter().collect();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &hidden);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "other.txt");
    }

    #[test]
    fn test_build_file_tree_special_characters_in_name() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("file with spaces.txt"), "").unwrap();
        fs::write(root.join("file-with-dashes.txt"), "").unwrap();
        fs::write(root.join("file_with_underscores.txt"), "").unwrap();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &HashSet::new());
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn test_build_file_tree_three_level_depth() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("l1/l2/l3")).unwrap();
        fs::write(root.join("l1/l2/l3/deep.txt"), "").unwrap();
        let mut expanded = HashSet::new();
        expanded.insert(root.join("l1"));
        expanded.insert(root.join("l1/l2"));
        expanded.insert(root.join("l1/l2/l3"));
        let entries = build_file_tree(&root.to_path_buf(), &expanded, &HashSet::new());
        // l1 (depth 0), l2 (depth 1), l3 (depth 2), deep.txt (depth 3)
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[3].depth, 3);
    }

    #[test]
    fn test_build_file_tree_node_modules_filter_applies_to_files_too() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // The filter removes ANY entry named "node_modules" — file or dir
        fs::write(root.join("node_modules"), "it's a file").unwrap();
        fs::write(root.join("keep.txt"), "").unwrap();
        let hidden: HashSet<String> = ["node_modules".into()].into_iter().collect();
        let entries = build_file_tree(&root.to_path_buf(), &HashSet::new(), &hidden);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "keep.txt");
    }
}
