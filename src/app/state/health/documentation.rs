//! Documentation Health system — scans project source files for doc-comment
//! coverage and spawns concurrent Claude sessions to add missing documentation.
//! Uses line-based heuristics (no AST) to detect documentable items and check
//! whether each has a preceding `///` or `//!` doc comment.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::app::types::DocEntry;
use crate::claude::ClaudeProcess;

use super::super::App;
use super::{SOURCE_EXTENSIONS, SOURCE_ROOTS, load_health_scope, collect_source_files};

impl App {
    /// Scan project source files for documentation coverage — counts documentable
    /// items (fn, struct, enum, trait, const, static, type, impl, mod) and checks
    /// whether each has a preceding `///` or `//!` doc comment.
    /// Returns (entries sorted by coverage ascending, overall score 0.0–100.0).
    pub(crate) fn scan_documentation(&self) -> (Vec<DocEntry>, f32) {
        let Some(ref project) = self.project else { return (Vec::new(), 0.0) };
        let root = &project.path;

        // Determine which directories to scan (same logic as god files)
        let dirs = load_health_scope(root).unwrap_or_else(|| {
            let found: HashSet<PathBuf> = SOURCE_ROOTS.iter()
                .map(|name| root.join(name))
                .filter(|p| p.is_dir())
                .collect();
            if found.is_empty() {
                let mut s = HashSet::new();
                s.insert(root.clone());
                s
            } else {
                found
            }
        });

        let mut entries = Vec::new();
        let mut total_all = 0usize;
        let mut documented_all = 0usize;

        // Collect all source files to scan
        let mut files = Vec::new();
        let scanning_root = dirs.contains(root) && dirs.len() == 1;
        if scanning_root {
            collect_source_files(root, &mut files);
        } else {
            for dir in &dirs {
                if dir.is_dir() { collect_source_files(dir, &mut files); }
            }
            // Top-level source files (e.g. main.rs, build.rs)
            if let Ok(rd) = fs::read_dir(root) {
                for entry in rd.filter_map(|e| e.ok()) {
                    let p = entry.path();
                    if p.is_file() {
                        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                        if SOURCE_EXTENSIONS.contains(&ext) { files.push(p); }
                    }
                }
            }
        }

        // Scan each file for documentable items and doc comments
        for path in &files {
            let (total, documented) = scan_file_doc_coverage(path);
            if total == 0 { continue; }
            let coverage_pct = documented as f32 / total as f32 * 100.0;
            let rel_path = path.strip_prefix(root).unwrap_or(path).display().to_string();
            total_all += total;
            documented_all += documented;
            entries.push(DocEntry { path: path.clone(), rel_path, total_items: total, documented_items: documented, coverage_pct, checked: false });
        }

        // Sort worst-documented first so user sees problem files at top
        entries.sort_by(|a, b| a.coverage_pct.partial_cmp(&b.coverage_pct).unwrap_or(std::cmp::Ordering::Equal));
        let doc_score = if total_all > 0 { documented_all as f32 / total_all as f32 * 100.0 } else { 100.0 };
        (entries, doc_score)
    }

    /// Toggle the check on the currently selected doc entry
    pub fn doc_toggle_check(&mut self) {
        if let Some(ref mut panel) = self.health_panel {
            if let Some(entry) = panel.doc_entries.get_mut(panel.doc_selected) {
                entry.checked = !entry.checked;
            }
        }
    }

    /// Toggle all non-100% entries: if all non-100% are checked, uncheck them;
    /// otherwise check all non-100% and uncheck any 100% entries
    pub fn doc_toggle_non100(&mut self) {
        if let Some(ref mut panel) = self.health_panel {
            let all_non100_checked = panel.doc_entries.iter()
                .filter(|e| e.coverage_pct < 100.0)
                .all(|e| e.checked);
            for entry in &mut panel.doc_entries {
                entry.checked = if entry.coverage_pct < 100.0 { !all_non100_checked } else { false };
            }
        }
    }

