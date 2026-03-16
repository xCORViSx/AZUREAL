//! Worktree Health system — scans project source files for structural and
//! documentation issues. Split into submodules:
//! - `god_files`: detects oversized files (>1000 LOC) and spawns modularization sessions
//! - `documentation`: measures doc-comment coverage and spawns documentation sessions

mod documentation;
mod god_files;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::app::types::HealthPanel;

use super::App;

/// Source file extensions we scan (~60 programming languages)
pub(crate) const SOURCE_EXTENSIONS: &[&str] = &[
    // Systems / compiled
    "rs", "go", "c", "h", "cpp", "hpp", "cc", "cxx", "hxx", "c++", "h++", "m",
    "mm", // Objective-C / Objective-C++
    "swift", "zig", "nim", "cr", "v", // Swift, Zig, Nim, Crystal, V
    "d", // D
    // JVM
    "java", "kt", "kts", "scala", "groovy", "gradle", // .NET
    "cs", "fs", "fsi", "vb", // Web / JS ecosystem
    "js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts", "vue", "svelte", "astro",
    // Scripting
    "py", "pyw", "rb", "pl", "pm", "php", "lua", "r",    // R
    "jl",   // Julia
    "dart", // Dart
    // Functional
    "hs", "lhs", // Haskell
    "ml", "mli", // OCaml
    "clj", "cljs", "cljc", "edn", // Clojure
    "ex", "exs", // Elixir
    "erl", "hrl",   // Erlang
    "elm",   // Elm
    "gleam", // Gleam
    "rkt",   // Racket
    // Shell / config-as-code
    "sh", "bash", "zsh", "fish", // Infrastructure / query
    "sql", "tf", "hcl", // Markup-adjacent that can grow huge
    "proto",  // Protocol Buffers
    "thrift", // Apache Thrift
    "graphql", "gql", // GraphQL schemas
];

/// Directories to skip during scan (build artifacts, deps, VCS, non-source content)
pub(crate) const SKIP_DIRS: &[&str] = &[
    // VCS
    ".git",
    // Build artifacts / output
    "target",
    "dist",
    "build",
    ".build",
    "out",
    "bin",
    "obj",
    // Dependency caches
    "node_modules",
    "__pycache__",
    ".next",
    ".nuxt",
    "vendor",
    "Pods",
    "venv",
    ".venv",
    "env",
    ".env",
    ".tox",
    "site-packages",
    // IDE / editor
    ".idea",
    ".vscode",
    ".vs",
    // Non-source content that accumulates large files with source extensions
    "refs",
    "reference",
    "references",
    "assets",
    "resources",
    "res",
    "data",
    "dataset",
    "datasets",
    "fixtures",
    "testdata",
    "test_data",
    "examples",
    "example",
    "samples",
    "sample",
    "demo",
    "demos",
    "docs",
    "doc",
    "documentation",
    "wiki",
    "migrations",
    "db",
    "seeds",
    "dump",
    "generated",
    "gen",
    "auto",
    "autogen",
    "third_party",
    "third-party",
    "thirdparty",
    "external",
    "extern",
    "archive",
    "archives",
    "backup",
    "backups",
    "old",
    "tmp",
    "temp",
    ".tmp",
    ".cache",
    "cache",
    "logs",
    "log",
    "coverage",
    ".nyc_output",
    "htmlcov",
    "snap",
    "snapshots",
    "__snapshots__",
    ".terraform",
    ".serverless",
];

/// Well-known source root directories across programming ecosystems.
/// If ANY of these exist under the project root, we ONLY scan inside them
/// (plus the root level itself for top-level source files like main.rs, lib.rs).
/// If NONE exist, we fall back to scanning the entire project root.
pub(crate) const SOURCE_ROOTS: &[&str] = &[
    // Rust
    "src",
    "crates",
    // Go
    "cmd",
    "pkg",
    "internal",
    // Java / Kotlin / Scala
    "app",
    "core",
    "common",
    "modules",
    "services",
    // Python
    // ("src" already listed)
    // JavaScript / TypeScript
    "lib",
    "packages",
    "components",
    // Swift / iOS
    "Sources",
    // C / C++
    "include",
    "source",
];

impl App {
    /// Effective root for health scans — uses the current worktree path so scans
    /// reflect the actual files on the working branch, falling back to project.path.
    pub(crate) fn health_scan_root(&self) -> Option<PathBuf> {
        self.current_worktree()
            .and_then(|wt| wt.worktree_path.clone())
            .or_else(|| self.project.as_ref().map(|p| p.path.clone()))
    }

