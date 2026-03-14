//! Per-project state snapshot for parallel project switching
//!
//! When the user switches projects, the current project's state is saved here
//! so it can be restored when switching back. Claude processes continue running
//! in the background — only the UI state is swapped.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::app::terminal::SessionTerminal;
use crate::app::types::{FileTreeEntry, PresetPrompt, RunCommand, ViewerTab};
use crate::models::{Project, Worktree};

/// Captures per-project state that survives a project switch.
/// Fields not included here are either global (agent_receivers, running_sessions)
/// or rebuilt on restore (display_events, render cache, todos, tokens).
pub struct ProjectSnapshot {
    pub project: Project,

    // ── Worktree state ──
    pub worktrees: Vec<Worktree>,
    pub selected_worktree: Option<usize>,
    pub main_worktree: Option<Worktree>,
    pub browsing_main: bool,
    pub pre_main_browse_selection: Option<usize>,

    // ── Process→branch mappings (per-project, branch names are project-scoped) ──
    pub branch_slots: HashMap<String, Vec<String>>,
    pub active_slot: HashMap<String, String>,
    pub pending_session_names: Vec<(String, String)>,

    // ── Session store state ──
    pub pid_session_target: HashMap<String, (i64, PathBuf)>,
    pub current_session_id: Option<i64>,

    // ── Session list state ──
    pub session_files: HashMap<String, Vec<(String, PathBuf, String)>>,
    pub session_selected_file_idx: HashMap<String, usize>,
    pub unread_sessions: HashSet<String>,
    pub unread_session_ids: HashSet<String>,

    // ── File tree state ──
    pub file_tree_entries: Vec<FileTreeEntry>,
    pub file_tree_selected: Option<usize>,
    pub file_tree_scroll: usize,
    pub file_tree_expanded: HashSet<PathBuf>,
    pub file_tree_hidden_dirs: HashSet<String>,

    // ── Viewer tabs ──
    pub viewer_tabs: Vec<ViewerTab>,
    pub viewer_active_tab: usize,

    // ── Per-worktree terminals (shell sessions) ──
    pub worktree_terminals: HashMap<String, SessionTerminal>,

    // ── Per-project config ──
    pub auto_rebase_enabled: HashSet<String>,
    pub run_commands: Vec<RunCommand>,
    pub preset_prompts: Vec<PresetPrompt>,
}
