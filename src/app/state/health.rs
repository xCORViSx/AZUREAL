//! Worktree Health system — scans project source files for structural and
//! documentation issues. Split into submodules:
//! - `god_files`: detects oversized files (>1000 LOC) and spawns modularization sessions
//! - `documentation`: measures doc-comment coverage and spawns documentation sessions

mod god_files;
mod documentation;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::app::types::HealthPanel;

use super::App;

/// Source file extensions we scan (~60 programming languages)
pub(crate) const SOURCE_EXTENSIONS: &[&str] = &[
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
pub(crate) const SKIP_DIRS: &[&str] = &[
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
pub(crate) const SOURCE_ROOTS: &[&str] = &[
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
    /// Uses persisted scope from `[healthscope]` in .azureal/azufig.toml if it exists;
    /// otherwise falls back to auto-detected source roots.
    pub fn open_health_panel(&mut self) {
        let god_files = if let Some(ref project) = self.project {
            if let Some(dirs) = load_health_scope(&project.path) {
                self.scan_god_files_with_dirs(&dirs)
            } else {
                self.scan_god_files()
            }
        } else {
            self.scan_god_files()
        };
        let (doc_entries, doc_score) = self.scan_documentation();
        self.health_panel = Some(HealthPanel {
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

    /// Close the health panel, remembering which tab was active
    pub fn close_health_panel(&mut self) {
        if let Some(ref panel) = self.health_panel {
            self.last_health_tab = panel.tab;
        }
        self.health_panel = None;
        self.god_file_filter_mode = false;
        self.god_file_filter_dirs.clear();
    }

    /// Switch the convo pane + sidebar selection to the main worktree.
    /// Called after spawning a GFM/DH session so output is immediately visible.
    pub(crate) fn switch_to_main_worktree(&mut self, main_branch: &str) {
        if let Some(idx) = self.worktrees.iter().position(|s| s.branch_name == main_branch) {
            if self.selected_worktree != Some(idx) {
                self.save_current_terminal();
                self.selected_worktree = Some(idx);
                self.load_session_output();
                self.invalidate_sidebar();
            }
        }
    }

    /// Find the main worktree's branch name and path
    pub(crate) fn find_main_worktree(&self) -> Option<(String, PathBuf)> {
        let project = self.project.as_ref()?;
        self.worktrees.iter()
            .find(|s| s.branch_name == project.main_branch)
            .and_then(|s| s.worktree_path.as_ref().map(|p| (s.branch_name.clone(), p.clone())))
    }
}

/// Load persisted health scope from `[healthscope]` in project azufig.
/// Returns None if no scope is configured or all dirs are gone,
/// signaling the caller to use auto-detection instead.
pub(crate) fn load_health_scope(project_root: &Path) -> Option<HashSet<PathBuf>> {
    let az = crate::azufig::load_project_azufig(project_root);
    let dirs: HashSet<PathBuf> = az.healthscope.dirs.iter()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect();
    if dirs.is_empty() { None } else { Some(dirs) }
}

/// Save health scope to `[healthscope]` in project azufig (load-modify-save).
pub(crate) fn save_health_scope(project_root: &Path, dirs: &HashSet<PathBuf>) {
    crate::azufig::update_project_azufig(project_root, |az| {
        az.healthscope.dirs = dirs.iter()
            .map(|p| p.display().to_string())
            .collect();
    });
}

/// Collect all source files recursively from a directory (for doc scanner).
/// Same skip logic as god file scanner but collects ALL source files, not just >1000 LOC.
pub(crate) fn collect_source_files(dir: &Path, results: &mut Vec<PathBuf>) {
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