    /// Open checked doc entries as viewer tabs (same pattern as god_file_view_checked)
    pub fn doc_view_checked(&mut self) {
        const MAX_TABS: usize = 12;
        let paths: Vec<PathBuf> = match self.health_panel {
            Some(ref panel) => panel.doc_entries.iter()
                .filter(|e| e.checked)
                .map(|e| e.path.clone())
                .collect(),
            None => return,
        };
        if paths.is_empty() {
            self.set_status("No files checked — use Space to check files");
            return;
        }
        let mut opened = 0usize;
        let mut skipped_dup = 0usize;
        let mut skipped_cap = 0usize;
        for path in &paths {
            if self.viewer_tabs.iter().any(|t| t.path.as_ref() == Some(path)) {
                skipped_dup += 1; continue;
            }
            if self.viewer_tabs.len() >= MAX_TABS {
                skipped_cap += paths.len() - opened - skipped_dup - skipped_cap;
                break;
            }
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let title = path.file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "Untitled".to_string());
            self.viewer_tabs.push(crate::app::types::ViewerTab {
                path: Some(path.clone()),
                content: Some(content),
                scroll: 0,
                mode: crate::app::ViewerMode::File,
                title,
            });
            opened += 1;
        }
        self.health_panel = None;
        if opened > 0 {
            self.viewer_active_tab = self.viewer_tabs.len() - 1;
            self.load_tab_to_viewer();
            self.focus = crate::app::Focus::Viewer;
        }
        let mut msg = format!("Opened {} file{}", opened, if opened == 1 { "" } else { "s" });
        if skipped_dup > 0 { msg.push_str(&format!(", {} already tabbed", skipped_dup)); }
        if skipped_cap > 0 { msg.push_str(&format!(", {} skipped (max {} tabs)", skipped_cap, MAX_TABS)); }
        self.set_status(msg);
    }

    /// Spawn [DH] (Documentation Health) Claude sessions for all checked doc entries.
    /// Each checked file gets its own concurrent Claude process with a prompt
    /// instructing Claude to add missing doc comments to all documentable items.
    pub fn doc_health_spawn(&mut self, claude_process: &ClaudeProcess) {
        let checked: Vec<(String, usize, usize)> = match self.health_panel {
            Some(ref panel) => panel.doc_entries.iter()
                .filter(|e| e.checked)
                .map(|e| (e.rel_path.clone(), e.documented_items, e.total_items))
                .collect(),
            None => return,
        };
        if checked.is_empty() {
            self.set_status("No files checked — use Space to check files");
            return;
        }

        let (main_branch, main_path) = match self.find_main_worktree() {
            Some(v) => v,
            None => { self.set_status("No main worktree found"); return; }
        };

        self.health_panel = None;

        let mut spawned = 0usize;
        let mut failed = 0usize;
        for (rel_path, documented, total) in &checked {
            let prompt = build_doc_health_prompt(rel_path, *documented, *total);
            let filename = Path::new(rel_path).file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| rel_path.clone());

            match claude_process.spawn(&main_path, &prompt, None) {
                Ok((rx, pid)) => {
                    let slot = pid.to_string();
                    self.pending_session_names.push((slot, format!("[DH] {}", filename)));
                    self.register_claude(main_branch.clone(), pid, rx);
                    spawned += 1;
                }
                Err(_) => { failed += 1; }
            }
        }

        self.switch_to_main_worktree(&main_branch);

        if failed == 0 {
            self.set_status(format!("Documenting {} files simultaneously", spawned));
        } else {
            self.set_status(format!("Documenting {} files ({} failed to start)", spawned, failed));
        }
    }
}

/// Scan a single file for documentable items and count how many have doc comments.
/// Uses line-based heuristics — no AST parsing, just pattern matching on trimmed lines.
/// Returns (total_items, documented_items).
fn scan_file_doc_coverage(path: &Path) -> (usize, usize) {
    let file = match File::open(path) { Ok(f) => f, Err(_) => return (0, 0) };
    let lines: Vec<String> = BufReader::new(file).lines().filter_map(|l| l.ok()).collect();

    let mut total = 0usize;
    let mut documented = 0usize;

    /// Patterns that indicate a documentable item (checked against trimmed line starts)
    const ITEM_PREFIXES: &[&str] = &[
        "pub fn ", "fn ", "pub struct ", "struct ", "pub enum ", "enum ",
        "pub trait ", "trait ", "pub const ", "const ", "pub static ", "static ",
        "pub type ", "type ", "pub async fn ", "async fn ", "pub unsafe fn ", "unsafe fn ",
        "impl ", "pub mod ", "mod ",
    ];

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        // Skip blank lines, comments, attributes, use/extern/cfg
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#')
            || trimmed.starts_with("use ") || trimmed.starts_with("extern ")
            || trimmed.starts_with('}') { continue; }

        // Check if this line starts a documentable item
        let is_item = ITEM_PREFIXES.iter().any(|p| trimmed.starts_with(p));
        if !is_item { continue; }

        total += 1;
        // Walk backwards from this line to find a doc comment (skip blanks + attributes)
        let mut j = i;
        while j > 0 {
            j -= 1;
            let prev = lines[j].trim();
            if prev.is_empty() || prev.starts_with("#[") || prev.starts_with("#![") { continue; }
            if prev.starts_with("///") || prev.starts_with("//!") { documented += 1; }
            break;
        }
    }
    (total, documented)
}

/// Build the documentation health prompt for a file missing doc comments.
/// Instructs Claude to read the file, identify undocumented items, and add
/// `///` or `//!` doc comments to every public and private item.
fn build_doc_health_prompt(rel_path: &str, documented: usize, total: usize) -> String {
    format!(
        "You are tasked with adding documentation comments to a source file that is missing them.\n\
        \n\
        File: {}\n\
        Current coverage: {}/{} items documented ({:.1}%)\n\
        \n\
        IMPORTANT: Before making any changes:\n\
        1. Read the entire file to understand its structure and context\n\
        2. Read other files that this file imports from or interacts with\n\
        3. Understand what each undocumented function, struct, enum, trait, constant, type alias, \
        impl block, and module declaration does\n\
        \n\
        Then add `///` doc comments to every item that is missing one. For module-level context, \
        use `//!` at the top of the file. Each doc comment should:\n\
        - Explain WHAT the item does and WHY it exists in plain language\n\
        - Be written as if explaining to someone seeing the codebase for the first time\n\
        - Include parameter/return descriptions for non-trivial functions\n\
        - Be concise but informative — one sentence is fine for simple items, \
        more for complex ones\n\
        \n\
        Do NOT modify any executable code — only add or improve doc comments. \
        Do NOT remove existing comments. Do NOT reformat or restructure the code.",
        rel_path, documented, total,
        if total > 0 { documented as f64 / total as f64 * 100.0 } else { 100.0 }
    )
}
