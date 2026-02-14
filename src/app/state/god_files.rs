//! God File System — scans project for oversized source files and spawns
//! modularization sessions. "God files" are source files that exceed 1000 lines,
//! indicating they've accumulated too many responsibilities and should be split
//! into smaller, focused modules.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::app::types::{GodFileEntry, GodFilePanel};
use crate::claude::ClaudeProcess;

use super::App;

/// Minimum line count for a file to be considered a "god file"
const GOD_FILE_THRESHOLD: usize = 1000;

/// Source file extensions we scan (common programming languages)
const SOURCE_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "cpp", "c", "h", "hpp",
    "swift", "kt", "rb", "cs", "vue", "svelte", "zig", "lua", "ex", "exs",
];

/// Directories to skip during scan (build artifacts, deps, VCS)
const SKIP_DIRS: &[&str] = &[
    ".git", "target", "node_modules", ".build", "dist", "build",
    "__pycache__", ".next", ".nuxt", "vendor", "Pods",
];

impl App {
    /// Open the God File panel — scans the project and shows results.
    /// Called when user presses 'g' in Worktrees pane.
    pub fn open_god_file_panel(&mut self) {
        let entries = self.scan_god_files();
        self.god_file_panel = Some(GodFilePanel {
            entries,
            selected: 0,
            scroll: 0,
        });
    }

    /// Close the God File panel without taking action
    pub fn close_god_file_panel(&mut self) {
        self.god_file_panel = None;
    }

    /// Toggle the check on the currently selected god file entry
    pub fn god_file_toggle_check(&mut self) {
        if let Some(ref mut panel) = self.god_file_panel {
            if let Some(entry) = panel.entries.get_mut(panel.selected) {
                entry.checked = !entry.checked;
            }
        }
    }

    /// Toggle all checks: if any are unchecked, check all; if all checked, uncheck all
    pub fn god_file_toggle_all(&mut self) {
        if let Some(ref mut panel) = self.god_file_panel {
            let all_checked = panel.entries.iter().all(|e| e.checked);
            let new_state = !all_checked;
            for entry in &mut panel.entries {
                entry.checked = new_state;
            }
        }
    }

    /// Spawn modularization sessions for all checked god files.
    /// First file starts immediately on the main worktree; remaining files
    /// are queued and auto-start as each prior session completes.
    pub fn god_file_modularize(&mut self, claude_process: &ClaudeProcess) {
        // Collect checked files and build prompts
        let checked: Vec<(String, String, usize)> = match self.god_file_panel {
            Some(ref panel) => panel.entries.iter()
                .filter(|e| e.checked)
                .map(|e| (e.rel_path.clone(), e.path.display().to_string(), e.line_count))
                .collect(),
            None => return,
        };

        if checked.is_empty() {
            self.set_status("No files checked — use Space to check files");
            return;
        }

        // Find main worktree path + branch
        let (main_branch, main_path) = match self.find_main_worktree() {
            Some(v) => v,
            None => {
                self.set_status("No main worktree found");
                return;
            }
        };

        // Build prompt queue — each entry is (rel_path, full_prompt)
        let mut queue: VecDeque<(String, String)> = checked.iter()
            .map(|(rel, _abs, lines)| (rel.clone(), build_modularize_prompt(rel, *lines)))
            .collect();

        // Pop the first file and spawn it immediately
        let (first_rel, first_prompt) = queue.pop_front().unwrap();

        // Store remaining in the app queue for auto-advance
        self.god_file_queue = queue;

        // Close the panel
        self.god_file_panel = None;

        // Extract just the filename for the session display name
        let filename = Path::new(&first_rel).file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| first_rel.clone());
        let session_name = format!("[GFM] {}", filename);

        // Set pending name so it gets saved to sessions.toml when Claude returns session_id
        self.pending_session_name = Some((main_branch.clone(), session_name));

