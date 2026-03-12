//! App struct definition and initialization

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use portable_pty::{Child as PtyChild, MasterPty};

use crate::app::terminal::SessionTerminal;
use crate::app::types::{BranchDialog, FileTreeAction, FileTreeEntry, Focus, GitActionsPanel, HealthPanel, HealthTab, RcrSession, PostMergeDialog, PresetPrompt, PresetPromptDialog, PresetPromptPicker, ProjectsPanel, RunCommand, RunCommandDialog, RunCommandPicker, ViewMode, ViewerMode};
use crate::events::EventParser;
use crate::models::{Project, Worktree};
use crate::syntax::SyntaxHighlighter;
use crate::tui::render_thread::RenderThread;
use super::ClaudeEvent;
use super::DisplayEvent;

/// Action to run after a loading indicator popup renders on-screen.
/// Part of the two-phase deferred draw pattern: set the loading message +
/// deferred action → draw renders the popup → event loop executes the action
/// after draw completes → triggers another draw to show the result.
pub enum DeferredAction {
    /// Load a session file from the session list overlay
    LoadSession { branch: String, idx: usize },
    /// Load a file into the viewer pane (from FileTree Enter/click)
    LoadFile { path: PathBuf },
    /// Open the Worktree Health panel (scans god files + documentation)
    OpenHealthPanel,
    /// Switch to a different project (kills processes, reloads everything)
    SwitchProject { path: PathBuf },
    /// Rescan all health features after scope mode changes
    RescanHealthScope { dirs: Vec<String> },
    /// Git commit from the commit overlay (message already captured)
    GitCommit { worktree: PathBuf, message: String },
    /// Git commit + push from the commit overlay
    GitCommitAndPush { worktree: PathBuf, message: String },
}

