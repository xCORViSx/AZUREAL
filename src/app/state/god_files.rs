//! God File System — scans project for oversized source files and spawns
//! modularization sessions. "God files" are source files that exceed 1000 lines,
//! indicating they've accumulated too many responsibilities and should be split
//! into smaller, focused modules.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::app::types::{
    DocEntry, GodFileEntry, HealthPanel, HealthTab,
    ModuleStyleDialog, RustModuleStyle, PythonModuleStyle,
};
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
    /// Open the Worktree Health panel — scans both god files and documentation.
    /// Uses persisted scope from .azureal/godfilescope if it exists;
    /// otherwise falls back to auto-detected source roots.
    pub fn open_health_panel(&mut self) {
        let god_files = if let Some(ref project) = self.project {
            if let Some(dirs) = load_god_file_scope(&project.path) {
                self.scan_god_files_with_dirs(&dirs)
            } else {
                self.scan_god_files()
            }
        } else {
            self.scan_god_files()
        };
        let (doc_entries, doc_score) = self.scan_documentation();
        self.health_panel = Some(HealthPanel {
            tab: HealthTab::GodFiles,
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

    /// Close the health panel without taking action
    pub fn close_health_panel(&mut self) {
        self.health_panel = None;
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
    /// rescan with the user's custom directory scope, then reopen the health
    /// panel with updated results on both tabs.
    pub fn exit_god_file_scope_mode(&mut self) {
        // Persist scope so it survives panel close / app restart
        if let Some(ref project) = self.project {
            save_god_file_scope(&project.path, &self.god_file_filter_dirs);
        }
        // Rescan both god files and documentation using the custom scope
        let god_files = self.scan_god_files_with_dirs(&self.god_file_filter_dirs.clone());
        let (doc_entries, doc_score) = self.scan_documentation();
        self.god_file_filter_mode = false;
        self.god_file_filter_dirs.clear();

        // Close file tree and reopen health panel with new results
        self.show_file_tree = false;
        self.focus = crate::app::Focus::Worktrees;
        self.health_panel = Some(HealthPanel {
            tab: HealthTab::GodFiles,
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
        // Collect absolute paths of checked entries
        let paths: Vec<std::path::PathBuf> = match self.health_panel {
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
        self.health_panel = None;
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

        // Check if any checked files are Rust or Python
        let has_rust = checked.iter().any(|(p, _)| p.ends_with(".rs"));
        let has_python = checked.iter().any(|(p, _)| p.ends_with(".py"));

        if has_rust || has_python {
            // Show module style selector before spawning
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
            // No dual-style languages — spawn immediately with no style override
            self.god_file_modularize(claude_process, None, None);
        }
    }

    /// Spawn modularization sessions for ALL checked god files simultaneously.
    /// Each file gets its own concurrent Claude process on the main worktree.
    /// The newest spawn becomes the active slot (its output is displayed).
    /// `rust_style`/`python_style` are embedded in the prompt for matching files.
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

        let (main_branch, main_path) = match self.find_main_worktree() {
            Some(v) => v,
            None => { self.set_status("No main worktree found"); return; }
        };

        self.health_panel = None;

        // Spawn ALL checked files concurrently — each gets its own PID slot
        let mut spawned = 0usize;
        let mut failed = 0usize;
        for (rel_path, lines) in &checked {
            let prompt = build_modularize_prompt(rel_path, *lines, rust_style, python_style);
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

    /// Scan project source files for documentation coverage — counts documentable
    /// items (fn, struct, enum, trait, const, static, type, impl) and checks
    /// whether each has a preceding `///` or `//!` doc comment.
    /// Returns (entries sorted by coverage ascending, overall score 0.0–100.0).
    fn scan_documentation(&self) -> (Vec<DocEntry>, f32) {
        let Some(ref project) = self.project else { return (Vec::new(), 0.0) };
        let root = &project.path;

        // Determine which directories to scan (same logic as god files)
        let dirs = load_god_file_scope(root).unwrap_or_else(|| {
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
            // Top-level source files
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
            entries.push(DocEntry { path: path.clone(), rel_path, total_items: total, documented_items: documented, coverage_pct });
        }

        // Sort worst-documented first so user sees problem files at top
        entries.sort_by(|a, b| a.coverage_pct.partial_cmp(&b.coverage_pct).unwrap_or(std::cmp::Ordering::Equal));
        let doc_score = if total_all > 0 { documented_all as f32 / total_all as f32 * 100.0 } else { 100.0 };
        (entries, doc_score)
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

/// Collect all source files recursively from a directory (for doc scanner).
/// Same skip logic as scan_dir_recursive but collects ALL source files, not just god files.
fn collect_source_files(dir: &Path, results: &mut Vec<PathBuf>) {
    let rd = match fs::read_dir(dir) { Ok(r) => r, Err(_) => return };
    let mut dir_entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    dir_entries.sort_by_key(|e| e.file_name());
    for entry in dir_entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') { continue; }
        if path.is_dir() {
            let name_lower = name.to_ascii_lowercase();
            if SKIP_DIRS.iter().any(|&s| s == name_lower) { continue; }
            collect_source_files(&path, results);
        } else if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if SOURCE_EXTENSIONS.contains(&ext) { results.push(path); }
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

/// Build the modularization prompt for a specific god file.
/// Instructs Claude to read context first, then split the file into
/// smaller focused modules. For .rs/.py files, embeds the user's chosen
/// module style so Claude follows the right convention.
fn build_modularize_prompt(
    rel_path: &str,
    line_count: usize,
    rust_style: Option<RustModuleStyle>,
    python_style: Option<PythonModuleStyle>,
) -> String {
    // Base prompt — language-agnostic modularization instructions
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

    // Append language-specific module style instructions
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