    /// Translate scope dirs persisted under `project.path` to the current worktree root.
    /// Scope dirs are absolute paths (e.g., `/repo/src`); when scanning a worktree
    /// (e.g., `/repo/worktrees/run`) we need `/repo/worktrees/run/src`.
    fn translate_scope_dirs(&self, dirs: &HashSet<PathBuf>) -> HashSet<PathBuf> {
        let Some(ref project) = self.project else {
            return dirs.clone();
        };
        let project_root = &project.path;
        let wt_root = match self.health_scan_root() {
            Some(r) => r,
            None => return dirs.clone(),
        };
        if wt_root == *project_root {
            return dirs.clone();
        }
        dirs.iter()
            .map(|p| {
                if let Ok(rel) = p.strip_prefix(project_root) {
                    let translated = wt_root.join(rel);
                    if translated.is_dir() {
                        translated
                    } else {
                        p.clone()
                    }
                } else {
                    p.clone()
                }
            })
            .collect()
    }

    /// Open the Worktree Health panel — scans both god files and documentation.
    /// Uses persisted scope from `[healthscope]` in .azureal/azufig.toml if it exists;
    /// otherwise falls back to auto-detected source roots.
    pub fn open_health_panel(&mut self) {
        let god_files = if let Some(ref project) = self.project {
            if let Some(dirs) = load_health_scope(&project.path) {
                let translated = self.translate_scope_dirs(&dirs);
                self.scan_god_files_with_dirs(&translated)
            } else {
                self.scan_god_files()
            }
        } else {
            self.scan_god_files()
        };
        let (doc_entries, doc_score) = self.scan_documentation();
        // Grab the display name of the currently selected worktree for the panel title
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

    /// Refresh health panel data in-place after file changes.
    /// Preserves tab, cursor positions, scroll offsets, and checked states.
    pub fn refresh_health_panel(&mut self) {
        let panel = match self.health_panel.as_ref() {
            Some(p) => p,
            None => return,
        };

        // Snapshot UI state to restore after rescan
        let tab = panel.tab;
        let god_scroll = panel.god_scroll;
        let doc_scroll = panel.doc_scroll;
        let dialog = panel.module_style_dialog.clone();

        // Collect checked file paths (rel_path is stable across rescans)
        let god_checked: HashSet<&str> = panel
            .god_files
            .iter()
            .filter(|f| f.checked)
            .map(|f| f.rel_path.as_str())
            .collect();
        let doc_checked: HashSet<&str> = panel
            .doc_entries
            .iter()
            .filter(|f| f.checked)
            .map(|f| f.rel_path.as_str())
            .collect();

        // Rescan
        let mut god_files = if let Some(ref project) = self.project {
            if let Some(dirs) = load_health_scope(&project.path) {
                let translated = self.translate_scope_dirs(&dirs);
                self.scan_god_files_with_dirs(&translated)
            } else {
                self.scan_god_files()
            }
        } else {
            self.scan_god_files()
        };
        let (mut doc_entries, doc_score) = self.scan_documentation();

        // Restore checked state
        for f in &mut god_files {
            f.checked = god_checked.contains(f.rel_path.as_str());
        }
        for f in &mut doc_entries {
            f.checked = doc_checked.contains(f.rel_path.as_str());
        }

        // Clamp cursors to new list bounds
        let god_selected = self
            .health_panel
            .as_ref()
            .map(|p| p.god_selected.min(god_files.len().saturating_sub(1)))
            .unwrap_or(0);
        let doc_selected = self
            .health_panel
            .as_ref()
            .map(|p| p.doc_selected.min(doc_entries.len().saturating_sub(1)))
            .unwrap_or(0);

        let worktree_name = self
            .selected_worktree
            .map(|i| self.worktrees[i].name().to_string())
            .unwrap_or_default();

        self.health_panel = Some(HealthPanel {
            worktree_name,
            tab,
            god_files,
            god_selected,
            god_scroll,
            doc_entries,
            doc_selected,
            doc_scroll,
            doc_score,
            module_style_dialog: dialog,
        });
    }

    /// Close the health panel, remembering which tab was active
    pub fn close_health_panel(&mut self) {
        if let Some(ref panel) = self.health_panel {
            self.last_health_tab = panel.tab;
        }
        self.health_panel = None;
        self.health_refresh_pending = false;
        self.god_file_filter_mode = false;
        self.god_file_filter_dirs.clear();
    }

    /// Get the current worktree's branch name and path for spawning sessions.
    /// Health sessions (GFM, DH) run on the current worktree — changes merge back to main.
    pub(crate) fn current_worktree_info(&self) -> Option<(String, PathBuf)> {
        let wt = self.current_worktree()?;
        wt.worktree_path
            .as_ref()
            .map(|p| (wt.branch_name.clone(), p.clone()))
    }
}

/// Load persisted health scope from `[healthscope]` in project azufig.
/// Returns None if no scope is configured or all dirs are gone,
/// signaling the caller to use auto-detection instead.
pub(crate) fn load_health_scope(project_root: &Path) -> Option<HashSet<PathBuf>> {
    let az = crate::azufig::load_project_azufig(project_root);
    let dirs: HashSet<PathBuf> = az
        .healthscope
        .dirs
        .iter()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect();
    if dirs.is_empty() {
        None
    } else {
        Some(dirs)
    }
}

/// Save health scope to `[healthscope]` in project azufig (load-modify-save).
pub(crate) fn save_health_scope(project_root: &Path, dirs: &HashSet<PathBuf>) {
    crate::azufig::update_project_azufig(project_root, |az| {
        az.healthscope.dirs = dirs.iter().map(|p| p.display().to_string()).collect();
    });
}

/// Collect all source files recursively from a directory (for doc scanner).
/// Same skip logic as god file scanner but collects ALL source files, not just >1000 LOC.
pub(crate) fn collect_source_files(dir: &Path, results: &mut Vec<PathBuf>) {
    let rd = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut dir_entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
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
            collect_source_files(&path, results);
        } else if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if SOURCE_EXTENSIONS.contains(&ext) {
                results.push(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── SOURCE_EXTENSIONS constant ──

    #[test]
    fn test_source_extensions_contains_rust() {
        assert!(SOURCE_EXTENSIONS.contains(&"rs"));
    }

    #[test]
    fn test_source_extensions_contains_python() {
        assert!(SOURCE_EXTENSIONS.contains(&"py"));
    }

    #[test]
    fn test_source_extensions_contains_javascript() {
        assert!(SOURCE_EXTENSIONS.contains(&"js"));
    }

    #[test]
    fn test_source_extensions_contains_typescript() {
        assert!(SOURCE_EXTENSIONS.contains(&"ts"));
        assert!(SOURCE_EXTENSIONS.contains(&"tsx"));
    }

    #[test]
    fn test_source_extensions_contains_go() {
        assert!(SOURCE_EXTENSIONS.contains(&"go"));
    }

    #[test]
    fn test_source_extensions_contains_c_cpp() {
        assert!(SOURCE_EXTENSIONS.contains(&"c"));
        assert!(SOURCE_EXTENSIONS.contains(&"h"));
        assert!(SOURCE_EXTENSIONS.contains(&"cpp"));
        assert!(SOURCE_EXTENSIONS.contains(&"hpp"));
    }

    #[test]
    fn test_source_extensions_contains_java_kotlin() {
        assert!(SOURCE_EXTENSIONS.contains(&"java"));
        assert!(SOURCE_EXTENSIONS.contains(&"kt"));
    }

    #[test]
    fn test_source_extensions_contains_swift() {
        assert!(SOURCE_EXTENSIONS.contains(&"swift"));
    }

    #[test]
    fn test_source_extensions_contains_shell() {
        assert!(SOURCE_EXTENSIONS.contains(&"sh"));
        assert!(SOURCE_EXTENSIONS.contains(&"bash"));
        assert!(SOURCE_EXTENSIONS.contains(&"zsh"));
    }

    #[test]
    fn test_source_extensions_contains_sql() {
        assert!(SOURCE_EXTENSIONS.contains(&"sql"));
    }

    #[test]
    fn test_source_extensions_contains_vue_svelte() {
        assert!(SOURCE_EXTENSIONS.contains(&"vue"));
        assert!(SOURCE_EXTENSIONS.contains(&"svelte"));
    }

    #[test]
    fn test_source_extensions_contains_ruby() {
        assert!(SOURCE_EXTENSIONS.contains(&"rb"));
    }

    #[test]
    fn test_source_extensions_contains_elixir() {
        assert!(SOURCE_EXTENSIONS.contains(&"ex"));
        assert!(SOURCE_EXTENSIONS.contains(&"exs"));
    }

    #[test]
    fn test_source_extensions_contains_haskell() {
        assert!(SOURCE_EXTENSIONS.contains(&"hs"));
    }

    #[test]
    fn test_source_extensions_does_not_contain_data_formats() {
        assert!(!SOURCE_EXTENSIONS.contains(&"json"));
        assert!(!SOURCE_EXTENSIONS.contains(&"yaml"));
        assert!(!SOURCE_EXTENSIONS.contains(&"xml"));
        assert!(!SOURCE_EXTENSIONS.contains(&"csv"));
        assert!(!SOURCE_EXTENSIONS.contains(&"toml"));
    }

    #[test]
    fn test_source_extensions_does_not_contain_images() {
        assert!(!SOURCE_EXTENSIONS.contains(&"png"));
        assert!(!SOURCE_EXTENSIONS.contains(&"jpg"));
        assert!(!SOURCE_EXTENSIONS.contains(&"gif"));
    }

    #[test]
    fn test_source_extensions_does_not_contain_markdown() {
        assert!(!SOURCE_EXTENSIONS.contains(&"md"));
    }

    #[test]
    fn test_source_extensions_is_not_empty() {
        assert!(!SOURCE_EXTENSIONS.is_empty());
        assert!(SOURCE_EXTENSIONS.len() > 50, "should have ~60+ extensions");
    }

    #[test]
    fn test_source_extensions_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for ext in SOURCE_EXTENSIONS {
            assert!(seen.insert(ext), "duplicate extension: {}", ext);
        }
    }

    // ── SKIP_DIRS constant ──

    #[test]
    fn test_skip_dirs_contains_git() {
        assert!(SKIP_DIRS.contains(&".git"));
    }

    #[test]
    fn test_skip_dirs_contains_target() {
        assert!(SKIP_DIRS.contains(&"target"));
    }

    #[test]
    fn test_skip_dirs_contains_node_modules() {
        assert!(SKIP_DIRS.contains(&"node_modules"));
    }

    #[test]
    fn test_skip_dirs_contains_build_artifacts() {
        assert!(SKIP_DIRS.contains(&"dist"));
        assert!(SKIP_DIRS.contains(&"build"));
        assert!(SKIP_DIRS.contains(&"out"));
        assert!(SKIP_DIRS.contains(&"bin"));
    }

    #[test]
    fn test_skip_dirs_contains_ide() {
        assert!(SKIP_DIRS.contains(&".idea"));
        assert!(SKIP_DIRS.contains(&".vscode"));
    }

    #[test]
    fn test_skip_dirs_contains_python_venvs() {
        assert!(SKIP_DIRS.contains(&"venv"));
        assert!(SKIP_DIRS.contains(&".venv"));
        assert!(SKIP_DIRS.contains(&"__pycache__"));
    }

    #[test]
    fn test_skip_dirs_contains_vendor() {
        assert!(SKIP_DIRS.contains(&"vendor"));
    }

    #[test]
    fn test_skip_dirs_contains_docs() {
        assert!(SKIP_DIRS.contains(&"docs"));
        assert!(SKIP_DIRS.contains(&"doc"));
    }

    #[test]
    fn test_skip_dirs_contains_examples() {
        assert!(SKIP_DIRS.contains(&"examples"));
        assert!(SKIP_DIRS.contains(&"example"));
    }

    #[test]
    fn test_skip_dirs_contains_generated() {
        assert!(SKIP_DIRS.contains(&"generated"));
        assert!(SKIP_DIRS.contains(&"gen"));
    }

    #[test]
    fn test_skip_dirs_contains_third_party() {
        assert!(SKIP_DIRS.contains(&"third_party"));
        assert!(SKIP_DIRS.contains(&"third-party"));
    }

    #[test]
    fn test_skip_dirs_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for d in SKIP_DIRS {
            assert!(seen.insert(d), "duplicate skip dir: {}", d);
        }
    }

