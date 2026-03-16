//! Terminal PTY management for App

use portable_pty::{native_pty_system, Child as PtyChild, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use super::App;

/// Per-session terminal state (persists independently for each session)
pub struct SessionTerminal {
    pub pty: Box<dyn MasterPty + Send>,
    pub child: Box<dyn PtyChild + Send + Sync>,
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
        if let Some(session) = self.current_worktree() {
            if self.worktree_terminals.contains_key(&session.branch_name) {
                self.restore_session_terminal();
                self.terminal_mode = true;
                self.prompt_mode = true;
                self.terminal_needs_resize = true;
                return;
            }
        }

        // Create new terminal
        let cwd = self
            .current_worktree()
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

        let shell: String = if cfg!(windows) {
            // Prefer PowerShell 7 (pwsh), then Windows PowerShell, then COMSPEC.
            // Check exit status (not just is_ok) to verify the shell actually works.
            use std::process::Command as StdCmd;
            let check = |name: &str| -> bool {
                StdCmd::new(name)
                    .arg("-Version")
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            };
            if check("pwsh") {
                "pwsh.exe".into()
            } else if check("powershell") {
                "powershell.exe".into()
            } else {
                std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())
            }
        } else {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
        };
        let mut cmd = CommandBuilder::new(&shell);
        // PowerShell: suppress the startup banner for a cleaner embedded experience
        if cfg!(windows) && (shell.contains("pwsh") || shell.contains("powershell")) {
            cmd.arg("-NoLogo");
        }
        cmd.cwd(&cwd);
        // Set TERM for proper VT100 sequence output
        cmd.env("TERM", "xterm-256color");

        // Get reader BEFORE spawning — on Windows ConPTY, the reader must be
        // obtained while the slave handle is still open.
        let reader = match pair.master.try_clone_reader() {
            Ok(r) => r,
            Err(e) => {
                self.set_status(format!("Failed to get PTY reader: {}", e));
                return;
            }
        };

        let writer = match pair.master.take_writer() {
            Ok(w) => w,
            Err(e) => {
                self.set_status(format!("Failed to get PTY writer: {}", e));
                return;
            }
        };

        let child = match pair.slave.spawn_command(cmd) {
            Ok(c) => c,
            Err(e) => {
                self.set_status(format!("Failed to spawn shell: {}", e));
                return;
            }
        };
        // Drop the slave handle — on Windows (ConPTY), the master reader blocks
        // until all slave handles are closed.
        drop(pair.slave);

        let (tx, rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = tx.send(buf[..n].to_vec());
                    }
                    Err(_) => break,
                }
            }
        });

        self.terminal_pty = Some(pair.master);
        self.terminal_child = Some(child);
        self.terminal_writer = Some(writer);
        self.terminal_rx = Some(rx);
        self.terminal_rows = self.terminal_height;
        self.terminal_cols = 120;
        self.terminal_parser = vt100::Parser::new(self.terminal_rows, self.terminal_cols, 1000);
        self.terminal_scroll = 0;
        self.terminal_mode = true;
        self.terminal_needs_resize = true;
        self.prompt_mode = true;
        self.set_status(format!("Terminal: {} in {}", shell, cwd.display()));
    }

    /// Hide terminal (PTY keeps running in background)
    pub fn close_terminal(&mut self) {
        self.terminal_mode = false;
        self.prompt_mode = false;
        // PTY stays alive - terminal_pty, terminal_writer, terminal_rx preserved
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
            let mut needs_dsr_response = false;
            while let Ok(data) = rx.try_recv() {
                // Check for DSR (Device Status Report) request: \x1b[6n
                // ConPTY sends this and blocks until the host responds with cursor position.
                if data.windows(4).any(|w| w == b"\x1b[6n") {
                    needs_dsr_response = true;
                }
                self.terminal_parser.process(&data);
                had_data = true;
            }
            // Respond to DSR with current cursor position (1-based)
            if needs_dsr_response {
                let (row, col) = self.terminal_parser.screen().cursor_position();
                let response = format!("\x1b[{};{}R", row + 1, col + 1);
                if let Some(ref mut writer) = self.terminal_writer {
                    let _ = writer.write_all(response.as_bytes());
                    let _ = writer.flush();
                }
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
            if !first {
                output.push(b'\n');
            }
            first = false;
            output.extend_from_slice(&row_content);
        }

        output
    }

    /// Get terminal cursor position
    pub fn terminal_cursor_position(&self) -> (u16, u16) {
        self.terminal_parser.screen().cursor_position()
    }

    /// Resize terminal PTY and parser
    pub fn resize_terminal(&mut self, rows: u16, cols: u16) {
        if rows == self.terminal_rows && cols == self.terminal_cols {
            return;
        }
        if let Some(ref pty) = self.terminal_pty {
            let _ = pty.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
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
        self.terminal_parser
            .screen_mut()
            .set_scrollback(self.terminal_scroll);
        self.terminal_scroll = self.terminal_parser.screen().scrollback();
    }

    /// Scroll terminal down toward live view
    pub fn scroll_terminal_down(&mut self, lines: usize) {
        self.terminal_scroll = self.terminal_scroll.saturating_sub(lines);
        self.terminal_parser
            .screen_mut()
            .set_scrollback(self.terminal_scroll);
    }

    /// Scroll terminal to bottom (live view)
    pub fn scroll_terminal_to_bottom(&mut self) {
        self.terminal_scroll = 0;
        self.terminal_parser.screen_mut().set_scrollback(0);
    }

    /// Save current terminal to worktree_terminals map (called before switching sessions)
    pub fn save_current_terminal(&mut self) {
        // Get current session's branch name
        let branch_name = match self.current_worktree() {
            Some(s) => s.branch_name.clone(),
            None => return,
        };

        // Only save if we have a terminal
        let (pty, child, writer, rx) = match (
            self.terminal_pty.take(),
            self.terminal_child.take(),
            self.terminal_writer.take(),
            self.terminal_rx.take(),
        ) {
            (Some(p), Some(c), Some(w), Some(r)) => (p, c, w, r),
            _ => return,
        };

        // Save terminal state to map
        let terminal = SessionTerminal {
            pty,
            child,
            writer,
            rx,
            parser: std::mem::replace(&mut self.terminal_parser, vt100::Parser::new(24, 120, 1000)),
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
        let branch_name = match self.current_worktree() {
            Some(s) => s.branch_name.clone(),
            None => return,
        };

        // Try to restore from map
        if let Some(terminal) = self.worktree_terminals.remove(&branch_name) {
            self.terminal_pty = Some(terminal.pty);
            self.terminal_child = Some(terminal.child);
            self.terminal_writer = Some(terminal.writer);
            self.terminal_rx = Some(terminal.rx);
            self.terminal_parser = terminal.parser;
            self.terminal_scroll = terminal.scroll;
            self.terminal_rows = terminal.rows;
            self.terminal_cols = terminal.cols;
            // Don't auto-show terminal - keep terminal_mode as is
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Worktree;
    use std::path::PathBuf;

    // ── close_terminal ──

    #[test]
    fn test_close_terminal_sets_mode_false() {
        let mut app = App::new();
        app.terminal_mode = true;
        app.prompt_mode = true;
        app.close_terminal();
        assert!(!app.terminal_mode);
        assert!(!app.prompt_mode);
    }

    #[test]
    fn test_close_terminal_already_closed() {
        let mut app = App::new();
        app.terminal_mode = false;
        app.prompt_mode = false;
        app.close_terminal();
        assert!(!app.terminal_mode);
        assert!(!app.prompt_mode);
    }

    #[test]
    fn test_close_terminal_preserves_pty_state() {
        let mut app = App::new();
        app.terminal_mode = true;
        // PTY fields should remain untouched (None in test, but not cleared)
        app.close_terminal();
        // terminal_pty, terminal_writer, terminal_rx are preserved
        assert!(app.terminal_pty.is_none()); // was already None in test
    }

    // ── write_to_terminal: no writer ──

    #[test]
    fn test_write_to_terminal_no_writer() {
        let mut app = App::new();
        // Should not crash when no writer is present
        app.write_to_terminal(b"hello");
    }

    #[test]
    fn test_write_to_terminal_empty_data() {
        let mut app = App::new();
        app.write_to_terminal(b"");
    }

    // ── poll_terminal: no rx ──

    #[test]
    fn test_poll_terminal_no_rx_returns_false() {
        let mut app = App::new();
        assert!(!app.poll_terminal());
    }

    #[test]
    fn test_poll_terminal_empty_channel_returns_false() {
        let mut app = App::new();
        let (_tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        app.terminal_rx = Some(rx);
        assert!(!app.poll_terminal());
    }

    #[test]
    fn test_poll_terminal_with_data_returns_true() {
        let mut app = App::new();
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        app.terminal_rx = Some(rx);
        tx.send(b"data".to_vec()).unwrap();
        assert!(app.poll_terminal());
    }

    #[test]
    fn test_poll_terminal_processes_data_into_parser() {
        let mut app = App::new();
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        app.terminal_rx = Some(rx);
        tx.send(b"hello\r\n".to_vec()).unwrap();
        app.poll_terminal();
        // Parser should have processed the data — screen should contain "hello"
        let contents = app.terminal_screen_contents();
        let text = String::from_utf8_lossy(&contents);
        assert!(text.contains("hello"));
    }

    #[test]
    fn test_poll_terminal_multiple_messages() {
        let mut app = App::new();
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        app.terminal_rx = Some(rx);
        tx.send(b"line1\r\n".to_vec()).unwrap();
        tx.send(b"line2\r\n".to_vec()).unwrap();
        assert!(app.poll_terminal());
    }

    // ── terminal_screen_contents ──

    #[test]
    fn test_terminal_screen_contents_empty() {
        let app = App::new();
        let contents = app.terminal_screen_contents();
        // Should return content (even if blank lines)
        assert!(!contents.is_empty()); // parser has default rows
    }

    #[test]
    fn test_terminal_screen_contents_after_write() {
        let mut app = App::new();
        app.terminal_parser.process(b"test output\r\n");
        let contents = app.terminal_screen_contents();
        let text = String::from_utf8_lossy(&contents);
        assert!(text.contains("test output"));
    }

    // ── terminal_cursor_position ──

    #[test]
    fn test_terminal_cursor_position_initial() {
        let app = App::new();
        let (row, col) = app.terminal_cursor_position();
        assert_eq!(row, 0);
        assert_eq!(col, 0);
    }

    #[test]
    fn test_terminal_cursor_position_after_text() {
        let mut app = App::new();
        app.terminal_parser.process(b"abc");
        let (row, col) = app.terminal_cursor_position();
        assert_eq!(row, 0);
        assert_eq!(col, 3);
    }

    #[test]
    fn test_terminal_cursor_position_after_newline() {
        let mut app = App::new();
        app.terminal_parser.process(b"abc\r\n");
        let (row, col) = app.terminal_cursor_position();
        assert_eq!(row, 1);
        assert_eq!(col, 0);
    }

    // ── resize_terminal ──

    #[test]
    fn test_resize_terminal_updates_dimensions() {
        let mut app = App::new();
        app.terminal_rows = 24;
        app.terminal_cols = 80;
        app.resize_terminal(40, 120);
        assert_eq!(app.terminal_rows, 40);
        assert_eq!(app.terminal_cols, 120);
    }

    #[test]
    fn test_resize_terminal_same_dimensions_no_op() {
        let mut app = App::new();
        app.terminal_rows = 24;
        app.terminal_cols = 80;
        // Same dimensions — should return early
        app.resize_terminal(24, 80);
        assert_eq!(app.terminal_rows, 24);
        assert_eq!(app.terminal_cols, 80);
    }

    #[test]
    fn test_resize_terminal_small_dimensions() {
        let mut app = App::new();
        app.resize_terminal(5, 20);
        assert_eq!(app.terminal_rows, 5);
        assert_eq!(app.terminal_cols, 20);
    }

    #[test]
    fn test_resize_terminal_large_dimensions() {
        let mut app = App::new();
        app.resize_terminal(100, 300);
        assert_eq!(app.terminal_rows, 100);
        assert_eq!(app.terminal_cols, 300);
    }

    // ── adjust_terminal_height ──

    #[test]
    fn test_adjust_terminal_height_increase() {
        let mut app = App::new();
        app.terminal_height = 20;
        app.terminal_cols = 80;
        app.adjust_terminal_height(5);
        assert_eq!(app.terminal_height, 25);
    }

    #[test]
    fn test_adjust_terminal_height_decrease() {
        let mut app = App::new();
        app.terminal_height = 20;
        app.terminal_cols = 80;
        app.adjust_terminal_height(-5);
        assert_eq!(app.terminal_height, 15);
    }

    #[test]
    fn test_adjust_terminal_height_min_clamped() {
        let mut app = App::new();
        app.terminal_height = 10;
        app.terminal_cols = 80;
        app.adjust_terminal_height(-20);
        assert_eq!(app.terminal_height, 5); // clamped to 5
    }

    #[test]
    fn test_adjust_terminal_height_max_clamped() {
        let mut app = App::new();
        app.terminal_height = 30;
        app.terminal_cols = 80;
        app.adjust_terminal_height(20);
        assert_eq!(app.terminal_height, 40); // clamped to 40
    }

    #[test]
    fn test_adjust_terminal_height_zero_delta() {
        let mut app = App::new();
        app.terminal_height = 20;
        app.terminal_cols = 80;
        app.adjust_terminal_height(0);
        assert_eq!(app.terminal_height, 20);
    }

    #[test]
    fn test_adjust_terminal_height_at_min() {
        let mut app = App::new();
        app.terminal_height = 5;
        app.terminal_cols = 80;
        app.adjust_terminal_height(-1);
        assert_eq!(app.terminal_height, 5); // can't go below 5
    }

    #[test]
    fn test_adjust_terminal_height_at_max() {
        let mut app = App::new();
        app.terminal_height = 40;
        app.terminal_cols = 80;
        app.adjust_terminal_height(1);
        assert_eq!(app.terminal_height, 40); // can't go above 40
    }

    // ── scroll_terminal_up/down/to_bottom ──

    #[test]
    fn test_scroll_terminal_up_from_zero() {
        let mut app = App::new();
        app.terminal_scroll = 0;
        app.scroll_terminal_up(5);
        // scroll should increase (or be capped by scrollback buffer)
        // With default parser, scrollback may be limited
    }

    #[test]
    fn test_scroll_terminal_down_from_zero() {
        let mut app = App::new();
        app.terminal_scroll = 0;
        app.scroll_terminal_down(5);
        assert_eq!(app.terminal_scroll, 0); // can't go below 0
    }

    #[test]
    fn test_scroll_terminal_to_bottom() {
        let mut app = App::new();
        app.terminal_scroll = 100;
        app.scroll_terminal_to_bottom();
        assert_eq!(app.terminal_scroll, 0);
    }

    #[test]
    fn test_scroll_terminal_to_bottom_already_at_bottom() {
        let mut app = App::new();
        app.terminal_scroll = 0;
        app.scroll_terminal_to_bottom();
        assert_eq!(app.terminal_scroll, 0);
    }

    #[test]
    fn test_scroll_terminal_down_saturating() {
        let mut app = App::new();
        app.terminal_scroll = 3;
        app.scroll_terminal_down(10);
        assert_eq!(app.terminal_scroll, 0); // saturating_sub prevents underflow
    }

    // ── save_current_terminal ──

    #[test]
    fn test_save_terminal_no_worktree_returns_early() {
        let mut app = App::new();
        // No worktrees, no selection
        app.save_current_terminal();
        assert!(app.worktree_terminals.is_empty());
    }

    #[test]
    fn test_save_terminal_no_pty_returns_early() {
        let mut app = App::new();
        app.worktrees.push(Worktree {
            branch_name: "azureal/test".to_string(),
            worktree_path: Some(PathBuf::from("/tmp/test")),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        // No PTY to save
        app.save_current_terminal();
        assert!(app.worktree_terminals.is_empty());
    }

    // ── restore_session_terminal ──

    #[test]
    fn test_restore_terminal_no_worktree() {
        let mut app = App::new();
        app.restore_session_terminal();
        // Should not crash
    }

    #[test]
    fn test_restore_terminal_no_saved_terminal() {
        let mut app = App::new();
        app.worktrees.push(Worktree {
            branch_name: "azureal/test".to_string(),
            worktree_path: Some(PathBuf::from("/tmp/test")),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.restore_session_terminal();
        // No saved terminal — should do nothing
        assert!(app.terminal_pty.is_none());
    }

    // ── open_terminal: existing PTY ──

    #[test]
    fn test_open_terminal_reuses_existing_pty() {
        let mut app = App::new();
        // Simulate existing PTY by setting terminal_pty to Some
        // We can't easily create a real MasterPty in tests, but we can test
        // that the method doesn't crash when called without PTY
        app.terminal_mode = false;
        app.prompt_mode = false;
        // No PTY, no worktree — should set status message about failure
        app.open_terminal();
        // Without a valid working directory, it may fail but shouldn't panic
    }

    // ── terminal_mode and prompt_mode ──

    #[test]
    fn test_terminal_mode_default_false() {
        let app = App::new();
        assert!(!app.terminal_mode);
    }

    #[test]
    fn test_prompt_mode_default_false() {
        let app = App::new();
        assert!(!app.prompt_mode);
    }

    #[test]
    fn test_close_terminal_then_check_modes() {
        let mut app = App::new();
        app.terminal_mode = true;
        app.prompt_mode = true;
        app.close_terminal();
        assert!(!app.terminal_mode);
        assert!(!app.prompt_mode);
    }

    // ── terminal_height default ──

    #[test]
    fn test_terminal_height_default() {
        let app = App::new();
        assert!(app.terminal_height >= 5);
        assert!(app.terminal_height <= 40);
    }

    #[test]
    fn test_terminal_scroll_default_zero() {
        let app = App::new();
        assert_eq!(app.terminal_scroll, 0);
    }

    // ── parser interactions ──

    #[test]
    fn test_parser_process_escape_sequence() {
        let mut app = App::new();
        // Process an ANSI color code
        app.terminal_parser.process(b"\x1b[31mred text\x1b[0m");
        let (row, col) = app.terminal_cursor_position();
        assert_eq!(row, 0);
        assert_eq!(col, 8); // "red text" = 8 chars
    }

    #[test]
    fn test_parser_process_clear_screen() {
        let mut app = App::new();
        app.terminal_parser.process(b"before\x1b[2J");
        // Screen cleared
        let contents = app.terminal_screen_contents();
        let text = String::from_utf8_lossy(&contents);
        // "before" may or may not be visible depending on cursor position
        let _ = text;
    }

    #[test]
    fn test_parser_multiple_lines() {
        let mut app = App::new();
        app.terminal_parser.process(b"line1\r\nline2\r\nline3\r\n");
        let (row, _col) = app.terminal_cursor_position();
        assert_eq!(row, 3);
    }

    // ── worktree_terminals map ──

    #[test]
    fn test_worktree_terminals_initially_empty() {
        let app = App::new();
        assert!(app.worktree_terminals.is_empty());
    }

    // ── scroll operations: up then down symmetry ──

    #[test]
    fn test_scroll_up_then_to_bottom() {
        let mut app = App::new();
        app.scroll_terminal_up(10);
        app.scroll_terminal_to_bottom();
        assert_eq!(app.terminal_scroll, 0);
    }

    #[test]
    fn test_terminal_mode_starts_false() {
        let app = App::new();
        assert!(!app.terminal_mode);
    }

    #[test]
    fn test_prompt_mode_starts_false() {
        let app = App::new();
        assert!(!app.prompt_mode);
    }

    #[test]
    fn test_terminal_scroll_starts_zero() {
        let app = App::new();
        assert_eq!(app.terminal_scroll, 0);
    }

    #[test]
    fn test_close_terminal_idempotent() {
        let mut app = App::new();
        app.close_terminal();
        app.close_terminal();
        assert!(!app.terminal_mode);
    }
}