/// Application state
pub struct App {
    pub project: Option<Project>,
    pub worktrees: Vec<Worktree>,
    pub selected_worktree: Option<usize>,
    pub session_lines: VecDeque<String>,
    pub max_session_lines: usize,
    pub session_buffer: String,
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
    /// Delete worktree confirmation dialog (⌘d)
    pub delete_worktree_dialog: Option<crate::app::types::DeleteWorktreeDialog>,
    pub view_mode: ViewMode,
    pub focus: Focus,
    pub prompt_mode: bool,
    pub should_quit: bool,
    pub status_message: Option<String>,
    /// Claude event receivers keyed by slot_id (PID string). One per running process.
    pub claude_receivers: HashMap<String, Receiver<ClaudeEvent>>,
    /// Set of currently running slot_ids (PID strings)
    pub running_sessions: HashSet<String>,
    /// Branches with at least one unread finished session (for tab rendering)
    pub unread_sessions: HashSet<String>,
    /// Individual session UUIDs that finished while user wasn't viewing them
    pub unread_session_ids: HashSet<String>,
    /// Last exit code per slot_id (shown in session pane title after Claude exits)
    pub claude_exit_codes: HashMap<String, i32>,
    /// Claude API session UUIDs per slot_id (for --resume)
    pub claude_session_ids: HashMap<String, String>,
    /// Maps branch_name → list of active slot_ids (PID strings, spawn order)
    pub branch_slots: HashMap<String, Vec<String>>,
    /// Which slot_id is actively displayed per branch (its output feeds display_events)
    pub active_slot: HashMap<String, String>,
    pub session_scroll: usize,
    pub syntax_highlighter: SyntaxHighlighter,
    pub show_help: bool,
    pub branch_dialog: Option<BranchDialog>,
    /// Projects panel state (full-screen overlay for project selection)
    pub projects_panel: Option<ProjectsPanel>,
    /// Pending session names to save when Claude returns session ID: Vec<(slot_id, custom_name)>.
    /// Multiple concurrent spawns (e.g. GFM) can each register their own pending name.
    pub pending_session_names: Vec<(String, String)>,
    pub terminal_mode: bool,
    pub terminal_pty: Option<Box<dyn MasterPty + Send>>,
    pub terminal_child: Option<Box<dyn PtyChild + Send + Sync>>,
    pub terminal_writer: Option<Box<dyn Write + Send>>,
    pub terminal_rx: Option<Receiver<Vec<u8>>>,
    pub terminal_parser: vt100::Parser,
    pub terminal_scroll: usize,
    pub terminal_height: u16,
    /// Actual terminal window height in rows (for modal page-scroll calculations).
    /// Updated on startup and resize events — NOT the embedded terminal pane height.
    pub screen_height: u16,
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
    /// Signals the event loop to reset the background ClaudeProcessor's parser
    /// state (e.g., on session switch). The event loop checks and clears this.
    pub claude_processor_needs_reset: bool,
    /// True when the user is viewing a session file that doesn't match the
    /// active slot's Claude session. Suppresses live event display and PID badge
    /// so content from a running process doesn't bleed into a historic view.
    pub viewing_historic_session: bool,
    /// Kernel-level file watcher (replaces stat() polling for change detection).
    /// None if notify failed to initialize — falls back to polling in that case.
    pub file_watcher: Option<crate::watcher::FileWatcher>,
    /// Whether the worktree directory changed (debounced file tree refresh)
    pub file_tree_refresh_pending: bool,
    /// Whether the health panel needs a rescan (debounced alongside file tree)
    pub health_refresh_pending: bool,
    /// Whether the worktree tab list needs a refresh (debounced alongside file tree)
    pub worktree_tabs_refresh_pending: bool,
    /// Timestamp of last worktree change notification (for 500ms debounce)
    pub worktree_last_notify: std::time::Instant,
    /// Receiver for background file tree scan (replaces synchronous load_file_tree in event loop)
    pub file_tree_receiver: Option<std::sync::mpsc::Receiver<Vec<FileTreeEntry>>>,
    /// Receiver for background worktree refresh (replaces synchronous refresh_worktrees in event loop)
    pub worktree_refresh_receiver: Option<std::sync::mpsc::Receiver<anyhow::Result<crate::app::types::WorktreeRefreshResult>>>,
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
    /// ratatui-image protocol state for the currently loaded image (adapts to terminal size)
    pub viewer_image_state: Option<ratatui_image::protocol::StatefulProtocol>,
    /// Terminal graphics protocol picker — detects capabilities once, reused for all images
    pub image_picker: Option<ratatui_image::picker::Picker>,
    /// Cached rendered lines for session pane (expensive to compute)
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
    /// Tool indicator positions (line_idx, span_idx, tool_use_id) for draw-time status patching.
    /// Tracks ALL tool calls (not just pending) so indicators update in real-time when tools
    /// complete or fail, without waiting for a full re-render.
    pub animation_line_indices: Vec<(usize, usize, String)>,
    /// Generation counter — incremented when pending_tool_calls or failed_tool_calls changes.
    /// Used to invalidate the viewport cache so status circles redraw immediately.
    pub tool_status_generation: u64,
    /// Background render thread — expensive session pane rendering runs here, never blocks the event loop
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
    /// When true, next terminal.draw() calls terminal.clear() first to reset
    /// ratatui's internal buffer. Needed after fast_draw_session() writes
    /// directly to the terminal — ratatui doesn't know those cells changed,
    /// so its diff misses them when switching to a different layout (e.g. git panel).
    pub force_full_redraw: bool,
    /// Cached CPU usage string for status bar (updated every ~1s via getrusage delta)
    pub cpu_usage_text: String,
    /// Last getrusage sample: (wall_time, cpu_time_micros)
    pub cpu_last_sample: (std::time::Instant, u64),
    /// Exponentially smoothed CPU percentage (reduces noise from Windows timer granularity)
    pub cpu_smoothed: f64,
    /// Cached input area rect from last full draw — used for fast-path direct
    /// input rendering that bypasses terminal.draw() during rapid typing.
    pub input_area: ratatui::layout::Rect,
    /// Cached pane rects from last full draw — used for mouse click hit-testing
    /// and scroll dispatch without recalculating layout
    pub pane_worktrees: ratatui::layout::Rect,
    pub pane_viewer: ratatui::layout::Rect,
    pub pane_session: ratatui::layout::Rect,
    /// The actual session content rect (excludes todo widget and search bar at bottom).
    /// Used by fast_draw_session to avoid overwriting those sub-areas.
    pub pane_session_content: ratatui::layout::Rect,
    /// Cached rect for the worktree tab row (mouse click hit-testing)
    pub pane_worktree_tabs: ratatui::layout::Rect,
    /// Hit-test regions for worktree tab bar clicks: (x_start, x_end, tab_target)
    /// None = [M] main branch tab, Some(idx) = worktree index
    pub worktree_tab_hits: Vec<(u16, u16, Option<usize>)>,
    /// Cached rect for the todo widget area (mouse scroll hit-testing)
    pub pane_todo: ratatui::layout::Rect,
    /// Scroll offset for the todo widget (lines scrolled from top)
    pub todo_scroll: u16,
    /// Total visual lines in the todo widget (for scroll bounds, set during draw)
    pub todo_total_lines: u16,
    /// Cached viewport slice for session pane — avoids cloning rendered_lines_cache every frame.
    /// Only rebuilt when scroll position, content, or animation tick changes.
    pub session_viewport_cache: Vec<ratatui::text::Line<'static>>,
    /// Scroll position and animation tick used to build the viewport cache (invalidation key)
    pub session_viewport_scroll: usize,
    pub session_viewport_anim_tick: u64,
    /// Tool status generation used to build the viewport cache
    pub session_viewport_status_gen: u64,
    /// Title string corresponding to the cached viewport
    pub session_viewport_title: String,
    /// Total lines in last parsed session file
    pub parse_total_lines: usize,
    /// Parse errors in last parsed session file
    pub parse_errors: usize,
    /// Assistant parsing diagnostics
    pub assistant_total: usize,
    pub assistant_no_message: usize,
    pub assistant_no_content_arr: usize,
    pub assistant_text_blocks: usize,
    /// Cached Claude session files per worktree branch (session_id, path, formatted_time)
    pub session_files: HashMap<String, Vec<(String, PathBuf, String)>>,
    /// Selected Claude session file index per worktree (0 = latest/newest)
    pub session_selected_file_idx: HashMap<String, usize>,
    /// Cached file tree lines (avoid rebuilding every frame)
    pub file_tree_lines_cache: Vec<ratatui::text::Line<'static>>,
    /// Flag indicating file tree cache needs refresh
    pub file_tree_dirty: bool,
    /// Scroll position used for file tree cache
    pub file_tree_scroll_cached: usize,
    /// Awaiting user response to plan approval (ExitPlanMode was called)
    pub awaiting_plan_approval: bool,
    /// Cached viewport height for viewer pane (set during render, used for scroll)
    pub viewer_viewport_height: usize,
    /// Cached viewport height for output/session pane (set during render, used for scroll)
    pub session_viewport_height: usize,
    /// Line indices where message bubbles start (for Up/Down navigation)
    /// Each entry is (line_index, is_user_message) - true for UserMessage, false for AssistantText
    pub message_bubble_positions: Vec<(usize, bool)>,
    /// Clickable file path links in output: (line_idx, start_col, end_col, file_path, old_string, new_string, wrap_line_count)
    pub clickable_paths: Vec<(usize, usize, usize, String, String, String, usize)>,
    /// Clickable table regions in output: (cache_line_start, cache_line_end, raw_markdown)
    pub clickable_tables: Vec<(usize, usize, String)>,
    /// Full-width table popup overlay (opened by clicking a table in session pane)
    pub table_popup: Option<crate::app::types::TablePopup>,
    /// Currently highlighted (clicked) file path in session pane: (line_idx, start_col, end_col, wrap_line_count)
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
    /// Text selection for session pane: (start_visual_line, start_col, end_visual_line, end_col)
    pub session_selection: Option<(usize, usize, usize, usize)>,
    /// Whether the git status box text is selected (for copy via Cmd+C)
    pub git_status_selected: bool,
    /// Cached output selection for viewport cache invalidation (rebuild viewport when selection changes)
    pub session_selection_cached: Option<(usize, usize, usize, usize)>,
    /// Mouse drag anchor in cache coordinates: (cache_line_or_char, cache_col, pane_id)
    /// pane_id: 0=viewer, 1=session, 2=input. Stored as cache coords so auto-scroll
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
    /// Saved preset prompts (quick-insert templates for the input box, ⌥P)
    pub preset_prompts: Vec<PresetPrompt>,
    /// Preset prompt picker overlay (select from saved presets)
    pub preset_prompt_picker: Option<PresetPromptPicker>,
    /// Preset prompt add/edit dialog
    pub preset_prompt_dialog: Option<PresetPromptDialog>,
    /// Latest token usage from most recent assistant event: (context_tokens, output_tokens)
    /// context_tokens = input_tokens + cache_read + cache_creation (effective context size)
    pub session_tokens: Option<(u64, u64)>,
    /// Context window size detected from model string (None = not yet known, default 200k)
    pub model_context_window: Option<u64>,
    /// Cached token usage badge: (formatted_string, color) — only recomputed when token data changes
    pub token_badge_cache: Option<(String, ratatui::style::Color)>,
    /// True when computed context usage ≥ 95% (triggers compaction inactivity watcher)
    pub context_pct_high: bool,
    /// Last time display_events were extended (new events parsed from session or stream)
    pub last_session_event_time: std::time::Instant,
    /// Whether we've already injected the MayBeCompacting banner for the current high-context period
    pub compaction_banner_injected: bool,
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
    /// Use Nerd Font icons in file tree (set from Config on startup)
    pub nerd_fonts: bool,
    /// File/directory names to hide in the file tree (e.g. ".git", ".DS_Store")
    pub file_tree_hidden_dirs: HashSet<String>,
    /// When true, the file tree pane switches to "options" overlay mode
    pub file_tree_options_mode: bool,
    /// Selected row in the file tree options overlay (0-indexed into OPTIONS list)
    pub file_tree_options_selected: usize,
    /// Worktree Health panel — tabbed modal overlay with god file scanner,
    /// documentation coverage, and future health checks. None when closed.
    pub health_panel: Option<HealthPanel>,
    /// Remembers which tab was last active so the panel reopens on the same tab
    pub last_health_tab: HealthTab,
    /// When true, the FileTree is in "god file filter mode" — directories included
    /// in the god file scan are highlighted green, and the user can press Enter to
    /// toggle directories in/out of the scan scope.
    pub god_file_filter_mode: bool,
    /// The set of directories currently included in the god file scan scope.
    /// Populated when entering filter mode from the auto-detected SOURCE_ROOTS.
    /// User can add/remove dirs via Enter in filter mode.
    pub god_file_filter_dirs: std::collections::HashSet<std::path::PathBuf>,
    /// Git Actions panel state (Shift+G overlay for git operations + changed files)
    pub git_actions_panel: Option<GitActionsPanel>,
    /// Debug dump naming dialog — Some(text) means the user is typing a dump name
    pub debug_dump_naming: Option<String>,
    /// Debug dump saving — Some(name) triggers the actual dump on next frame
    pub debug_dump_saving: Option<String>,
    /// Active Merge Conflict Resolution session — when Some, session pane shows green
    /// borders, routes prompts to repo root, and displays approval dialog after Claude exits
    pub rcr_session: Option<RcrSession>,
    /// Post-merge dialog — shown after successful squash merge or RCR accept.
    /// Asks user to keep (rebase), archive, or delete the worktree/branch.
    pub post_merge_dialog: Option<PostMergeDialog>,
    /// Branch names with auto-rebase enabled (persisted in project azufig.toml)
    pub auto_rebase_enabled: HashSet<String>,
    /// Throttle for periodic auto-rebase checks (every 2 seconds)
    pub last_auto_rebase_check: std::time::Instant,
    /// Auto-rebase success dialog: (branch_display_name, dismiss_at). Shown for 2 seconds.
    pub auto_rebase_success_until: Option<(String, std::time::Instant)>,
    /// True when user is browsing the main/master branch (via Shift+M).
    /// Main acts like any other worktree — the ★ yellow tab is visual distinction only.
    pub browsing_main: bool,
    /// Saved worktree selection before entering main browse mode (restored on exit)
    pub pre_main_browse_selection: Option<usize>,
    /// Main worktree data — stored separately from app.worktrees so browse mode
    /// can display main's files/sessions without main polluting the sidebar list
    pub main_worktree: Option<Worktree>,
    /// Whether the session list overlay is shown in the Session pane (toggled with 's')
    pub show_session_list: bool,
    /// True while session list message counts are being computed (shows loading dialog)
    pub session_list_loading: bool,
    /// Generic loading indicator — Some(message) shows a centered popup while
    /// a deferred action runs on the next frame (two-phase deferred draw pattern)
    pub loading_indicator: Option<String>,
    /// Action to execute after the loading indicator has rendered on-screen
    pub deferred_action: Option<DeferredAction>,
    /// Receiver for background worktree/git operations (archive, unarchive,
    /// create, delete, pull, push). Polled in the event loop.
    pub background_op_receiver: Option<std::sync::mpsc::Receiver<crate::app::types::BackgroundOpProgress>>,
    /// Receiver for background rebase operations (separate because rebase
    /// has conflict handling that needs the full RebaseOutcome)
    pub rebase_op_receiver: Option<std::sync::mpsc::Receiver<crate::app::types::BackgroundRebaseOutcome>>,
    /// Selected index in session list overlay
    pub session_list_selected: usize,
    /// Scroll offset in session list overlay
    pub session_list_scroll: usize,
    /// Cached message counts per session_id → (count, file_size) — only recomputed when size changes
    pub session_msg_counts: HashMap<String, (usize, u64)>,

