//! Terminal PTY management for App

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use super::App;

/// Per-session terminal state (persists independently for each session)
pub struct SessionTerminal {
    pub pty: Box<dyn MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub rx: Receiver<Vec<u8>>,
    pub parser: vt100::Parser,
    pub scroll: usize,
    pub rows: u16,
    pub cols: u16,
}

impl App {
    /// Open terminal with PTY shell in session's worktree
    pub fn open_terminal(&mut self) {
        // If PTY already exists (active), just show it
        if self.terminal_pty.is_some() {
            self.terminal_mode = true;
            self.prompt_mode = true;
            self.terminal_needs_resize = true;
            return;
        }

        // Check if this session has a saved terminal
        if let Some(session) = self.current_session() {
            if self.worktree_terminals.contains_key(&session.branch_name) {
                self.restore_session_terminal();
                self.terminal_mode = true;
                self.prompt_mode = true;
                self.terminal_needs_resize = true;
                return;
            }
        }

        // Create new terminal
        let cwd = self.current_session()
            .and_then(|s| s.worktree_path.clone())
            .or_else(|| self.project.as_ref().map(|p| p.path.clone()))
            .unwrap_or_else(|| std::env::current_dir().unwrap());

        let pty_system = native_pty_system();
        let pair = match pty_system.openpty(PtySize {
            rows: self.terminal_height,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        }) {
            Ok(p) => p,
            Err(e) => {
                self.set_status(format!("Failed to create PTY: {}", e));
                return;
            }
        };

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.cwd(&cwd);

        if let Err(e) = pair.slave.spawn_command(cmd) {
            self.set_status(format!("Failed to spawn shell: {}", e));
            return;
        }

        let writer = match pair.master.take_writer() {
            Ok(w) => w,
            Err(e) => {
                self.set_status(format!("Failed to get PTY writer: {}", e));
                return;
            }
        };

        let (tx, rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel();
        if let Ok(mut reader) = pair.master.try_clone_reader() {
            thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => { let _ = tx.send(buf[..n].to_vec()); }
                        Err(_) => break,
                    }
                }
            });
        }

        self.terminal_pty = Some(pair.master);
        self.terminal_writer = Some(writer);
        self.terminal_rx = Some(rx);
        self.terminal_rows = self.terminal_height;
        self.terminal_cols = 120;
        self.terminal_parser = vt100::Parser::new(self.terminal_rows, self.terminal_cols, 1000);
        self.terminal_scroll = 0;
        self.terminal_mode = true;
        self.terminal_needs_resize = true;
        self.prompt_mode = true;
    }

    /// Hide terminal (PTY keeps running in background)
    pub fn close_terminal(&mut self) {
        self.terminal_mode = false;
        self.prompt_mode = false;
        // PTY stays alive - terminal_pty, terminal_writer, terminal_rx preserved
    }

    /// Fully destroy terminal PTY (use when switching sessions or quitting)
    pub fn destroy_terminal(&mut self) {
        self.terminal_writer = None;
        self.terminal_pty = None;
        self.terminal_rx = None;
        self.terminal_mode = false;
        self.prompt_mode = false;
    }

    /// Write bytes to terminal PTY
    pub fn write_to_terminal(&mut self, data: &[u8]) {
        if let Some(ref mut writer) = self.terminal_writer {
            let _ = writer.write_all(data);
            let _ = writer.flush();
        }
    }

    /// Poll terminal for new output. Returns true if there was data.
    pub fn poll_terminal(&mut self) -> bool {
        if let Some(ref rx) = self.terminal_rx {
            let was_at_bottom = self.terminal_scroll == 0;
            let mut had_data = false;
            while let Ok(data) = rx.try_recv() {
                self.terminal_parser.process(&data);
                had_data = true;
            }
            if was_at_bottom {
                self.terminal_scroll = 0;
                self.terminal_parser.screen_mut().set_scrollback(0);
            }
            had_data
        } else {
            false
        }
    }

    /// Get terminal screen contents with ANSI formatting
    /// Uses row-by-row rendering to ensure proper line separation
    pub fn terminal_screen_contents(&self) -> Vec<u8> {
        let screen = self.terminal_parser.screen();
        let (_, cols) = screen.size();
        let mut output = Vec::new();
        let mut first = true;

        for row_content in screen.rows_formatted(0, cols) {
            if !first { output.push(b'\n'); }
            first = false;
            output.extend_from_slice(&row_content);
        }

        output
    }

    /// Get total scrollback lines
    pub fn terminal_scrollback_len(&self) -> usize {
        self.terminal_parser.screen().scrollback()
    }

    /// Get terminal cursor position
    pub fn terminal_cursor_position(&self) -> (u16, u16) {
        self.terminal_parser.screen().cursor_position()
    }

    /// Resize terminal PTY and parser
    pub fn resize_terminal(&mut self, rows: u16, cols: u16) {
        if rows == self.terminal_rows && cols == self.terminal_cols { return; }
        if let Some(ref pty) = self.terminal_pty {
            let _ = pty.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
        self.terminal_parser.screen_mut().set_size(rows, cols);
        self.terminal_rows = rows;
        self.terminal_cols = cols;
    }

    /// Adjust terminal height
    pub fn adjust_terminal_height(&mut self, delta: i16) {
        let new_height = (self.terminal_height as i16 + delta).max(5).min(40) as u16;
        self.terminal_height = new_height;
        self.resize_terminal(new_height, self.terminal_cols);
    }

    /// Scroll terminal up into history
    pub fn scroll_terminal_up(&mut self, lines: usize) {
        self.terminal_scroll = self.terminal_scroll.saturating_add(lines);
        self.terminal_parser.screen_mut().set_scrollback(self.terminal_scroll);
        self.terminal_scroll = self.terminal_parser.screen().scrollback();
    }

    /// Scroll terminal down toward live view
    pub fn scroll_terminal_down(&mut self, lines: usize) {
        self.terminal_scroll = self.terminal_scroll.saturating_sub(lines);
        self.terminal_parser.screen_mut().set_scrollback(self.terminal_scroll);
    }

    /// Scroll terminal to bottom (live view)
    pub fn scroll_terminal_to_bottom(&mut self) {
        self.terminal_scroll = 0;
        self.terminal_parser.screen_mut().set_scrollback(0);
    }

    /// Save current terminal to worktree_terminals map (called before switching sessions)
    pub fn save_current_terminal(&mut self) {
        // Get current session's branch name
        let branch_name = match self.current_session() {
            Some(s) => s.branch_name.clone(),
            None => return,
        };

        // Only save if we have a terminal
        let (pty, writer, rx) = match (
            self.terminal_pty.take(),
            self.terminal_writer.take(),
            self.terminal_rx.take(),
        ) {
            (Some(p), Some(w), Some(r)) => (p, w, r),
            _ => return,
        };

        // Save terminal state to map
        let terminal = SessionTerminal {
            pty,
            writer,
            rx,
            parser: std::mem::replace(
                &mut self.terminal_parser,
                vt100::Parser::new(24, 120, 1000),
            ),
            scroll: self.terminal_scroll,
            rows: self.terminal_rows,
            cols: self.terminal_cols,
        };
        self.worktree_terminals.insert(branch_name, terminal);

        // Reset current terminal state
        self.terminal_scroll = 0;
        self.terminal_mode = false;
    }

    /// Restore terminal for current session from worktree_terminals map
    pub fn restore_session_terminal(&mut self) {
        let branch_name = match self.current_session() {
            Some(s) => s.branch_name.clone(),
            None => return,
        };

        // Try to restore from map
        if let Some(terminal) = self.worktree_terminals.remove(&branch_name) {
            self.terminal_pty = Some(terminal.pty);
            self.terminal_writer = Some(terminal.writer);
            self.terminal_rx = Some(terminal.rx);
            self.terminal_parser = terminal.parser;
            self.terminal_scroll = terminal.scroll;
            self.terminal_rows = terminal.rows;
            self.terminal_cols = terminal.cols;
            // Don't auto-show terminal - keep terminal_mode as is
        }
    }

    /// Check if current session has a terminal (saved or active)
    pub fn session_has_terminal(&self) -> bool {
        if self.terminal_pty.is_some() {
            return true;
        }
        if let Some(session) = self.current_session() {
            return self.worktree_terminals.contains_key(&session.branch_name);
        }
        false
    }
}
