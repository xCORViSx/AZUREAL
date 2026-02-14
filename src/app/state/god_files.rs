//! God File System — scans project for oversized source files and spawns
//! modularization sessions. "God files" are source files that exceed 1000 lines,
//! indicating they've accumulated too many responsibilities and should be split
//! into smaller, focused modules.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::app::types::{GodFileEntry, GodFilePanel};
use crate::claude::ClaudeProcess;

use super::App;

/// Minimum line count for a file to be considered a "god file"
const GOD_FILE_THRESHOLD: usize = 1000;

/// Source file extensions we scan (~60 programming languages)
const SOURCE_EXTENSIONS: &[&str] = &[
    // Systems / compiled
    "rs", "go", "c", "h", "cpp", "hpp", "cc", "cxx", "hxx", "c++", "h++",
    "m", "mm",                          // Objective-C / Objective-C++
    "swift", "zig", "nim", "cr", "v",   // Swift, Zig, Nim, Crystal, V
    "d",                                // D
    // JVM
    "java", "kt", "kts", "scala", "groovy", "gradle",
    // .NET
    "cs", "fs", "fsi", "vb",
    // Web / JS ecosystem
    "js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts",
    "vue", "svelte", "astro",
    // Scripting
    "py", "pyw", "rb", "pl", "pm", "php", "lua",
    "r",                                // R
    "jl",                               // Julia
    "dart",                             // Dart
    // Functional
    "hs", "lhs",                        // Haskell
    "ml", "mli",                        // OCaml
    "clj", "cljs", "cljc", "edn",      // Clojure
    "ex", "exs",                        // Elixir
    "erl", "hrl",                       // Erlang
    "elm",                              // Elm
    "gleam",                            // Gleam
    "rkt",                              // Racket
    // Shell / config-as-code
    "sh", "bash", "zsh", "fish",
    // Infrastructure / query
    "sql", "tf", "hcl",
    // Markup-adjacent that can grow huge
    "proto",                            // Protocol Buffers
    "thrift",                           // Apache Thrift
    "graphql", "gql",                   // GraphQL schemas
];

/// Directories to skip during scan (build artifacts, deps, VCS, non-source content)
const SKIP_DIRS: &[&str] = &[
    // VCS
    ".git",
    // Build artifacts / output
    "target", "dist", "build", ".build", "out", "bin", "obj",
    // Dependency caches
    "node_modules", "__pycache__", ".next", ".nuxt", "vendor", "Pods",
    "venv", ".venv", "env", ".env", ".tox", "site-packages",
    // IDE / editor
    ".idea", ".vscode", ".vs",
    // Non-source content that accumulates large files with source extensions
    "refs", "reference", "references", "assets", "resources", "res",
    "data", "dataset", "datasets", "fixtures", "testdata", "test_data",
    "examples", "example", "samples", "sample", "demo", "demos",
    "docs", "doc", "documentation", "wiki",
    "migrations", "db", "seeds", "dump",
    "generated", "gen", "auto", "autogen",
    "third_party", "third-party", "thirdparty", "external", "extern",
    "archive", "archives", "backup", "backups", "old",
    "tmp", "temp", ".tmp", ".cache", "cache",
    "logs", "log",
    "coverage", ".nyc_output", "htmlcov",
    "snap", "snapshots", "__snapshots__",
    ".terraform", ".serverless",
];

/// Well-known source root directories across programming ecosystems.
/// If ANY of these exist under the project root, we ONLY scan inside them
/// (plus the root level itself for top-level source files like main.rs, lib.rs).
/// If NONE exist, we fall back to scanning the entire project root.
const SOURCE_ROOTS: &[&str] = &[
    // Rust
    "src", "crates",
    // Go
    "cmd", "pkg", "internal",
    // Java / Kotlin / Scala
    "app", "core", "common", "modules", "services",
    // Python
    // ("src" already listed)
    // JavaScript / TypeScript
    "lib", "packages", "components",
    // Swift / iOS
    "Sources",
    // C / C++
    "include", "source",
];

