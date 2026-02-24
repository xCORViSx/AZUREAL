//! God File System — scans project for oversized source files (>1000 LOC)
//! and spawns concurrent Claude sessions to modularize them. Includes scope
//! mode for user-customizable directory filtering with persistence.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::app::types::{
    GodFileEntry, HealthPanel,
    ModuleStyleDialog, RustModuleStyle, PythonModuleStyle,
};
use crate::claude::ClaudeProcess;

use super::super::App;
use super::{SOURCE_EXTENSIONS, SOURCE_ROOTS, SKIP_DIRS, load_health_scope, save_health_scope};

/// Minimum line count for a file to be considered a "god file"
const GOD_FILE_THRESHOLD: usize = 1000;

impl App {
    /// Enter god file scope mode — opens the FileTree overlay with green highlights
    /// on directories that are currently in the scan scope. User can toggle dirs
    /// with Enter, then Esc to rescan and return to the god file panel.
    /// Loads persisted scope from `[healthscope]` in .azureal/azufig.toml; otherwise
    /// falls back to auto-detected SOURCE_ROOTS.
    pub fn enter_god_file_scope_mode(&mut self) {
        let Some(ref project) = self.project else { return };
        let root = &project.path;

        // Try loading persisted scope first
        let dirs = load_health_scope(root).unwrap_or_else(|| {
            let found: Vec<PathBuf> = SOURCE_ROOTS.iter()
                .map(|name| root.join(name))
                .filter(|p| p.is_dir())
                .collect();
            if found.is_empty() {
                let mut s = HashSet::new();
                s.insert(root.clone());
                s
            } else {
                found.into_iter().collect()
            }
        });
        self.god_file_filter_dirs = dirs;
        self.god_file_filter_mode = true;

        self.show_file_tree = true;
        self.focus = crate::app::Focus::FileTree;
        self.load_file_tree();
        self.invalidate_file_tree();
    }

    /// Toggle a directory in/out of the god file filter scope.
    /// Called when user presses Enter on a directory while in filter mode.
    pub fn god_file_filter_toggle_dir(&mut self, path: PathBuf) {
        if self.god_file_filter_dirs.contains(&path) {
            self.god_file_filter_dirs.remove(&path);
        } else {
            self.god_file_filter_dirs.insert(path);
        }
        self.invalidate_file_tree();
    }

    /// Exit health scope mode — persist the scope to `[healthscope]` in azufig.toml,
    /// rescan with the user's custom directory scope, then reopen the health
    /// panel with updated results on both tabs.
    pub fn exit_god_file_scope_mode(&mut self) {
        if let Some(ref project) = self.project {
            save_health_scope(&project.path, &self.god_file_filter_dirs);
        }
        let god_files = self.scan_god_files_with_dirs(&self.god_file_filter_dirs.clone());
        let (doc_entries, doc_score) = self.scan_documentation();
        self.god_file_filter_mode = false;
        self.god_file_filter_dirs.clear();

        self.show_file_tree = false;
        self.focus = crate::app::Focus::Worktrees;
        let worktree_name = self.selected_worktree
            .map(|i| self.worktrees[i].name().to_string())
            .unwrap_or_default();
        self.health_panel = Some(HealthPanel {
            worktree_name,
            tab: self.last_health_tab,
            god_files,
            god_selected: 0,
            god_scroll: 0,
            doc_entries,
            doc_selected: 0,
            doc_scroll: 0,
            doc_score,
            module_style_dialog: None,
        });
    }

    /// Rescan god files + documentation using the given scope directories
    /// and rebuild the health panel. Called from deferred action after the
    /// "Rescanning god file scope…" loading indicator renders.
    pub fn rescan_health_with_dirs(&mut self, dirs: &[String]) {
        let dir_set: HashSet<PathBuf> = dirs.iter().map(PathBuf::from).collect();
        let god_files = self.scan_god_files_with_dirs(&dir_set);
        let (doc_entries, doc_score) = self.scan_documentation();
        let worktree_name = self.selected_worktree
            .map(|i| self.worktrees[i].name().to_string())
            .unwrap_or_default();
        self.health_panel = Some(HealthPanel {
            worktree_name,
            tab: self.last_health_tab,
            god_files,
            god_selected: 0,
            god_scroll: 0,
            doc_entries,
            doc_selected: 0,
            doc_scroll: 0,
            doc_score,
            module_style_dialog: None,
        });
    }

