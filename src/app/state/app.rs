//! App struct definition and initialization

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use portable_pty::MasterPty;

use crate::app::terminal::SessionTerminal;
use crate::app::types::{BranchDialog, ContextMenu, FileTreeAction, FileTreeEntry, Focus, GodFilePanel, ProjectsPanel, RunCommand, RunCommandDialog, RunCommandPicker, SidebarRowAction, ViewMode, ViewerMode};
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
    pub selected_worktree: Option<usize>,
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
    /// Projects panel state (full-screen overlay for project selection)
    pub projects_panel: Option<ProjectsPanel>,
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
    /// Kernel-level file watcher (replaces stat() polling for change detection).
    /// None if notify failed to initialize — falls back to polling in that case.
    pub file_watcher: Option<crate::watcher::FileWatcher>,
    /// Whether the worktree directory changed (debounced file tree refresh)
    pub file_tree_refresh_pending: bool,
    /// Timestamp of last worktree change notification (for 500ms debounce)
    pub worktree_last_notify: std::time::Instant,
    /// Per-worktree terminals (persist when switching worktrees)
    pub worktree_terminals: HashMap<String, SessionTerminal>,
    /// FileTree entries for the current worktree
    pub file_tree_entries: Vec<FileTreeEntry>,
    /// Selected index in file tree
    pub file_tree_selected: Option<usize>,
    /// Scroll offset in file tree
    pub file_tree_scroll: usize,
    /// Expanded directories in file tree
    pub file_tree_expanded: HashSet<PathBuf>,
    /// Active file action (add/rename/copy/move/delete) — None when idle
    pub file_tree_action: Option<FileTreeAction>,
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
    /// When the last render request was submitted — used to throttle submit frequency
    /// during rapid Claude streaming. Without this, every poll_render_result completion
    /// immediately triggers another submit (cloning the full events array at ~60Hz).
    pub last_render_submit: std::time::Instant,
    /// True when state changed and a draw is needed. Draw is deferred if keys
    /// are arriving (to avoid the ~18ms terminal.draw() blocking window).
    pub draw_pending: bool,
    /// Cached CPU usage string for status bar (updated every ~1s via getrusage delta)
    pub cpu_usage_text: String,
    /// Last getrusage sample: (wall_time, cpu_time_micros)
    pub cpu_last_sample: (std::time::Instant, u64),
    /// Cached input area rect from last full draw — used for fast-path direct
    /// input rendering that bypasses terminal.draw() during rapid typing.
    pub input_area: ratatui::layout::Rect,
    /// Cached pane rects from last full draw — used for mouse click hit-testing
    /// and scroll dispatch without recalculating layout
    pub pane_worktrees: ratatui::layout::Rect,
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
    /// Expanded worktrees in sidebar (shows dropdown of Claude session files)
    pub worktrees_expanded: HashSet<String>,
    /// Cached Claude session files per worktree branch (session_id, path, formatted_time)
    pub session_files: HashMap<String, Vec<(String, PathBuf, String)>>,
    /// Selected Claude session file index per worktree (0 = latest/newest)
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
    /// Clickable file path links in output: (line_idx, start_col, end_col, file_path, old_string, new_string, wrap_line_count)
    pub clickable_paths: Vec<(usize, usize, usize, String, String, String, usize)>,
    /// Currently highlighted (clicked) file path in convo pane: (line_idx, start_col, end_col, wrap_line_count)
    /// Rendered with inverted colors so the user sees which path they clicked
    pub clicked_path_highlight: Option<(usize, usize, usize, usize)>,
    /// Cached title bar Claude session name (updated on worktree switch, avoids file I/O in render)
    pub title_session_name: String,
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
    /// Wrap width for edit mode (viewport chars available per visual line).
    /// Cached from draw_edit_mode so cursor movement can navigate wrapped visual lines.
    pub viewer_edit_content_width: usize,
    /// Monotonically increasing counter — bumped on every content mutation.
    /// Used as cache key for syntax highlight invalidation (undo stack length
    /// can't be used because it caps at 100, causing the cache key to stall).
    pub viewer_edit_version: usize,
    /// Cached syntax-highlighted spans per source line. Only recomputed when
    /// `viewer_edit_highlight_ver` doesn't match `viewer_edit_version`.
    pub viewer_edit_highlight_cache: Vec<Vec<ratatui::text::Span<'static>>>,
    /// Version counter for highlight cache invalidation (tracks edit version at last highlight)
    pub viewer_edit_highlight_ver: usize,
    /// Clipboard for copy/cut/paste operations
    pub clipboard: String,
    /// Text selection for read-only viewer: (start_visual_line, start_col, end_visual_line, end_col)
    pub viewer_selection: Option<(usize, usize, usize, usize)>,
    /// Text selection for output/convo pane: (start_visual_line, start_col, end_visual_line, end_col)
    pub output_selection: Option<(usize, usize, usize, usize)>,
    /// Cached output selection for viewport cache invalidation (rebuild viewport when selection changes)
    pub output_selection_cached: Option<(usize, usize, usize, usize)>,
    /// Mouse drag anchor in cache coordinates: (cache_line_or_char, cache_col, pane_id)
    /// pane_id: 0=viewer, 1=convo, 2=input. Stored as cache coords so auto-scroll
    /// during drag doesn't shift the anchor.
    pub mouse_drag_start: Option<(usize, usize, u8)>,
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
    /// Latest token usage from most recent assistant event: (context_tokens, output_tokens)
    /// context_tokens = input_tokens + cache_read + cache_creation (effective context size)
    pub session_tokens: Option<(u64, u64)>,
    /// Context window size detected from model string (None = not yet known, default 200k)
    pub model_context_window: Option<u64>,
    /// Cached token usage badge: (formatted_string, color) — only recomputed when token data changes
    pub token_badge_cache: Option<(String, ratatui::style::Color)>,
    /// Sidebar search filter text (empty = no filter). Case-insensitive substring match on session names.
    pub sidebar_filter: String,
    /// Whether the sidebar filter input is active (typing goes to filter, not commands)
    pub sidebar_filter_active: bool,
    /// Current todo list from latest TodoWrite tool call (main agent)
    pub current_todos: Vec<TodoItem>,
    /// Subagent todo list — shown as indented subtasks under the parent todo
    pub subagent_todos: Vec<TodoItem>,
    /// Tool use IDs of currently active Task (subagent) calls.
    /// While non-empty, any incoming TodoWrite goes to subagent_todos instead.
    pub active_task_tool_ids: std::collections::HashSet<String>,
    /// Index into current_todos of the in_progress item when first Task was spawned.
    /// Subagent todos render directly after this item in the widget.
    pub subagent_parent_idx: Option<usize>,
    /// Awaiting user response to AskUserQuestion tool call
    pub awaiting_ask_user_question: bool,
    /// Cached questions from last AskUserQuestion (for context prefix on response)
    pub ask_user_questions_cache: Option<serde_json::Value>,
    /// Speech-to-text engine handle (lazy-initialized on first ⌃s press)
    pub stt_handle: Option<crate::stt::SttHandle>,
    /// Whether STT is currently recording audio from the microphone
    pub stt_recording: bool,
    /// Whether STT is currently transcribing recorded audio (between stop and result)
    pub stt_transcribing: bool,
    /// Whether the file tree overlay is shown in the Worktrees pane (toggled with 'f')
    pub show_file_tree: bool,
    /// God File System panel — scans project for oversized source files (>1k LOC)
    /// and lets user batch-spawn modularization sessions. None when closed.
    pub god_file_panel: Option<GodFilePanel>,
    /// Queue of god file modularization prompts waiting to be spawned on main worktree.
    /// Each entry is (rel_path, full_prompt). When the current session completes, the
    /// next item is popped and spawned automatically.
    pub god_file_queue: VecDeque<(String, String)>,
    /// Whether the session list overlay is shown in the Convo pane (toggled with 's')
    pub show_session_list: bool,
    /// Selected index in session list overlay
    pub session_list_selected: usize,
    /// Scroll offset in session list overlay
    pub session_list_scroll: usize,
    /// Cached message counts per session_id (computed on session list open)
    pub session_msg_counts: HashMap<String, usize>,

    // ── Convo search (find text in current session's rendered output) ──
    /// Whether the search bar is active (accepting keystrokes)
    pub convo_search_active: bool,
    /// Current search query text
    pub convo_search: String,
    /// All matches: (cache_line_idx, start_col, end_col). Recomputed on each keystroke.
    pub convo_search_matches: Vec<(usize, usize, usize)>,
    /// Index into convo_search_matches for the "current" highlighted match
    pub convo_search_current: usize,

    // ── Session list filter (name-based, single `/`) ──
    /// Whether the filter bar is active in session list overlay
    pub session_filter_active: bool,
    /// Filter text for session list name search
    pub session_filter: String,

    // ── Cross-session content search (double `//`) ──
    /// True when in "//" content search mode vs "/" name filter mode
    pub session_content_search: bool,
    /// Results: (flat_row_idx, session_id, matched_line_preview)
    pub session_search_results: Vec<(usize, String, String)>,
}