    // ── Session find (find text in current session's rendered output) ──
    /// Whether the search bar is active (accepting keystrokes)
    pub session_find_active: bool,
    /// Current search query text
    pub session_find: String,
    /// All matches: (cache_line_idx, start_col, end_col). Recomputed on each keystroke.
    pub session_find_matches: Vec<(usize, usize, usize)>,
    /// Index into session_find_matches for the "current" highlighted match
    pub session_find_current: usize,

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

    // ── Model selection (⌃m cycle) ──
    /// User-selected model override (None = use Claude CLI default)
    pub selected_model: Option<String>,
    /// Model detected from the live stream's assistant event (e.g. "claude-opus-4-6")
    pub detected_model: Option<String>,
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
            worktrees: Vec::new(),
            selected_worktree: None,
            session_lines: VecDeque::with_capacity(10000),
            max_session_lines: 10000,
            session_buffer: String::new(),
            display_events: Vec::new(),
            pending_user_message: None,
            staged_prompt: None,
            event_parser: EventParser::new(),
            selected_event: None,
            input: String::new(),
            input_cursor: 0,
            input_selection: None,
            delete_worktree_dialog: None,
            view_mode: ViewMode::Session,
            focus: Focus::FileTree,
            prompt_mode: false,
            should_quit: false,
            status_message: None,
            claude_receivers: HashMap::new(),
            running_sessions: HashSet::new(),
            unread_sessions: HashSet::new(),
            unread_session_ids: HashSet::new(),
            claude_exit_codes: HashMap::new(),
            claude_session_ids: HashMap::new(),
            branch_slots: HashMap::new(),
            active_slot: HashMap::new(),
            session_scroll: usize::MAX, // Start at bottom (most recent messages)
            syntax_highlighter: SyntaxHighlighter::new(),
            show_help: false,
            branch_dialog: None,
            projects_panel: None,
            pending_session_names: Vec::new(),
            terminal_mode: false,
            terminal_pty: None,
            terminal_child: None,
            terminal_writer: None,
            terminal_rx: None,
            terminal_parser: vt100::Parser::new(24, 120, 1000),
            terminal_scroll: 0,
            terminal_height: 12,
            screen_height: crossterm::terminal::size().map(|(_, h)| h).unwrap_or(24),
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
            claude_processor_needs_reset: false,
            viewing_historic_session: false,
            file_watcher: crate::watcher::FileWatcher::spawn(),
            file_tree_refresh_pending: false,
            health_refresh_pending: false,
            worktree_tabs_refresh_pending: false,
            worktree_last_notify: std::time::Instant::now(),
            file_tree_receiver: None,
            worktree_refresh_receiver: None,
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
            viewer_image_state: None,
            image_picker: None,
            rendered_lines_cache: Vec::new(),
            rendered_lines_width: 0,
            rendered_lines_dirty: true,
            rendered_events_count: 0,
            rendered_content_line_count: 0,
            rendered_events_start: 0,
            animation_line_indices: Vec::new(),
            tool_status_generation: 0,
            render_thread: RenderThread::spawn(),
            render_seq_applied: 0,
            render_in_flight: false,
            last_render_submit: std::time::Instant::now(),
            draw_pending: false,
            force_full_redraw: false,
            cpu_usage_text: String::new(),
            cpu_last_sample: (std::time::Instant::now(), get_cpu_time_micros()),
            cpu_smoothed: 0.0,
            input_area: ratatui::layout::Rect::default(),
            pane_worktrees: ratatui::layout::Rect::default(),
            pane_viewer: ratatui::layout::Rect::default(),
            pane_session: ratatui::layout::Rect::default(),
            pane_session_content: ratatui::layout::Rect::default(),
            pane_worktree_tabs: ratatui::layout::Rect::default(),
            worktree_tab_hits: Vec::new(),
            pane_todo: ratatui::layout::Rect::default(),
            todo_scroll: 0,
            todo_total_lines: 0,
            session_viewport_cache: Vec::new(),
            session_viewport_scroll: usize::MAX,
            session_viewport_anim_tick: u64::MAX,
            session_viewport_status_gen: u64::MAX,
            session_viewport_title: String::new(),
            parse_total_lines: 0,
            parse_errors: 0,
            assistant_total: 0,
            assistant_no_message: 0,
            assistant_no_content_arr: 0,
            assistant_text_blocks: 0,
            session_files: HashMap::new(),
            session_selected_file_idx: HashMap::new(),
            file_tree_lines_cache: Vec::new(),
            file_tree_dirty: true,
            file_tree_scroll_cached: usize::MAX,
            awaiting_plan_approval: false,
            viewer_viewport_height: 20,
            session_viewport_height: 20,
            message_bubble_positions: Vec::new(),
            selected_tool_diff: None,
            clickable_paths: Vec::new(),
            clickable_tables: Vec::new(),
            table_popup: None,
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
            session_selection: None,
            git_status_selected: false,
            session_selection_cached: None,
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
            preset_prompts: Vec::new(),
            preset_prompt_picker: None,
            preset_prompt_dialog: None,
            session_tokens: None,
            model_context_window: None,
            token_badge_cache: None,
            context_pct_high: false,
            last_session_event_time: std::time::Instant::now(),
            compaction_banner_injected: false,
            current_todos: Vec::new(),
            subagent_todos: Vec::new(),
            active_task_tool_ids: std::collections::HashSet::new(),
            subagent_parent_idx: None,
            awaiting_ask_user_question: false,
            ask_user_questions_cache: None,
            stt_handle: None,
            stt_recording: false,
            stt_transcribing: false,
            nerd_fonts: true,
            file_tree_hidden_dirs: HashSet::new(), // populated from azufig in load()
            file_tree_options_mode: false,
            file_tree_options_selected: 0,
            health_panel: None,
            last_health_tab: HealthTab::GodFiles,
            god_file_filter_mode: false,
            god_file_filter_dirs: std::collections::HashSet::new(),
            git_actions_panel: None,
            debug_dump_naming: None,
            debug_dump_saving: None,
            rcr_session: None,
            post_merge_dialog: None,
            auto_rebase_enabled: HashSet::new(), // populated from azufig in load()
            last_auto_rebase_check: std::time::Instant::now(),
            auto_rebase_success_until: None,
            browsing_main: false,
            pre_main_browse_selection: None,
            main_worktree: None,
            show_session_list: false,
            session_list_loading: false,
            loading_indicator: None,
            deferred_action: None,
            background_op_receiver: None,
            rebase_op_receiver: None,
            session_list_selected: 0,
            session_list_scroll: 0,
            session_msg_counts: HashMap::new(),
            session_find_active: false,
            session_find: String::new(),
            session_find_matches: Vec::new(),
            session_find_current: 0,
            session_filter_active: false,
            session_filter: String::new(),
            session_content_search: false,
            session_search_results: Vec::new(),
            selected_model: Some("opus".to_string()),
            detected_model: None,
        }
    }

    /// Mark rendered lines cache as dirty (call when display_events change)
    pub fn invalidate_render_cache(&mut self) {
        self.rendered_lines_dirty = true;
    }

    /// Mark sidebar cache as dirty (call when worktrees/selection/expansion changes)
    pub fn invalidate_sidebar(&mut self) {
        // Sidebar replaced by worktree tab row — no cache to invalidate
    }

    /// Mark file tree cache as dirty
    pub fn invalidate_file_tree(&mut self) {
        self.file_tree_dirty = true;
    }

    /// Rebuild file tree entries from disk (preserves expanded set, resets selection)
    pub fn refresh_file_tree(&mut self) {
        let Some(wt) = self.current_worktree() else { return };
        let Some(ref worktree_path) = wt.worktree_path else { return };
        let wt_path = worktree_path.clone();
        self.file_tree_entries = super::helpers::build_file_tree(&wt_path, &self.file_tree_expanded, &self.file_tree_hidden_dirs);
        if self.file_tree_selected.map_or(true, |i| i >= self.file_tree_entries.len()) {
            self.file_tree_selected = if self.file_tree_entries.is_empty() { None } else { Some(0) };
        }
        self.invalidate_file_tree();
    }

    /// Recompute the cached token usage badge from current session_tokens + model_context_window.
    /// Call this whenever session_tokens or model_context_window changes — draw path just reads the cache.
    pub fn update_token_badge(&mut self) {
        let mut pct_value = 0.0_f64;
        self.token_badge_cache = self.session_tokens.map(|(ctx_tokens, _)| {
            let base_window = self.model_context_window.unwrap_or(200_000);
            let window = if ctx_tokens > base_window { 1_000_000 } else { base_window };
            // Claude reserves ~33k tokens as auto-compact buffer (compacts at ~83.5% raw).
            // Subtract the buffer so percentage reflects usable context, not total window.
            let usable = window.saturating_sub(33_000);
            let pct = (ctx_tokens as f64 / usable as f64 * 100.0).min(100.0);
            pct_value = pct;
            let color = if pct < 60.0 { ratatui::style::Color::Green }
                else if pct < 90.0 { ratatui::style::Color::Yellow }
                else { ratatui::style::Color::Red };
            (format!(" {:.0}% ", pct), color)
        });
        // Track 95% threshold for compaction inactivity watcher
        let was_high = self.context_pct_high;
        self.context_pct_high = pct_value >= 95.0;
        // Reset banner state when context drops below threshold (e.g. after compaction)
        if was_high && !self.context_pct_high {
            self.compaction_banner_injected = false;
        }
    }

    /// Short display name for the active model. Always returns the selected_model
    /// alias since it's always set (never None).
    pub fn display_model_name(&self) -> &str {
        self.selected_model.as_deref().unwrap_or("opus")
    }

    /// Cycle selected_model through: opus → sonnet → haiku → opus.
    /// Always set — the displayed model is exactly what gets passed as --model to spawn().
    pub fn cycle_model(&mut self) {
        self.selected_model = Some(match self.selected_model.as_deref() {
            Some("opus") => "sonnet",
            Some("sonnet") => "haiku",
            _ => "opus",
        }.to_string());
    }

    /// Sample getrusage and update cached CPU% string. Called from draw path;
    /// only recomputes if ≥1s has elapsed since last sample (avoids overhead).
    /// Normalizes by core count to match OS task manager conventions.
    pub fn update_cpu_usage(&mut self) {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.cpu_last_sample.0);
        // Sample every 3s — longer window averages out Windows timer tick noise
        // (GetProcessTimes only updates at ~15.6ms granularity)
        if elapsed.as_millis() < 3000 { return; }
        let cpu_now = get_cpu_time_micros();
        let cpu_delta = cpu_now.saturating_sub(self.cpu_last_sample.1) as f64;
        let wall_delta = elapsed.as_micros() as f64;
        let cores = std::thread::available_parallelism()
            .map(|n| n.get() as f64)
            .unwrap_or(1.0);
        let raw_pct = if wall_delta > 0.0 { cpu_delta / wall_delta / cores * 100.0 } else { 0.0 };
        // Exponential moving average (alpha=0.2) for heavy smoothing
        self.cpu_smoothed = if self.cpu_smoothed == 0.0 { raw_pct } else { self.cpu_smoothed * 0.8 + raw_pct * 0.2 };
        // Floor: show "0%" for values under 0.5 to match Task Manager conventions
        let display = if self.cpu_smoothed < 0.5 { 0.0 } else { self.cpu_smoothed };
        self.cpu_usage_text = format!("{:.0}%", display);
        self.cpu_last_sample = (now, cpu_now);
    }

    pub fn current_project(&self) -> Option<&Project> { self.project.as_ref() }
    /// When browsing main, returns the separate main_worktree; otherwise indexes into worktrees vec
    pub fn current_worktree(&self) -> Option<&Worktree> {
        if self.browsing_main { return self.main_worktree.as_ref(); }
        self.selected_worktree.and_then(|idx| self.worktrees.get(idx))
    }

    /// True if ANY Claude process is running on this branch (any slot)
    pub fn is_session_running(&self, branch_name: &str) -> bool {
        self.branch_slots.get(branch_name)
            .map(|slots| slots.iter().any(|s| self.running_sessions.contains(s)))
            .unwrap_or(false)
    }

    /// True if the ACTIVE slot (the one feeding display_events) is running
    pub fn is_active_slot_running(&self) -> bool {
        self.current_worktree().and_then(|s| {
            self.active_slot.get(&s.branch_name)
                .map(|slot| self.running_sessions.contains(slot))
        }).unwrap_or(false)
    }

    /// Look up which branch a slot_id belongs to (reverse lookup)
    pub fn branch_for_slot(&self, slot_id: &str) -> Option<String> {
        self.branch_slots.iter()
            .find(|(_, slots)| slots.contains(&slot_id.to_string()))
            .map(|(branch, _)| branch.clone())
    }

    /// Check if a Claude session UUID has a running process (for status dots in session list)
    pub fn is_claude_session_running(&self, claude_session_id: &str) -> bool {
        self.claude_session_ids.iter()
            .any(|(slot, sid)| sid == claude_session_id && self.running_sessions.contains(slot))
    }

    pub fn set_status(&mut self, msg: impl Into<String>) { self.status_message = Some(msg.into()); }
    pub fn clear_status(&mut self) { self.status_message = None; }

    /// Open a full-width table popup for the given raw markdown table text.
    /// Re-renders the table at near-terminal width so columns aren't truncated.
    pub fn open_table_popup(&mut self, raw_markdown: &str) {
        let term_width = crossterm::terminal::size().map(|(w, _)| w as usize).unwrap_or(120);
        let popup_width = term_width.saturating_sub(8).max(60);
        let lines = crate::tui::render_markdown::render_table_for_popup(raw_markdown, popup_width);
        let total_lines = lines.len();
        self.table_popup = Some(crate::app::types::TablePopup { lines, scroll: 0, total_lines });
    }

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
        let events: Vec<_> = self.stt_handle.as_ref()
            .map(|h| std::iter::from_fn(|| h.try_recv()).collect())
            .unwrap_or_default();
        if events.is_empty() { return false; }
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
#[cfg(unix)]
fn get_cpu_time_micros() -> u64 {
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        let user = usage.ru_utime.tv_sec as u64 * 1_000_000 + usage.ru_utime.tv_usec as u64;
        let sys = usage.ru_stime.tv_sec as u64 * 1_000_000 + usage.ru_stime.tv_usec as u64;
        user + sys
    }
}