    #[test]
    fn test_skip_dirs_is_not_empty() {
        assert!(!SKIP_DIRS.is_empty());
        assert!(SKIP_DIRS.len() > 30);
    }

    // ── SOURCE_ROOTS constant ──

    #[test]
    fn test_source_roots_contains_src() {
        assert!(SOURCE_ROOTS.contains(&"src"));
    }

    #[test]
    fn test_source_roots_contains_lib() {
        assert!(SOURCE_ROOTS.contains(&"lib"));
    }

    #[test]
    fn test_source_roots_contains_go_dirs() {
        assert!(SOURCE_ROOTS.contains(&"cmd"));
        assert!(SOURCE_ROOTS.contains(&"pkg"));
        assert!(SOURCE_ROOTS.contains(&"internal"));
    }

    #[test]
    fn test_source_roots_contains_java_dirs() {
        assert!(SOURCE_ROOTS.contains(&"app"));
        assert!(SOURCE_ROOTS.contains(&"core"));
    }

    #[test]
    fn test_source_roots_contains_swift_sources() {
        assert!(SOURCE_ROOTS.contains(&"Sources"));
    }

    #[test]
    fn test_source_roots_contains_cpp_include() {
        assert!(SOURCE_ROOTS.contains(&"include"));
    }

    #[test]
    fn test_source_roots_is_not_empty() {
        assert!(!SOURCE_ROOTS.is_empty());
    }

