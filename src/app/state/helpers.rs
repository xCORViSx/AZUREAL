//! Helper functions for session naming and file tree building

use std::collections::HashSet;
use std::path::PathBuf;

use super::FileTreeEntry;

/// Build file tree entries for a directory (respects expanded state)
pub fn build_file_tree(root: &PathBuf, expanded: &HashSet<PathBuf>, hidden_dirs: &HashSet<String>) -> Vec<FileTreeEntry> {
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
    let Ok(read_dir) = std::fs::read_dir(dir) else { return };

    let mut items: Vec<_> = read_dir
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            // Skip common build/dependency directories (too noisy)
            if name == "target" || name == "node_modules" { return false; }
            // Hide entries configured in Filetree Options overlay
            if hidden_dirs.contains(&name) { return false; }
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
            }
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