impl App {
    /// Open the God File panel — scans the project and shows results.
    /// Uses persisted scope from .azureal/godfilescope if it exists;
    /// otherwise falls back to auto-detected source roots.
    pub fn open_god_file_panel(&mut self) {
        let entries = if let Some(ref project) = self.project {
            if let Some(dirs) = load_god_file_scope(&project.path) {
                self.scan_god_files_with_dirs(&dirs)
            } else {
                self.scan_god_files()
            }
        } else {
            self.scan_god_files()
        };
        self.god_file_panel = Some(GodFilePanel {
            entries,
            selected: 0,
            scroll: 0,
        });
    }

    /// Close the God File panel without taking action
    pub fn close_god_file_panel(&mut self) {
        self.god_file_panel = None;
        self.god_file_filter_mode = false;
        self.god_file_filter_dirs.clear();
    }

    /// Enter god file scope mode — opens the FileTree overlay with green highlights
    /// on directories that are currently in the scan scope. User can toggle dirs
    /// with Enter, then Esc to rescan and return to the god file panel.
    /// Loads persisted scope from .azureal/godfilescope if it exists; otherwise
    /// falls back to auto-detected SOURCE_ROOTS.
    pub fn enter_god_file_scope_mode(&mut self) {
        let Some(ref project) = self.project else { return };
        let root = &project.path;

        // Try loading persisted scope first
        let dirs = load_god_file_scope(root).unwrap_or_else(|| {
            // Fall back to auto-detected source roots
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

        // Open file tree overlay so the user can see and modify scope
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
        // Redraw file tree to update green highlights
        self.invalidate_file_tree();
    }

    /// Exit god file scope mode — persist the scope to .azureal/godfilescope,
    /// rescan with the user's custom directory scope, then reopen the god file
    /// panel with updated results.
    pub fn exit_god_file_scope_mode(&mut self) {
        // Persist scope so it survives panel close / app restart
        if let Some(ref project) = self.project {
            save_god_file_scope(&project.path, &self.god_file_filter_dirs);
        }
        // Rescan using the custom scope dirs
        let entries = self.scan_god_files_with_dirs(&self.god_file_filter_dirs.clone());
        self.god_file_filter_mode = false;
        self.god_file_filter_dirs.clear();

        // Close file tree and reopen god file panel with new results
        self.show_file_tree = false;
        self.focus = crate::app::Focus::Worktrees;
        self.god_file_panel = Some(GodFilePanel {
            entries,
            selected: 0,
            scroll: 0,
        });
    }

    /// Open checked god files as viewer tabs. Fills available tab slots
    /// (up to the 12-tab max), skipping files that are already open in a tab.
    /// Closes the panel and focuses the Viewer pane on the last opened tab.
    pub fn god_file_view_checked(&mut self) {
        const MAX_TABS: usize = 12;
        // Collect absolute paths of checked entries
        let paths: Vec<std::path::PathBuf> = match self.god_file_panel {
            Some(ref panel) => panel.entries.iter()
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
            // Skip if this file is already open in a tab
            if self.viewer_tabs.iter().any(|t| t.path.as_ref() == Some(path)) {
                skipped_dup += 1;
                continue;
            }
            if self.viewer_tabs.len() >= MAX_TABS {
                skipped_cap += paths.len() - opened - skipped_dup - skipped_cap;
                break;
            }
            // Read file content and create tab
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

        // Close panel, load last tab into viewer, focus viewer
        self.god_file_panel = None;
        if opened > 0 {
            self.viewer_active_tab = self.viewer_tabs.len() - 1;
            self.load_tab_to_viewer();
            self.focus = crate::app::Focus::Viewer;
        }

        // Status message tells user what happened
        let mut msg = format!("Opened {} file{}", opened, if opened == 1 { "" } else { "s" });
        if skipped_dup > 0 { msg.push_str(&format!(", {} already tabbed", skipped_dup)); }
        if skipped_cap > 0 { msg.push_str(&format!(", {} skipped (max {} tabs)", skipped_cap, MAX_TABS)); }
        self.set_status(msg);
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

    /// Spawn modularization sessions for ALL checked god files simultaneously.
    /// Each file gets its own concurrent Claude process on the main worktree.
    /// The newest spawn becomes the active slot (its output is displayed).
    pub fn god_file_modularize(&mut self, claude_process: &ClaudeProcess) {
        // Collect checked files and build prompts
        let checked: Vec<(String, usize)> = match self.god_file_panel {
            Some(ref panel) => panel.entries.iter()
                .filter(|e| e.checked)
                .map(|e| (e.rel_path.clone(), e.line_count))
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

        self.god_file_panel = None;

        // Spawn ALL checked files concurrently — each gets its own PID slot
        let mut spawned = 0usize;
        let mut failed = 0usize;
        for (rel_path, lines) in &checked {
            let prompt = build_modularize_prompt(rel_path, *lines);
            let filename = Path::new(rel_path).file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| rel_path.clone());

            match claude_process.spawn(&main_path, &prompt, None) {
                Ok((rx, pid)) => {
                    let slot = pid.to_string();
                    self.pending_session_names.push((slot, format!("[GFM] {}", filename)));
                    self.register_claude(main_branch.clone(), pid, rx);
                    spawned += 1;
                }
                Err(_) => { failed += 1; }
            }
        }

        // Switch view to main worktree so GFM output is visible
        self.switch_to_main_worktree(&main_branch);

        if failed == 0 {
            self.set_status(format!("Modularizing {} files simultaneously", spawned));
        } else {
            self.set_status(format!("Modularizing {} files ({} failed to start)", spawned, failed));
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
    /// Uses source-root detection: if well-known source directories (src/, lib/,
    /// crates/, etc.) exist, only scans those + top-level files. Otherwise falls
    /// back to scanning the entire project root.
    fn scan_god_files(&self) -> Vec<GodFileEntry> {
        let Some(ref project) = self.project else { return Vec::new() };
        let root = &project.path;

        // Detect which source roots exist in this project
        let found_roots: HashSet<PathBuf> = SOURCE_ROOTS.iter()
            .map(|name| root.join(name))
            .filter(|p| p.is_dir())
            .collect();

        if found_roots.is_empty() {
            // No recognized source roots — scan entire project
            let mut all = HashSet::new();
            all.insert(root.clone());
            self.scan_god_files_with_dirs(&all)
        } else {
            self.scan_god_files_with_dirs(&found_roots)
        }
    }

    /// Scan specific directories for god files. Used by both the auto-detect path
    /// and the user-customized filter mode path.
    fn scan_god_files_with_dirs(&self, dirs: &HashSet<PathBuf>) -> Vec<GodFileEntry> {
        let Some(ref project) = self.project else { return Vec::new() };
        let root = &project.path;
        let mut entries = Vec::new();

        // Check if the project root itself is in the set (full-project scan)
        let scanning_root = dirs.contains(root);

        if scanning_root && dirs.len() == 1 {
            // Full-project scan — recursive from root
            scan_dir_recursive(root, root, &mut entries);
        } else {
            // Scan each specified directory
            for dir in dirs {
                if dir.is_dir() {
                    scan_dir_recursive(root, dir, &mut entries);
                }
            }
            // Also scan top-level files (e.g. main.rs, build.rs)
            scan_top_level_files(root, &mut entries);
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
        if !path.is_file() { continue; }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !SOURCE_EXTENSIONS.contains(&ext) { continue; }
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

/// Recursively scan a directory for source files exceeding the LOC threshold.
/// Skips hidden directories and known build/dependency/non-source directories.
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
            // Skip known non-source directories (case-insensitive for common variations)
            let name_lower = name.to_ascii_lowercase();
            if SKIP_DIRS.iter().any(|&s| s == name_lower) { continue; }
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

/// Load persisted god file scope from .azureal/godfilescope.
/// File format: one absolute path per line. Returns None if file doesn't exist
/// or is empty, signaling the caller to use auto-detection instead.
fn load_god_file_scope(project_root: &Path) -> Option<HashSet<PathBuf>> {
    let scope_path = project_root.join(".azureal").join("godfilescope");
    let content = fs::read_to_string(&scope_path).ok()?;
    let dirs: HashSet<PathBuf> = content.lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect();
    if dirs.is_empty() { None } else { Some(dirs) }
}

/// Save god file scope to .azureal/godfilescope.
/// Stores one absolute path per line — simple, human-readable, no serde needed.
fn save_god_file_scope(project_root: &Path, dirs: &HashSet<PathBuf>) {
    let azureal_dir = project_root.join(".azureal");
    let _ = fs::create_dir_all(&azureal_dir);
    let content: String = dirs.iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let _ = fs::write(azureal_dir.join("godfilescope"), content);
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
