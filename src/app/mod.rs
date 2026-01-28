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
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use portable_pty::MasterPty;

use crate::claude::{ClaudeEvent, InteractiveSession};
use crate::events::{DisplayEvent, EventParser};
use crate::git::Git;
use crate::models::{OutputType, Project, RebaseStatus, Session, SessionStatus};
use crate::syntax::DiffHighlighter;
use crate::wizard::SessionCreationWizard;

use util::{parse_stream_json_for_display, strip_ansi_escapes};

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
            pending_tool_calls: HashSet::new(),
            failed_tool_calls: HashSet::new(),
            animation_tick: 0,
            session_file_path: None,
            session_file_modified: None,
        }
    }

    pub fn is_session_running(&self, branch_name: &str) -> bool {
        self.running_sessions.contains(branch_name)
    }

    pub fn is_current_session_running(&self) -> bool {
        self.current_session().map(|s| self.running_sessions.contains(&s.branch_name)).unwrap_or(false)
    }

    /// Load project and sessions from git (stateless discovery)
    pub fn load(&mut self) -> anyhow::Result<()> {
        let cwd = std::env::current_dir()?;

        // Find git repo root
        if !Git::is_git_repo(&cwd) {
            return Ok(());
        }

        let repo_root = Git::repo_root(&cwd)?;
        let main_branch = Git::get_main_branch(&repo_root)?;

        self.project = Some(Project::from_path(repo_root, main_branch));
        self.load_sessions()?;

        Ok(())
    }

    /// Load sessions from git worktrees and branches
    pub fn load_sessions(&mut self) -> anyhow::Result<()> {
        let Some(project) = &self.project else { return Ok(()) };

        let worktrees = Git::list_worktrees_detailed(&project.path)?;
        let azural_branches = Git::list_azural_branches(&project.path)?;

        let mut sessions = Vec::new();
        let mut active_branches: HashSet<String> = HashSet::new();

        // First, add main worktree
        for wt in &worktrees {
            if wt.is_main {
                let branch_name = wt.branch.clone().unwrap_or_else(|| project.main_branch.clone());
                let claude_id = crate::config::find_latest_claude_session(&wt.path);
                if let Some(ref id) = claude_id {
                    self.claude_session_ids.insert(branch_name.clone(), id.clone());
                }
                sessions.push(Session {
                    branch_name: branch_name.clone(),
                    worktree_path: Some(wt.path.clone()),
                    claude_session_id: claude_id,
                    archived: false,
                });
                active_branches.insert(branch_name);
            }
        }

        // Add feature worktrees (azural/* branches with active worktrees)
        for wt in &worktrees {
            if !wt.is_main {
                let branch_name = wt.branch.clone().unwrap_or_default();
                let claude_id = crate::config::find_latest_claude_session(&wt.path);
                if let Some(ref id) = claude_id {
                    self.claude_session_ids.insert(branch_name.clone(), id.clone());
                }
                sessions.push(Session {
                    branch_name: branch_name.clone(),
                    worktree_path: Some(wt.path.clone()),
                    claude_session_id: claude_id,
                    archived: false,
                });
                active_branches.insert(branch_name);
            }
        }

        // Add archived sessions (azural/* branches without worktrees)
        for branch in azural_branches {
            if !active_branches.contains(&branch) {
                sessions.push(Session {
                    branch_name: branch,
                    worktree_path: None,
                    claude_session_id: None,
                    archived: true,
                });
            }
        }

        self.sessions = sessions;
        self.selected_session = if self.sessions.is_empty() { None } else { Some(0) };

        Ok(())
    }

    pub fn current_project(&self) -> Option<&Project> { self.project.as_ref() }
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
            let branch_name = session.branch_name.clone();
            let worktree_path = session.worktree_path.clone();

            // Try to get Claude session ID, or auto-discover from Claude's files
            let mut claude_session_id = session.claude_session_id.clone()
                .or_else(|| self.claude_session_ids.get(&branch_name).cloned());

            // Auto-discover Claude session ID if not set and we have a worktree
            if claude_session_id.is_none() {
                if let Some(ref wt_path) = worktree_path {
                    if let Some(discovered_id) = crate::config::find_latest_claude_session(wt_path) {
                        self.claude_session_ids.insert(branch_name.clone(), discovered_id.clone());
                        claude_session_id = Some(discovered_id);
                    }
                }
            }

            // Try loading from Claude's session files
            if let (Some(claude_id), Some(ref wt_path)) = (claude_session_id, &worktree_path) {
                if let Some(session_file) = crate::config::claude_session_file(wt_path, &claude_id) {
                    // Track file for live polling
                    self.session_file_path = Some(session_file.clone());
                    self.session_file_modified = std::fs::metadata(&session_file)
                        .and_then(|m| m.modified())
                        .ok();

                    let timed_events = self.load_claude_session_events(&session_file);

                    for (_, event) in timed_events {
                        self.display_events.push(event);
                    }
                }
            }
        }

        // Auto-dump debug output on debug builds
        #[cfg(debug_assertions)]
        let _ = self.dump_debug_output();
    }

    /// Poll session file for changes and reload if modified
    pub fn poll_session_file(&mut self) -> bool {
        let Some(path) = &self.session_file_path else { return false };
        let Ok(metadata) = std::fs::metadata(path) else { return false };
        let Ok(modified) = metadata.modified() else { return false };

        if self.session_file_modified.map(|t| modified > t).unwrap_or(true) {
            self.load_session_output();
            true
        } else {
            false
        }
    }

    /// Extract hook events from system-reminder tags in content
    fn extract_hooks_from_content(content: &str, timestamp: chrono::DateTime<chrono::Utc>) -> Vec<(chrono::DateTime<chrono::Utc>, DisplayEvent)> {
        let mut hooks = Vec::new();
        let mut search_start = 0;
        while let Some(start) = content[search_start..].find("<system-reminder>") {
            let abs_start = search_start + start + 17;
            if let Some(end) = content[abs_start..].find("</system-reminder>") {
                let reminder_content = &content[abs_start..abs_start + end];
                if let Some(hook_pos) = reminder_content.find(" hook success:") {
                    let name = reminder_content[..hook_pos]
                        .trim()
                        .trim_start_matches("\\n")
                        .trim_end_matches("\\n")
                        .to_string();
                    let output = reminder_content[hook_pos + 14..]
                        .trim()
                        .trim_start_matches("\\n")
                        .trim_end_matches("\\n")
                        .to_string();

                    if !output.is_empty() && output != "..." && !name.is_empty() {
                        hooks.push((timestamp, DisplayEvent::Hook { name, output }));
                    } else if output == "..." && !name.is_empty() {
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
                search_start = abs_start + end + 18;
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
        let mut user_msg_by_parent: HashMap<String, (usize, chrono::DateTime<chrono::Utc>)> = HashMap::new();
        let mut tool_calls: HashMap<String, (String, Option<String>)> = HashMap::new();
        let mut pending_tools: HashSet<String> = HashSet::new();
        let mut last_user_msg: Option<(usize, chrono::DateTime<chrono::Utc>)> = None;
        let mut ups_hooks: Vec<(usize, chrono::DateTime<chrono::Utc>, DisplayEvent)> = Vec::new();

        for line in reader.lines().map_while(Result::ok) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
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

                        let is_compaction_summary = content_str.as_ref()
                            .map(|c| c.starts_with("This session is being continued from a previous conversation"))
                            .unwrap_or(false);

                        if is_compaction_summary {
                            events.push((timestamp, DisplayEvent::Compacting));
                            continue;
                        }

                        if let Some(ref content) = content_str {
                            for hook in Self::extract_hooks_from_content(content, timestamp) {
                                events.push(hook);
                            }
                        }

                        if is_meta { continue; }

                        if let Some(content) = content_val.and_then(|c| c.as_str()) {
                            if content.contains("<local-command-caveat>") { continue; }

                            if content.contains("<local-command-stdout>") {
                                if content.contains("Compacted") {
                                    events.push((timestamp, DisplayEvent::Compacted));
                                }
                                continue;
                            }

                            if content.starts_with("<command-name>") {
                                if let Some(end) = content.find("</command-name>") {
                                    let cmd = &content[14..end];
                                    events.push((timestamp, DisplayEvent::Command { name: cmd.to_string() }));
                                    continue;
                                }
                            }

                            let parent_uuid = json.get("parentUuid").and_then(|p| p.as_str()).unwrap_or("").to_string();
                            let event_idx = events.len();

                            if !parent_uuid.is_empty() {
                                if let Some((old_idx, old_ts)) = user_msg_by_parent.get(&parent_uuid) {
                                    if timestamp > *old_ts {
                                        events[*old_idx] = (chrono::DateTime::<chrono::Utc>::MIN_UTC, DisplayEvent::Filtered);
                                        user_msg_by_parent.insert(parent_uuid, (event_idx, timestamp));
                                    } else {
                                        continue;
                                    }
                                } else {
                                    user_msg_by_parent.insert(parent_uuid, (event_idx, timestamp));
                                }
                            }

                            last_user_msg = Some((events.len(), timestamp));
                            events.push((timestamp, DisplayEvent::UserMessage {
                                uuid: json.get("uuid").and_then(|u| u.as_str()).unwrap_or("").to_string(),
                                content: content.to_string(),
                            }));
                        } else if let Some(content_arr) = content_val.and_then(|c| c.as_array()) {
                            for block in content_arr {
                                if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                                    let tool_use_id = block.get("tool_use_id")
                                        .and_then(|i| i.as_str())
                                        .unwrap_or("")
                                        .to_string();

                                    let (tool_name, file_path) = tool_calls
                                        .get(&tool_use_id)
                                        .cloned()
                                        .unwrap_or(("Unknown".to_string(), None));

                                    let content = if let Some(s) = block.get("content").and_then(|c| c.as_str()) {
                                        s.to_string()
                                    } else if let Some(arr) = block.get("content").and_then(|c| c.as_array()) {
                                        arr.iter()
                                            .filter_map(|b| {
                                                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                                                    b.get("text").and_then(|t| t.as_str())
                                                } else { None }
                                            })
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    } else {
                                        String::new()
                                    };

                                    pending_tools.remove(&tool_use_id);

                                    let is_error = match tool_name.as_str() {
                                        "Read" | "Write" | "Edit" | "Glob" | "Grep" => {
                                            let first = content.lines().next().unwrap_or("").to_lowercase();
                                            first.starts_with("error") || first.contains("enoent")
                                                || first.contains("file does not exist")
                                                || first.contains("does not exist")
                                                || first.contains("<tool_use_error>")
                                        }
                                        "Bash" => content.lines().any(|line| {
                                            let l = line.to_lowercase();
                                            l.contains(": no such file") || l.contains(": permission denied")
                                                || l.contains(": command not found")
                                                || ((l.contains("exit code") || l.contains("exit status"))
                                                    && !l.ends_with("0") && !l.ends_with("0\n"))
                                        }),
                                        "WebFetch" => {
                                            let first = content.lines().next().unwrap_or("").to_lowercase();
                                            first.contains("status code 4") || first.contains("status code 5")
                                                || first.contains("failed") || first.starts_with("error")
                                        }
                                        _ => {
                                            let first = content.lines().next().unwrap_or("").to_lowercase();
                                            first.starts_with("error")
                                        }
                                    };
                                    if is_error {
                                        self.failed_tool_calls.insert(tool_use_id.clone());
                                    }

                                    let extracted = Self::extract_hooks_from_content(&content, timestamp);
                                    for hook in extracted {
                                        if let (_, DisplayEvent::Hook { ref name, .. }) = &hook {
                                            if name == "UserPromptSubmit" {
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
                                                if let Some(thinking) = block.get("thinking").and_then(|t| t.as_str()) {
                                                    let extracted = Self::extract_hooks_from_content(thinking, timestamp);
                                                    for hook in extracted {
                                                        if let (_, DisplayEvent::Hook { ref name, .. }) = &hook {
                                                            if name == "UserPromptSubmit" {
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

                                                tool_calls.insert(tool_id.clone(), (tool_name.clone(), file_path.clone()));
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
                        let subtype = json.get("subtype").and_then(|s| s.as_str()).unwrap_or("");
                        if subtype == "local_command" {
                            if let Some(content) = json.get("content").and_then(|c| c.as_str()) {
                                if content.starts_with("<command-name>") {
                                    if let Some(end) = content.find("</command-name>") {
                                        let cmd = &content[14..end];
                                        events.push((timestamp, DisplayEvent::Command { name: cmd.to_string() }));
                                        continue;
                                    }
                                }
                                if content.contains("<local-command-stdout>") { continue; }
                            }
                        }
                    }
                    "progress" => {
                        if let Some(data) = json.get("data") {
                            if data.get("type").and_then(|t| t.as_str()) == Some("hook_progress") {
                                let hook_name = data.get("hookName")
                                    .or_else(|| data.get("hookEvent"))
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let command = data.get("command").and_then(|c| c.as_str()).unwrap_or("");

                                if !hook_name.is_empty() {
                                    let output = if command.starts_with("echo '") && command.ends_with('\'') {
                                        command[6..command.len()-1].to_string()
                                    } else if command.starts_with("echo \"") && command.ends_with('"') {
                                        command[6..command.len()-1].to_string()
                                    } else if command.contains("; echo \"$OUT\"") || command.contains("; echo '$OUT'") {
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

                                    if !output.is_empty() {
                                        events.push((timestamp, DisplayEvent::Hook { name: hook_name, output }));
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        for (_idx, ts, hook_event) in ups_hooks {
            events.push((ts, hook_event));
        }

        self.pending_tool_calls = pending_tools;
        events.into_iter().filter(|(_, e)| !matches!(e, DisplayEvent::Filtered)).collect()
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

    pub fn add_user_message(&mut self, content: String) {
        self.display_events.push(DisplayEvent::UserMessage {
            uuid: String::new(),
            content,
        });
        self.output_scroll = usize::MAX;
    }

    pub fn scroll_output_down(&mut self, lines: usize, _viewport_height: usize) {
        self.output_scroll = self.output_scroll.saturating_add(lines);
    }

    pub fn scroll_output_up(&mut self, lines: usize) {
        self.output_scroll = self.output_scroll.saturating_sub(lines);
    }

    pub fn scroll_output_to_bottom(&mut self, _viewport_height: usize) {
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

    pub fn refresh_sessions(&mut self) -> anyhow::Result<()> { self.load_sessions() }

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

    pub fn handle_claude_started(&mut self, branch_name: &str, pid: u32) {
        self.running_sessions.insert(branch_name.to_string());
        self.set_status(format!("Claude started in {} (PID: {})", branch_name, pid));
    }

    pub fn handle_claude_exited(&mut self, branch_name: &str, code: Option<i32>) {
        self.running_sessions.remove(branch_name);
        self.claude_receivers.remove(branch_name);
        self.interactive_sessions.remove(branch_name);
        self.set_status(format!("{} exited: {:?}", branch_name, code));
    }

    pub fn handle_claude_output(&mut self, branch_name: &str, output_type: OutputType, data: String) {
        let is_viewing = self.current_session().map(|s| s.branch_name == branch_name).unwrap_or(false);
        if is_viewing {
            let events = self.event_parser.parse(&data);

            for event in &events {
                match event {
                    DisplayEvent::ToolCall { tool_use_id, .. } => {
                        self.pending_tool_calls.insert(tool_use_id.clone());
                    }
                    DisplayEvent::ToolResult { tool_use_id, content, .. } => {
                        self.pending_tool_calls.remove(tool_use_id);
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

            if output_type == OutputType::Stdout || output_type == OutputType::Json {
                if let Some(display_text) = parse_stream_json_for_display(&data) {
                    self.process_output_chunk(&display_text);
                }
            } else {
                self.process_output_chunk(&data);
            }

            self.output_scroll = usize::MAX;
        }
    }

    pub fn handle_claude_error(&mut self, branch_name: &str, error: String) {
        let is_viewing = self.current_session().map(|s| s.branch_name == branch_name).unwrap_or(false);
        if is_viewing { self.add_output(format!("Error: {}", error)); }
        self.set_status(format!("{}: {}", branch_name, error));
    }

    pub fn register_claude(&mut self, branch_name: String, receiver: Receiver<ClaudeEvent>) {
        self.claude_receivers.insert(branch_name.clone(), receiver);
        self.running_sessions.insert(branch_name);
    }

    pub fn set_claude_session_id(&mut self, branch_name: &str, claude_session_id: String) {
        self.claude_session_ids.insert(branch_name.to_string(), claude_session_id);
    }

    pub fn get_claude_session_id(&self, branch_name: &str) -> Option<&String> {
        self.claude_session_ids.get(branch_name)
    }

    pub fn create_new_session(&mut self, prompt: String) -> anyhow::Result<Session> {
        let Some(project) = self.project.clone() else {
            anyhow::bail!("No project loaded")
        };

        // Generate session name from prompt
        let name = generate_session_name(&prompt);
        let worktree_name = sanitize_for_branch(&name);
        let branch_name = format!("azural/{}", worktree_name);
        let worktree_path = project.worktrees_dir().join(&worktree_name);

        if worktree_path.exists() {
            anyhow::bail!("Worktree already exists: {}", worktree_path.display());
        }

        // Create git worktree
        Git::create_worktree(&project.path, &worktree_path, &branch_name)?;

        let session = Session {
            branch_name: branch_name.clone(),
            worktree_path: Some(worktree_path),
            claude_session_id: None,
            archived: false,
        };

        self.refresh_sessions()?;

        // Select the new session
        if let Some(idx) = self.sessions.iter().position(|s| s.branch_name == branch_name) {
            self.selected_session = Some(idx);
            self.load_session_output();
        }

        Ok(session)
    }

    pub fn archive_current_session(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_session() {
            if let Some(ref wt_path) = session.worktree_path {
                if let Some(project) = &self.project {
                    Git::remove_worktree(&project.path, wt_path)?;
                }
            }
            self.set_status("Session archived");
            self.refresh_sessions()?;
        }
        Ok(())
    }

    pub fn load_diff(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_session() {
            if let Some(ref wt_path) = session.worktree_path {
                if let Some(project) = self.current_project() {
                    let diff = Git::get_diff(wt_path, &project.main_branch)?;
                    self.diff_text = Some(diff.diff_text);
                    self.view_mode = ViewMode::Diff;
                    self.focus = Focus::Output;
                    return Ok(());
                }
            }
        }
        anyhow::bail!("No active session with worktree")
    }

    pub fn rebase_current_session(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_session() {
            if let Some(ref wt_path) = session.worktree_path {
                if let Some(project) = self.current_project() {
                    Git::rebase_onto_main(wt_path, &project.main_branch)?;
                    self.set_status("Rebased successfully");
                    return Ok(());
                }
            }
        }
        anyhow::bail!("No active session with worktree")
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
            let status = session.status(&self.running_sessions);
            let actions = SessionAction::available_for_status(status);
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
        self.creation_wizard = Some(SessionCreationWizard::new_single_project(self.project.as_ref()));
        self.focus = Focus::Input;
    }

    pub fn cancel_wizard(&mut self) {
        self.creation_wizard = None;
        self.focus = Focus::Sessions;
    }

    pub fn is_wizard_active(&self) -> bool { self.creation_wizard.is_some() }

    pub fn poll_interactive_sessions(&mut self) -> bool {
        let current_branch = self.current_session().map(|s| s.branch_name.clone());
        let Some(branch_name) = current_branch else { return false };

        let events = if let Some(interactive) = self.interactive_sessions.get_mut(&branch_name) {
            interactive.poll_events()
        } else {
            return false;
        };

        if events.is_empty() { return false; }

        for event in &events {
            match event {
                DisplayEvent::ToolCall { tool_use_id, .. } => {
                    self.pending_tool_calls.insert(tool_use_id.clone());
                }
                DisplayEvent::ToolResult { tool_use_id, content, .. } => {
                    self.pending_tool_calls.remove(tool_use_id);
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

    pub fn cleanup_interactive_session(&mut self, branch_name: &str) {
        self.interactive_sessions.remove(branch_name);
    }

    pub fn dump_debug_output(&self) -> anyhow::Result<()> {
        use std::io::Write;

        let debug_dir = self.current_session()
            .and_then(|s| s.worktree_path.as_ref())
            .map(|p| p.join(".azural"))
            .unwrap_or_else(crate::config::config_dir);
        std::fs::create_dir_all(&debug_dir)?;
        let debug_path = debug_dir.join("debug-output.txt");
        let mut file = std::fs::File::create(&debug_path)?;

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

/// Generate a session name from the prompt
fn generate_session_name(prompt: &str) -> String {
    let name: String = prompt
        .chars()
        .take(40)
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-' || *c == '_')
        .collect();

    let name = name.trim();

    if name.is_empty() {
        format!("session-{}", &uuid::Uuid::new_v4().to_string()[..8])
    } else {
        let name = if name.len() > 30 {
            if let Some(pos) = name[..30].rfind(' ') {
                &name[..pos]
            } else {
                &name[..30]
            }
        } else {
            name
        };
        name.to_string()
    }
}

/// Sanitize a string for use as a git branch name
fn sanitize_for_branch(s: &str) -> String {
    let sanitized: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();

    let mut result = String::new();
    let mut last_was_dash = false;

    for c in sanitized.chars() {
        if c == '-' {
            if !last_was_dash && !result.is_empty() {
                result.push(c);
                last_was_dash = true;
            }
        } else {
            result.push(c);
            last_was_dash = false;
        }
    }

    result.trim_end_matches('-').to_string()
}
