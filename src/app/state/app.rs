//! App struct definition and initialization
//!
//! Submodules handle specific concerns:
//! - `cpu`: CPU usage monitoring for the status bar
//! - `deferred`: Deferred actions for two-phase draw pattern
//! - `model`: Model selection and token usage badge
//! - `queries`: Session status queries and project/worktree accessors
//! - `stt`: Speech-to-text integration
//! - `todo`: Todo item types from Claude's TodoWrite tool call

mod cpu;
mod deferred;
mod model;
mod queries;
mod stt;
mod todo;

pub(crate) use cpu::get_cpu_time_micros;
pub use deferred::DeferredAction;
pub use todo::{TodoItem, TodoStatus};

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use portable_pty::{Child as PtyChild, MasterPty};

use crate::app::terminal::SessionTerminal;
use crate::app::types::{BranchDialog, FileTreeAction, FileTreeEntry, Focus, GitActionsPanel, HealthPanel, HealthTab, RcrSession, PostMergeDialog, PresetPrompt, PresetPromptDialog, PresetPromptPicker, ProjectsPanel, RunCommand, RunCommandDialog, RunCommandPicker, ViewMode, ViewerMode};
use crate::backend::Backend;
use crate::events::EventParser;
use crate::models::{Project, Worktree};
use crate::syntax::SyntaxHighlighter;
use crate::tui::render_thread::RenderThread;
use super::AgentEvent;
use super::DisplayEvent;

/// Application state
pub struct App {
    /// Which agent backend is active (Claude or Codex)
    pub backend: Backend,
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
    pub agent_receivers: HashMap<String, Receiver<AgentEvent>>,
    /// Set of currently running slot_ids (PID strings)
    pub running_sessions: HashSet<String>,
    /// Branches with at least one unread finished session (for tab rendering)
    pub unread_sessions: HashSet<String>,
    /// Individual session UUIDs that finished while user wasn't viewing them
    pub unread_session_ids: HashSet<String>,
    /// Last exit code per slot_id (shown in session pane title after Claude exits)
    pub agent_exit_codes: HashMap<String, i32>,
    /// Claude API session UUIDs per slot_id (for --resume)
    pub agent_session_ids: HashMap<String, String>,
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
    /// When set, the raw session file is gone but we loaded from cache.
    /// Holds the missing file's path. Input is blocked (session is read-only).
    pub source_file_missing: Option<std::path::PathBuf>,
    /// Signals the event loop to reset the background ClaudeProcessor's parser
    /// state (e.g., on session switch). The event loop checks and clears this.
    pub agent_processor_needs_reset: bool,
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
    /// Cached rect for the status bar (mouse click → copy status message)
    pub pane_status: ratatui::layout::Rect,
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
    /// One-shot flag: correct viewer_scroll to match the actual visual line on next cache rebuild
    pub viewer_scroll_to_diff: bool,
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

    // ── Session rename (inline rename in session list overlay) ──
    /// Whether the rename input is active in session list overlay
    pub session_rename_active: bool,
    /// Text buffer for the rename input
    pub session_rename_input: String,
    /// Cursor position within the rename input
    pub session_rename_cursor: usize,
    /// Session ID being renamed (resolved when 'r' is pressed)
    pub session_rename_id: Option<String>,

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

impl App {
    pub fn new() -> Self {
        Self {
            backend: Backend::Claude,
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
            agent_receivers: HashMap::new(),
            running_sessions: HashSet::new(),
            unread_sessions: HashSet::new(),
            unread_session_ids: HashSet::new(),
            agent_exit_codes: HashMap::new(),
            agent_session_ids: HashMap::new(),
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
            source_file_missing: None,
            agent_processor_needs_reset: false,
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
            pane_status: ratatui::layout::Rect::default(),
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
            viewer_scroll_to_diff: false,
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
            session_rename_active: false,
            session_rename_input: String::new(),
            session_rename_cursor: 0,
            session_rename_id: None,
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
}
