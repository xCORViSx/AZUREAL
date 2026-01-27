//! Terminal PTY management for App

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use super::App;

impl App {
    /// Open terminal with PTY shell in session's worktree
    pub fn open_terminal(&mut self) {
        if self.terminal_pty.is_some() { return; }

        let cwd = self.current_session()
            .map(|s| s.worktree_path.clone())
            .or_else(|| self.projects.get(self.selected_project).map(|p| p.path.clone()))
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
        self.insert_mode = true;
    }

    /// Close terminal PTY
    pub fn close_terminal(&mut self) {
        self.terminal_writer = None;
        self.terminal_pty = None;
        self.terminal_rx = None;
        self.terminal_mode = false;
        self.insert_mode = false;
    }

    /// Write bytes to terminal PTY
    pub fn write_to_terminal(&mut self, data: &[u8]) {
        if let Some(ref mut writer) = self.terminal_writer {
            let _ = writer.write_all(data);
            let _ = writer.flush();
        }
    }

    /// Poll terminal for new output
    pub fn poll_terminal(&mut self) {
        if let Some(ref rx) = self.terminal_rx {
            let was_at_bottom = self.terminal_scroll == 0;
            while let Ok(data) = rx.try_recv() {
                self.terminal_parser.process(&data);
            }
            if was_at_bottom {
                self.terminal_scroll = 0;
                self.terminal_parser.screen_mut().set_scrollback(0);
            }
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
}
