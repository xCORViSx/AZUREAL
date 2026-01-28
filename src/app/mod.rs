//! Application state module
//!
//! Split into focused submodules:
//! - `types`: Enums and dialog types (BranchDialog, ContextMenu, SessionAction, etc.)
//! - `input`: Input handling methods
//! - `terminal`: PTY terminal management
//! - `util`: Utility functions (ANSI stripping, JSON parsing)

mod input;
mod terminal;
mod types;
mod util;

pub use types::{BranchDialog, ContextMenu, Focus, SessionAction, ViewMode};

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use portable_pty::MasterPty;

use crate::claude::{ClaudeEvent, InteractiveSession};
use crate::db::Database;
use crate::events::{DisplayEvent, EventParser};
use crate::git::Git;
use crate::models::{OutputType, Project, RebaseStatus, Session, SessionStatus};
use crate::session::SessionManager;
use crate::syntax::DiffHighlighter;
use crate::wizard::SessionCreationWizard;

use util::{parse_stream_json_for_display, strip_ansi_escapes};

/// Application state
pub struct App {
    pub db: Database,
    pub projects: Vec<Project>,
    pub selected_project: usize,
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
    pub output_scroll: usize,
    pub diff_scroll: usize,
    pub diff_highlighter: DiffHighlighter,
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
    /// Tracks position in hooks.jsonl for incremental reading
    pub hooks_file_pos: u64,
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
}

