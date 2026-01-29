//! App struct definition and initialization

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use portable_pty::MasterPty;

use crate::app::terminal::SessionTerminal;
use crate::app::types::{BranchDialog, ContextMenu, FileTreeEntry, Focus, ViewMode, ViewerMode};
use crate::claude::InteractiveSession;
use crate::events::EventParser;
use crate::models::{Project, RebaseStatus, Session};
use crate::syntax::{DiffHighlighter, SyntaxHighlighter};
use crate::wizard::SessionCreationWizard;

use super::ClaudeEvent;
use super::DisplayEvent;

/// Application state
pub struct App {
    pub project: Option<Project>,
    pub sessions: Vec<Session>,
    pub selected_session: Option<usize>,
    pub output_lines: VecDeque<String>,
    pub max_output_lines: usize,
    pub output_buffer: String,
    pub display_events: Vec<DisplayEvent>,
    pub event_parser: EventParser,
    pub selected_event: Option<usize>,
    pub input: String,
    pub input_cursor: usize,
    pub session_creation_input: String,
    pub session_creation_cursor: usize,
    pub view_mode: ViewMode,
    pub focus: Focus,
    pub insert_mode: bool,
    pub should_quit: bool,
    pub status_message: Option<String>,
    pub claude_receivers: HashMap<String, Receiver<ClaudeEvent>>,
    pub running_sessions: HashSet<String>,
    pub claude_session_ids: HashMap<String, String>,
    /// Interactive PTY sessions (kept alive between prompts)
    pub interactive_sessions: HashMap<String, InteractiveSession>,
    pub diff_text: Option<String>,
    /// Cached colorized diff lines (expensive highlighting done once, not per-frame)
    pub diff_lines_cache: Vec<Vec<ratatui::text::Span<'static>>>,
    /// Flag indicating diff cache needs refresh
    pub diff_lines_dirty: bool,
    pub output_scroll: usize,
    pub diff_scroll: usize,
    pub diff_highlighter: DiffHighlighter,
    pub syntax_highlighter: SyntaxHighlighter,
    pub show_help: bool,
    pub branch_dialog: Option<BranchDialog>,
    pub rebase_status: Option<RebaseStatus>,
    pub selected_conflict: Option<usize>,
    pub context_menu: Option<ContextMenu>,
    pub creation_wizard: Option<SessionCreationWizard>,
    pub terminal_mode: bool,
    pub terminal_pty: Option<Box<dyn MasterPty + Send>>,
    pub terminal_writer: Option<Box<dyn Write + Send>>,
    pub terminal_rx: Option<Receiver<Vec<u8>>>,
    pub terminal_parser: vt100::Parser,
    pub terminal_scroll: usize,
    pub terminal_height: u16,
    pub terminal_rows: u16,
    pub terminal_cols: u16,
    pub terminal_needs_resize: bool,
    /// Tool calls awaiting results (for progress indicator animation)
    pub pending_tool_calls: HashSet<String>,
    /// Tool calls that failed (for red indicator)
    pub failed_tool_calls: HashSet<String>,
    /// Animation tick counter for pulsating effects
    pub animation_tick: u64,
    /// Current session file path for live polling
    pub session_file_path: Option<PathBuf>,
    /// Last modified time of session file (for change detection)
    pub session_file_modified: Option<std::time::SystemTime>,
    /// Per-session terminals (persist when switching sessions)
    pub session_terminals: HashMap<String, SessionTerminal>,
    /// FileTree entries for current session's worktree
    pub file_tree_entries: Vec<FileTreeEntry>,
    /// Selected index in file tree
    pub file_tree_selected: Option<usize>,
    /// Scroll offset in file tree
    pub file_tree_scroll: usize,
    /// Expanded directories in file tree
    pub file_tree_expanded: HashSet<PathBuf>,
    /// Viewer pane content (file or diff text)
    pub viewer_content: Option<String>,
    /// Path of file displayed in viewer (if ViewerMode::File)
    pub viewer_path: Option<PathBuf>,
    /// Scroll offset in viewer
    pub viewer_scroll: usize,
    /// Current viewer display mode
    pub viewer_mode: ViewerMode,
    /// Cached rendered lines for viewer pane (syntax highlighting is expensive)
    pub viewer_lines_cache: Vec<ratatui::text::Line<'static>>,
    /// Width used for viewer cache (invalidate on resize)
    pub viewer_lines_width: usize,
    /// Flag indicating viewer cache needs refresh
    pub viewer_lines_dirty: bool,
    /// Cached rendered lines for convo pane (expensive to compute)
    pub rendered_lines_cache: Vec<ratatui::text::Line<'static>>,
    /// Width used for cached render (invalidate on resize)
    pub rendered_lines_width: u16,
    /// Animation tick used for cached render (for pending tool indicators)
    pub rendered_lines_tick: u64,
    /// Flag indicating cache needs refresh
    pub rendered_lines_dirty: bool,
    /// Total lines in last parsed session file
    pub parse_total_lines: usize,
    /// Parse errors in last parsed session file
    pub parse_errors: usize,
    /// Assistant parsing diagnostics
    pub assistant_total: usize,
    pub assistant_no_message: usize,
    pub assistant_no_content_arr: usize,
    pub assistant_text_blocks: usize,
    /// Expanded sessions in sidebar (shows dropdown of session files)
    pub sessions_expanded: HashSet<String>,
    /// Cached session files per worktree branch (session_id, path, formatted_time)
    pub session_files: HashMap<String, Vec<(String, PathBuf, String)>>,
    /// Selected file index per session (0 = latest/newest)
    pub session_selected_file_idx: HashMap<String, usize>,
    /// Cached sidebar ListItems (avoid rebuilding every frame)
    pub sidebar_cache: Vec<ratatui::widgets::ListItem<'static>>,
    /// Flag indicating sidebar cache needs refresh
    pub sidebar_dirty: bool,
    /// Last known focus state for sidebar (styling changes on focus)
    pub sidebar_focus_cached: bool,
    /// Cached output viewport lines (avoid cloning from full cache every frame)
    pub output_viewport_cache: Vec<ratatui::text::Line<'static>>,
    /// Scroll position used for output viewport cache
    pub output_viewport_scroll: usize,
    /// Height used for output viewport cache
    pub output_viewport_height: usize,
    /// Cached viewer viewport lines (avoid cloning from full cache every frame)
    pub viewer_viewport_cache: Vec<ratatui::text::Line<'static>>,
    /// Scroll position used for viewer viewport cache
    pub viewer_viewport_scroll: usize,
    /// Height used for viewer viewport cache
    pub viewer_viewport_height: usize,
    /// Cached output title string (avoid format! every frame)
    pub output_title_cache: String,
    /// Cached viewer title string
    pub viewer_title_cache: String,
}

