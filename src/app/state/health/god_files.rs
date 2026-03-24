//! God File System — scans project for oversized source files (>1000 LOC)
//! and spawns concurrent Claude sessions to modularize them. Includes scope
//! mode for user-customizable directory filtering with persistence.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::app::types::{
    GodFileEntry, HealthPanel, ModuleStyleDialog, PythonModuleStyle, RustModuleStyle,
};
use crate::backend::AgentProcess;

use super::super::App;
use super::{load_health_scope, SKIP_DIRS, SOURCE_EXTENSIONS, SOURCE_ROOTS};

/// Minimum line count for a file to be considered a "god file"
const GOD_FILE_THRESHOLD: usize = 1000;

/// Count source lines in a file, excluding `#[cfg(test)]` module blocks for Rust files.
///
/// For `.rs` files, detects `#[cfg(test)]` lines and tracks brace depth to skip
/// the entire test module. For all other languages, counts all lines.
fn count_source_lines(path: &Path) -> Option<usize> {
    let content = fs::read_to_string(path).ok()?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext != "rs" {
        return Some(content.lines().count());
    }

    let mut count = 0usize;
    let mut in_test_block = false;
    let mut brace_depth = 0i32;
    let mut saw_cfg_test = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Look for #[cfg(test)] on its own line (or with mod tests)
        if !in_test_block && trimmed.contains("#[cfg(test)]") {
            saw_cfg_test = true;
        }

        if saw_cfg_test && !in_test_block {
            // Count opening/closing braces to find where the test module starts
            for ch in trimmed.chars() {
                if ch == '{' {
                    if !in_test_block {
                        in_test_block = true;
                        brace_depth = 1;
                    } else {
                        brace_depth += 1;
                    }
                }
            }
            if in_test_block {
                // This line is part of the test block — skip it
                continue;
            }
            // Haven't found the opening brace yet (e.g. #[cfg(test)] on its own line)
            // Don't count lines between #[cfg(test)] and the opening brace
            continue;
        }

        if in_test_block {
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => {
                        brace_depth -= 1;
                        if brace_depth == 0 {
                            in_test_block = false;
                            saw_cfg_test = false;
                        }
                    }
                    _ => {}
                }
            }
            continue; // skip test block lines
        }

        count += 1;
    }

    Some(count)
}

