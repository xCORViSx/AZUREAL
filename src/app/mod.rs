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
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use portable_pty::MasterPty;

use crate::claude::ClaudeEvent;
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
            diff_text: None,
            output_scroll: 0,
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
        self.output_scroll = 0;
        self.display_events.clear();
        self.event_parser = EventParser::new();
        self.selected_event = None;

        if let Some(session) = self.current_session() {
            if let Ok(outputs) = self.db.get_session_outputs(&session.id) {
                for output in outputs {
                    match output.output_type {
                        OutputType::System => self.process_output_chunk(&output.data),
                        OutputType::Json | OutputType::Stdout => {
                            if let Some(display_text) = parse_stream_json_for_display(&output.data) {
                                self.process_output_chunk(&display_text);
                            }
                        }
                        _ => self.process_output_chunk(&output.data),
                    }
                }
            }
        }
    }

    fn process_output_chunk(&mut self, chunk: &str) {
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

    pub fn scroll_output_down(&mut self, lines: usize, viewport_height: usize) {
        let max_scroll = self.output_lines.len().saturating_sub(viewport_height);
        self.output_scroll = self.output_scroll.saturating_add(lines).min(max_scroll);
    }

    pub fn scroll_output_up(&mut self, lines: usize) {
        self.output_scroll = self.output_scroll.saturating_sub(lines);
    }

    pub fn scroll_output_to_bottom(&mut self, viewport_height: usize) {
        self.output_scroll = self.output_lines.len().saturating_sub(viewport_height);
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
        self.set_status(format!("{} exited: {:?}", session_id, code));
    }

    pub fn handle_claude_output(&mut self, session_id: &str, output_type: OutputType, data: String) {
        let _ = self.db.add_session_output(session_id, output_type, &data);
        let is_viewing = self.current_session().map(|s| s.id == session_id).unwrap_or(false);
        if is_viewing {
            if let Some(display_text) = parse_stream_json_for_display(&data) {
                self.add_output(display_text);
            }
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
}