    /// Open checked god files as viewer tabs. Fills available tab slots
    /// (up to the 12-tab max), skipping files that are already open in a tab.
    /// Closes the panel and focuses the Viewer pane on the last opened tab.
    pub fn god_file_view_checked(&mut self) {
        const MAX_TABS: usize = 12;
        let paths: Vec<PathBuf> = match self.health_panel {
            Some(ref panel) => panel.god_files.iter()
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
                skipped_dup += 1;
                continue;
            }
            if self.viewer_tabs.len() >= MAX_TABS {
                skipped_cap += paths.len() - opened - skipped_dup - skipped_cap;
                break;
            }
            let content = match std::fs::read_to_string(path) {
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

    /// Toggle the check on the currently selected god file entry
    pub fn god_file_toggle_check(&mut self) {
        if let Some(ref mut panel) = self.health_panel {
            if let Some(entry) = panel.god_files.get_mut(panel.god_selected) {
                entry.checked = !entry.checked;
            }
        }
    }

    /// Toggle all checks: if any are unchecked, check all; if all checked, uncheck all
    pub fn god_file_toggle_all(&mut self) {
        if let Some(ref mut panel) = self.health_panel {
            let all_checked = panel.god_files.iter().all(|e| e.checked);
            let new_state = !all_checked;
            for entry in &mut panel.god_files {
                entry.checked = new_state;
            }
        }
    }

    /// Entry point for modularize action (Enter/m on God Files tab).
    /// If checked files include .rs or .py, shows the module style selector
    /// dialog first. Otherwise spawns immediately with generic prompts.
    pub fn god_file_start_modularize(&mut self, claude_process: &ClaudeProcess) {
        let checked: Vec<(String, usize)> = match self.health_panel {
            Some(ref panel) => panel.god_files.iter()
                .filter(|e| e.checked)
                .map(|e| (e.rel_path.clone(), e.line_count))
                .collect(),
            None => return,
        };
        if checked.is_empty() {
            self.set_status("No files checked — use Space to check files");
            return;
        }

        let has_rust = checked.iter().any(|(p, _)| p.ends_with(".rs"));
        let has_python = checked.iter().any(|(p, _)| p.ends_with(".py"));

        if has_rust || has_python {
            if let Some(ref mut panel) = self.health_panel {
                panel.module_style_dialog = Some(ModuleStyleDialog {
                    has_rust,
                    has_python,
                    rust_style: RustModuleStyle::FileBased,
                    python_style: PythonModuleStyle::Package,
                    selected: 0,
                });
            }
        } else {
            self.god_file_modularize(claude_process, None, None);
        }
    }

    /// Spawn modularization sessions for ALL checked god files simultaneously.
    /// Each file gets its own concurrent Claude process on the main worktree.
    pub fn god_file_modularize(
        &mut self,
        claude_process: &ClaudeProcess,
        rust_style: Option<RustModuleStyle>,
        python_style: Option<PythonModuleStyle>,
    ) {
        let checked: Vec<(String, usize)> = match self.health_panel {
            Some(ref panel) => panel.god_files.iter()
                .filter(|e| e.checked)
                .map(|e| (e.rel_path.clone(), e.line_count))
                .collect(),
            None => return,
        };
        if checked.is_empty() {
            self.set_status("No files checked — use Space to check files");
            return;
        }

        // Spawn GFM sessions on the current worktree — changes merge back to main
        let (branch, wt_path) = match self.current_worktree_info() {
            Some(v) => v,
            None => { self.set_status("No active worktree"); return; }
        };

        self.health_panel = None;

        let mut spawned = 0usize;
        let mut failed = 0usize;
        for (rel_path, lines) in &checked {
            let prompt = build_modularize_prompt(rel_path, *lines, rust_style, python_style);
            let filename = Path::new(rel_path).file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| rel_path.clone());

            match claude_process.spawn(&wt_path, &prompt, None) {
                Ok((rx, pid)) => {
                    let slot = pid.to_string();
                    self.pending_session_names.push((slot, format!("[GFM] {}", filename)));
                    self.register_claude(branch.clone(), pid, rx);
                    spawned += 1;
                }
                Err(_) => { failed += 1; }
            }
        }

        if failed == 0 {
            self.set_status(format!("Modularizing {} files simultaneously", spawned));
        } else {
            self.set_status(format!("Modularizing {} files ({} failed to start)", spawned, failed));
        }
    }

    /// Scan the project for source files exceeding the LOC threshold.
    /// Uses source-root detection: if well-known source directories exist,
    /// only scans those + top-level files. Otherwise scans the entire project.
    pub(crate) fn scan_god_files(&self) -> Vec<GodFileEntry> {
        let Some(ref project) = self.project else { return Vec::new() };
        let root = &project.path;

        let found_roots: HashSet<PathBuf> = SOURCE_ROOTS.iter()
            .map(|name| root.join(name))
            .filter(|p| p.is_dir())
            .collect();

        if found_roots.is_empty() {
            let mut all = HashSet::new();
            all.insert(root.clone());
            self.scan_god_files_with_dirs(&all)
        } else {
            self.scan_god_files_with_dirs(&found_roots)
        }
    }

    /// Scan specific directories for god files. Used by both auto-detect and
    /// user-customized scope mode.
    pub(crate) fn scan_god_files_with_dirs(&self, dirs: &HashSet<PathBuf>) -> Vec<GodFileEntry> {
        let Some(ref project) = self.project else { return Vec::new() };
        let root = &project.path;
        let mut entries = Vec::new();

        let scanning_root = dirs.contains(root);

        if scanning_root && dirs.len() == 1 {
            scan_dir_recursive(root, root, &mut entries);
        } else {
            for dir in dirs {
                if dir.is_dir() {
                    scan_dir_recursive(root, dir, &mut entries);
                }
            }
            scan_top_level_files(root, &mut entries);
        }

        entries.sort_by(|a, b| b.line_count.cmp(&a.line_count));
        entries
    }
}

/// Scan only the immediate files in a directory (no recursion).
/// Catches top-level source files like main.rs, build.rs, setup.py, etc.
fn scan_top_level_files(root: &Path, results: &mut Vec<GodFileEntry>) {
    let read_dir = match fs::read_dir(root) { Ok(rd) => rd, Err(_) => return };
    for entry in read_dir.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() { continue; }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !SOURCE_EXTENSIONS.contains(&ext) { continue; }
        let line_count = match File::open(&path) {
            Ok(f) => BufReader::new(f).lines().count(),
            Err(_) => continue,
        };
        if line_count > GOD_FILE_THRESHOLD {
            let rel_path = path.strip_prefix(root).unwrap_or(&path).display().to_string();
            results.push(GodFileEntry { path: path.clone(), rel_path, line_count, checked: false });
        }
    }
}