        // Spawn Claude on the main worktree
        match claude_process.spawn(&main_path, &first_prompt, None) {
            Ok(rx) => {
                self.register_claude(main_branch.clone(), rx);
                // Switch convo pane to the main worktree so GFM output is visible
                self.switch_to_main_worktree(&main_branch);
                let remaining = self.god_file_queue.len();
                if remaining > 0 {
                    self.set_status(format!("Modularizing {} ({} queued)", first_rel, remaining));
                } else {
                    self.set_status(format!("Modularizing {}", first_rel));
                }
            }
            Err(e) => {
                self.set_status(format!("Failed to start: {}", e));
                self.god_file_queue.clear();
            }
        }
    }

    /// Called when a Claude session exits on the main branch. If there are
    /// queued god file modularizations, pop the next and spawn it.
    pub fn god_file_advance_queue(&mut self, claude_process: &ClaudeProcess) {
        if self.god_file_queue.is_empty() { return; }

        let (main_branch, main_path) = match self.find_main_worktree() {
            Some(v) => v,
            None => return,
        };

        let (rel_path, prompt) = self.god_file_queue.pop_front().unwrap();

        let filename = Path::new(&rel_path).file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| rel_path.clone());
        let session_name = format!("[GFM] {}", filename);
        self.pending_session_name = Some((main_branch.clone(), session_name));

        match claude_process.spawn(&main_path, &prompt, None) {
            Ok(rx) => {
                self.register_claude(main_branch.clone(), rx);
                // Switch convo pane to the main worktree so GFM output is visible
                self.switch_to_main_worktree(&main_branch);
                let remaining = self.god_file_queue.len();
                if remaining > 0 {
                    self.set_status(format!("Modularizing {} ({} queued)", rel_path, remaining));
                } else {
                    self.set_status(format!("Modularizing {} (last in queue)", rel_path));
                }
            }
            Err(e) => {
                self.set_status(format!("Queue failed: {}", e));
                self.god_file_queue.clear();
            }
        }
    }

    /// Switch the convo pane + sidebar selection to the main worktree.
    /// Called after spawning a GFM session so output is immediately visible.
    fn switch_to_main_worktree(&mut self, main_branch: &str) {
        if let Some(idx) = self.sessions.iter().position(|s| s.branch_name == main_branch) {
            if self.selected_worktree != Some(idx) {
                self.save_current_terminal();
                self.selected_worktree = Some(idx);
                self.load_session_output();
                self.invalidate_sidebar();
            }
        }
    }

    /// Find the main worktree's branch name and path
    fn find_main_worktree(&self) -> Option<(String, PathBuf)> {
        let project = self.project.as_ref()?;
        // Main branch is always sessions[0] by convention (loaded first in load.rs)
        self.sessions.iter()
            .find(|s| s.branch_name == project.main_branch)
            .and_then(|s| s.worktree_path.as_ref().map(|p| (s.branch_name.clone(), p.clone())))
    }

    /// Scan the project for source files exceeding the LOC threshold.
    /// Walks the project root recursively, skipping hidden/build directories,
    /// counting lines in each source file. Returns entries sorted by line
    /// count descending (biggest offenders first).
    fn scan_god_files(&self) -> Vec<GodFileEntry> {
        let Some(ref project) = self.project else { return Vec::new() };
        let root = &project.path;
        let mut entries = Vec::new();
        scan_dir_recursive(root, root, &mut entries);
        // Sort biggest files first so the worst offenders are at the top
        entries.sort_by(|a, b| b.line_count.cmp(&a.line_count));
        entries
    }
}

/// Recursively scan a directory for source files exceeding the LOC threshold.
/// Skips hidden directories and known build/dependency directories.
fn scan_dir_recursive(root: &Path, dir: &Path, results: &mut Vec<GodFileEntry>) {
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };

    // Collect and sort entries for deterministic output
    let mut dir_entries: Vec<_> = read_dir.filter_map(|e| e.ok()).collect();
    dir_entries.sort_by_key(|e| e.file_name());

    for entry in dir_entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden entries (dot-prefixed)
        if name.starts_with('.') { continue; }

        if path.is_dir() {
            // Skip known build/dependency directories
            if SKIP_DIRS.contains(&name.as_str()) { continue; }
            scan_dir_recursive(root, &path, results);
        } else if path.is_file() {
            // Check if this is a source file by extension
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !SOURCE_EXTENSIONS.contains(&ext) { continue; }

            // Count lines — fast: just count newlines, no parsing needed
            let line_count = match File::open(&path) {
                Ok(f) => BufReader::new(f).lines().count(),
                Err(_) => continue,
            };

            if line_count > GOD_FILE_THRESHOLD {
                let rel_path = path.strip_prefix(root)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                results.push(GodFileEntry {
                    path: path.clone(),
                    rel_path,
                    line_count,
                    checked: false,
                });
            }
        }
    }
}

/// Build the modularization prompt for a specific god file.
/// Instructs Claude to read context first, then split the file into
/// smaller focused modules following project conventions.
fn build_modularize_prompt(rel_path: &str, line_count: usize) -> String {
    format!(
        "You are tasked with modularizing a large \"god file\" that has accumulated too many responsibilities.\n\
        \n\
        File: {} ({} lines)\n\
        \n\
        IMPORTANT: Before making any changes:\n\
        1. First, read the entire file to understand its structure and responsibilities\n\
        2. Read other files in the project that import from or depend on this file\n\
        3. Understand the project's existing module structure and naming conventions\n\
        4. Plan your decomposition strategy — identify distinct responsibilities that should become separate modules\n\
        \n\
        Then proceed to split the file into smaller, focused modules following the project's conventions. Each new module should:\n\
        - Have a single, clear responsibility\n\
        - Be named descriptively (not util.rs or helpers.rs)\n\
        - Re-export public types from the original file's module so existing imports don't break\n\
        - Include appropriate module documentation\n\
        \n\
        Update the original file to re-export from the new modules for backwards compatibility.",
        rel_path, line_count
    )
}
