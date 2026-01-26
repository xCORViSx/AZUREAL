use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use crate::claude::ClaudeEvent;
use crate::db::Database;
use crate::models::{Project, Session, SessionStatus};
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
        self.output_scroll = 0;

        if let Some(session) = self.current_session() {
            if let Ok(outputs) = self.db.get_session_outputs(&session.id) {
                for output in outputs {
                    self.output_lines.push_back(output.data);
                    if self.output_lines.len() > self.max_output_lines {
                        self.output_lines.pop_front();
                    }
                }
            }
        }
    }

    /// Add output line
    pub fn add_output(&mut self, line: String) {
        self.output_lines.push_back(line);
        if self.output_lines.len() > self.max_output_lines {
            self.output_lines.pop_front();
        }
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
}