/// Recursively scan a directory for source files exceeding the LOC threshold.
/// Skips hidden directories and known build/dependency/non-source directories.
fn scan_dir_recursive(root: &Path, dir: &Path, results: &mut Vec<GodFileEntry>) {
    let read_dir = match fs::read_dir(dir) { Ok(rd) => rd, Err(_) => return };
    let mut dir_entries: Vec<_> = read_dir.filter_map(|e| e.ok()).collect();
    dir_entries.sort_by_key(|e| e.file_name());

    for entry in dir_entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') { continue; }

        if path.is_dir() {
            let name_lower = name.to_ascii_lowercase();
            if SKIP_DIRS.iter().any(|&s| s == name_lower) { continue; }
            scan_dir_recursive(root, &path, results);
        } else if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !SOURCE_EXTENSIONS.contains(&ext) { continue; }
            let line_count = match File::open(&path) {
                Ok(f) => BufReader::new(f).lines().count(),
                Err(_) => continue,
            };
            if line_count > GOD_FILE_THRESHOLD {
                let rel_path = path.strip_prefix(root).unwrap_or(&path).display().to_string();
                results.push(GodFileEntry { path: path.clone(), rel_path, line_count, checked: false });
            }
        }
    }
}

/// Build the modularization prompt for a specific god file.
/// For .rs/.py files, embeds the user's chosen module style.
fn build_modularize_prompt(
    rel_path: &str,
    line_count: usize,
    rust_style: Option<RustModuleStyle>,
    python_style: Option<PythonModuleStyle>,
) -> String {
    let mut prompt = format!(
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
    );

    if rel_path.ends_with(".rs") {
        if let Some(style) = rust_style {
            prompt.push_str("\n\n");
            match style {
                RustModuleStyle::FileBased => prompt.push_str(
                    "Module structure: Use file-based module roots (modern Rust convention). \
                    Create `modulename.rs` as the module root file alongside a `modulename/` directory \
                    for submodule files. Do NOT use `mod.rs` inside directories.\n\
                    Example:\n  src/apu.rs           (module root — declares submodules with mod statements)\n  \
                    src/apu/channel1.rs  (submodule)\n  src/apu/channel2.rs  (submodule)"
                ),
                RustModuleStyle::ModRs => prompt.push_str(
                    "Module structure: Use directory modules with mod.rs (legacy Rust convention). \
                    Create a `modulename/` directory containing `mod.rs` as the module root, \
                    with submodule files alongside it in the same directory.\n\
                    Example:\n  src/apu/mod.rs       (module root — declares submodules with mod statements)\n  \
                    src/apu/channel1.rs  (submodule)\n  src/apu/channel2.rs  (submodule)"
                ),
            }
        }
    } else if rel_path.ends_with(".py") {
        if let Some(style) = python_style {
            prompt.push_str("\n\n");
            match style {
                PythonModuleStyle::Package => prompt.push_str(
                    "Module structure: Create Python packages. Each new module becomes a directory \
                    containing `__init__.py` (which re-exports public names for clean imports) \
                    plus submodule `.py` files inside the directory."
                ),
                PythonModuleStyle::SingleFile => prompt.push_str(
                    "Module structure: Use single-file Python modules. Each new module is a standalone \
                    `.py` file with explicit imports between them. Do not create `__init__.py` package \
                    directories — keep modules as flat individual files."
                ),
            }
        }
    }

    prompt
}
