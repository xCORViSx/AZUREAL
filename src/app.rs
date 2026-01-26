use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use crate::claude::ClaudeEvent;
use crate::db::Database;
use crate::git::Git;
use crate::models::{Project, Session, SessionStatus};
use crate::session::SessionManager;
use crate::syntax::DiffHighlighter;

/// Application state
pub struct App {
    /// Database connection
    pub db: Database,
    /// All projects
    pub projects: Vec<Project>,
    /// Currently selected project index
    pub selected_project: usize,
    /// Sessions for current project
    pub sessions: Vec<Session>,
    /// Currently selected session index
    pub selected_session: Option<usize>,
    /// Output lines for current session
    pub output_lines: VecDeque<String>,
    /// Maximum output lines to keep
    pub max_output_lines: usize,
    /// Buffer for incomplete lines (streaming chunks)
    pub output_buffer: String,
    /// Current input text
    pub input: String,
    /// Input cursor position
    pub input_cursor: usize,
    /// Current view mode
    pub view_mode: ViewMode,
    /// Current focus
    pub focus: Focus,
    /// Whether the app should quit
    pub should_quit: bool,
    /// Status message to display
    pub status_message: Option<String>,
    /// Active Claude process receiver
    pub claude_receiver: Option<Receiver<ClaudeEvent>>,
    /// Current diff text (if viewing diff)
    pub diff_text: Option<String>,
    /// Scroll offset for output
    pub output_scroll: usize,
    /// Scroll offset for diff
    pub diff_scroll: usize,
    /// Syntax highlighter for diff view
    pub diff_highlighter: DiffHighlighter,
    /// Whether to show help overlay
    pub show_help: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Output,
    Diff,
    Messages,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sessions,
    Output,
    Input,
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
            input: String::new(),
            input_cursor: 0,
            view_mode: ViewMode::Output,
            focus: Focus::Sessions,
            should_quit: false,
            status_message: None,
            claude_receiver: None,
            diff_text: None,
            output_scroll: 0,
            diff_scroll: 0,
            diff_highlighter: DiffHighlighter::new(),
            show_help: false,
        }
    }

    /// Load initial data
    pub fn load(&mut self) -> anyhow::Result<()> {
        self.projects = self.db.list_projects()?;

        if !self.projects.is_empty() {
            self.load_sessions_for_project()?;
        }

        Ok(())
    }

    /// Load sessions for the currently selected project
    pub fn load_sessions_for_project(&mut self) -> anyhow::Result<()> {
        if let Some(project) = self.projects.get(self.selected_project) {
            self.sessions = self.db.list_sessions_for_project(project.id)?;
            self.selected_session = if self.sessions.is_empty() {
                None
            } else {
                Some(0)
            };
        }
        Ok(())
    }

    /// Get the currently selected project
    pub fn current_project(&self) -> Option<&Project> {
        self.projects.get(self.selected_project)
    }

    /// Get the currently selected session
    pub fn current_session(&self) -> Option<&Session> {
        self.selected_session
            .and_then(|idx| self.sessions.get(idx))
    }

    /// Select next session
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

    /// Select previous session
    pub fn select_prev_session(&mut self) {
        if let Some(idx) = self.selected_session {
            if idx > 0 {
                self.selected_session = Some(idx - 1);
                self.load_session_output();
            }
        }
    }

    /// Select next project
    pub fn select_next_project(&mut self) {
        if self.selected_project + 1 < self.projects.len() {
            self.selected_project += 1;
            let _ = self.load_sessions_for_project();
            self.load_session_output();
        }
    }

    /// Select previous project
    pub fn select_prev_project(&mut self) {
        if self.selected_project > 0 {
            self.selected_project -= 1;
            let _ = self.load_sessions_for_project();
            self.load_session_output();
        }
    }

    /// Load output for the current session
    pub fn load_session_output(&mut self) {
        self.output_lines.clear();
        self.output_buffer.clear();
        self.output_scroll = 0;

        if let Some(session) = self.current_session() {
            if let Ok(outputs) = self.db.get_session_outputs(&session.id) {
                for output in outputs {
                    // Process stored output chunks
                    self.process_output_chunk(&output.data);
                }
            }
        }
    }

    /// Process an output chunk (may contain partial lines)
    fn process_output_chunk(&mut self, chunk: &str) {
        // Strip ANSI escape sequences for cleaner display
        let cleaned = strip_ansi_escapes(chunk);

        for ch in cleaned.chars() {
            match ch {
                '\n' => {
                    // Complete line - add to output_lines
                    let line = self.output_buffer.clone();
                    self.output_lines.push_back(line);
                    self.output_buffer.clear();

                    if self.output_lines.len() > self.max_output_lines {
                        self.output_lines.pop_front();
                    }
                }
                '\r' => {
                    // Carriage return - overwrite current line buffer
                    // This handles progress indicators that use \r to update in place
                    self.output_buffer.clear();
                }
                _ => {
                    // Regular character - append to buffer
                    self.output_buffer.push(ch);
                }
            }
        }
    }

    /// Add output chunk (streaming mode)
    pub fn add_output(&mut self, chunk: String) {
        self.process_output_chunk(&chunk);
        // Auto-scroll to bottom
        self.scroll_output_to_bottom();
    }

    /// Scroll output down
    pub fn scroll_output_down(&mut self, lines: usize) {
        self.output_scroll = self.output_scroll.saturating_add(lines);
    }

    /// Scroll output up
    pub fn scroll_output_up(&mut self, lines: usize) {
        self.output_scroll = self.output_scroll.saturating_sub(lines);
    }

    /// Scroll to bottom of output
    pub fn scroll_output_to_bottom(&mut self) {
        self.output_scroll = self.output_lines.len().saturating_sub(1);
    }

    /// Set status message
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }

    /// Clear status message
    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    /// Handle input character
    pub fn input_char(&mut self, c: char) {
        self.input.insert(self.input_cursor, c);
        self.input_cursor += 1;
    }

    /// Handle backspace
    pub fn input_backspace(&mut self) {
        if self.input_cursor > 0 {
            self.input_cursor -= 1;
            self.input.remove(self.input_cursor);
        }
    }

    /// Handle delete
    pub fn input_delete(&mut self) {
        if self.input_cursor < self.input.len() {
            self.input.remove(self.input_cursor);
        }
    }

    /// Move cursor left
    pub fn input_left(&mut self) {
        self.input_cursor = self.input_cursor.saturating_sub(1);
    }

    /// Move cursor right
    pub fn input_right(&mut self) {
        if self.input_cursor < self.input.len() {
            self.input_cursor += 1;
        }
    }

    /// Move cursor to start
    pub fn input_home(&mut self) {
        self.input_cursor = 0;
    }

    /// Move cursor to end
    pub fn input_end(&mut self) {
        self.input_cursor = self.input.len();
    }

    /// Clear input
    pub fn clear_input(&mut self) {
        self.input.clear();
        self.input_cursor = 0;
    }

    /// Add a project by path
    pub fn add_project(&mut self, path: PathBuf) -> anyhow::Result<()> {
        let project = self.db.get_or_create_project(&path)?;
        self.projects.push(project);
        self.selected_project = self.projects.len() - 1;
        self.load_sessions_for_project()?;
        Ok(())
    }

    /// Refresh session list
    pub fn refresh_sessions(&mut self) -> anyhow::Result<()> {
        self.load_sessions_for_project()
    }

    /// Update session status in the list
    pub fn update_session_status(&mut self, session_id: &str, status: SessionStatus) {
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == session_id) {
            session.status = status;
        }
    }

    /// Handle Claude process started event
    pub fn handle_claude_started(&mut self, pid: u32) {
        if let Some(session) = self.current_session() {
            let session_id = session.id.clone();
            let _ = self.db.update_session_pid(&session_id, Some(pid));
            let _ = self.db.update_session_status(&session_id, SessionStatus::Running);
            self.update_session_status(&session_id, SessionStatus::Running);
        }
        self.set_status(format!("Claude started (PID: {})", pid));
    }

    /// Handle Claude process exited event
    pub fn handle_claude_exited(&mut self, code: Option<i32>) -> bool {
        if let Some(session) = self.current_session() {
            let session_id = session.id.clone();
            let status = if code == Some(0) {
                SessionStatus::Completed
            } else {
                SessionStatus::Failed
            };
            let _ = self.db.update_session_status(&session_id, status);
            self.update_session_status(&session_id, status);
        }
        self.set_status(format!("Claude exited with code: {:?}", code));
        true // Signal to clear receiver
    }

    /// Handle Claude output event
    pub fn handle_claude_output(&mut self, output_type: crate::models::OutputType, data: String) {
        // Save to database first
        if let Some(session) = self.current_session() {
            let session_id = session.id.clone();
            let _ = self.db.add_session_output(&session_id, output_type, &data);
        }
        self.add_output(data);
    }

    /// Handle Claude error event
    pub fn handle_claude_error(&mut self, error: String) {
        self.add_output(format!("Error: {}", error));
        self.set_status(format!("Error: {}", error));
    }

    /// Create a new session with the given prompt
    pub fn create_new_session(&mut self, prompt: String) -> anyhow::Result<crate::models::Session> {
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

    /// Archive the current session
    pub fn archive_current_session(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_session() {
            let session_id = session.id.clone();
            SessionManager::new(&self.db).archive_session(&session_id)?;
            self.set_status("Session archived");
            self.refresh_sessions()?;
        }
        Ok(())
    }

    /// Get diff for current session
    pub fn load_diff(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_session() {
            if let Some(project) = self.current_project() {
                let diff = Git::get_diff(&session.worktree_path, &project.main_branch)?;
                self.diff_text = Some(diff.diff_text);
                self.view_mode = ViewMode::Diff;
                self.focus = Focus::Output;
                Ok(())
            } else {
                anyhow::bail!("No project selected")
            }
        } else {
            anyhow::bail!("No session selected")
        }
    }

    /// Rebase current session onto main
    pub fn rebase_current_session(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_session() {
            if let Some(project) = self.current_project() {
                Git::rebase_onto_main(&session.worktree_path, &project.main_branch)?;
                self.set_status("Rebased successfully");
                Ok(())
            } else {
                anyhow::bail!("No project selected")
            }
        } else {
            anyhow::bail!("No session selected")
        }
    }

    /// Cycle focus forward
    pub fn focus_next(&mut self) {
        self.focus = match self.focus {
            Focus::Sessions => Focus::Output,
            Focus::Output => Focus::Input,
            Focus::Input => Focus::Sessions,
        };
    }

    /// Cycle focus backward
    pub fn focus_prev(&mut self) {
        self.focus = match self.focus {
            Focus::Sessions => Focus::Input,
            Focus::Output => Focus::Sessions,
            Focus::Input => Focus::Output,
        };
    }

    /// Toggle help overlay
    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }
}

/// Strip ANSI escape sequences from text
fn strip_ansi_escapes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // ESC character - start of ANSI sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Skip until we find a letter (the command character)
                while let Some(&next_ch) = chars.peek() {
                    chars.next();
                    if next_ch.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                // Other escape sequences (less common)
                chars.next();
            }
        } else {
            result.push(ch);
        }
    }

    result
}