impl App {
    /// Enter god file scope mode — opens the FileTree overlay with green highlights
    /// on directories that are currently in the scan scope. User can toggle dirs
    /// with Enter, then Esc to rescan and return to the god file panel.
    /// Loads persisted scope from `[healthscope]` in .azureal/azufig.toml; otherwise
    /// falls back to auto-detected SOURCE_ROOTS.
    ///
    /// Scope is persisted relative to project.path (main repo root), but the file
    /// tree shows entries from the current worktree path. This method translates
    /// project-root paths → worktree paths so highlighting matches the file tree.
    pub fn enter_god_file_scope_mode(&mut self) {
        let Some(ref project) = self.project else {
            return;
        };
        let project_root = project.path.clone();

        // The file tree is rooted at the current worktree path, which may differ
        // from project.path (e.g., /repo/worktrees/health vs /repo). Translate
        // scope paths so they match the file tree entries.
        let wt_root = self
            .current_worktree()
            .and_then(|wt| wt.worktree_path.clone())
            .unwrap_or_else(|| project_root.clone());

        // Try loading persisted scope first (stored as project-root-relative)
        let dirs = load_health_scope(&project_root).unwrap_or_else(|| {
            let found: Vec<PathBuf> = SOURCE_ROOTS
                .iter()
                .map(|name| project_root.join(name))
                .filter(|p| p.is_dir())
                .collect();
            if found.is_empty() {
                let mut s = HashSet::new();
                s.insert(project_root.clone());
                s
            } else {
                found.into_iter().collect()
            }
        });

        // Translate project-root paths → worktree paths for file tree highlighting.
        // Paths that can't be translated (wrong project, missing dir) are dropped.
        let dirs = if wt_root != project_root {
            dirs.into_iter()
                .filter_map(|p| {
                    if let Ok(rel) = p.strip_prefix(&project_root) {
                        let translated = wt_root.join(rel);
                        if translated.is_dir() {
                            Some(translated)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            dirs
        };

        self.god_file_filter_dirs = dirs;
        self.god_file_filter_mode = true;

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

    /// Rescan god files + documentation using the given scope directories
    /// and rebuild the health panel. Called from deferred action after the
    /// "Rescanning god file scope…" loading indicator renders.
    pub fn rescan_health_with_dirs(&mut self, dirs: &[String]) {
        let dir_set: HashSet<PathBuf> = dirs.iter().map(PathBuf::from).collect();
        let god_files = self.scan_god_files_with_dirs(&dir_set);
        let (doc_entries, doc_score) = self.scan_documentation();
        let worktree_name = self
            .selected_worktree
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
            Some(ref panel) => panel
                .god_files
                .iter()
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
            if self
                .viewer_tabs
                .iter()
                .any(|t| t.path.as_ref() == Some(path))
            {
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
            let title = path
                .file_name()
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

        let mut msg = format!(
            "Opened {} file{}",
            opened,
            if opened == 1 { "" } else { "s" }
        );
        if skipped_dup > 0 {
            msg.push_str(&format!(", {} already tabbed", skipped_dup));
        }
        if skipped_cap > 0 {
            msg.push_str(&format!(
                ", {} skipped (max {} tabs)",
                skipped_cap, MAX_TABS
            ));
        }
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
    pub fn god_file_start_modularize(&mut self, claude_process: &AgentProcess) {
        let checked: Vec<(String, usize)> = match self.health_panel {
            Some(ref panel) => panel
                .god_files
                .iter()
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
        claude_process: &AgentProcess,
        rust_style: Option<RustModuleStyle>,
        python_style: Option<PythonModuleStyle>,
    ) {
        let checked: Vec<(String, usize)> = match self.health_panel {
            Some(ref panel) => panel
                .god_files
                .iter()
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
            None => {
                self.set_status("No active worktree");
                return;
            }
        };

        self.health_panel = None;

        // Ensure the SQLite session store exists so each GFM file gets its own session
        self.ensure_session_store();

        let selected_model = self.selected_model.clone();
        let mut spawned = 0usize;
        let mut failed = 0usize;
        let mut last_session_id: Option<i64> = None;
        for (rel_path, lines) in &checked {
            let prompt = build_modularize_prompt(rel_path, *lines, rust_style, python_style);
            let filename = Path::new(rel_path)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| rel_path.clone());
            let session_name = format!("[GFM] {}", filename);

            // Create a dedicated store session for this GFM file
            let store_id = self.session_store.as_ref().and_then(|store| {
                store.create_session(&branch).ok().map(|id| {
                    let _ = store.rename_session(id, &session_name);
                    id
                })
            });

            match claude_process.spawn(&wt_path, &prompt, None, selected_model.as_deref()) {
                Ok((rx, pid)) => {
                    let slot = pid.to_string();
                    // Map PID to the store session so post-exit flow persists events correctly
                    if let Some(sid) = store_id {
                        self.pid_session_target
                            .insert(slot.clone(), (sid, wt_path.clone(), 0, 0));
                        last_session_id = Some(sid);
                    }
                    self.pending_session_names
                        .push((slot, session_name));
                    self.register_claude(branch.clone(), pid, rx, selected_model.as_deref());
                    spawned += 1;
                }
                Err(_) => {
                    failed += 1;
                }
            }
        }

        // Clear session pane so GFM output starts fresh (otherwise old session
        // content stays visible and new output appends below it)
        if spawned > 0 {
            // Point current_session_id to the last (active slot's) session
            if let Some(sid) = last_session_id {
                self.current_session_id = Some(sid);
            }
            self.display_events.clear();
            self.session_lines.clear();
            self.session_buffer.clear();
            self.session_scroll = usize::MAX;
            self.rendered_lines_cache.clear();
            self.session_viewport_cache.clear();
            self.animation_line_indices.clear();
            self.message_bubble_positions.clear();
            self.clickable_paths.clear();
            self.clickable_tables.clear();
            self.rendered_events_count = 0;
            self.rendered_content_line_count = 0;
            self.rendered_events_start = 0;
            self.render_seq_applied = self.render_thread.current_seq();
            self.render_in_flight = false;
            self.invalidate_render_cache();
            self.event_parser = crate::events::EventParser::new();
            self.agent_processor_needs_reset = true;
            self.session_file_path = None;
            self.session_file_modified = None;
            self.session_file_size = 0;
            self.session_file_parse_offset = 0;
            self.session_file_dirty = false;
            self.current_todos.clear();
            self.subagent_todos.clear();
            self.active_task_tool_ids.clear();
            self.chars_since_compaction = 0;
            self.token_badge_cache = None;
            self.update_title_session_name();
        }

        if failed == 0 {
            self.set_status(format!("Modularizing {} files simultaneously", spawned));
        } else {
            self.set_status(format!(
                "Modularizing {} files ({} failed to start)",
                spawned, failed
            ));
        }
    }

    /// Scan the project for source files exceeding the LOC threshold.
    /// Uses source-root detection: if well-known source directories exist,
    /// only scans those + top-level files. Otherwise scans the entire project.
    /// Scans the current worktree path (not project root) so results reflect
    /// the actual files on the working branch.
    pub(crate) fn scan_god_files(&self) -> Vec<GodFileEntry> {
        let Some(root) = self.health_scan_root() else {
            return Vec::new();
        };

        let found_roots: HashSet<PathBuf> = SOURCE_ROOTS
            .iter()
            .map(|name| root.join(name))
            .filter(|p| p.is_dir())
            .collect();

        if found_roots.is_empty() {
            let mut all = HashSet::new();
            all.insert(root);
            self.scan_god_files_with_dirs(&all)
        } else {
            self.scan_god_files_with_dirs(&found_roots)
        }
    }

    /// Scan specific directories for god files. Used by both auto-detect and
    /// user-customized scope mode. Dirs should already be translated to the
    /// current worktree path (via `translate_scope_dirs`).
    pub(crate) fn scan_god_files_with_dirs(&self, dirs: &HashSet<PathBuf>) -> Vec<GodFileEntry> {
        let Some(root) = self.health_scan_root() else {
            return Vec::new();
        };
        let mut entries = Vec::new();

        let scanning_root = dirs.contains(&root);

        if scanning_root && dirs.len() == 1 {
            scan_dir_recursive(&root, &root, &mut entries);
        } else {
            for dir in dirs {
                if dir.is_dir() {
                    scan_dir_recursive(&root, dir, &mut entries);
                }
            }
            scan_top_level_files(&root, &mut entries);
        }

        entries.sort_by(|a, b| b.line_count.cmp(&a.line_count));
        entries
    }
}

/// Scan only the immediate files in a directory (no recursion).
/// Catches top-level source files like main.rs, build.rs, setup.py, etc.
fn scan_top_level_files(root: &Path, results: &mut Vec<GodFileEntry>) {
    let read_dir = match fs::read_dir(root) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for entry in read_dir.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !SOURCE_EXTENSIONS.contains(&ext) {
            continue;
        }
        let line_count = match count_source_lines(&path) {
            Some(c) => c,
            None => continue,
        };
        if line_count > GOD_FILE_THRESHOLD {
            let rel_path = path
                .strip_prefix(root)
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

/// Recursively scan a directory for source files exceeding the LOC threshold.
/// Skips hidden directories and known build/dependency/non-source directories.
fn scan_dir_recursive(root: &Path, dir: &Path, results: &mut Vec<GodFileEntry>) {
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    let mut dir_entries: Vec<_> = read_dir.filter_map(|e| e.ok()).collect();
    dir_entries.sort_by_key(|e| e.file_name());

    for entry in dir_entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            let name_lower = name.to_ascii_lowercase();
            if SKIP_DIRS.iter().any(|&s| s == name_lower) {
                continue;
            }
            scan_dir_recursive(root, &path, results);
        } else if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !SOURCE_EXTENSIONS.contains(&ext) {
                continue;
            }
            let line_count = match count_source_lines(&path) {
                Some(c) => c,
                None => continue,
            };
            if line_count > GOD_FILE_THRESHOLD {
                let rel_path = path
                    .strip_prefix(root)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── GOD_FILE_THRESHOLD constant ──

    #[test]
    fn test_god_file_threshold_is_1000() {
        assert_eq!(GOD_FILE_THRESHOLD, 1000);
    }

    // ── build_modularize_prompt: basic ──

    #[test]
    fn test_prompt_contains_file_path() {
        let prompt = build_modularize_prompt("src/main.rs", 1500, None, None);
        assert!(prompt.contains("src/main.rs"));
    }

    #[test]
    fn test_prompt_contains_line_count() {
        let prompt = build_modularize_prompt("src/main.rs", 1500, None, None);
        assert!(prompt.contains("1500 lines"));
    }

    #[test]
    fn test_prompt_contains_modularizing_instructions() {
        let prompt = build_modularize_prompt("src/lib.rs", 2000, None, None);
        assert!(prompt.contains("modularizing"));
        assert!(prompt.contains("god file"));
    }

    #[test]
    fn test_prompt_contains_step_instructions() {
        let prompt = build_modularize_prompt("app.py", 1200, None, None);
        assert!(prompt.contains("read the entire file"));
        assert!(prompt.contains("decomposition strategy"));
    }

    // ── build_modularize_prompt: Rust styles ──

    #[test]
    fn test_prompt_rust_file_based() {
        let prompt =
            build_modularize_prompt("src/state.rs", 1500, Some(RustModuleStyle::FileBased), None);
        assert!(prompt.contains("file-based module roots"));
        assert!(prompt.contains("Do NOT use `mod.rs`"));
    }

    #[test]
    fn test_prompt_rust_mod_rs() {
        let prompt =
            build_modularize_prompt("src/state.rs", 1500, Some(RustModuleStyle::ModRs), None);
        assert!(prompt.contains("mod.rs"));
        assert!(prompt.contains("legacy Rust convention"));
    }

    #[test]
    fn test_prompt_rust_no_style() {
        let prompt = build_modularize_prompt("src/state.rs", 1500, None, None);
        assert!(!prompt.contains("file-based module roots"));
        assert!(!prompt.contains("legacy Rust convention"));
    }

    #[test]
    fn test_prompt_rust_style_ignored_for_non_rs() {
        let prompt =
            build_modularize_prompt("app.js", 1500, Some(RustModuleStyle::FileBased), None);
        assert!(!prompt.contains("file-based module roots"));
    }

    // ── build_modularize_prompt: Python styles ──

    #[test]
    fn test_prompt_python_package() {
        let prompt =
            build_modularize_prompt("app.py", 1500, None, Some(PythonModuleStyle::Package));
        assert!(prompt.contains("Python packages"));
        assert!(prompt.contains("__init__.py"));
    }

    #[test]
    fn test_prompt_python_single_file() {
        let prompt =
            build_modularize_prompt("app.py", 1500, None, Some(PythonModuleStyle::SingleFile));
        assert!(prompt.contains("single-file Python modules"));
        assert!(prompt.contains("standalone"));
    }

    #[test]
    fn test_prompt_python_no_style() {
        let prompt = build_modularize_prompt("app.py", 1500, None, None);
        assert!(!prompt.contains("Python packages"));
        assert!(!prompt.contains("single-file Python modules"));
    }

    #[test]
    fn test_prompt_python_style_ignored_for_non_py() {
        let prompt =
            build_modularize_prompt("app.rs", 1500, None, Some(PythonModuleStyle::Package));
        assert!(!prompt.contains("Python packages"));
    }

    // ── build_modularize_prompt: both styles ──

    #[test]
    fn test_prompt_rs_with_both_styles_only_uses_rust() {
        let prompt = build_modularize_prompt(
            "src/app.rs",
            2000,
            Some(RustModuleStyle::FileBased),
            Some(PythonModuleStyle::Package),
        );
        assert!(prompt.contains("file-based module roots"));
        assert!(!prompt.contains("Python packages"));
    }

    #[test]
    fn test_prompt_py_with_both_styles_only_uses_python() {
        let prompt = build_modularize_prompt(
            "app.py",
            2000,
            Some(RustModuleStyle::FileBased),
            Some(PythonModuleStyle::Package),
        );
        assert!(!prompt.contains("file-based module roots"));
        assert!(prompt.contains("Python packages"));
    }

    #[test]
    fn test_prompt_generic_file_no_style_section() {
        let prompt = build_modularize_prompt(
            "app.go",
            2000,
            Some(RustModuleStyle::FileBased),
            Some(PythonModuleStyle::Package),
        );
        assert!(!prompt.contains("Module structure:"));
    }

    // ── build_modularize_prompt: edge cases ──

    #[test]
    fn test_prompt_zero_lines() {
        let prompt = build_modularize_prompt("empty.rs", 0, None, None);
        assert!(prompt.contains("0 lines"));
    }

    #[test]
    fn test_prompt_very_large_line_count() {
        let prompt = build_modularize_prompt("huge.rs", 999999, Some(RustModuleStyle::ModRs), None);
        assert!(prompt.contains("999999 lines"));
    }

    #[test]
    fn test_prompt_nested_path() {
        let prompt = build_modularize_prompt("src/app/state/health.rs", 1200, None, None);
        assert!(prompt.contains("src/app/state/health.rs"));
    }

    // ── scan_dir_recursive ──

    fn make_source_file(dir: &Path, name: &str, lines: usize) {
        let content: String = (0..lines).map(|i| format!("line {}\n", i)).collect();
        fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn test_scan_dir_recursive_finds_god_files() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        make_source_file(root, "big.rs", 1500);
        let mut results = Vec::new();
        scan_dir_recursive(root, root, &mut results);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line_count, 1500);
    }

    #[test]
    fn test_scan_dir_recursive_ignores_small_files() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        make_source_file(root, "small.rs", 100);
        let mut results = Vec::new();
        scan_dir_recursive(root, root, &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_scan_dir_recursive_threshold_boundary() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        make_source_file(root, "exact.rs", 1000); // exactly 1000 — NOT a god file
        make_source_file(root, "over.rs", 1001); // 1001 — IS a god file
        let mut results = Vec::new();
        scan_dir_recursive(root, root, &mut results);
        assert_eq!(results.len(), 1);
        assert!(results[0].rel_path.contains("over.rs"));
    }

    #[test]
    fn test_scan_dir_recursive_rel_path() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("src")).unwrap();
        make_source_file(&root.join("src"), "big.rs", 1500);
        let mut results = Vec::new();
        scan_dir_recursive(root, root, &mut results);
        let expected = std::path::Path::new("src").join("big.rs");
        assert_eq!(results[0].rel_path, expected.to_string_lossy());
    }

    #[test]
    fn test_scan_dir_recursive_skips_hidden() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join(".hidden")).unwrap();
        make_source_file(&root.join(".hidden"), "big.rs", 2000);
        let mut results = Vec::new();
        scan_dir_recursive(root, root, &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_scan_dir_recursive_skips_target() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("target")).unwrap();
        make_source_file(&root.join("target"), "gen.rs", 5000);
        let mut results = Vec::new();
        scan_dir_recursive(root, root, &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_scan_dir_recursive_skips_node_modules() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("node_modules")).unwrap();
        make_source_file(&root.join("node_modules"), "huge.js", 5000);
        let mut results = Vec::new();
        scan_dir_recursive(root, root, &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_scan_dir_recursive_ignores_non_source_ext() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let content: String = (0..2000).map(|i| format!("line {}\n", i)).collect();
        fs::write(root.join("data.json"), &content).unwrap();
        fs::write(root.join("readme.md"), &content).unwrap();
        let mut results = Vec::new();
        scan_dir_recursive(root, root, &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_scan_dir_recursive_checked_defaults_false() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        make_source_file(root, "big.rs", 1500);
        let mut results = Vec::new();
        scan_dir_recursive(root, root, &mut results);
        assert!(!results[0].checked);
    }

    #[test]
    fn test_scan_dir_recursive_multiple_files() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        make_source_file(root, "a.rs", 1500);
        make_source_file(root, "b.py", 2000);
        make_source_file(root, "c.go", 3000);
        make_source_file(root, "small.js", 50);
        let mut results = Vec::new();
        scan_dir_recursive(root, root, &mut results);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_scan_dir_recursive_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let mut results = Vec::new();
        scan_dir_recursive(tmp.path(), tmp.path(), &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_scan_dir_recursive_nonexistent() {
        let mut results = Vec::new();
        scan_dir_recursive(Path::new("/nope"), Path::new("/nope"), &mut results);
        assert!(results.is_empty());
    }

    // ── scan_top_level_files ──

    #[test]
    fn test_scan_top_level_finds_god_files() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        make_source_file(root, "main.rs", 1500);
        let mut results = Vec::new();
        scan_top_level_files(root, &mut results);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_scan_top_level_ignores_small() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        make_source_file(root, "small.rs", 500);
        let mut results = Vec::new();
        scan_top_level_files(root, &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_scan_top_level_does_not_recurse() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("sub")).unwrap();
        make_source_file(&root.join("sub"), "big.rs", 2000);
        let mut results = Vec::new();
        scan_top_level_files(root, &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_scan_top_level_skips_dirs() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("src")).unwrap();
        let mut results = Vec::new();
        scan_top_level_files(root, &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_scan_top_level_skips_non_source() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let content: String = (0..2000).map(|i| format!("line {}\n", i)).collect();
        fs::write(root.join("data.json"), &content).unwrap();
        let mut results = Vec::new();
        scan_top_level_files(root, &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_scan_top_level_nonexistent() {
        let mut results = Vec::new();
        scan_top_level_files(Path::new("/nonexistent"), &mut results);
        assert!(results.is_empty());
    }

    // ── GodFileEntry struct ──

    #[test]
    fn test_god_file_entry_construction() {
        let entry = GodFileEntry {
            path: PathBuf::from("/src/main.rs"),
            rel_path: "src/main.rs".to_string(),
            line_count: 2500,
            checked: false,
        };
        assert_eq!(entry.path, PathBuf::from("/src/main.rs"));
        assert_eq!(entry.rel_path, "src/main.rs");
        assert_eq!(entry.line_count, 2500);
        assert!(!entry.checked);
    }

    #[test]
    fn test_god_file_entry_checked_toggle() {
        let mut entry = GodFileEntry {
            path: PathBuf::from("/a.rs"),
            rel_path: "a.rs".to_string(),
            line_count: 1500,
            checked: false,
        };
        entry.checked = true;
        assert!(entry.checked);
        entry.checked = false;
        assert!(!entry.checked);
    }

    #[test]
    fn test_god_file_entry_clone() {
        let entry = GodFileEntry {
            path: PathBuf::from("/x.rs"),
            rel_path: "x.rs".to_string(),
            line_count: 3000,
            checked: true,
        };
        let cloned = entry.clone();
        assert_eq!(entry.path, cloned.path);
        assert_eq!(entry.line_count, cloned.line_count);
        assert_eq!(entry.checked, cloned.checked);
    }

    // ── RustModuleStyle enum ──

    #[test]
    fn test_rust_module_style_eq() {
        assert_eq!(RustModuleStyle::FileBased, RustModuleStyle::FileBased);
        assert_eq!(RustModuleStyle::ModRs, RustModuleStyle::ModRs);
        assert_ne!(RustModuleStyle::FileBased, RustModuleStyle::ModRs);
    }

    #[test]
    fn test_rust_module_style_copy() {
        let s = RustModuleStyle::FileBased;
        let s2 = s;
        assert_eq!(s, s2);
    }

    #[test]
    fn test_rust_module_style_debug() {
        assert_eq!(format!("{:?}", RustModuleStyle::FileBased), "FileBased");
        assert_eq!(format!("{:?}", RustModuleStyle::ModRs), "ModRs");
    }

    // ── PythonModuleStyle enum ──

    #[test]
    fn test_python_module_style_eq() {
        assert_eq!(PythonModuleStyle::Package, PythonModuleStyle::Package);
        assert_eq!(PythonModuleStyle::SingleFile, PythonModuleStyle::SingleFile);
        assert_ne!(PythonModuleStyle::Package, PythonModuleStyle::SingleFile);
    }

    #[test]
    fn test_python_module_style_copy() {
        let s = PythonModuleStyle::Package;
        let s2 = s;
        assert_eq!(s, s2);
    }

    #[test]
    fn test_python_module_style_debug() {
        assert_eq!(format!("{:?}", PythonModuleStyle::Package), "Package");
        assert_eq!(format!("{:?}", PythonModuleStyle::SingleFile), "SingleFile");
    }

    // ── ModuleStyleDialog ──

    #[test]
    fn test_module_style_dialog_construction() {
        let dialog = ModuleStyleDialog {
            has_rust: true,
            has_python: false,
            rust_style: RustModuleStyle::FileBased,
            python_style: PythonModuleStyle::Package,
            selected: 0,
        };
        assert!(dialog.has_rust);
        assert!(!dialog.has_python);
        assert_eq!(dialog.rust_style, RustModuleStyle::FileBased);
        assert_eq!(dialog.selected, 0);
    }

    #[test]
    fn test_module_style_dialog_both_languages() {
        let dialog = ModuleStyleDialog {
            has_rust: true,
            has_python: true,
            rust_style: RustModuleStyle::ModRs,
            python_style: PythonModuleStyle::SingleFile,
            selected: 1,
        };
        assert!(dialog.has_rust && dialog.has_python);
        assert_eq!(dialog.selected, 1);
    }

    // ── Prompt content verification ──

    #[test]
    fn test_prompt_mentions_re_export() {
        let prompt = build_modularize_prompt("lib.rs", 1500, None, None);
        assert!(prompt.contains("Re-export") || prompt.contains("re-export"));
    }

    #[test]
    fn test_prompt_mentions_backwards_compatibility() {
        let prompt = build_modularize_prompt("lib.rs", 1500, None, None);
        assert!(prompt.contains("backwards compatibility"));
    }

    #[test]
    fn test_prompt_mentions_single_responsibility() {
        let prompt = build_modularize_prompt("lib.rs", 1500, None, None);
        assert!(prompt.contains("single, clear responsibility"));
    }

    #[test]
    fn test_prompt_mentions_not_util_or_helpers() {
        let prompt = build_modularize_prompt("lib.rs", 1500, None, None);
        assert!(prompt.contains("not util.rs or helpers.rs"));
    }

    // ── scan_dir_recursive with different root ──

    // ── count_source_lines ──

    #[test]
    fn test_count_source_lines_no_test_module() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("clean.rs");
        fs::write(&path, "fn main() {\n    println!(\"hi\");\n}\n").unwrap();
        assert_eq!(count_source_lines(&path), Some(3));
    }

    #[test]
    fn test_count_source_lines_with_test_module_at_end() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("with_tests.rs");
        let content = "\
fn add(a: i32, b: i32) -> i32 { a + b }
fn sub(a: i32, b: i32) -> i32 { a - b }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        assert_eq!(add(1, 2), 3);
    }

    #[test]
    fn test_sub() {
        assert_eq!(sub(3, 1), 2);
    }
}
";
        fs::write(&path, content).unwrap();
        // Only 3 source lines (2 fns + 1 blank), test block excluded
        assert_eq!(count_source_lines(&path), Some(3));
    }

    #[test]
    fn test_count_source_lines_nested_braces_in_test() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nested.rs");
        let content = "\
fn prod() {}

#[cfg(test)]
mod tests {
    fn helper() {
        if true {
            let v = vec![1, 2, 3];
            for x in v {
                println!(\"{}\", x);
            }
        }
    }

    #[test]
    fn test_it() { assert!(true); }
}
";
        fs::write(&path, content).unwrap();
        // 2 source lines: fn prod() {} and blank line
        assert_eq!(count_source_lines(&path), Some(2));
    }

    #[test]
    fn test_count_source_lines_non_rust_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("app.py");
        let content = "def main():\n    pass\n\n# test stuff\ndef test_main():\n    assert True\n";
        fs::write(&path, content).unwrap();
        // Non-Rust files count all lines
        assert_eq!(count_source_lines(&path), Some(6));
    }

    #[test]
    fn test_count_source_lines_cfg_test_on_same_line_as_mod() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("inline.rs");
        let content = "\
fn foo() {}
#[cfg(test)] mod tests {
    #[test]
    fn t() {}
}
";
        fs::write(&path, content).unwrap();
        assert_eq!(count_source_lines(&path), Some(1));
    }

    #[test]
    fn test_count_source_lines_no_test_block_all_counted() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("all.rs");
        let content: String = (0..500).map(|i| format!("// line {}\n", i)).collect();
        fs::write(&path, &content).unwrap();
        assert_eq!(count_source_lines(&path), Some(500));
    }

    #[test]
    fn test_count_source_lines_empty_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("empty.rs");
        fs::write(&path, "").unwrap();
        assert_eq!(count_source_lines(&path), Some(0));
    }

