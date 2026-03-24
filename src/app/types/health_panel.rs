//! Worktree Health panel types — god files, documentation coverage, module style selection

use std::path::PathBuf;

/// A source file detected as a "god file" (>1k LOC) — candidate for modularization
#[derive(Debug, Clone)]
pub struct GodFileEntry {
    /// Absolute path to the file
    pub path: PathBuf,
    /// Path relative to project root (for display)
    pub rel_path: String,
    /// Total line count in the file
    pub line_count: usize,
    /// Whether the user checked this file for modularization
    pub checked: bool,
}

/// Rust module organization: file-based root (modern) vs mod.rs (legacy)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustModuleStyle {
    /// Modern: `modulename.rs` as root alongside `modulename/` directory
    FileBased,
    /// Legacy: `modulename/mod.rs` as root inside the directory
    ModRs,
}

/// Python module organization: package with __init__.py vs single-file modules
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PythonModuleStyle {
    /// Directory package with `__init__.py` re-exporting public names
    Package,
    /// Standalone `.py` files with explicit imports (no __init__.py)
    SingleFile,
}

/// Pre-modularize dialog: lets user pick module style for Rust/Python files
/// before spawning GFM sessions. Only shown when checked files include .rs/.py.
#[derive(Debug, Clone)]
pub struct ModuleStyleDialog {
    /// Whether any checked files are .rs
    pub has_rust: bool,
    /// Whether any checked files are .py
    pub has_python: bool,
    /// Currently selected Rust module style
    pub rust_style: RustModuleStyle,
    /// Currently selected Python module style
    pub python_style: PythonModuleStyle,
    /// Cursor row: 0 = first visible language, 1 = second (if both present)
    pub selected: usize,
}

/// Which tab is active in the Worktree Health panel
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HealthTab {
    /// God Files — source files exceeding 1000 LOC
    GodFiles,
    /// Documentation — measures doc-comment coverage across source files
    Documentation,
}

/// State for the Worktree Health panel — tabbed modal overlay housing
/// multiple health-check systems (god files, documentation coverage, etc.)
#[derive(Debug)]
pub struct HealthPanel {
    /// Worktree display name shown in the panel title (e.g. "Health: my-feature")
    pub worktree_name: String,
    /// Which tab is currently active/visible
    pub tab: HealthTab,
    // ── God Files tab ──
    /// All source files exceeding the LOC threshold
    pub god_files: Vec<GodFileEntry>,
    /// Navigation cursor in god files list
    pub god_selected: usize,
    /// Scroll offset for god files list
    pub god_scroll: usize,
    // ── Documentation tab ──
    /// All source files with documentation coverage metrics
    pub doc_entries: Vec<DocEntry>,
    /// Navigation cursor in doc entries list
    pub doc_selected: usize,
    /// Scroll offset for doc entries list
    pub doc_scroll: usize,
    /// Overall documentation score 0.0–100.0 across all scanned files
    pub doc_score: f32,
    /// When Some, the module style selector is shown before modularizing.
    /// Set when Enter/m pressed and checked files include .rs or .py.
    pub module_style_dialog: Option<ModuleStyleDialog>,
}

/// A source file with documentation coverage metrics — how many documentable
/// items (fns, structs, enums, traits, consts, etc.) have doc comments
#[derive(Debug, Clone)]
pub struct DocEntry {
    /// Absolute path to the file
    pub path: PathBuf,
    /// Path relative to project root (for display)
    pub rel_path: String,
    /// Total documentable items found (fns, structs, enums, traits, consts, types, impls)
    pub total_items: usize,
    /// How many of those items have a preceding /// or //! doc comment
    pub documented_items: usize,
    /// Per-file coverage percentage 0.0–100.0
    pub coverage_pct: f32,
    /// Whether this entry is checked for batch doc-health session spawning
    pub checked: bool,
}