impl App {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            projects: Vec::new(),
            selected_project: 0,
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
            focus: Focus::Sessions,
            insert_mode: false,
            should_quit: false,
            status_message: None,
            claude_receivers: HashMap::new(),
            running_sessions: HashSet::new(),
            claude_session_ids: HashMap::new(),
            interactive_sessions: HashMap::new(),
            diff_text: None,
            output_scroll: usize::MAX, // Start at bottom (most recent messages)
            diff_scroll: 0,
            diff_highlighter: DiffHighlighter::new(),
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
            hooks_file_pos: 0,
            pending_tool_calls: HashSet::new(),
            failed_tool_calls: HashSet::new(),
            animation_tick: 0,
            session_file_path: None,
            session_file_modified: None,
        }
    }

    pub fn is_session_running(&self, session_id: &str) -> bool {
        self.running_sessions.contains(session_id)
    }

    pub fn is_current_session_running(&self) -> bool {
        self.current_session().map(|s| self.running_sessions.contains(&s.id)).unwrap_or(false)
    }

    pub fn load(&mut self) -> anyhow::Result<()> {
        self.projects = self.db.list_projects()?;
        if !self.projects.is_empty() { self.load_sessions_for_project()?; }
        Ok(())
    }

    pub fn load_sessions_for_project(&mut self) -> anyhow::Result<()> {
        if let Some(project) = self.projects.get(self.selected_project) {
            let worktrees = Git::list_worktrees_detailed(&project.path)?;
            let (main_wts, feature_wts): (Vec<_>, Vec<_>) = worktrees.into_iter().partition(|wt| wt.is_main);
            let mut sessions = Vec::new();

            // Add main worktree first
            if let Some(main_wt) = main_wts.into_iter().next() {
                let branch_name = main_wt.branch.unwrap_or_else(|| "main".to_string());
                let main_claude_id = self.db.get_session("__main__").ok().flatten().and_then(|s| s.claude_session_id);
                if let Some(ref id) = main_claude_id {
                    self.claude_session_ids.insert("__main__".to_string(), id.clone());
                }
                let main_session = Session {
                    id: "__main__".to_string(),
                    name: format!("[{}]", branch_name),
                    initial_prompt: String::new(),
                    worktree_name: "main".to_string(),
                    worktree_path: main_wt.path,
                    branch_name,
                    status: SessionStatus::Pending,
                    project_id: project.id,
                    pid: None,
                    exit_code: None,
                    archived: false,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    claude_session_id: main_claude_id,
                };
                let _ = self.db.ensure_session(&main_session);
                sessions.push(main_session);
            }

            // Add feature worktrees
            for wt in feature_wts {
                let name = wt.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "unknown".to_string());
                let claude_id = self.db.get_session(&name).ok().flatten().and_then(|s| s.claude_session_id);
                if let Some(ref id) = claude_id {
                    self.claude_session_ids.insert(name.clone(), id.clone());
                }
                let feature_session = Session {
                    id: name.clone(),
                    name: name.clone(),
                    initial_prompt: String::new(),
                    worktree_name: name,
                    worktree_path: wt.path,
                    branch_name: wt.branch.unwrap_or_default(),
                    status: SessionStatus::Pending,
                    project_id: project.id,
                    pid: None,
                    exit_code: None,
                    archived: false,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    claude_session_id: claude_id,
                };
                let _ = self.db.ensure_session(&feature_session);
                sessions.push(feature_session);
            }

            self.sessions = sessions;
            self.selected_session = if self.sessions.is_empty() { None } else { Some(0) };
        }
        Ok(())
    }

    pub fn current_project(&self) -> Option<&Project> { self.projects.get(self.selected_project) }
    pub fn current_session(&self) -> Option<&Session> { self.selected_session.and_then(|idx| self.sessions.get(idx)) }

    pub fn select_next_session(&mut self) {
        if let Some(idx) = self.selected_session {
            if idx + 1 < self.sessions.len() {
                self.selected_session = Some(idx + 1);
                self.load_session_output();
            }
        } else if !self.sessions.is_empty() {
            self.selected_session = Some(0);
            self.load_session_output();
        }
    }

    pub fn select_prev_session(&mut self) {
        if let Some(idx) = self.selected_session {
            if idx > 0 {
                self.selected_session = Some(idx - 1);
                self.load_session_output();
            }
        }
    }

    pub fn select_next_project(&mut self) {
        if self.selected_project + 1 < self.projects.len() {
            self.selected_project += 1;
            let _ = self.load_sessions_for_project();
            self.load_session_output();
        }
    }

    pub fn select_prev_project(&mut self) {
        if self.selected_project > 0 {
            self.selected_project -= 1;
            let _ = self.load_sessions_for_project();
            self.load_session_output();
        }
    }

    pub fn load_session_output(&mut self) {
        self.output_lines.clear();
        self.output_buffer.clear();
        self.output_scroll = usize::MAX; // Start at bottom (most recent messages)
        self.display_events.clear();
        self.event_parser = EventParser::new();
        self.selected_event = None;
        self.pending_tool_calls.clear();
        self.failed_tool_calls.clear();

        if let Some(session) = self.current_session() {
            let session_id = session.id.clone();
            let worktree_path = session.worktree_path.clone();
            let session_created = session.created_at;
            let session_updated = session.updated_at;

            // Try to get Claude session ID, or auto-discover from Claude's files
            let mut claude_session_id = session.claude_session_id.clone()
                .or_else(|| self.claude_session_ids.get(&session_id).cloned());

            // Auto-discover Claude session ID if not set
            if claude_session_id.is_none() {
                if let Some(discovered_id) = crate::config::find_latest_claude_session(&worktree_path) {
                    // Save discovered ID for future use
                    self.claude_session_ids.insert(session_id.clone(), discovered_id.clone());
                    let _ = self.db.update_session_claude_id(&session_id, Some(&discovered_id));
                    claude_session_id = Some(discovered_id);
                }
            }

            let mut loaded_from_claude = false;

            // Try loading from Claude's session files
            if let Some(claude_id) = claude_session_id {
                if let Some(session_file) = crate::config::claude_session_file(&worktree_path, &claude_id) {
                    // Track file for live polling
                    self.session_file_path = Some(session_file.clone());
                    self.session_file_modified = std::fs::metadata(&session_file)
                        .and_then(|m| m.modified())
                        .ok();

                    let mut timed_events = self.load_claude_session_events(&session_file);

                    // Load and merge hooks - include all hooks from session start to now
                    let hooks = self.load_hooks_with_timestamps();
                    let session_start = timed_events.first().map(|(ts, _)| *ts);

                    if let Some(start) = session_start {
                        let buffer = chrono::Duration::seconds(5);
                        let now = chrono::Utc::now();
                        for (ts, event) in hooks {
                            if ts >= start - buffer && ts <= now {
                                timed_events.push((ts, event));
                            }
                        }
                    }

                    timed_events.sort_by_key(|(ts, _)| *ts);

                    // UPS hooks already injected inline during parsing via prescan
                    for (_, event) in timed_events {
                        self.display_events.push(event);
                    }

                    loaded_from_claude = !self.display_events.is_empty();
                }
            }

            // Fallback: load from database if Claude files unavailable
            if !loaded_from_claude {
                if let Ok(outputs) = self.db.get_session_outputs(&session_id) {
                    for output in outputs {
                        let events = self.event_parser.parse(&output.data);
                        self.display_events.extend(events);

                        // Also add to output_lines for legacy display
                        if output.output_type == OutputType::Stdout || output.output_type == OutputType::Json {
                            if let Some(display_text) = parse_stream_json_for_display(&output.data) {
                                self.process_output_chunk(&display_text);
                            }
                        }
                    }
                }

                // Load hooks from session start to now
                let hooks = self.load_hooks_with_timestamps();
                let buffer = chrono::Duration::seconds(5);
                let now = chrono::Utc::now();
                for (ts, event) in hooks {
                    if ts >= session_created - buffer && ts <= now {
                        self.display_events.push(event);
                    }
                }
            }
        }

        // Set hooks file position to end so we only capture new hooks going forward
        if let Ok(metadata) = std::fs::metadata(crate::config::config_dir().join("hooks.jsonl")) {
            self.hooks_file_pos = metadata.len();
        }

        // Auto-dump debug output on debug builds
        #[cfg(debug_assertions)]
        let _ = self.dump_debug_output();
    }

    /// Poll session file for changes and reload if modified
    /// Returns true if the session was reloaded
    pub fn poll_session_file(&mut self) -> bool {
        let Some(path) = &self.session_file_path else { return false };
        let Ok(metadata) = std::fs::metadata(path) else { return false };
        let Ok(modified) = metadata.modified() else { return false };

        // Check if file was modified since last check
        if self.session_file_modified.map(|t| modified > t).unwrap_or(true) {
            self.load_session_output();
            true
        } else {
            false
        }
    }

    /// Extract hook events from system-reminder tags in content
    /// Parses patterns like "<system-reminder>HookName hook success: output</system-reminder>"
    fn extract_hooks_from_content(content: &str, timestamp: chrono::DateTime<chrono::Utc>) -> Vec<(chrono::DateTime<chrono::Utc>, DisplayEvent)> {
        let mut hooks = Vec::new();
        let mut search_start = 0;
        while let Some(start) = content[search_start..].find("<system-reminder>") {
            let abs_start = search_start + start + 17; // skip the opening tag
            if let Some(end) = content[abs_start..].find("</system-reminder>") {
                let reminder_content = &content[abs_start..abs_start + end];
                // Parse "HookName hook success: output" or "HookName hook failed: output"
                if let Some(hook_pos) = reminder_content.find(" hook success:") {
                    // Clean name: trim whitespace AND literal \n (some JSON has double-escaped newlines)
                    let name = reminder_content[..hook_pos]
                        .trim()
                        .trim_start_matches("\\n")
                        .trim_end_matches("\\n")
                        .to_string();
                    let mut output = reminder_content[hook_pos + 14..]
                        .trim()
                        .trim_start_matches("\\n")
                        .trim_end_matches("\\n")
                        .to_string();

                    // Include hooks even if output is just "..." (Claude Code truncates some hooks)
                    if !output.is_empty() && output != "..." && !name.is_empty() {
                        hooks.push((timestamp, DisplayEvent::Hook { name, output }));
                    } else if output == "..." && !name.is_empty() {
                        // Still show hooks with truncated output, use hook name as context
                        hooks.push((timestamp, DisplayEvent::Hook { name: name.clone(), output: format!("[{}]", name) }));
                    }
                } else if let Some(hook_pos) = reminder_content.find(" hook failed:") {
                    let name = reminder_content[..hook_pos]
                        .trim()
                        .trim_start_matches("\\n")
                        .trim_end_matches("\\n")
                        .to_string();
                    let output = reminder_content[hook_pos + 13..]
                        .trim()
                        .trim_start_matches("\\n")
                        .trim_end_matches("\\n")
                        .to_string();
                    if !name.is_empty() {
                        hooks.push((timestamp, DisplayEvent::Hook { name, output: format!("FAILED: {}", output) }));
                    }
                }
                search_start = abs_start + end + 18; // skip past </system-reminder>
            } else {
                break;
            }
        }
        hooks
    }

    /// Load events from Claude's session file with timestamps
    fn load_claude_session_events(&mut self, session_file: &std::path::Path) -> Vec<(chrono::DateTime<chrono::Utc>, DisplayEvent)> {
        let file = match File::open(session_file) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let reader = BufReader::new(file);
        let mut events = Vec::new();
        // Track user messages by parentUuid to deduplicate rewound messages (keep most recent)
        let mut user_msg_by_parent: HashMap<String, (usize, chrono::DateTime<chrono::Utc>)> = HashMap::new();
        // Track tool calls by tool_use_id so we can match results to their calls
        let mut tool_calls: HashMap<String, (String, Option<String>)> = HashMap::new(); // id -> (name, file_path)
        // Track tool calls that haven't received results yet (for in-progress indicator)
        let mut pending_tools: HashSet<String> = HashSet::new();
        // Track most recent user message (index AND timestamp) for UPS hook placement
        let mut last_user_msg: Option<(usize, chrono::DateTime<chrono::Utc>)> = None;
        // Collect UPS hooks with user message timestamp (for correct sorting after merge)
        let mut ups_hooks: Vec<(usize, chrono::DateTime<chrono::Utc>, DisplayEvent)> = Vec::new();

        for line in reader.lines().map_while(Result::ok) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                // Parse timestamp
                let timestamp = json.get("timestamp")
                    .and_then(|t| t.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(chrono::Utc::now);

                let event_type = json.get("type").and_then(|t| t.as_str()).unwrap_or("");

                match event_type {
                    "user" => {
                        let message = json.get("message");
                        let content_val = message.and_then(|m| m.get("content"));
                        let is_meta = json.get("isMeta").and_then(|m| m.as_bool()).unwrap_or(false);

                        // Get string content for early checks
                        let content_str = if let Some(s) = content_val.and_then(|c| c.as_str()) {
                            Some(s.to_string())
                        } else if let Some(arr) = content_val.and_then(|c| c.as_array()) {
                            Some(arr.iter()
                                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                                .collect::<Vec<_>>()
                                .join("\n"))
                        } else {
                            None
                        };

                        // Check for compaction summary FIRST (before extracting hooks)
                        // Summary contains quoted <system-reminder> tags that shouldn't be treated as real hooks
                        let is_compaction_summary = content_str.as_ref()
                            .map(|c| c.starts_with("This session is being continued from a previous conversation"))
                            .unwrap_or(false);

                        if is_compaction_summary {
                            events.push((timestamp, DisplayEvent::Compacting));
                            continue;
                        }

                        // Extract hooks from user messages - push directly (they're part of this turn)
                        if let Some(ref content) = content_str {
                            for hook in Self::extract_hooks_from_content(content, timestamp) {
                                events.push(hook);
                            }
                        }

                        // Skip meta messages for display
                        if is_meta {
                            continue;
                        }

                        // Handle string content (user prompts)
                        if let Some(content) = content_val.and_then(|c| c.as_str()) {
                            // Skip local-command-caveat messages (internal Claude instruction)
                            if content.contains("<local-command-caveat>") {
                                continue;
                            }

                            // Handle local-command-stdout (output of local commands)
                            if content.contains("<local-command-stdout>") {
                                // Check for "Compacted" indicator - show CONVERSATION COMPACTED banner
                                if content.contains("Compacted") {
                                    events.push((timestamp, DisplayEvent::Compacted));
                                }
                                // Skip the raw stdout content either way
                                continue;
                            }

                            // Check for slash commands: <command-name>/xxx</command-name>
                            // Only match when tag is at START of message (not embedded in user text)
                            if content.starts_with("<command-name>") {
                                if let Some(end) = content.find("</command-name>") {
                                    let cmd = &content[14..end]; // 14 = "<command-name>".len()
                                    events.push((timestamp, DisplayEvent::Command {
                                        name: cmd.to_string(),
                                    }));
                                    continue;
                                }
                            }

                            // Handle rewound messages: when a user rewinds/edits a message, both
                            // the original and corrected message share the same parentUuid.
                            // We keep only the most recent one (the corrected version).
                            let parent_uuid = json.get("parentUuid").and_then(|p| p.as_str()).unwrap_or("").to_string();
                            let event_idx = events.len();

                            if !parent_uuid.is_empty() {
                                if let Some((old_idx, old_ts)) = user_msg_by_parent.get(&parent_uuid) {
                                    if timestamp > *old_ts {
                                        // New message is more recent - mark old for removal, add new
                                        events[*old_idx] = (chrono::DateTime::<chrono::Utc>::MIN_UTC, DisplayEvent::Filtered);
                                        user_msg_by_parent.insert(parent_uuid, (event_idx, timestamp));
                                    } else {
                                        // Old message is more recent - skip this one
                                        continue;
                                    }
                                } else {
                                    user_msg_by_parent.insert(parent_uuid, (event_idx, timestamp));
                                }
                            }

                            // Track user message index AND timestamp for UPS hook placement
                            last_user_msg = Some((events.len(), timestamp));
                            events.push((timestamp, DisplayEvent::UserMessage {
                                uuid: json.get("uuid").and_then(|u| u.as_str()).unwrap_or("").to_string(),
                                content: content.to_string(),
                            }));
                        }
                        // Handle array content (tool results)
                        else if let Some(content_arr) = content_val.and_then(|c| c.as_array()) {
                            for block in content_arr {
                                if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                                    let tool_use_id = block.get("tool_use_id")
                                        .and_then(|i| i.as_str())
                                        .unwrap_or("")
                                        .to_string();

                                    // Get tool name and file_path from the original call
                                    let (tool_name, file_path) = tool_calls
                                        .get(&tool_use_id)
                                        .cloned()
                                        .unwrap_or(("Unknown".to_string(), None));

                                    // Extract content - can be string or array of content blocks
                                    let content = if let Some(s) = block.get("content").and_then(|c| c.as_str()) {
                                        s.to_string()
                                    } else if let Some(arr) = block.get("content").and_then(|c| c.as_array()) {
                                        arr.iter()
                                            .filter_map(|b| {
                                                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                                                    b.get("text").and_then(|t| t.as_str())
                                                } else {
                                                    None
                                                }
                                            })
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    } else {
                                        String::new()
                                    };

                                    // Tool result received - mark as no longer pending
                                    pending_tools.remove(&tool_use_id);

                                    // Detect failed tools - tool-specific logic to avoid false positives
                                    let is_error = match tool_name.as_str() {
                                        // Read/Write/Edit: errors on first line
                                        "Read" | "Write" | "Edit" | "Glob" | "Grep" => {
                                            let first = content.lines().next().unwrap_or("").to_lowercase();
                                            first.starts_with("error") || first.contains("enoent")
                                                || first.contains("file does not exist")
                                                || first.contains("does not exist")
                                                || first.contains("<tool_use_error>")
                                        }
                                        // Bash: shell errors can appear on any line
                                        "Bash" => content.lines().any(|line| {
                                            let l = line.to_lowercase();
                                            // Shell command errors: "grep:", "tail:", "bash:", etc.
                                            l.contains(": no such file") || l.contains(": permission denied")
                                                || l.contains(": command not found")
                                                // Exit code errors
                                                || ((l.contains("exit code") || l.contains("exit status"))
                                                    && !l.ends_with("0") && !l.ends_with("0\n"))
                                        }),
                                        // Other tools: check first line only
                                        _ => {
                                            let first = content.lines().next().unwrap_or("").to_lowercase();
                                            first.starts_with("error")
                                        }
                                    };
                                    if is_error {
                                        self.failed_tool_calls.insert(tool_use_id.clone());
                                    }

                                    // Extract hooks from system-reminder tags in tool results
                                    let extracted = Self::extract_hooks_from_content(&content, timestamp);
                                    for hook in extracted {
                                        if let (_, DisplayEvent::Hook { ref name, .. }) = &hook {
                                            if name == "UserPromptSubmit" {
                                                // Use user message timestamp (+1ms offset) for correct sorting
                                                if let Some((idx, user_ts)) = last_user_msg {
                                                    let hook_ts = user_ts + chrono::Duration::milliseconds(1);
                                                    ups_hooks.push((idx, hook_ts, hook.1.clone()));
                                                }
                                                continue;
                                            }
                                        }
                                        events.push(hook);
                                    }

                                    if !content.is_empty() {
                                        events.push((timestamp, DisplayEvent::ToolResult {
                                            tool_use_id,
                                            tool_name,
                                            file_path,
                                            content,
                                        }));
                                    }
                                }
                            }
                        }
                    }
                    "assistant" => {
                        if let Some(message) = json.get("message") {
                            if let Some(content_arr) = message.get("content").and_then(|c| c.as_array()) {
                                for block in content_arr {
                                    if let Some(block_type) = block.get("type").and_then(|t| t.as_str()) {
                                        match block_type {
                                            "thinking" => {
                                                // Extract UPS hooks from thinking blocks
                                                // Claude Code injects system-reminder into context, which appears in thinking
                                                if let Some(thinking) = block.get("thinking").and_then(|t| t.as_str()) {
                                                    let extracted = Self::extract_hooks_from_content(thinking, timestamp);
                                                    for hook in extracted {
                                                        if let (_, DisplayEvent::Hook { ref name, .. }) = &hook {
                                                            if name == "UserPromptSubmit" {
                                                                // Use user message timestamp (+1ms offset) for correct sorting
                                                                // This ensures UPS hooks appear right after user messages when sorted
                                                                if let Some((_idx, user_ts)) = last_user_msg {
                                                                    let hook_ts = user_ts + chrono::Duration::milliseconds(1);
                                                                    ups_hooks.push((_idx, hook_ts, hook.1.clone()));
                                                                }
                                                                continue;
                                                            }
                                                        }
                                                        events.push(hook);
                                                    }
                                                }
                                            }
                                            "text" => {
                                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                                    events.push((timestamp, DisplayEvent::AssistantText {
                                                        uuid: json.get("uuid").and_then(|u| u.as_str()).unwrap_or("").to_string(),
                                                        message_id: message.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string(),
                                                        text: text.to_string(),
                                                    }));
                                                }
                                            }
                                            "tool_use" => {
                                                let tool_name = block.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                                                let tool_id = block.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                                                let input = block.get("input").cloned().unwrap_or(serde_json::Value::Null);
                                                let file_path = input.get("file_path").or(input.get("path")).and_then(|p| p.as_str()).map(|s| s.to_string());

                                                // Track this tool call so we can match it to its result
                                                tool_calls.insert(tool_id.clone(), (tool_name.clone(), file_path.clone()));
                                                // Mark as pending until we see its result
                                                pending_tools.insert(tool_id.clone());

                                                events.push((timestamp, DisplayEvent::ToolCall {
                                                    uuid: json.get("uuid").and_then(|u| u.as_str()).unwrap_or("").to_string(),
                                                    tool_use_id: tool_id,
                                                    tool_name,
                                                    file_path,
                                                    input,
                                                }));
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "result" => {
                        // Completion event
                        if let Some(duration) = json.get("durationMs").and_then(|d| d.as_f64()) {
                            let cost = json.get("costUsd").and_then(|c| c.as_f64()).unwrap_or(0.0);
                            events.push((timestamp, DisplayEvent::Complete {
                                session_id: json.get("sessionId").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                                duration_ms: duration as u64,
                                cost_usd: cost,
                                success: true,
                            }));
                        }
                    }
                    "system" => {
                        // Handle local_command system events (e.g., /memory, /status)
                        let subtype = json.get("subtype").and_then(|s| s.as_str()).unwrap_or("");
                        if subtype == "local_command" {
                            if let Some(content) = json.get("content").and_then(|c| c.as_str()) {
                                // Extract command name if present (must be at start)
                                if content.starts_with("<command-name>") {
                                    if let Some(end) = content.find("</command-name>") {
                                        let cmd = &content[14..end];
                                        events.push((timestamp, DisplayEvent::Command {
                                            name: cmd.to_string(),
                                        }));
                                        continue;
                                    }
                                }
                                // Skip local-command-stdout (output of local commands)
                                if content.contains("<local-command-stdout>") {
                                    continue;
                                }
                            }
                        }
                    }
                    "progress" => {
                        // Handle hook_progress events (PreToolUse, PostToolUse, etc.)
                        if let Some(data) = json.get("data") {
                            if data.get("type").and_then(|t| t.as_str()) == Some("hook_progress") {
                                let hook_name = data.get("hookName")
                                    .or_else(|| data.get("hookEvent"))
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let command = data.get("command").and_then(|c| c.as_str()).unwrap_or("");

                                if !hook_name.is_empty() {
                                    // Extract output from echo commands
                                    let output = if command.starts_with("echo '") && command.ends_with('\'') {
                                        command[6..command.len()-1].to_string()
                                    } else if command.starts_with("echo \"") && command.ends_with('"') {
                                        command[6..command.len()-1].to_string()
                                    } else if command.contains("; echo \"$OUT\"") || command.contains("; echo '$OUT'") {
                                        // Pattern: OUT='message'; ...; echo "$OUT"
                                        if let Some(start) = command.find("OUT='") {
                                            let rest = &command[start + 5..];
                                            if let Some(end) = rest.find('\'') {
                                                rest[..end].to_string()
                                            } else { String::new() }
                                        } else if let Some(start) = command.find("OUT=\"") {
                                            let rest = &command[start + 5..];
                                            if let Some(end) = rest.find('"') {
                                                rest[..end].to_string()
                                            } else { String::new() }
                                        } else { String::new() }
                                    } else { String::new() };

                                    // Only show hooks with meaningful output
                                    if !output.is_empty() {
                                        events.push((timestamp, DisplayEvent::Hook {
                                            name: hook_name,
                                            output,
                                        }));
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Add UPS hooks to events - they have user message timestamp (+1ms)
        // so they'll sort to appear right after their user message
        for (_idx, ts, hook_event) in ups_hooks {
            events.push((ts, hook_event));
        }

        // Copy pending tools to self for in-progress indicator rendering
        // (tools that have tool_use but no tool_result yet in the file)
        self.pending_tool_calls = pending_tools;

        // Filter out rewound/superseded messages (marked as Filtered during deduplication)
        events.into_iter().filter(|(_, e)| !matches!(e, DisplayEvent::Filtered)).collect()
    }

    /// Load hooks from hooks.jsonl with timestamps
    fn load_hooks_with_timestamps(&self) -> Vec<(chrono::DateTime<chrono::Utc>, DisplayEvent)> {
        let hooks_path = crate::config::config_dir().join("hooks.jsonl");
        if !hooks_path.exists() { return Vec::new(); }

        let file = match File::open(&hooks_path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let reader = BufReader::new(file);
        let mut hooks = Vec::new();

        for line in reader.lines().map_while(Result::ok) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                let timestamp_str = json.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");
                let hook_name = json.get("hook_name").and_then(|n| n.as_str()).unwrap_or("hook").to_string();
                let output = json.get("output").and_then(|o| o.as_str()).unwrap_or("").trim().to_string();

                let timestamp = chrono::DateTime::parse_from_rfc3339(timestamp_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .or_else(|_| timestamp_str.parse::<chrono::DateTime<chrono::Utc>>())
                    .unwrap_or_else(|_| chrono::Utc::now());

                // Skip UserPromptSubmit hooks from file - they're extracted from session
                // file with adjusted timestamps for correct positioning after user messages
                if !output.is_empty() && hook_name != "UserPromptSubmit" {
                    hooks.push((timestamp, DisplayEvent::Hook { name: hook_name, output }));
                }
            }
        }
        hooks
    }

    pub fn process_output_chunk(&mut self, chunk: &str) {
        let cleaned = strip_ansi_escapes(chunk);
        for ch in cleaned.chars() {
            match ch {
                '\n' => {
                    let line = self.output_buffer.clone();
                    self.output_lines.push_back(line);
                    self.output_buffer.clear();
                    if self.output_lines.len() > self.max_output_lines { self.output_lines.pop_front(); }
                }
                '\r' => self.output_buffer.clear(),
                _ => self.output_buffer.push(ch),
            }
        }
    }

    pub fn add_output(&mut self, chunk: String) {
        let events = self.event_parser.parse(&chunk);
        self.display_events.extend(events);
        self.process_output_chunk(&chunk);
        self.output_scroll = usize::MAX;
    }

    /// Add a user message directly to display_events (for local prompt echo)
    pub fn add_user_message(&mut self, content: String) {
        self.display_events.push(DisplayEvent::UserMessage {
            uuid: String::new(),
            content,
        });
        self.output_scroll = usize::MAX;
    }

    pub fn scroll_output_down(&mut self, lines: usize, _viewport_height: usize) {
        // Don't cap here - draw_output will clamp to actual rendered line count
        self.output_scroll = self.output_scroll.saturating_add(lines);
    }

    pub fn scroll_output_up(&mut self, lines: usize) {
        self.output_scroll = self.output_scroll.saturating_sub(lines);
    }

    pub fn scroll_output_to_bottom(&mut self, _viewport_height: usize) {
        // Set to MAX, draw_output will compute actual bottom position
        self.output_scroll = usize::MAX;
    }

    pub fn scroll_diff_down(&mut self, lines: usize, viewport_height: usize) {
        if let Some(ref diff) = self.diff_text {
            let total_lines = diff.lines().count();
            let max_scroll = total_lines.saturating_sub(viewport_height);
            self.diff_scroll = self.diff_scroll.saturating_add(lines).min(max_scroll);
        }
    }

    pub fn scroll_diff_up(&mut self, lines: usize) {
        self.diff_scroll = self.diff_scroll.saturating_sub(lines);
    }

    pub fn scroll_diff_to_bottom(&mut self, viewport_height: usize) {
        if let Some(ref diff) = self.diff_text {
            self.diff_scroll = diff.lines().count().saturating_sub(viewport_height);
        }
    }

    pub fn toggle_terminal(&mut self) {
        if self.terminal_mode { self.close_terminal(); } else { self.open_terminal(); }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) { self.status_message = Some(msg.into()); }
    pub fn clear_status(&mut self) { self.status_message = None; }

    pub fn add_project(&mut self, path: PathBuf) -> anyhow::Result<()> {
        let project = self.db.get_or_create_project(&path)?;
        self.projects.push(project);
        self.selected_project = self.projects.len() - 1;
        self.load_sessions_for_project()?;
        Ok(())
    }

    pub fn refresh_sessions(&mut self) -> anyhow::Result<()> { self.load_sessions_for_project() }

    pub fn open_branch_dialog(&mut self, branches: Vec<String>) {
        if branches.is_empty() {
            self.set_status("No available branches to checkout");
            return;
        }
        self.branch_dialog = Some(BranchDialog::new(branches));
        self.focus = Focus::BranchDialog;
    }

    pub fn close_branch_dialog(&mut self) {
        self.branch_dialog = None;
        self.focus = Focus::Sessions;
    }

    pub fn update_session_status(&mut self, session_id: &str, status: SessionStatus) {
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == session_id) {
            session.status = status;
        }
    }

    pub fn handle_claude_started(&mut self, session_id: &str, pid: u32) {
        let _ = self.db.update_session_pid(session_id, Some(pid));
        let _ = self.db.update_session_status(session_id, SessionStatus::Running);
        self.update_session_status(session_id, SessionStatus::Running);
        self.set_status(format!("Claude started in {} (PID: {})", session_id, pid));
    }

    pub fn handle_claude_exited(&mut self, session_id: &str, code: Option<i32>) {
        let status = if code == Some(0) { SessionStatus::Completed } else { SessionStatus::Failed };
        let _ = self.db.update_session_status(session_id, status);
        self.update_session_status(session_id, status);
        self.running_sessions.remove(session_id);
        self.claude_receivers.remove(session_id);
        self.interactive_sessions.remove(session_id);
        self.set_status(format!("{} exited: {:?}", session_id, code));
    }

    pub fn handle_claude_output(&mut self, session_id: &str, output_type: OutputType, data: String) {
        // Save to DB as fallback for sessions without claude_session_id
        let _ = self.db.add_session_output(session_id, output_type, &data);
        let is_viewing = self.current_session().map(|s| s.id == session_id).unwrap_or(false);
        if is_viewing {
            // Parse raw data into display events (works for JSON and plain text hooks)
            let events = self.event_parser.parse(&data);

            // Track pending/completed/failed tool calls for progress animation
            for event in &events {
                match event {
                    DisplayEvent::ToolCall { tool_use_id, .. } => {
                        self.pending_tool_calls.insert(tool_use_id.clone());
                    }
                    DisplayEvent::ToolResult { tool_use_id, content, .. } => {
                        self.pending_tool_calls.remove(tool_use_id);
                        // Detect failed tools by checking content for error indicators
                        let lower = content.to_lowercase();
                        if lower.contains("error:") || lower.contains("failed")
                            || lower.starts_with("error") || content.contains("ENOENT")
                            || content.contains("permission denied") {
                            self.failed_tool_calls.insert(tool_use_id.clone());
                        }
                    }
                    _ => {}
                }
            }

            self.display_events.extend(events);

            // For stdout JSON, also update output_lines (fallback display)
            if output_type == OutputType::Stdout || output_type == OutputType::Json {
                if let Some(display_text) = parse_stream_json_for_display(&data) {
                    self.process_output_chunk(&display_text);
                }
            } else {
                // For stderr and other types, just add raw text
                self.process_output_chunk(&data);
            }

            self.output_scroll = usize::MAX;
        }
    }

    pub fn handle_claude_error(&mut self, session_id: &str, error: String) {
        let is_viewing = self.current_session().map(|s| s.id == session_id).unwrap_or(false);
        if is_viewing { self.add_output(format!("Error: {}", error)); }
        self.set_status(format!("{}: {}", session_id, error));
    }

    pub fn register_claude(&mut self, session_id: String, receiver: Receiver<ClaudeEvent>) {
        self.claude_receivers.insert(session_id.clone(), receiver);
        self.running_sessions.insert(session_id);
    }

    pub fn set_claude_session_id(&mut self, azural_session_id: &str, claude_session_id: String) {
        self.claude_session_ids.insert(azural_session_id.to_string(), claude_session_id.clone());
        if let Err(e) = self.db.update_session_claude_id(azural_session_id, Some(&claude_session_id)) {
            tracing::error!("Failed to persist claude_session_id: {}", e);
        }
    }

    pub fn get_claude_session_id(&self, azural_session_id: &str) -> Option<&String> {
        self.claude_session_ids.get(azural_session_id)
    }

    pub fn create_new_session(&mut self, prompt: String) -> anyhow::Result<Session> {
        if let Some(project) = self.current_project().cloned() {
            let session = SessionManager::new(&self.db).create_session(&project, &prompt)?;
            self.refresh_sessions()?;
            self.selected_session = Some(0);
            self.load_session_output();
            Ok(session)
        } else {
            anyhow::bail!("No project selected")
        }
    }

    pub fn archive_current_session(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_session() {
            let session_id = session.id.clone();
            SessionManager::new(&self.db).archive_session(&session_id)?;
            self.set_status("Session archived");
            self.refresh_sessions()?;
        }
        Ok(())
    }

    pub fn load_diff(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_session() {
            if let Some(project) = self.current_project() {
                let diff = Git::get_diff(&session.worktree_path, &project.main_branch)?;
                self.diff_text = Some(diff.diff_text);
                self.view_mode = ViewMode::Diff;
                self.focus = Focus::Output;
                Ok(())
            } else { anyhow::bail!("No project selected") }
        } else { anyhow::bail!("No session selected") }
    }

    pub fn rebase_current_session(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_session() {
            if let Some(project) = self.current_project() {
                Git::rebase_onto_main(&session.worktree_path, &project.main_branch)?;
                self.set_status("Rebased successfully");
                Ok(())
            } else { anyhow::bail!("No project selected") }
        } else { anyhow::bail!("No session selected") }
    }

    pub fn focus_next(&mut self) {
        self.focus = match self.focus {
            Focus::Sessions => Focus::Output,
            Focus::Output => Focus::Input,
            Focus::Input => Focus::Sessions,
            Focus::SessionCreation | Focus::BranchDialog => self.focus,
        };
    }

    pub fn focus_prev(&mut self) {
        self.focus = match self.focus {
            Focus::Sessions => Focus::Input,
            Focus::Output => Focus::Sessions,
            Focus::Input => Focus::Output,
            Focus::SessionCreation | Focus::BranchDialog => self.focus,
        };
    }

    pub fn toggle_help(&mut self) { self.show_help = !self.show_help; }

    pub fn exit_session_creation_mode(&mut self) {
        self.focus = Focus::Sessions;
        self.clear_session_creation_input();
        self.clear_status();
    }

    pub fn set_rebase_status(&mut self, status: RebaseStatus) {
        self.rebase_status = Some(status);
        self.selected_conflict = if self.rebase_status.as_ref().map_or(false, |s| !s.conflicted_files.is_empty()) { Some(0) } else { None };
        self.view_mode = ViewMode::Rebase;
        self.focus = Focus::Output;
    }

    pub fn clear_rebase_status(&mut self) {
        self.rebase_status = None;
        self.selected_conflict = None;
        if self.view_mode == ViewMode::Rebase { self.view_mode = ViewMode::Output; }
    }

    pub fn select_next_conflict(&mut self) {
        if let Some(ref status) = self.rebase_status {
            if let Some(idx) = self.selected_conflict {
                if idx + 1 < status.conflicted_files.len() { self.selected_conflict = Some(idx + 1); }
            }
        }
    }

    pub fn select_prev_conflict(&mut self) {
        if let Some(idx) = self.selected_conflict {
            if idx > 0 { self.selected_conflict = Some(idx - 1); }
        }
    }

    pub fn current_conflict_file(&self) -> Option<&str> {
        self.rebase_status.as_ref().and_then(|status| {
            self.selected_conflict.and_then(|idx| status.conflicted_files.get(idx).map(|s| s.as_str()))
        })
    }

    pub fn open_context_menu(&mut self) {
        if let Some(session) = self.current_session() {
            let actions = SessionAction::available_for_status(session.status);
            if !actions.is_empty() { self.context_menu = Some(ContextMenu { actions, selected: 0 }); }
        }
    }

    pub fn close_context_menu(&mut self) { self.context_menu = None; }

    pub fn context_menu_next(&mut self) {
        if let Some(ref mut menu) = self.context_menu {
            if menu.selected + 1 < menu.actions.len() { menu.selected += 1; }
        }
    }

    pub fn context_menu_prev(&mut self) {
        if let Some(ref mut menu) = self.context_menu {
            if menu.selected > 0 { menu.selected -= 1; }
        }
    }

    pub fn selected_action(&self) -> Option<SessionAction> {
        self.context_menu.as_ref().map(|menu| menu.actions[menu.selected].clone())
    }

    pub fn start_wizard(&mut self) {
        self.creation_wizard = Some(SessionCreationWizard::new(&self.projects));
        self.focus = Focus::Input;
    }

    pub fn cancel_wizard(&mut self) {
        self.creation_wizard = None;
        self.focus = Focus::Sessions;
    }

    pub fn is_wizard_active(&self) -> bool { self.creation_wizard.is_some() }

    /// Poll hooks.jsonl for new entries and add them to display_events
    /// Also saves hooks to database for persistence across session switches
    /// Returns true if new hooks were found
    pub fn poll_hooks_file(&mut self) -> bool {
        let hooks_path = crate::config::config_dir().join("hooks.jsonl");
        if !hooks_path.exists() { return false; }

        let file = match File::open(&hooks_path) {
            Ok(f) => f,
            Err(_) => return false,
        };

        let metadata = match file.metadata() {
            Ok(m) => m,
            Err(_) => return false,
        };

        // Skip if file hasn't grown
        let file_len = metadata.len();
        if file_len <= self.hooks_file_pos { return false; }

        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(self.hooks_file_pos)).is_err() { return false; }

        let mut found = false;
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                let hook_name = json.get("hook_name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("hook")
                    .to_string();
                let output = json.get("output")
                    .and_then(|o| o.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();

                // Only add if we have meaningful output
                // Note: We no longer save to DB - hooks persist in hooks.jsonl
                if !output.is_empty() {
                    self.display_events.push(DisplayEvent::Hook { name: hook_name.clone(), output: output.clone() });
                    self.output_scroll = usize::MAX;
                    found = true;
                }
            }
            line.clear();
        }

        self.hooks_file_pos = file_len;
        found
    }

    /// Poll all interactive sessions for new events from their session files
    /// Returns true if any new events were found
    pub fn poll_interactive_sessions(&mut self) -> bool {
        let current_session_id = self.current_session().map(|s| s.id.clone());
        let Some(session_id) = current_session_id else { return false };

        let events = if let Some(interactive) = self.interactive_sessions.get_mut(&session_id) {
            interactive.poll_events()
        } else {
            return false;
        };

        if events.is_empty() {
            return false;
        }

        // Track pending/completed/failed tool calls for progress animation
        for event in &events {
            match event {
                DisplayEvent::ToolCall { tool_use_id, .. } => {
                    self.pending_tool_calls.insert(tool_use_id.clone());
                }
                DisplayEvent::ToolResult { tool_use_id, content, .. } => {
                    self.pending_tool_calls.remove(tool_use_id);
                    // Detect failed tools by checking content for error indicators
                    let lower = content.to_lowercase();
                    if lower.contains("error:") || lower.contains("failed")
                        || lower.starts_with("error") || content.contains("ENOENT")
                        || content.contains("permission denied") {
                        self.failed_tool_calls.insert(tool_use_id.clone());
                    }
                }
                _ => {}
            }
        }

        self.display_events.extend(events);
        self.output_scroll = usize::MAX;
        true
    }

    /// Clean up interactive session when Claude exits
    pub fn cleanup_interactive_session(&mut self, session_id: &str) {
        self.interactive_sessions.remove(session_id);
    }

    /// Dump rendered output to file - exact replica of TUI output pane
    /// Writes to <session_worktree>/.azural/debug-output.txt
    pub fn dump_debug_output(&self) -> anyhow::Result<()> {
        use std::io::Write;

        let debug_dir = self.current_session()
            .map(|s| s.worktree_path.join(".azural"))
            .unwrap_or_else(|| crate::config::config_dir());
        std::fs::create_dir_all(&debug_dir)?;
        let debug_path = debug_dir.join("debug-output.txt");
        let mut file = std::fs::File::create(&debug_path)?;

        // Render exactly as TUI does - plain text only, no metadata
        let rendered_lines = crate::tui::util::render_display_events(
            &self.display_events,
            120,
            &self.pending_tool_calls,
            &self.failed_tool_calls,
            self.animation_tick,
        );

        for line in rendered_lines.iter() {
            let text: String = line.spans.iter().map(|span| span.content.as_ref()).collect();
            writeln!(file, "{}", text)?;
        }

        Ok(())
    }
}