    #[test]
    fn test_count_source_lines_nonexistent() {
        assert_eq!(count_source_lines(Path::new("/no/such/file.rs")), None);
    }

    #[test]
    fn test_count_source_lines_excludes_large_test_block() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("big_tests.rs");
        let mut content = String::new();
        // 200 lines of production code
        for i in 0..200 {
            content.push_str(&format!("fn func_{}() {{}}\n", i));
        }
        // 900 lines of test code
        content.push_str("#[cfg(test)]\nmod tests {\n");
        for i in 0..896 {
            content.push_str(&format!("    // test line {}\n", i));
        }
        content.push_str("    #[test]\n    fn t() {}\n}\n");
        fs::write(&path, &content).unwrap();
        // Only 200 production lines should be counted
        assert_eq!(count_source_lines(&path), Some(200));
    }

    #[test]
    fn test_count_source_lines_scan_integration() {
        // A file with 600 source + 500 test lines = 1100 total.
        // Without exclusion it would be flagged as god file (>1000).
        // With exclusion, 600 source lines < 1000 — not a god file.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let mut content = String::new();
        for i in 0..600 {
            content.push_str(&format!("fn func_{}() {{}}\n", i));
        }
        content.push_str("#[cfg(test)]\nmod tests {\n");
        for i in 0..498 {
            content.push_str(&format!("    // test {}\n", i));
        }
        content.push_str("}\n");
        fs::write(root.join("almost.rs"), &content).unwrap();
        let mut results = Vec::new();
        scan_dir_recursive(root, root, &mut results);
        assert!(results.is_empty(), "600 source lines should not be flagged");
    }

    #[test]
    fn test_count_source_lines_scan_still_flags_large_source() {
        // 1100 source lines + 200 test lines = file still a god file
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let mut content = String::new();
        for i in 0..1100 {
            content.push_str(&format!("fn func_{}() {{}}\n", i));
        }
        content.push_str("#[cfg(test)]\nmod tests {\n");
        for i in 0..198 {
            content.push_str(&format!("    // test {}\n", i));
        }
        content.push_str("}\n");
        fs::write(root.join("big.rs"), &content).unwrap();
        let mut results = Vec::new();
        scan_dir_recursive(root, root, &mut results);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line_count, 1100);
    }

    #[test]
    fn test_scan_dir_recursive_subdir_scan() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("src")).unwrap();
        make_source_file(&root.join("src"), "big.rs", 2000);
        // Scan only src/ but with root as root for rel_path
        let mut results = Vec::new();
        scan_dir_recursive(root, &root.join("src"), &mut results);
        assert_eq!(results.len(), 1);
        let expected = std::path::Path::new("src").join("big.rs");
        assert_eq!(results[0].rel_path, expected.to_string_lossy());
    }
}