/// A single todo item from Claude's TodoWrite tool call
#[derive(Clone, Debug)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
    pub active_form: String,
}

/// Status of a todo item
#[derive(Clone, Debug, PartialEq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

impl App {
    pub fn new() -> Self {
        Self {
            project: None,
            sessions: Vec::new(),
            selected_worktree: None,
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
            projects_panel: None,
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
            file_watcher: crate::watcher::FileWatcher::spawn(),
            file_tree_refresh_pending: false,
            worktree_last_notify: std::time::Instant::now(),
            worktree_terminals: HashMap::new(),
            file_tree_entries: Vec::new(),
            file_tree_selected: None,
            file_tree_scroll: 0,
            file_tree_expanded: HashSet::new(),
            file_tree_action: None,
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
            last_render_submit: std::time::Instant::now(),
            draw_pending: false,
            cpu_usage_text: String::new(),
            cpu_last_sample: (std::time::Instant::now(), get_cpu_time_micros()),
            input_area: ratatui::layout::Rect::default(),
            pane_worktrees: ratatui::layout::Rect::default(),
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
            worktrees_expanded: HashSet::new(),
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
            clicked_path_highlight: None,
            title_session_name: String::new(),
            viewer_edit_mode: false,
            viewer_edit_content: Vec::new(),
            viewer_edit_cursor: (0, 0),
            viewer_edit_undo: Vec::new(),
            viewer_edit_redo: Vec::new(),
            viewer_edit_dirty: false,
            viewer_edit_discard_dialog: false,
            viewer_edit_save_dialog: false,
            viewer_edit_selection: None,
            viewer_edit_content_width: 80,
            viewer_edit_version: 0,
            viewer_edit_highlight_cache: Vec::new(),
            viewer_edit_highlight_ver: usize::MAX,
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
            session_tokens: None,
            model_context_window: None,
            token_badge_cache: None,
            sidebar_filter: String::new(),
            sidebar_filter_active: false,
            current_todos: Vec::new(),
            subagent_todos: Vec::new(),
            active_task_tool_ids: std::collections::HashSet::new(),
            subagent_parent_idx: None,
            awaiting_ask_user_question: false,
            ask_user_questions_cache: None,
            stt_handle: None,
            stt_recording: false,
            stt_transcribing: false,
            show_file_tree: false,
            god_file_panel: None,
            god_file_queue: VecDeque::new(),
            show_session_list: false,
            session_list_selected: 0,
            session_list_scroll: 0,
            session_msg_counts: HashMap::new(),
            convo_search_active: false,
            convo_search: String::new(),
            convo_search_matches: Vec::new(),
            convo_search_current: 0,
            session_filter_active: false,
            session_filter: String::new(),
            session_content_search: false,
            session_search_results: Vec::new(),
        }
    }