    #[test]
    fn test_source_roots_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for r in SOURCE_ROOTS {
            assert!(seen.insert(r), "duplicate source root: {}", r);
        }
    }

    // ── collect_source_files ──

    #[test]
    fn test_collect_source_files_finds_rs() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        assert_eq!(results.len(), 1);
        assert!(results[0].ends_with("main.rs"));
    }

    #[test]
    fn test_collect_source_files_finds_multiple_types() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("app.rs"), "").unwrap();
        fs::write(tmp.path().join("script.py"), "").unwrap();
        fs::write(tmp.path().join("index.js"), "").unwrap();
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_collect_source_files_ignores_non_source() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("data.json"), "{}").unwrap();
        fs::write(tmp.path().join("readme.md"), "# hi").unwrap();
        fs::write(tmp.path().join("image.png"), &[0u8; 10]).unwrap();
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_collect_source_files_skips_hidden_dirs() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join(".hidden")).unwrap();
        fs::write(tmp.path().join(".hidden/secret.rs"), "").unwrap();
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_collect_source_files_skips_skip_dirs() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("node_modules")).unwrap();
        fs::write(tmp.path().join("node_modules/pkg.js"), "").unwrap();
        fs::create_dir(tmp.path().join("target")).unwrap();
        fs::write(tmp.path().join("target/build.rs"), "").unwrap();
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_collect_source_files_recurses_into_subdirs() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("nested")).unwrap();
        fs::write(tmp.path().join("nested/mod.rs"), "").unwrap();
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_collect_source_files_deeply_nested() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("a/b/c")).unwrap();
        fs::write(tmp.path().join("a/b/c/deep.rs"), "").unwrap();
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_collect_source_files_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_collect_source_files_nonexistent_dir() {
        let mut results = Vec::new();
        collect_source_files(Path::new("/nonexistent/dir"), &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_collect_source_files_sorted_by_name() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("z.rs"), "").unwrap();
        fs::write(tmp.path().join("a.rs"), "").unwrap();
        fs::write(tmp.path().join("m.rs"), "").unwrap();
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        let names: Vec<_> = results
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["a.rs", "m.rs", "z.rs"]);
    }

    #[test]
    fn test_collect_source_files_skips_hidden_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".hidden.rs"), "").unwrap();
        fs::write(tmp.path().join("visible.rs"), "").unwrap();
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        assert_eq!(results.len(), 1);
        assert!(results[0].ends_with("visible.rs"));
    }

    #[test]
    fn test_collect_source_files_case_insensitive_skip_dirs() {
        let tmp = TempDir::new().unwrap();
        // SKIP_DIRS are lowercase; directory names are lowered before comparison
        fs::create_dir(tmp.path().join("Target")).unwrap();
        fs::write(tmp.path().join("Target/build.rs"), "").unwrap();
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        assert!(results.is_empty(), "Target (capital T) should be skipped");
    }

    #[test]
    fn test_collect_source_files_no_extension() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Makefile"), "").unwrap();
        fs::write(tmp.path().join("Dockerfile"), "").unwrap();
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_collect_source_files_appends_to_existing() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("new.rs"), "").unwrap();
        let mut results = vec![PathBuf::from("/existing/file.rs")];
        collect_source_files(tmp.path(), &mut results);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], PathBuf::from("/existing/file.rs"));
    }

    #[test]
    fn test_collect_source_files_all_source_types() {
        let tmp = TempDir::new().unwrap();
        for ext in &[
            "rs", "py", "js", "ts", "go", "java", "c", "cpp", "swift", "rb",
        ] {
            fs::write(tmp.path().join(format!("file.{}", ext)), "").unwrap();
        }
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        assert_eq!(results.len(), 10);
    }

    #[test]
    fn test_collect_source_files_skips_refs_dir() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("refs")).unwrap();
        fs::write(tmp.path().join("refs/helper.rs"), "").unwrap();
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_collect_source_files_skips_vendor_dir() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("vendor")).unwrap();
        fs::write(tmp.path().join("vendor/dep.go"), "").unwrap();
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_collect_source_files_skips_coverage_dirs() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("coverage")).unwrap();
        fs::write(tmp.path().join("coverage/report.js"), "").unwrap();
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        assert!(results.is_empty());
    }

    #[test]
    fn test_collect_source_files_returns_absolute_paths() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("main.rs"), "").unwrap();
        let mut results = Vec::new();
        collect_source_files(tmp.path(), &mut results);
        assert!(results[0].is_absolute());
    }
}
