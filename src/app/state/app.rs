//! App struct definition and initialization

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use portable_pty::MasterPty;

use crate::app::terminal::SessionTerminal;
use crate::app::types::{BranchDialog, ContextMenu, FileTreeEntry, Focus, RunCommand, RunCommandDialog, RunCommandPicker, SidebarRowAction, ViewMode, ViewerMode};
use crate::claude::InteractiveSession;
use crate::events::EventParser;
use crate::models::{Project, RebaseStatus, Session};
use crate::syntax::{DiffHighlighter, SyntaxHighlighter};
use crate::tui::render_thread::RenderThread;
use crate::wizard::CreationWizard;

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
    /// User message sent but not yet in session file (shown until file updates)
    pub pending_user_message: Option<String>,
    /// Prompt staged to send after cancelling current Claude process
    pub staged_prompt: Option<String>,
    pub event_parser: EventParser,
    pub selected_event: Option<usize>,
    pub input: String,
    pub input_cursor: usize,
    /// Selection range in prompt input: (start, end) as char indices
    pub input_selection: Option<(usize, usize)>,
    pub worktree_creation_input: String,
    pub worktree_creation_cursor: usize,
    pub view_mode: ViewMode,
    pub focus: Focus,
    pub prompt_mode: bool,
    pub should_quit: bool,
    pub should_restart: bool,
    pub status_message: Option<String>,
    pub claude_receivers: HashMap<String, Receiver<ClaudeEvent>>,
    pub running_sessions: HashSet<String>,
    /// PIDs of running Claude processes per branch (for killing)
    pub claude_pids: HashMap<String, u32>,
    /// Last exit code per branch (shown in convo pane title after Claude exits)
    pub claude_exit_codes: HashMap<String, i32>,
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
    pub creation_wizard: Option<CreationWizard>,
    /// Pending session name to save when Claude returns session ID (branch_name, custom_name)
    pub pending_session_name: Option<(String, String)>,
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
    /// Last known file size (for incremental change detection)
    pub session_file_size: u64,
    /// Byte offset of last successful parse (for incremental parsing)
    pub session_file_parse_offset: u64,
    /// Session file needs re-parse (deferred during user interaction)
    pub session_file_dirty: bool,
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
    /// Original line number for each cached viewer line (1-indexed, for title display)
    pub viewer_line_numbers: Vec<usize>,
    /// Total original line count in viewer file
    pub viewer_original_line_count: usize,
    /// Width used for viewer cache (invalidate on resize)
    pub viewer_lines_width: usize,
    /// Flag indicating viewer cache needs refresh
    pub viewer_lines_dirty: bool,
    /// Cached rendered lines for convo pane (expensive to compute)
    pub rendered_lines_cache: Vec<ratatui::text::Line<'static>>,
    /// Width used for cached render (invalidate on resize)
    pub rendered_lines_width: u16,
    /// Flag indicating cache needs refresh
    pub rendered_lines_dirty: bool,
    /// How many display_events were rendered into current cache (for incremental append)
    pub rendered_events_count: usize,
    /// Line count in cache BEFORE the pending user message bubble was appended.
    /// Used by incremental renders to trim the stale pending bubble before re-appending.
    pub rendered_content_line_count: usize,
    /// Start index of deferred render (events before this are not yet rendered).
    /// 0 means everything is rendered. >0 means we skipped early events for fast initial load.
    pub rendered_events_start: usize,
    /// Line indices containing pending tool indicators (line_idx, span_idx) for animation patching
    pub animation_line_indices: Vec<(usize, usize)>,
    /// Background render thread — expensive convo rendering runs here, never blocks the event loop
    pub render_thread: RenderThread,
    /// Sequence number of the last applied render result (discard results with lower seq)
    pub render_seq_applied: u64,
    /// True while a render request is in-flight (waiting for background thread to finish)
    pub render_in_flight: bool,
    /// True when state changed and a draw is needed. Draw is deferred if keys
    /// are arriving (to avoid the ~18ms terminal.draw() blocking window).
    pub draw_pending: bool,
    /// Cached input area rect from last full draw — used for fast-path direct
    /// input rendering that bypasses terminal.draw() during rapid typing.
    pub input_area: ratatui::layout::Rect,
    /// Cached pane rects from last full draw — used for mouse click hit-testing
    /// and scroll dispatch without recalculating layout
    pub pane_sessions: ratatui::layout::Rect,
    pub pane_file_tree: ratatui::layout::Rect,
    pub pane_viewer: ratatui::layout::Rect,
    pub pane_convo: ratatui::layout::Rect,
    /// Maps sidebar visual rows (0-indexed) to clickable actions.
    /// Built alongside sidebar_cache in draw_sidebar::build_sidebar_items().
    pub sidebar_row_map: Vec<SidebarRowAction>,
    /// Cached viewport slice for convo pane — avoids cloning rendered_lines_cache every frame.
    /// Only rebuilt when scroll position, content, or animation tick changes.
    pub output_viewport_cache: Vec<ratatui::text::Line<'static>>,
    /// Scroll position and animation tick used to build the viewport cache (invalidation key)
    pub output_viewport_scroll: usize,
    pub output_viewport_anim_tick: u64,
    /// Title string corresponding to the cached viewport
    pub output_viewport_title: String,
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
    /// Cached file tree lines (avoid rebuilding every frame)
    pub file_tree_lines_cache: Vec<ratatui::text::Line<'static>>,
    /// Flag indicating file tree cache needs refresh
    pub file_tree_dirty: bool,
    /// Cached file tree title string
    pub file_tree_title_cache: String,
    /// Scroll position used for file tree cache
    pub file_tree_scroll_cached: usize,
    /// Awaiting user response to plan approval (ExitPlanMode was called)
    pub awaiting_plan_approval: bool,
    /// Cached viewport height for viewer pane (set during render, used for scroll)
    pub viewer_viewport_height: usize,
    /// Cached viewport height for output/convo pane (set during render, used for scroll)
    pub output_viewport_height: usize,
    /// Line indices where message bubbles start (for Up/Down navigation)
    /// Each entry is (line_index, is_user_message) - true for UserMessage, false for AssistantText
    pub message_bubble_positions: Vec<(usize, bool)>,
    /// Edit/Write tool diffs for navigation: (line_idx, tool_name, file_path, diff_text)
    pub tool_diff_positions: Vec<(usize, String, String, String)>,
    /// Clickable file path links in output: (line_idx, start_col, end_col, file_path, old_string, new_string)
    pub clickable_paths: Vec<(usize, usize, usize, String, String, String)>,
    /// Currently selected tool diff index (for e/E navigation in Output)
    pub selected_tool_diff: Option<usize>,
    /// Edit mode active in viewer
    pub viewer_edit_mode: bool,
    /// Editable content (copy of file when entering edit mode)
    pub viewer_edit_content: Vec<String>,
    /// Cursor position in edit mode (line, column)
    pub viewer_edit_cursor: (usize, usize),
    /// Undo stack for edit mode (each entry is full content snapshot)
    pub viewer_edit_undo: Vec<Vec<String>>,
    /// Redo stack for edit mode
    pub viewer_edit_redo: Vec<Vec<String>>,
    /// Whether edits have been made since entering edit mode
    pub viewer_edit_dirty: bool,
    /// Show discard confirmation dialog
    pub viewer_edit_discard_dialog: bool,
    /// Show post-save dialog when editing from Edit diff view
    pub viewer_edit_save_dialog: bool,
    /// Text selection in edit mode: (start_line, start_col, end_line, end_col)
    pub viewer_edit_selection: Option<(usize, usize, usize, usize)>,
    /// Clipboard for copy/cut/paste operations
    pub clipboard: String,
    /// Text selection for read-only viewer: (start_visual_line, start_col, end_visual_line, end_col)
    pub viewer_selection: Option<(usize, usize, usize, usize)>,
    /// Text selection for output/convo pane: (start_visual_line, start_col, end_visual_line, end_col)
    pub output_selection: Option<(usize, usize, usize, usize)>,
    /// Cached output selection for viewport cache invalidation (rebuild viewport when selection changes)
    pub output_selection_cached: Option<(usize, usize, usize, usize)>,
    /// Mouse drag in progress
    pub mouse_drag_start: Option<(u16, u16)>,
    /// Last click time and position for double-click detection
    pub last_click: Option<(std::time::Instant, u16, u16)>,
    /// Edit diff overlay: (old_string, new_string) to highlight in viewer
    pub viewer_edit_diff: Option<(String, String)>,
    /// Line number where the edit diff starts (for scrolling to it)
    pub viewer_edit_diff_line: Option<usize>,
    /// Previous viewer state before Edit diff (content, path, scroll) for restoration on Esc
    pub viewer_prev_state: Option<(Option<String>, Option<PathBuf>, usize)>,
    /// Current position in prompt history (None = new input, Some(idx) = browsing history)
    /// History is pulled from display_events UserMessage entries (last 50)
    pub prompt_history_idx: Option<usize>,
    /// Saved current input when browsing history (restored when returning to bottom)
    pub prompt_history_temp: Option<String>,
    /// Viewer tabs (each tab holds file state)
    pub viewer_tabs: Vec<crate::app::types::ViewerTab>,
    /// Currently active tab index
    pub viewer_active_tab: usize,
    /// Show tab selection dialog
    pub viewer_tab_dialog: bool,
    /// Saved run commands
    pub run_commands: Vec<RunCommand>,
    /// Run command creation/edit dialog
    pub run_command_dialog: Option<RunCommandDialog>,
    /// Run command picker dialog (shown when multiple commands exist)
    pub run_command_picker: Option<RunCommandPicker>,
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
            pending_user_message: None,
            staged_prompt: None,
            event_parser: EventParser::new(),
            selected_event: None,
            input: String::new(),
            input_cursor: 0,
            input_selection: None,
            worktree_creation_input: String::new(),
            worktree_creation_cursor: 0,
            view_mode: ViewMode::Output,
            focus: Focus::Worktrees,
            prompt_mode: false,
            should_quit: false,
            should_restart: false,
            status_message: None,
            claude_receivers: HashMap::new(),
            running_sessions: HashSet::new(),
            claude_pids: HashMap::new(),
            claude_exit_codes: HashMap::new(),
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
            pending_session_name: None,
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
            session_file_size: 0,
            session_file_parse_offset: 0,
            session_file_dirty: false,
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
            viewer_line_numbers: Vec::new(),
            viewer_original_line_count: 0,
            viewer_lines_width: 0,
            viewer_lines_dirty: true,
            rendered_lines_cache: Vec::new(),
            rendered_lines_width: 0,
            rendered_lines_dirty: true,
            rendered_events_count: 0,
            rendered_content_line_count: 0,
            rendered_events_start: 0,
            animation_line_indices: Vec::new(),
            render_thread: RenderThread::spawn(),
            render_seq_applied: 0,
            render_in_flight: false,
            draw_pending: false,
            input_area: ratatui::layout::Rect::default(),
            pane_sessions: ratatui::layout::Rect::default(),
            pane_file_tree: ratatui::layout::Rect::default(),
            pane_viewer: ratatui::layout::Rect::default(),
            pane_convo: ratatui::layout::Rect::default(),
            sidebar_row_map: Vec::new(),
            output_viewport_cache: Vec::new(),
            output_viewport_scroll: usize::MAX,
            output_viewport_anim_tick: u64::MAX,
            output_viewport_title: String::new(),
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
            file_tree_lines_cache: Vec::new(),
            file_tree_dirty: true,
            file_tree_title_cache: String::new(),
            file_tree_scroll_cached: usize::MAX,
            awaiting_plan_approval: false,
            viewer_viewport_height: 20,
            output_viewport_height: 20,
            message_bubble_positions: Vec::new(),
            tool_diff_positions: Vec::new(),
            selected_tool_diff: None,
            clickable_paths: Vec::new(),
            viewer_edit_mode: false,
            viewer_edit_content: Vec::new(),
            viewer_edit_cursor: (0, 0),
            viewer_edit_undo: Vec::new(),
            viewer_edit_redo: Vec::new(),
            viewer_edit_dirty: false,
            viewer_edit_discard_dialog: false,
            viewer_edit_save_dialog: false,
            viewer_edit_selection: None,
            clipboard: String::new(),
            viewer_selection: None,
            output_selection: None,
            output_selection_cached: None,
            mouse_drag_start: None,
            last_click: None,
            viewer_edit_diff: None,
            viewer_edit_diff_line: None,
            viewer_prev_state: None,
            prompt_history_idx: None,
            prompt_history_temp: None,
            viewer_tabs: Vec::new(),
            viewer_active_tab: 0,
            viewer_tab_dialog: false,
            run_commands: Vec::new(),
            run_command_dialog: None,
            run_command_picker: None,
        }
    }

    /// Mark rendered lines cache as dirty (call when display_events change)
    pub fn invalidate_render_cache(&mut self) {
        self.rendered_lines_dirty = true;
    }

    /// Mark sidebar cache as dirty (call when sessions/selection/expansion changes)
    pub fn invalidate_sidebar(&mut self) {
        self.sidebar_dirty = true;
    }

    /// Mark file tree cache as dirty
    pub fn invalidate_file_tree(&mut self) {
        self.file_tree_dirty = true;
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