    /// Mark rendered lines cache as dirty (call when display_events change)
    pub fn invalidate_render_cache(&mut self) {
        self.rendered_lines_dirty = true;
    }

    /// Mark sidebar cache as dirty (call when worktrees/selection/expansion changes)
    pub fn invalidate_sidebar(&mut self) {
        self.sidebar_dirty = true;
    }

    /// Mark file tree cache as dirty
    pub fn invalidate_file_tree(&mut self) {
        self.file_tree_dirty = true;
    }

    /// Recompute the cached token usage badge from current session_tokens + model_context_window.
    /// Call this whenever session_tokens or model_context_window changes — draw path just reads the cache.
    pub fn update_token_badge(&mut self) {
        self.token_badge_cache = self.session_tokens.map(|(ctx_tokens, _)| {
            let base_window = self.model_context_window.unwrap_or(200_000);
            let window = if ctx_tokens > base_window { 1_000_000 } else { base_window };
            // Claude reserves ~33k tokens as auto-compact buffer (compacts at ~83.5% raw).
            // Subtract the buffer so percentage reflects usable context, not total window.
            let usable = window.saturating_sub(33_000);
            let pct = (ctx_tokens as f64 / usable as f64 * 100.0).min(100.0);
            let color = if pct < 60.0 { ratatui::style::Color::Green }
                else if pct < 80.0 { ratatui::style::Color::Yellow }
                else { ratatui::style::Color::Red };
            (format!(" {:.0}% ", pct), color)
        });
    }