impl App {
    pub fn new() -> Self {
        Self {
            project: None,
            sessions: Vec::new(),
            selected_session: None,
            output_lines: VecDeque::with_capacity(10000),
            max_output_lines: 10000,
            output_buffer: String::new(),
            display_events: Vec::new(),
            event_parser: EventParser::new(),
            selected_event: None,
            input: String::new(),
            input_cursor: 0,
            session_creation_input: String::new(),
            session_creation_cursor: 0,
            view_mode: ViewMode::Output,
            focus: Focus::Worktrees,
            insert_mode: false,
            should_quit: false,
            status_message: None,
            claude_receivers: HashMap::new(),
            running_sessions: HashSet::new(),
            claude_session_ids: HashMap::new(),
            interactive_sessions: HashMap::new(),
            diff_text: None,
            diff_lines_cache: Vec::new(),
            diff_lines_dirty: true,
            output_scroll: usize::MAX, // Start at bottom (most recent messages)
            diff_scroll: 0,
            diff_highlighter: DiffHighlighter::new(),
            syntax_highlighter: SyntaxHighlighter::new(),
            show_help: false,
            branch_dialog: None,
            rebase_status: None,
            selected_conflict: None,
            context_menu: None,
            creation_wizard: None,
            terminal_mode: false,
            terminal_pty: None,
            terminal_writer: None,
            terminal_rx: None,
            terminal_parser: vt100::Parser::new(24, 120, 1000),
            terminal_scroll: 0,
            terminal_height: 12,
            terminal_rows: 24,
            terminal_cols: 120,
            terminal_needs_resize: false,
            pending_tool_calls: HashSet::new(),
            failed_tool_calls: HashSet::new(),
            animation_tick: 0,
            session_file_path: None,
            session_file_modified: None,
            session_terminals: HashMap::new(),
            file_tree_entries: Vec::new(),
            file_tree_selected: None,
            file_tree_scroll: 0,
            file_tree_expanded: HashSet::new(),
            viewer_content: None,
            viewer_path: None,
            viewer_scroll: 0,
            viewer_mode: ViewerMode::Empty,
            viewer_lines_cache: Vec::new(),
            viewer_lines_width: 0,
            viewer_lines_dirty: true,
            rendered_lines_cache: Vec::new(),
            rendered_lines_width: 0,
            rendered_lines_tick: 0,
            rendered_lines_dirty: true,
            parse_total_lines: 0,
            parse_errors: 0,
            assistant_total: 0,
            assistant_no_message: 0,
            assistant_no_content_arr: 0,
            assistant_text_blocks: 0,
            sessions_expanded: HashSet::new(),
            session_files: HashMap::new(),
            session_selected_file_idx: HashMap::new(),
            sidebar_cache: Vec::new(),
            sidebar_dirty: true,
            sidebar_focus_cached: false,
            output_viewport_cache: Vec::new(),
            output_viewport_scroll: usize::MAX,
            output_viewport_height: 0,
            viewer_viewport_cache: Vec::new(),
            viewer_viewport_scroll: usize::MAX,
            viewer_viewport_height: 0,
            output_title_cache: String::new(),
            viewer_title_cache: String::new(),
        }
    }

    /// Mark rendered lines cache as dirty (call when display_events change)
    pub fn invalidate_render_cache(&mut self) {
        self.rendered_lines_dirty = true;
        self.output_viewport_scroll = usize::MAX; // Force viewport rebuild on next draw
    }

    /// Mark sidebar cache as dirty (call when sessions/selection/expansion changes)
    pub fn invalidate_sidebar(&mut self) {
        self.sidebar_dirty = true;
    }

    /// Mark output viewport cache as dirty (call when scroll/content changes)
    pub fn invalidate_output_viewport(&mut self) {
        self.output_viewport_scroll = usize::MAX;
    }

    /// Mark viewer viewport cache as dirty
    pub fn invalidate_viewer_viewport(&mut self) {
        self.viewer_viewport_scroll = usize::MAX;
    }

    pub fn current_project(&self) -> Option<&Project> { self.project.as_ref() }
    pub fn current_session(&self) -> Option<&Session> { self.selected_session.and_then(|idx| self.sessions.get(idx)) }

    pub fn is_session_running(&self, branch_name: &str) -> bool {
        self.running_sessions.contains(branch_name)
    }

    pub fn is_current_session_running(&self) -> bool {
        self.current_session().map(|s| self.running_sessions.contains(&s.branch_name)).unwrap_or(false)
    }

    pub fn set_status(&mut self, msg: impl Into<String>) { self.status_message = Some(msg.into()); }
    pub fn clear_status(&mut self) { self.status_message = None; }
}