/// Get cumulative user+system CPU time for this process in microseconds.
#[cfg(windows)]
fn get_cpu_time_micros() -> u64 {
    use std::mem::MaybeUninit;
    unsafe {
        let handle = windows_sys::Win32::System::Threading::GetCurrentProcess();
        let mut creation = MaybeUninit::zeroed();
        let mut exit = MaybeUninit::zeroed();
        let mut kernel = MaybeUninit::zeroed();
        let mut user = MaybeUninit::zeroed();
        if windows_sys::Win32::System::Threading::GetProcessTimes(
            handle,
            creation.as_mut_ptr(),
            exit.as_mut_ptr(),
            kernel.as_mut_ptr(),
            user.as_mut_ptr(),
        ) != 0
        {
            let k = kernel.assume_init();
            let u = user.assume_init();
            // FILETIME is 100ns intervals → divide by 10 for microseconds
            let kernel_us = (k.dwLowDateTime as u64 | (k.dwHighDateTime as u64) << 32) / 10;
            let user_us = (u.dwLowDateTime as u64 | (u.dwHighDateTime as u64) << 32) / 10;
            kernel_us + user_us
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── App::new() constructor defaults ──

    #[test]
    fn new_project_is_none() {
        let app = App::new();
        assert!(app.project.is_none());
    }

    #[test]
    fn new_worktrees_empty() {
        let app = App::new();
        assert!(app.worktrees.is_empty());
    }

    #[test]
    fn new_selected_worktree_none() {
        let app = App::new();
        assert!(app.selected_worktree.is_none());
    }

    #[test]
    fn new_session_lines_empty_with_capacity() {
        let app = App::new();
        assert!(app.session_lines.is_empty());
        assert_eq!(app.max_session_lines, 10000);
    }

    #[test]
    fn new_session_buffer_empty() {
        let app = App::new();
        assert!(app.session_buffer.is_empty());
    }

    #[test]
    fn new_display_events_empty() {
        let app = App::new();
        assert!(app.display_events.is_empty());
    }

    #[test]
    fn new_pending_user_message_none() {
        let app = App::new();
        assert!(app.pending_user_message.is_none());
    }

    #[test]
    fn new_staged_prompt_none() {
        let app = App::new();
        assert!(app.staged_prompt.is_none());
    }

    #[test]
    fn new_selected_event_none() {
        let app = App::new();
        assert!(app.selected_event.is_none());
    }

    #[test]
    fn new_input_empty() {
        let app = App::new();
        assert!(app.input.is_empty());
        assert_eq!(app.input_cursor, 0);
    }

    #[test]
    fn new_input_selection_none() {
        let app = App::new();
        assert!(app.input_selection.is_none());
    }

    #[test]
    fn new_delete_worktree_dialog_none() {
        let app = App::new();
        assert!(app.delete_worktree_dialog.is_none());
    }

    #[test]
    fn new_view_mode_session() {
        let app = App::new();
        assert_eq!(app.view_mode, ViewMode::Session);
    }

    #[test]
    fn new_focus_filetree() {
        let app = App::new();
        assert_eq!(app.focus, Focus::FileTree);
    }

    #[test]
    fn new_prompt_mode_false() {
        let app = App::new();
        assert!(!app.prompt_mode);
    }

    #[test]
    fn new_should_quit_false() {
        let app = App::new();
        assert!(!app.should_quit);
    }

    #[test]
    fn new_status_message_none() {
        let app = App::new();
        assert!(app.status_message.is_none());
    }

    #[test]
    fn new_claude_receivers_empty() {
        let app = App::new();
        assert!(app.claude_receivers.is_empty());
    }

    #[test]
    fn new_running_sessions_empty() {
        let app = App::new();
        assert!(app.running_sessions.is_empty());
    }

    #[test]
    fn new_unread_sessions_empty() {
        let app = App::new();
        assert!(app.unread_sessions.is_empty());
        assert!(app.unread_session_ids.is_empty());
    }

    #[test]
    fn new_claude_exit_codes_empty() {
        let app = App::new();
        assert!(app.claude_exit_codes.is_empty());
    }

    #[test]
    fn new_claude_session_ids_empty() {
        let app = App::new();
        assert!(app.claude_session_ids.is_empty());
    }

    #[test]
    fn new_branch_slots_empty() {
        let app = App::new();
        assert!(app.branch_slots.is_empty());
        assert!(app.active_slot.is_empty());
    }

    #[test]
    fn new_session_scroll_at_max() {
        let app = App::new();
        assert_eq!(app.session_scroll, usize::MAX);
    }

    #[test]
    fn new_show_help_false() {
        let app = App::new();
        assert!(!app.show_help);
    }

    #[test]
    fn new_branch_dialog_none() {
        let app = App::new();
        assert!(app.branch_dialog.is_none());
    }

    #[test]
    fn new_projects_panel_none() {
        let app = App::new();
        assert!(app.projects_panel.is_none());
    }

    #[test]
    fn new_terminal_mode_false() {
        let app = App::new();
        assert!(!app.terminal_mode);
    }

    #[test]
    fn new_terminal_pty_none() {
        let app = App::new();
        assert!(app.terminal_pty.is_none());
        assert!(app.terminal_child.is_none());
        assert!(app.terminal_writer.is_none());
        assert!(app.terminal_rx.is_none());
    }

    #[test]
    fn new_terminal_scroll_zero() {
        let app = App::new();
        assert_eq!(app.terminal_scroll, 0);
    }

    #[test]
    fn new_terminal_height_defaults() {
        let app = App::new();
        assert_eq!(app.terminal_height, 12);
        assert_eq!(app.terminal_rows, 24);
        assert_eq!(app.terminal_cols, 120);
    }

    #[test]
    fn new_tool_status_generation_zero() {
        let app = App::new();
        assert_eq!(app.tool_status_generation, 0);
    }

    #[test]
    fn new_pending_tool_calls_empty() {
        let app = App::new();
        assert!(app.pending_tool_calls.is_empty());
        assert!(app.failed_tool_calls.is_empty());
    }

    #[test]
    fn new_animation_tick_zero() {
        let app = App::new();
        assert_eq!(app.animation_tick, 0);
    }

    #[test]
    fn new_session_file_defaults() {
        let app = App::new();
        assert!(app.session_file_path.is_none());
        assert!(app.session_file_modified.is_none());
        assert_eq!(app.session_file_size, 0);
        assert_eq!(app.session_file_parse_offset, 0);
        assert!(!app.session_file_dirty);
    }

    #[test]
    fn new_viewing_historic_session_false() {
        let app = App::new();
        assert!(!app.viewing_historic_session);
    }

    #[test]
    fn new_file_tree_empty() {
        let app = App::new();
        assert!(app.file_tree_entries.is_empty());
        assert!(app.file_tree_selected.is_none());
        assert_eq!(app.file_tree_scroll, 0);
        assert!(app.file_tree_expanded.is_empty());
    }

    #[test]
    fn new_viewer_defaults() {
        let app = App::new();
        assert!(app.viewer_content.is_none());
        assert!(app.viewer_path.is_none());
        assert_eq!(app.viewer_scroll, 0);
        assert_eq!(app.viewer_mode, ViewerMode::Empty);
    }

    #[test]
    fn new_viewer_lines_dirty() {
        let app = App::new();
        assert!(app.viewer_lines_dirty);
    }

    #[test]
    fn new_rendered_lines_dirty() {
        let app = App::new();
        assert!(app.rendered_lines_dirty);
    }

    #[test]
    fn new_render_state() {
        let app = App::new();
        assert_eq!(app.render_seq_applied, 0);
        assert!(!app.render_in_flight);
        assert!(!app.draw_pending);
    }

    #[test]
    fn new_parse_stats_zero() {
        let app = App::new();
        assert_eq!(app.parse_total_lines, 0);
        assert_eq!(app.parse_errors, 0);
        assert_eq!(app.assistant_total, 0);
        assert_eq!(app.assistant_no_message, 0);
        assert_eq!(app.assistant_no_content_arr, 0);
        assert_eq!(app.assistant_text_blocks, 0);
    }

    #[test]
    fn new_session_files_empty() {
        let app = App::new();
        assert!(app.session_files.is_empty());
        assert!(app.session_selected_file_idx.is_empty());
    }

    #[test]
    fn new_viewer_edit_defaults() {
        let app = App::new();
        assert!(!app.viewer_edit_mode);
        assert!(app.viewer_edit_content.is_empty());
        assert_eq!(app.viewer_edit_cursor, (0, 0));
        assert!(app.viewer_edit_undo.is_empty());
        assert!(app.viewer_edit_redo.is_empty());
        assert!(!app.viewer_edit_dirty);
    }

    #[test]
    fn new_clipboard_empty() {
        let app = App::new();
        assert!(app.clipboard.is_empty());
    }

    #[test]
    fn new_viewer_tabs_empty() {
        let app = App::new();
        assert!(app.viewer_tabs.is_empty());
        assert_eq!(app.viewer_active_tab, 0);
        assert!(!app.viewer_tab_dialog);
    }

    #[test]
    fn new_run_commands_empty() {
        let app = App::new();
        assert!(app.run_commands.is_empty());
        assert!(app.run_command_dialog.is_none());
        assert!(app.run_command_picker.is_none());
    }

    #[test]
    fn new_preset_prompts_empty() {
        let app = App::new();
        assert!(app.preset_prompts.is_empty());
        assert!(app.preset_prompt_picker.is_none());
        assert!(app.preset_prompt_dialog.is_none());
    }

    #[test]
    fn new_token_state() {
        let app = App::new();
        assert!(app.session_tokens.is_none());
        assert!(app.model_context_window.is_none());
        assert!(app.token_badge_cache.is_none());
        assert!(!app.context_pct_high);
    }

    #[test]
    fn new_todo_state() {
        let app = App::new();
        assert!(app.current_todos.is_empty());
        assert!(app.subagent_todos.is_empty());
        assert!(app.active_task_tool_ids.is_empty());
        assert!(app.subagent_parent_idx.is_none());
    }

    #[test]
    fn new_stt_state() {
        let app = App::new();
        assert!(app.stt_handle.is_none());
        assert!(!app.stt_recording);
        assert!(!app.stt_transcribing);
    }

    #[test]
    fn new_nerd_fonts_true() {
        let app = App::new();
        assert!(app.nerd_fonts);
    }

    #[test]
    fn new_file_tree_options() {
        let app = App::new();
        assert!(!app.file_tree_options_mode);
        assert_eq!(app.file_tree_options_selected, 0);
    }

    #[test]
    fn new_health_panel_none() {
        let app = App::new();
        assert!(app.health_panel.is_none());
        assert_eq!(app.last_health_tab, HealthTab::GodFiles);
    }

    #[test]
    fn new_git_actions_panel_none() {
        let app = App::new();
        assert!(app.git_actions_panel.is_none());
    }

    #[test]
    fn new_browsing_main_false() {
        let app = App::new();
        assert!(!app.browsing_main);
        assert!(app.pre_main_browse_selection.is_none());
        assert!(app.main_worktree.is_none());
    }

    #[test]
    fn new_session_list_defaults() {
        let app = App::new();
        assert!(!app.show_session_list);
        assert!(!app.session_list_loading);
        assert!(app.loading_indicator.is_none());
        assert!(app.deferred_action.is_none());
        assert_eq!(app.session_list_selected, 0);
        assert_eq!(app.session_list_scroll, 0);
    }

    #[test]
    fn new_session_find_defaults() {
        let app = App::new();
        assert!(!app.session_find_active);
        assert!(app.session_find.is_empty());
        assert!(app.session_find_matches.is_empty());
        assert_eq!(app.session_find_current, 0);
    }

    #[test]
    fn new_session_filter_defaults() {
        let app = App::new();
        assert!(!app.session_filter_active);
        assert!(app.session_filter.is_empty());
        assert!(!app.session_content_search);
        assert!(app.session_search_results.is_empty());
    }

    #[test]
    fn new_selected_model_opus() {
        let app = App::new();
        assert_eq!(app.selected_model, Some("opus".to_string()));
        assert!(app.detected_model.is_none());
    }

    // ── invalidate_render_cache ──

    #[test]
    fn invalidate_render_cache_sets_dirty() {
        let mut app = App::new();
        app.rendered_lines_dirty = false;
        app.invalidate_render_cache();
        assert!(app.rendered_lines_dirty);
    }

    #[test]
    fn invalidate_render_cache_idempotent() {
        let mut app = App::new();
        app.invalidate_render_cache();
        app.invalidate_render_cache();
        assert!(app.rendered_lines_dirty);
    }

    // ── invalidate_file_tree ──

    #[test]
    fn invalidate_file_tree_sets_dirty() {
        let mut app = App::new();
        app.file_tree_dirty = false;
        app.invalidate_file_tree();
        assert!(app.file_tree_dirty);
    }

    // ── set_status / clear_status ──

    #[test]
    fn set_status_stores_message() {
        let mut app = App::new();
        app.set_status("hello world");
        assert_eq!(app.status_message.as_deref(), Some("hello world"));
    }

    #[test]
    fn set_status_overwrites_previous() {
        let mut app = App::new();
        app.set_status("first");
        app.set_status("second");
        assert_eq!(app.status_message.as_deref(), Some("second"));
    }

    #[test]
    fn set_status_accepts_string() {
        let mut app = App::new();
        app.set_status(String::from("owned string"));
        assert_eq!(app.status_message.as_deref(), Some("owned string"));
    }

    #[test]
    fn set_status_accepts_format() {
        let mut app = App::new();
        app.set_status(format!("count: {}", 42));
        assert_eq!(app.status_message.as_deref(), Some("count: 42"));
    }

    #[test]
    fn clear_status_removes_message() {
        let mut app = App::new();
        app.set_status("something");
        app.clear_status();
        assert!(app.status_message.is_none());
    }

    #[test]
    fn clear_status_noop_when_none() {
        let mut app = App::new();
        app.clear_status();
        assert!(app.status_message.is_none());
    }

    // ── current_project / current_worktree ──

    #[test]
    fn current_project_none_by_default() {
        let app = App::new();
        assert!(app.current_project().is_none());
    }

    #[test]
    fn current_worktree_none_by_default() {
        let app = App::new();
        assert!(app.current_worktree().is_none());
    }

    #[test]
    fn current_worktree_returns_selected() {
        let mut app = App::new();
        app.worktrees.push(Worktree {
            branch_name: "azureal/feat-a".to_string(),
            worktree_path: Some(PathBuf::from("/tmp/a")),
            claude_session_id: None,
            archived: false,
        });
        app.worktrees.push(Worktree {
            branch_name: "azureal/feat-b".to_string(),
            worktree_path: Some(PathBuf::from("/tmp/b")),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(1);
        let wt = app.current_worktree().unwrap();
        assert_eq!(wt.branch_name, "azureal/feat-b");
    }

    #[test]
    fn current_worktree_browsing_main_returns_main() {
        let mut app = App::new();
        app.worktrees.push(Worktree {
            branch_name: "azureal/feat".to_string(),
            worktree_path: None,
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.main_worktree = Some(Worktree {
            branch_name: "main".to_string(),
            worktree_path: Some(PathBuf::from("/repo")),
            claude_session_id: None,
            archived: false,
        });
        app.browsing_main = true;
        let wt = app.current_worktree().unwrap();
        assert_eq!(wt.branch_name, "main");
    }

    #[test]
    fn current_worktree_out_of_bounds() {
        let mut app = App::new();
        app.selected_worktree = Some(5);
        assert!(app.current_worktree().is_none());
    }

    // ── is_session_running ──

    #[test]
    fn is_session_running_no_slots() {
        let app = App::new();
        assert!(!app.is_session_running("any-branch"));
    }

    #[test]
    fn is_session_running_with_running_slot() {
        let mut app = App::new();
        app.branch_slots.insert("branch-a".to_string(), vec!["pid-123".to_string()]);
        app.running_sessions.insert("pid-123".to_string());
        assert!(app.is_session_running("branch-a"));
    }

    #[test]
    fn is_session_running_slot_not_running() {
        let mut app = App::new();
        app.branch_slots.insert("branch-a".to_string(), vec!["pid-123".to_string()]);
        // pid-123 not in running_sessions
        assert!(!app.is_session_running("branch-a"));
    }

    // ── is_active_slot_running ──

    #[test]
    fn is_active_slot_running_no_worktree() {
        let app = App::new();
        assert!(!app.is_active_slot_running());
    }

    #[test]
    fn is_active_slot_running_slot_running() {
        let mut app = App::new();
        app.worktrees.push(Worktree {
            branch_name: "azureal/feat".to_string(),
            worktree_path: None,
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.active_slot.insert("azureal/feat".to_string(), "pid-5".to_string());
        app.running_sessions.insert("pid-5".to_string());
        assert!(app.is_active_slot_running());
    }

    #[test]
    fn is_active_slot_running_slot_stopped() {
        let mut app = App::new();
        app.worktrees.push(Worktree {
            branch_name: "azureal/feat".to_string(),
            worktree_path: None,
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.active_slot.insert("azureal/feat".to_string(), "pid-5".to_string());
        // pid-5 not in running_sessions
        assert!(!app.is_active_slot_running());
    }

    // ── branch_for_slot ──

    #[test]
    fn branch_for_slot_found() {
        let mut app = App::new();
        app.branch_slots.insert("branch-x".to_string(), vec!["pid-10".to_string(), "pid-11".to_string()]);
        assert_eq!(app.branch_for_slot("pid-11"), Some("branch-x".to_string()));
    }

    #[test]
    fn branch_for_slot_not_found() {
        let mut app = App::new();
        app.branch_slots.insert("branch-x".to_string(), vec!["pid-10".to_string()]);
        assert!(app.branch_for_slot("pid-999").is_none());
    }

    #[test]
    fn branch_for_slot_empty_slots() {
        let app = App::new();
        assert!(app.branch_for_slot("anything").is_none());
    }

    // ── is_claude_session_running ──

    #[test]
    fn is_claude_session_running_true() {
        let mut app = App::new();
        app.claude_session_ids.insert("pid-1".to_string(), "uuid-abc".to_string());
        app.running_sessions.insert("pid-1".to_string());
        assert!(app.is_claude_session_running("uuid-abc"));
    }

    #[test]
    fn is_claude_session_running_false_not_running() {
        let mut app = App::new();
        app.claude_session_ids.insert("pid-1".to_string(), "uuid-abc".to_string());
        // pid-1 not in running_sessions
        assert!(!app.is_claude_session_running("uuid-abc"));
    }

    #[test]
    fn is_claude_session_running_false_no_match() {
        let mut app = App::new();
        app.claude_session_ids.insert("pid-1".to_string(), "uuid-abc".to_string());
        app.running_sessions.insert("pid-1".to_string());
        assert!(!app.is_claude_session_running("uuid-xyz"));
    }

    // ── display_model_name ──

    #[test]
    fn display_model_name_default() {
        let app = App::new();
        assert_eq!(app.display_model_name(), "opus");
    }

    #[test]
    fn display_model_name_custom() {
        let mut app = App::new();
        app.selected_model = Some("sonnet".to_string());
        assert_eq!(app.display_model_name(), "sonnet");
    }

    #[test]
    fn display_model_name_none_fallback() {
        let mut app = App::new();
        app.selected_model = None;
        assert_eq!(app.display_model_name(), "opus");
    }

    // ── cycle_model ──

    #[test]
    fn cycle_model_opus_to_sonnet() {
        let mut app = App::new();
        app.selected_model = Some("opus".to_string());
        app.cycle_model();
        assert_eq!(app.selected_model.as_deref(), Some("sonnet"));
    }

    #[test]
    fn cycle_model_sonnet_to_haiku() {
        let mut app = App::new();
        app.selected_model = Some("sonnet".to_string());
        app.cycle_model();
        assert_eq!(app.selected_model.as_deref(), Some("haiku"));
    }

    #[test]
    fn cycle_model_haiku_to_opus() {
        let mut app = App::new();
        app.selected_model = Some("haiku".to_string());
        app.cycle_model();
        assert_eq!(app.selected_model.as_deref(), Some("opus"));
    }

    #[test]
    fn cycle_model_unknown_to_opus() {
        let mut app = App::new();
        app.selected_model = Some("unknown-model".to_string());
        app.cycle_model();
        assert_eq!(app.selected_model.as_deref(), Some("opus"));
    }

    #[test]
    fn cycle_model_none_to_opus() {
        let mut app = App::new();
        app.selected_model = None;
        app.cycle_model();
        assert_eq!(app.selected_model.as_deref(), Some("opus"));
    }

    #[test]
    fn cycle_model_full_cycle() {
        let mut app = App::new();
        assert_eq!(app.selected_model.as_deref(), Some("opus"));
        app.cycle_model();
        assert_eq!(app.selected_model.as_deref(), Some("sonnet"));
        app.cycle_model();
        assert_eq!(app.selected_model.as_deref(), Some("haiku"));
        app.cycle_model();
        assert_eq!(app.selected_model.as_deref(), Some("opus"));
    }

    // ── update_token_badge ──

    #[test]
    fn update_token_badge_no_tokens() {
        let mut app = App::new();
        app.update_token_badge();
        assert!(app.token_badge_cache.is_none());
        assert!(!app.context_pct_high);
    }

    #[test]
    fn update_token_badge_low_usage() {
        let mut app = App::new();
        app.session_tokens = Some((50_000, 1000));
        app.model_context_window = Some(200_000);
        app.update_token_badge();
        let (text, color) = app.token_badge_cache.unwrap();
        assert!(text.contains('%'));
        assert_eq!(color, ratatui::style::Color::Green);
        assert!(!app.context_pct_high);
    }

    #[test]
    fn update_token_badge_medium_usage() {
        let mut app = App::new();
        // 120k out of (200k - 33k = 167k usable) ≈ 71.8%
        app.session_tokens = Some((120_000, 1000));
        app.model_context_window = Some(200_000);
        app.update_token_badge();
        let (_, color) = app.token_badge_cache.unwrap();
        assert_eq!(color, ratatui::style::Color::Yellow);
    }

    #[test]
    fn update_token_badge_high_usage() {
        let mut app = App::new();
        // 160k out of (200k - 33k = 167k usable) ≈ 95.8%
        app.session_tokens = Some((160_000, 1000));
        app.model_context_window = Some(200_000);
        app.update_token_badge();
        let (_, color) = app.token_badge_cache.unwrap();
        assert_eq!(color, ratatui::style::Color::Red);
        assert!(app.context_pct_high);
    }

    #[test]
    fn update_token_badge_defaults_to_200k_window() {
        let mut app = App::new();
        app.session_tokens = Some((50_000, 500));
        app.model_context_window = None;
        app.update_token_badge();
        assert!(app.token_badge_cache.is_some());
    }

    #[test]
    fn update_token_badge_context_drops_below_threshold() {
        let mut app = App::new();
        // First set high
        app.session_tokens = Some((160_000, 1000));
        app.model_context_window = Some(200_000);
        app.update_token_badge();
        assert!(app.context_pct_high);
        app.compaction_banner_injected = true;
        // Now drop below
        app.session_tokens = Some((50_000, 1000));
        app.update_token_badge();
        assert!(!app.context_pct_high);
        assert!(!app.compaction_banner_injected);
    }

    // ── TodoItem / TodoStatus ──

    #[test]
    fn todo_status_pending() {
        assert_eq!(TodoStatus::Pending, TodoStatus::Pending);
        assert_ne!(TodoStatus::Pending, TodoStatus::InProgress);
        assert_ne!(TodoStatus::Pending, TodoStatus::Completed);
    }

    #[test]
    fn todo_status_in_progress() {
        assert_eq!(TodoStatus::InProgress, TodoStatus::InProgress);
    }

    #[test]
    fn todo_status_completed() {
        assert_eq!(TodoStatus::Completed, TodoStatus::Completed);
    }

    #[test]
    fn todo_item_construction() {
        let item = TodoItem {
            content: "Implement feature".to_string(),
            status: TodoStatus::InProgress,
            active_form: "Implementing feature".to_string(),
        };
        assert_eq!(item.content, "Implement feature");
        assert_eq!(item.status, TodoStatus::InProgress);
        assert_eq!(item.active_form, "Implementing feature");
    }

    #[test]
    fn todo_item_clone() {
        let item = TodoItem {
            content: "test".to_string(),
            status: TodoStatus::Pending,
            active_form: "testing".to_string(),
        };
        let cloned = item.clone();
        assert_eq!(cloned.content, "test");
        assert_eq!(cloned.status, TodoStatus::Pending);
    }

    #[test]
    fn todo_status_debug() {
        assert_eq!(format!("{:?}", TodoStatus::Pending), "Pending");
        assert_eq!(format!("{:?}", TodoStatus::InProgress), "InProgress");
        assert_eq!(format!("{:?}", TodoStatus::Completed), "Completed");
    }

    // ── DeferredAction ──

    #[test]
    fn deferred_action_load_session() {
        let action = DeferredAction::LoadSession { branch: "main".to_string(), idx: 0 };
        assert!(matches!(action, DeferredAction::LoadSession { branch, idx } if branch == "main" && idx == 0));
    }

    #[test]
    fn deferred_action_load_file() {
        let action = DeferredAction::LoadFile { path: PathBuf::from("/tmp/file.rs") };
        assert!(matches!(action, DeferredAction::LoadFile { .. }));
    }

    #[test]
    fn deferred_action_switch_project() {
        let action = DeferredAction::SwitchProject { path: PathBuf::from("/new/project") };
        assert!(matches!(action, DeferredAction::SwitchProject { .. }));
    }

    #[test]
    fn deferred_action_git_commit() {
        let action = DeferredAction::GitCommit {
            worktree: PathBuf::from("/wt"),
            message: "fix bug".to_string(),
        };
        assert!(matches!(action, DeferredAction::GitCommit { .. }));
    }

    #[test]
    fn deferred_action_git_commit_and_push() {
        let action = DeferredAction::GitCommitAndPush {
            worktree: PathBuf::from("/wt"),
            message: "feat: new".to_string(),
        };
        assert!(matches!(action, DeferredAction::GitCommitAndPush { .. }));
    }

    #[test]
    fn deferred_action_open_health_panel() {
        let action = DeferredAction::OpenHealthPanel;
        assert!(matches!(action, DeferredAction::OpenHealthPanel));
    }

    #[test]
    fn deferred_action_rescan_health_scope() {
        let action = DeferredAction::RescanHealthScope { dirs: vec!["src".to_string()] };
        assert!(matches!(action, DeferredAction::RescanHealthScope { .. }));
    }

    // ── cancel_all_claude ──

    #[test]
    fn cancel_all_claude_clears_state() {
        let mut app = App::new();
        app.running_sessions.insert("pid-1".to_string());
        app.running_sessions.insert("pid-2".to_string());
        app.branch_slots.insert("b".to_string(), vec!["pid-1".to_string()]);
        app.active_slot.insert("b".to_string(), "pid-1".to_string());
        app.cancel_all_claude();
        assert!(app.running_sessions.is_empty());
        assert!(app.branch_slots.is_empty());
        assert!(app.active_slot.is_empty());
    }

    // ── git_action_in_progress ──

    #[test]
    fn git_action_in_progress_default_false() {
        let app = App::new();
        assert!(!app.git_action_in_progress());
    }

    #[test]
    fn git_action_in_progress_deferred_commit() {
        let mut app = App::new();
        app.deferred_action = Some(DeferredAction::GitCommit {
            worktree: PathBuf::from("/wt"),
            message: "msg".to_string(),
        });
        assert!(app.git_action_in_progress());
    }

    #[test]
    fn git_action_in_progress_deferred_commit_push() {
        let mut app = App::new();
        app.deferred_action = Some(DeferredAction::GitCommitAndPush {
            worktree: PathBuf::from("/wt"),
            message: "msg".to_string(),
        });
        assert!(app.git_action_in_progress());
    }

    #[test]
    fn git_action_not_in_progress_load_session() {
        let mut app = App::new();
        app.deferred_action = Some(DeferredAction::LoadSession {
            branch: "b".to_string(),
            idx: 0,
        });
        assert!(!app.git_action_in_progress());
    }

    // ── get_cpu_time_micros ──

    #[test]
    fn get_cpu_time_micros_returns_nonzero() {
        let cpu = get_cpu_time_micros();
        assert!(cpu > 0, "CPU time should be non-zero for a running process");
    }
}
