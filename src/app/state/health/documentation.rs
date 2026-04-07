//! Documentation Health system — scans project source files for doc-comment
//! coverage and spawns concurrent Claude sessions to add missing documentation.
//! Uses line-based heuristics (no AST) to detect documentable items and check
//! whether each has a preceding `///` or `//!` doc comment.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::app::types::DocEntry;
use crate::backend::AgentProcess;

use super::super::App;
use super::{collect_source_files, load_health_scope, SOURCE_EXTENSIONS, SOURCE_ROOTS};

impl App {
    /// Scan project source files for documentation coverage — counts documentable
    /// items (fn, struct, enum, trait, const, static, type, impl, mod) and checks
    /// whether each has a preceding `///` or `//!` doc comment.
    /// Returns (entries sorted by coverage ascending, overall score 0.0–100.0).
    pub(crate) fn scan_documentation(&self) -> (Vec<DocEntry>, f32) {
        let Some(root) = self.health_scan_root() else {
            return (Vec::new(), 0.0);
        };

        // Determine which directories to scan (same logic as god files)
        let dirs = self
            .project
            .as_ref()
            .and_then(|p| load_health_scope(&p.path))
            .map(|d| self.translate_scope_dirs(&d))
            .unwrap_or_else(|| {
                let found: HashSet<PathBuf> = SOURCE_ROOTS
                    .iter()
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
        let scanning_root = dirs.contains(&root) && dirs.len() == 1;
        if scanning_root {
            collect_source_files(&root, &mut files);
        } else {
            for dir in &dirs {
                if dir.is_dir() {
                    collect_source_files(dir, &mut files);
                }
            }
            // Top-level source files (e.g. main.rs, build.rs)
            if let Ok(rd) = fs::read_dir(&root) {
                for entry in rd.filter_map(|e| e.ok()) {
                    let p = entry.path();
                    if p.is_file() {
                        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                        if SOURCE_EXTENSIONS.contains(&ext) {
                            files.push(p);
                        }
                    }
                }
            }
        }

        // Scan each file for documentable items and doc comments
        for path in &files {
            let (total, documented) = scan_file_doc_coverage(path);
            if total == 0 {
                continue;
            }
            let coverage_pct = documented as f32 / total as f32 * 100.0;
            let rel_path = path
                .strip_prefix(&root)
                .unwrap_or(path)
                .display()
                .to_string();
            total_all += total;
            documented_all += documented;
            entries.push(DocEntry {
                path: path.clone(),
                rel_path,
                total_items: total,
                documented_items: documented,
                coverage_pct,
                checked: false,
            });
        }

        // Sort worst-documented first so user sees problem files at top
        entries.sort_by(|a, b| {
            a.coverage_pct
                .partial_cmp(&b.coverage_pct)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let doc_score = if total_all > 0 {
            documented_all as f32 / total_all as f32 * 100.0
        } else {
            100.0
        };
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
            let all_non100_checked = panel
                .doc_entries
                .iter()
                .filter(|e| e.coverage_pct < 100.0)
                .all(|e| e.checked);
            for entry in &mut panel.doc_entries {
                entry.checked = if entry.coverage_pct < 100.0 {
                    !all_non100_checked
                } else {
                    false
                };
            }
        }
    }

    /// Open checked doc entries as viewer tabs (same pattern as god_file_view_checked)
    pub fn doc_view_checked(&mut self) {
        const MAX_TABS: usize = 12;
        let paths: Vec<PathBuf> = match self.health_panel {
            Some(ref panel) => panel
                .doc_entries
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
            let content = match std::fs::read_to_string(&path) {
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

    /// Spawn [DH] (Documentation Health) Claude sessions for all checked doc entries.
    /// Each checked file gets its own concurrent Claude process with a prompt
    /// instructing Claude to add missing doc comments to all documentable items.
    pub fn doc_health_spawn(&mut self, claude_process: &AgentProcess) {
        let checked: Vec<(String, usize, usize)> = match self.health_panel {
            Some(ref panel) => panel
                .doc_entries
                .iter()
                .filter(|e| e.checked)
                .map(|e| (e.rel_path.clone(), e.documented_items, e.total_items))
                .collect(),
            None => return,
        };
        if checked.is_empty() {
            self.set_status("No files checked — use Space to check files");
            return;
        }

        // Spawn DH sessions on the current worktree — changes merge back to main
        let (branch, wt_path) = match self.current_worktree_info() {
            Some(v) => v,
            None => {
                self.set_status("No active worktree");
                return;
            }
        };

        self.health_panel = None;

        // Ensure the SQLite session store exists so each DH file gets its own session
        self.ensure_session_store();

        let selected_model = self.selected_model.clone();
        let mut spawned = 0usize;
        let mut failed = 0usize;
        let mut last_session_id: Option<i64> = None;
        for (rel_path, documented, total) in &checked {
            let prompt = build_doc_health_prompt(rel_path, *documented, *total);
            let filename = Path::new(rel_path)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| rel_path.clone());
            let session_name = format!("[DH] {}", filename);

            // Create a dedicated store session for this DH file
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
                    self.pending_session_names.push((slot, session_name));
                    self.register_claude(branch.clone(), pid, rx, selected_model.as_deref());
                    spawned += 1;
                }
                Err(_) => {
                    failed += 1;
                }
            }
        }

        // Clear session pane so DH output starts fresh (same as GFM)
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
            self.set_status(format!("Documenting {} files simultaneously", spawned));
        } else {
            self.set_status(format!(
                "Documenting {} files ({} failed to start)",
                spawned, failed
            ));
        }
    }
}

/// Scan a single file for documentable items and count how many have doc comments.
/// Uses line-based heuristics — no AST parsing, just pattern matching on trimmed lines.
/// Returns (total_items, documented_items).
fn scan_file_doc_coverage(path: &Path) -> (usize, usize) {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return (0, 0),
    };
    let lines: Vec<String> = BufReader::new(file)
        .lines()
        .filter_map(|l| l.ok())
        .collect();

    let mut total = 0usize;
    let mut documented = 0usize;

    /// Patterns that indicate a documentable item (checked against trimmed line starts)
    const ITEM_PREFIXES: &[&str] = &[
        "pub fn ",
        "fn ",
        "pub struct ",
        "struct ",
        "pub enum ",
        "enum ",
        "pub trait ",
        "trait ",
        "pub const ",
        "const ",
        "pub static ",
        "static ",
        "pub type ",
        "type ",
        "pub async fn ",
        "async fn ",
        "pub unsafe fn ",
        "unsafe fn ",
        "impl ",
        "pub mod ",
        "mod ",
    ];

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        // Skip blank lines, comments, attributes, use/extern/cfg
        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with('#')
            || trimmed.starts_with("use ")
            || trimmed.starts_with("extern ")
            || trimmed.starts_with('}')
        {
            continue;
        }

        // Check if this line starts a documentable item
        let is_item = ITEM_PREFIXES.iter().any(|p| trimmed.starts_with(p));
        if !is_item {
            continue;
        }

        total += 1;
        // Walk backwards from this line to find a doc comment (skip blanks + attributes)
        let mut j = i;
        while j > 0 {
            j -= 1;
            let prev = lines[j].trim();
            if prev.is_empty() || prev.starts_with("#[") || prev.starts_with("#![") {
                continue;
            }
            if prev.starts_with("///") || prev.starts_with("//!") {
                documented += 1;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper: write source content to a temp file and scan for doc coverage
    fn scan_content(content: &str) -> (usize, usize) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.rs");
        fs::write(&path, content).unwrap();
        scan_file_doc_coverage(&path)
    }

    // ── scan_file_doc_coverage: basic ──

    #[test]
    fn test_scan_empty_file() {
        let (total, documented) = scan_content("");
        assert_eq!(total, 0);
        assert_eq!(documented, 0);
    }

    #[test]
    fn test_scan_single_documented_fn() {
        let (total, documented) = scan_content("/// Does something\nfn foo() {}");
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_single_undocumented_fn() {
        let (total, documented) = scan_content("fn foo() {}");
        assert_eq!(total, 1);
        assert_eq!(documented, 0);
    }

    #[test]
    fn test_scan_pub_fn() {
        let (total, documented) = scan_content("/// Documented\npub fn bar() {}");
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_pub_fn_undocumented() {
        let (total, documented) = scan_content("pub fn bar() {}");
        assert_eq!(total, 1);
        assert_eq!(documented, 0);
    }

    // ── scan_file_doc_coverage: struct/enum/trait ──

    #[test]
    fn test_scan_struct_documented() {
        let (total, documented) = scan_content("/// A struct\nstruct Foo {}");
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_struct_undocumented() {
        let (total, documented) = scan_content("struct Foo {}");
        assert_eq!(total, 1);
        assert_eq!(documented, 0);
    }

    #[test]
    fn test_scan_pub_struct() {
        let (total, documented) = scan_content("/// Public struct\npub struct Bar {}");
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_enum_documented() {
        let (total, documented) = scan_content("/// An enum\nenum Color { Red, Blue }");
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_pub_enum_undocumented() {
        let (total, documented) = scan_content("pub enum Color { Red }");
        assert_eq!(total, 1);
        assert_eq!(documented, 0);
    }

    #[test]
    fn test_scan_trait_documented() {
        let (total, documented) = scan_content("/// A trait\ntrait Drawable {}");
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_pub_trait() {
        let (total, documented) = scan_content("/// Public trait\npub trait Drawable {}");
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    // ── scan_file_doc_coverage: const/static/type/impl/mod ──

    #[test]
    fn test_scan_const_documented() {
        let (total, documented) = scan_content("/// A constant\nconst MAX: u32 = 100;");
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_const_undocumented() {
        let (total, documented) = scan_content("const MAX: u32 = 100;");
        assert_eq!(total, 1);
        assert_eq!(documented, 0);
    }

    #[test]
    fn test_scan_static_documented() {
        let (total, documented) = scan_content("/// A static\nstatic COUNT: u32 = 0;");
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_type_alias_documented() {
        let (total, documented) =
            scan_content("/// A type alias\ntype Result<T> = std::result::Result<T, Error>;");
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_impl_block() {
        let (total, documented) = scan_content("/// Impl block\nimpl Foo {}");
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_mod_declaration() {
        let (total, documented) = scan_content("/// A module\nmod utils;");
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_pub_mod() {
        let (total, documented) = scan_content("pub mod utils;");
        assert_eq!(total, 1);
        assert_eq!(documented, 0);
    }

    // ── scan_file_doc_coverage: async/unsafe ──

    #[test]
    fn test_scan_async_fn() {
        let (total, documented) = scan_content("/// Async function\nasync fn fetch() {}");
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_pub_async_fn() {
        let (total, documented) = scan_content("pub async fn fetch() {}");
        assert_eq!(total, 1);
        assert_eq!(documented, 0);
    }

    #[test]
    fn test_scan_unsafe_fn() {
        let (total, documented) = scan_content("/// Unsafe function\nunsafe fn danger() {}");
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_pub_unsafe_fn() {
        let (total, documented) = scan_content("pub unsafe fn danger() {}");
        assert_eq!(total, 1);
        assert_eq!(documented, 0);
    }

    // ── scan_file_doc_coverage: multiple items ──

    #[test]
    fn test_scan_mixed_documented_and_not() {
        let content = "\
/// Documented
fn foo() {}

fn bar() {}

/// Also documented
struct Baz {}
";
        let (total, documented) = scan_content(content);
        assert_eq!(total, 3);
        assert_eq!(documented, 2);
    }

    #[test]
    fn test_scan_all_documented() {
        let content = "\
/// Fn
fn a() {}
/// Struct
struct B {}
/// Enum
enum C {}
/// Trait
trait D {}
/// Const
const E: i32 = 0;
";
        let (total, documented) = scan_content(content);
        assert_eq!(total, 5);
        assert_eq!(documented, 5);
    }

    #[test]
    fn test_scan_none_documented() {
        let content = "\
fn a() {}
struct B {}
enum C {}
trait D {}
const E: i32 = 0;
";
        let (total, documented) = scan_content(content);
        assert_eq!(total, 5);
        assert_eq!(documented, 0);
    }

    // ── scan_file_doc_coverage: doc comment variants ──

    #[test]
    fn test_scan_module_doc_comment_counts() {
        let content = "\
//! Module-level doc
fn foo() {}
";
        let (total, documented) = scan_content(content);
        assert_eq!(total, 1);
        // //! on line before fn counts as documented
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_multiline_doc_comment() {
        let content = "\
/// First line of doc
/// Second line of doc
/// Third line of doc
fn well_documented() {}
";
        let (total, documented) = scan_content(content);
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_attribute_between_doc_and_fn() {
        let content = "\
/// Documented with attribute
#[inline]
fn optimized() {}
";
        let (total, documented) = scan_content(content);
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_blank_line_between_doc_and_fn() {
        let content = "\
/// Doc comment

fn separated() {}
";
        let (total, documented) = scan_content(content);
        assert_eq!(total, 1);
        // blank line between doc and fn — the walkback skips blanks
        assert_eq!(documented, 1);
    }

    // ── scan_file_doc_coverage: skipped lines ──

    #[test]
    fn test_scan_skips_use_statements() {
        let content = "\
use std::io;
fn actual() {}
";
        let (total, _documented) = scan_content(content);
        assert_eq!(total, 1); // only fn, not use
    }

    #[test]
    fn test_scan_skips_extern() {
        let content = "\
extern crate foo;
fn actual() {}
";
        let (total, _documented) = scan_content(content);
        assert_eq!(total, 1);
    }

    #[test]
    fn test_scan_skips_regular_comments() {
        let content = "\
// regular comment
fn foo() {}
";
        let (total, documented) = scan_content(content);
        assert_eq!(total, 1);
        // Regular // comment is not a doc comment
        assert_eq!(documented, 0);
    }

    #[test]
    fn test_scan_skips_closing_braces() {
        let content = "\
fn foo() {
}
fn bar() {}
";
        let (total, _documented) = scan_content(content);
        assert_eq!(total, 2); // both fns counted
    }

    // ── scan_file_doc_coverage: edge cases ──

    #[test]
    fn test_scan_nonexistent_file() {
        let (total, documented) = scan_file_doc_coverage(Path::new("/nonexistent/file.rs"));
        assert_eq!(total, 0);
        assert_eq!(documented, 0);
    }

    #[test]
    fn test_scan_file_only_comments() {
        let content = "\
// Just comments
// Nothing documentable
/// Orphan doc comment
";
        let (total, _documented) = scan_content(content);
        assert_eq!(total, 0);
    }

    #[test]
    fn test_scan_file_only_use_statements() {
        let content = "\
use std::io;
use std::path::Path;
";
        let (total, _documented) = scan_content(content);
        assert_eq!(total, 0);
    }

    #[test]
    fn test_scan_indented_fn() {
        let content = "\
    /// Indented doc
    fn indented() {}
";
        let (total, documented) = scan_content(content);
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_pub_static_undocumented() {
        let (total, documented) = scan_content("pub static GLOBAL: i32 = 42;");
        assert_eq!(total, 1);
        assert_eq!(documented, 0);
    }

    #[test]
    fn test_scan_pub_const_documented() {
        let (total, documented) = scan_content("/// A public const\npub const SIZE: usize = 10;");
        assert_eq!(total, 1);
        assert_eq!(documented, 1);
    }

    #[test]
    fn test_scan_pub_type_alias() {
        let (total, documented) = scan_content("pub type MyResult<T> = Result<T, MyError>;");
        assert_eq!(total, 1);
        assert_eq!(documented, 0);
    }

    // ── build_doc_health_prompt ──

    #[test]
    fn test_doc_prompt_contains_file_path() {
        let prompt = build_doc_health_prompt("src/app.rs", 5, 10);
        assert!(prompt.contains("src/app.rs"));
    }

    #[test]
    fn test_doc_prompt_contains_coverage_counts() {
        let prompt = build_doc_health_prompt("lib.rs", 3, 10);
        assert!(prompt.contains("3/10 items documented"));
    }

    #[test]
    fn test_doc_prompt_contains_percentage() {
        let prompt = build_doc_health_prompt("lib.rs", 5, 10);
        assert!(prompt.contains("50.0%"));
    }

    #[test]
    fn test_doc_prompt_zero_total() {
        let prompt = build_doc_health_prompt("empty.rs", 0, 0);
        assert!(prompt.contains("100.0%"));
    }

    #[test]
    fn test_doc_prompt_all_documented() {
        let prompt = build_doc_health_prompt("good.rs", 10, 10);
        assert!(prompt.contains("100.0%"));
    }

    #[test]
    fn test_doc_prompt_none_documented() {
        let prompt = build_doc_health_prompt("bad.rs", 0, 10);
        assert!(prompt.contains("0.0%"));
    }

    #[test]
    fn test_doc_prompt_mentions_doc_comments() {
        let prompt = build_doc_health_prompt("lib.rs", 0, 5);
        assert!(prompt.contains("///"));
        assert!(prompt.contains("//!"));
    }

    #[test]
    fn test_doc_prompt_mentions_no_code_modification() {
        let prompt = build_doc_health_prompt("lib.rs", 0, 5);
        assert!(prompt.contains("Do NOT modify any executable code"));
    }

    #[test]
    fn test_doc_prompt_mentions_no_reformatting() {
        let prompt = build_doc_health_prompt("lib.rs", 0, 5);
        assert!(prompt.contains("Do NOT reformat"));
    }

    #[test]
    fn test_doc_prompt_mentions_read_file() {
        let prompt = build_doc_health_prompt("lib.rs", 0, 5);
        assert!(prompt.contains("Read the entire file"));
    }

    // ── DocEntry struct ──

    #[test]
    fn test_doc_entry_construction() {
        let entry = DocEntry {
            path: PathBuf::from("/src/lib.rs"),
            rel_path: "src/lib.rs".to_string(),
            total_items: 10,
            documented_items: 7,
            coverage_pct: 70.0,
            checked: false,
        };
        assert_eq!(entry.total_items, 10);
        assert_eq!(entry.documented_items, 7);
        assert!((entry.coverage_pct - 70.0).abs() < f32::EPSILON);
        assert!(!entry.checked);
    }

    #[test]
    fn test_doc_entry_checked_toggle() {
        let mut entry = DocEntry {
            path: PathBuf::from("/a.rs"),
            rel_path: "a.rs".to_string(),
            total_items: 5,
            documented_items: 2,
            coverage_pct: 40.0,
            checked: false,
        };
        entry.checked = true;
        assert!(entry.checked);
    }

    #[test]
    fn test_doc_entry_zero_coverage() {
        let entry = DocEntry {
            path: PathBuf::from("/bad.rs"),
            rel_path: "bad.rs".to_string(),
            total_items: 20,
            documented_items: 0,
            coverage_pct: 0.0,
            checked: false,
        };
        assert_eq!(entry.coverage_pct, 0.0);
    }

    #[test]
    fn test_doc_entry_full_coverage() {
        let entry = DocEntry {
            path: PathBuf::from("/good.rs"),
            rel_path: "good.rs".to_string(),
            total_items: 15,
            documented_items: 15,
            coverage_pct: 100.0,
            checked: false,
        };
        assert_eq!(entry.coverage_pct, 100.0);
    }

    #[test]
    fn test_doc_entry_clone() {
        let entry = DocEntry {
            path: PathBuf::from("/x.rs"),
            rel_path: "x.rs".to_string(),
            total_items: 3,
            documented_items: 1,
            coverage_pct: 33.3,
            checked: true,
        };
        let cloned = entry.clone();
        assert_eq!(entry.path, cloned.path);
        assert_eq!(entry.total_items, cloned.total_items);
        assert_eq!(entry.checked, cloned.checked);
    }
}