    /// Sample getrusage and update cached CPU% string. Called from draw path;
    /// only recomputes if ≥1s has elapsed since last sample (avoids overhead).
    pub fn update_cpu_usage(&mut self) {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.cpu_last_sample.0);
        if elapsed.as_millis() < 1000 { return; }
        let cpu_now = get_cpu_time_micros();
        let cpu_delta = cpu_now.saturating_sub(self.cpu_last_sample.1) as f64;
        let wall_delta = elapsed.as_micros() as f64;
        let pct = if wall_delta > 0.0 { cpu_delta / wall_delta * 100.0 } else { 0.0 };
        self.cpu_usage_text = format!("{:.0}%", pct);
        self.cpu_last_sample = (now, cpu_now);
    }

    pub fn current_project(&self) -> Option<&Project> { self.project.as_ref() }
    pub fn current_session(&self) -> Option<&Session> { self.selected_worktree.and_then(|idx| self.sessions.get(idx)) }

    pub fn is_session_running(&self, branch_name: &str) -> bool {
        self.running_sessions.contains(branch_name)
    }

    pub fn is_current_session_running(&self) -> bool {
        self.current_session().map(|s| self.running_sessions.contains(&s.branch_name)).unwrap_or(false)
    }

    pub fn set_status(&mut self, msg: impl Into<String>) { self.status_message = Some(msg.into()); }
    pub fn clear_status(&mut self) { self.status_message = None; }

    /// Toggle speech-to-text recording. Lazy-initializes the STT background thread on first use.
    /// Press once to start recording (magenta border), press again to stop and transcribe.
    pub fn toggle_stt(&mut self) {
        // Lazy-init: spawn the STT thread only when the user first presses ⌃s
        if self.stt_handle.is_none() {
            self.stt_handle = Some(crate::stt::SttHandle::spawn());
        }
        let handle = self.stt_handle.as_ref().unwrap();
        if self.stt_recording {
            handle.send(crate::stt::SttCommand::StopRecording);
        } else {
            handle.send(crate::stt::SttCommand::StartRecording);
        }
    }

    /// Poll STT events from background thread (non-blocking). Returns true if state changed.
    /// Called every event loop iteration when stt_handle exists.
    /// Collects events first to avoid borrow conflict (try_recv borrows handle, processing borrows &mut self).
    pub fn poll_stt(&mut self) -> bool {
        // Drain all pending events into a local vec, then drop the handle borrow
        let events: Vec<_> = self.stt_handle.as_ref()
            .map(|h| std::iter::from_fn(|| h.try_recv()).collect())
            .unwrap_or_default();
        if events.is_empty() { return false; }
        // Now we can freely mutate self while processing each event
        for event in events {
            match event {
                crate::stt::SttEvent::RecordingStarted => {
                    self.stt_recording = true;
                    self.set_status("Recording...");
                }
                crate::stt::SttEvent::RecordingStopped { duration_secs } => {
                    self.stt_recording = false;
                    self.set_status(format!("Transcribing {:.1}s of audio...", duration_secs));
                }
                crate::stt::SttEvent::Transcribed(text) => {
                    self.stt_transcribing = false;
                    self.insert_stt_text(&text);
                    self.clear_status();
                }
                crate::stt::SttEvent::Error(msg) => {
                    self.stt_recording = false;
                    self.stt_transcribing = false;
                    self.set_status(format!("STT: {}", msg));
                }
                crate::stt::SttEvent::ModelLoading => {
                    self.stt_transcribing = true;
                    self.set_status("Loading Whisper model...");
                }
                crate::stt::SttEvent::ModelReady => {}
            }
        }
        true
    }

    /// Insert transcribed text at the current cursor position.
    /// Routes to viewer edit buffer when in edit mode, otherwise to prompt input.
    /// Adds a leading space if the previous char isn't whitespace.
    fn insert_stt_text(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() { return; }

        if self.viewer_edit_mode {
            // Insert into viewer edit buffer at cursor position
            let (line, col) = self.viewer_edit_cursor;
            if let Some(line_str) = self.viewer_edit_content.get(line) {
                // Add space if previous char isn't whitespace
                if col > 0 {
                    if let Some(prev) = line_str.chars().nth(col - 1) {
                        if !prev.is_whitespace() {
                            self.viewer_edit_char(' ');
                        }
                    }
                }
            }
            for c in trimmed.chars() {
                self.viewer_edit_char(c);
            }
            self.viewer_edit_scroll_to_cursor();
        } else {
            // Insert into prompt input at cursor position
            if self.input_cursor > 0 {
                let chars: Vec<char> = self.input.chars().collect();
                if let Some(&prev) = chars.get(self.input_cursor - 1) {
                    if !prev.is_whitespace() {
                        self.input_char(' ');
                    }
                }
            }
            for c in trimmed.chars() {
                self.input_char(c);
            }
        }
    }
}

/// Get cumulative user+system CPU time for this process in microseconds.
/// Uses libc::getrusage(RUSAGE_SELF) — works on macOS and Linux.
fn get_cpu_time_micros() -> u64 {
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        let user = usage.ru_utime.tv_sec as u64 * 1_000_000 + usage.ru_utime.tv_usec as u64;
        let sys = usage.ru_stime.tv_sec as u64 * 1_000_000 + usage.ru_stime.tv_usec as u64;
        user + sys
    }
}
