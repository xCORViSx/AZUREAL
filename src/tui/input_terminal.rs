//! Terminal and Claude prompt input handling

use anyhow::Result;
use crossterm::event::{self, KeyCode, KeyModifiers};

use crate::app::{App, Focus};
use crate::claude::ClaudeProcess;

/// Handle keyboard input when Input field is focused (terminal mode or Claude prompt)
pub fn handle_input_mode(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    // PTY Terminal mode - forward keys directly to shell
    if app.terminal_mode {
        if app.insert_mode {
            // Insert mode: send keystrokes to PTY
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.insert_mode = false;
                    app.scroll_terminal_to_bottom();
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => app.write_to_terminal(&[0x03]),
                (KeyModifiers::CONTROL, KeyCode::Char('d')) => app.write_to_terminal(&[0x04]),
                (KeyModifiers::CONTROL, KeyCode::Char('z')) => app.write_to_terminal(&[0x1a]),
                (KeyModifiers::CONTROL, KeyCode::Char('l')) => app.write_to_terminal(&[0x0c]),
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                    let mut buf = [0u8; 4];
                    let s = c.encode_utf8(&mut buf);
                    app.write_to_terminal(s.as_bytes());
                }
                (KeyModifiers::NONE, KeyCode::Enter) => app.write_to_terminal(b"\n"),
                (KeyModifiers::NONE, KeyCode::Backspace) => app.write_to_terminal(&[0x7f]),
                (KeyModifiers::NONE, KeyCode::Tab) => app.write_to_terminal(b"\t"),
                (KeyModifiers::NONE, KeyCode::Up) => app.write_to_terminal(b"\x1b[A"),
                (KeyModifiers::NONE, KeyCode::Down) => app.write_to_terminal(b"\x1b[B"),
                (KeyModifiers::NONE, KeyCode::Right) => app.write_to_terminal(b"\x1b[C"),
                (KeyModifiers::NONE, KeyCode::Left) => app.write_to_terminal(b"\x1b[D"),
                (KeyModifiers::NONE, KeyCode::Home) => app.write_to_terminal(b"\x1b[H"),
                (KeyModifiers::NONE, KeyCode::End) => app.write_to_terminal(b"\x1b[F"),
                (KeyModifiers::NONE, KeyCode::Delete) => app.write_to_terminal(b"\x1b[3~"),
                _ => {}
            }
        } else {
            // Command mode: scrolling and mode switches
            match key.code {
                KeyCode::Char('i') => {
                    app.insert_mode = true;
                    app.scroll_terminal_to_bottom();
                }
                KeyCode::Char('t') => app.close_terminal(),
                KeyCode::Esc => app.focus = Focus::Worktrees,
                KeyCode::Char('+') | KeyCode::Char('=') => app.adjust_terminal_height(2),
                KeyCode::Char('-') => app.adjust_terminal_height(-2),
                KeyCode::Char('k') | KeyCode::Up => app.scroll_terminal_up(1),
                KeyCode::Char('j') | KeyCode::Down => app.scroll_terminal_down(1),
                KeyCode::Char('K') => app.scroll_terminal_up(10),
                KeyCode::Char('J') => app.scroll_terminal_down(10),
                KeyCode::Char('g') => {
                    app.terminal_scroll = 10000;
                    app.terminal_parser.screen_mut().set_scrollback(10000);
                    app.terminal_scroll = app.terminal_parser.screen().scrollback();
                }
                KeyCode::Char('G') => app.scroll_terminal_to_bottom(),
                _ => {}
            }
        }
        return Ok(());
    }

    // Non-terminal: vim-style insert mode check
    if !app.insert_mode {
        match key.code {
            KeyCode::Char('i') => app.insert_mode = true,
            KeyCode::Esc => app.focus = Focus::Worktrees,
            _ => {}
        }
        return Ok(());
    }

    // Claude prompt mode - handle text editing
    match (key.modifiers, key.code) {
        (_, KeyCode::Esc) => app.insert_mode = false,
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => app.input_char(c),
        (KeyModifiers::NONE, KeyCode::Backspace) => app.input_backspace(),
        (KeyModifiers::NONE, KeyCode::Delete) => app.input_delete(),
        (KeyModifiers::NONE, KeyCode::Left) => app.input_left(),
        (KeyModifiers::NONE, KeyCode::Right) => app.input_right(),
        (KeyModifiers::NONE, KeyCode::Home) => app.input_home(),
        (KeyModifiers::NONE, KeyCode::End) => app.input_end(),
        (KeyModifiers::CONTROL, KeyCode::Left) | (KeyModifiers::ALT, KeyCode::Left) => app.input_word_left(),
        (KeyModifiers::CONTROL, KeyCode::Right) | (KeyModifiers::ALT, KeyCode::Right) => app.input_word_right(),
        (KeyModifiers::CONTROL, KeyCode::Backspace) | (KeyModifiers::CONTROL, KeyCode::Char('w')) => app.input_delete_word(),
        (KeyModifiers::NONE, KeyCode::Enter) => {
            if !app.input.is_empty() {
                let input = app.input.clone();
                app.clear_input();

                // Get session info: branch_name and worktree_path
                let session_data = app.current_session().map(|s| (s.branch_name.clone(), s.worktree_path.clone()));

                if let Some((branch_name, worktree_opt)) = session_data {
                    if let Some(wt_path) = worktree_opt {
                        if app.is_session_running(&branch_name) {
                            app.set_status("Claude already running - wait for response");
                            app.input = input;
                            app.input_cursor = app.input.len();
                        } else {
                            // Display user prompt (Claude's session files store the actual messages)
                            let prompt_text = format!("You: {}\n", input.clone());
                            app.add_user_message(input.clone());
                            app.process_output_chunk(&prompt_text);

                            let resume_id = app.get_claude_session_id(&branch_name).cloned();

                            match claude_process.spawn(&wt_path, &input, resume_id.as_deref()) {
                                Ok(rx) => {
                                    app.register_claude(branch_name, rx);
                                    app.set_status("Running...");
                                }
                                Err(e) => app.set_status(format!("Failed to start: {}", e)),
                            }
                        }
                    } else {
                        app.set_status("Session has no worktree (archived?)");
                        app.input = input;
                        app.input_cursor = app.input.len();
                    }
                } else {
                    app.set_status("Select a session first");
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Handle keyboard input when session creation modal is focused
pub fn handle_session_creation_input(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Enter) => {
            if !app.session_creation_input.is_empty() {
                let prompt = app.session_creation_input.clone();
                app.exit_session_creation_mode();

                match app.create_new_session(prompt.clone()) {
                    Ok(session) => {
                        let branch_name = session.branch_name.clone();
                        app.set_status(format!("Created session: {}", session.name()));

                        if let Some(ref wt_path) = session.worktree_path {
                            match claude_process.spawn(wt_path, &prompt, None) {
                                Ok(rx) => app.register_claude(branch_name, rx),
                                Err(e) => app.set_status(format!("Failed to start: {}", e)),
                            }
                        }
                    }
                    Err(e) => app.set_status(format!("Failed to create session: {}", e)),
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Enter) => app.session_creation_char('\n'),
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => app.session_creation_char(c),
        (_, KeyCode::Backspace) => app.session_creation_backspace(),
        (_, KeyCode::Delete) => app.session_creation_delete(),
        (_, KeyCode::Left) => app.session_creation_left(),
        (_, KeyCode::Right) => app.session_creation_right(),
        (_, KeyCode::Home) => app.session_creation_home(),
        (_, KeyCode::End) => app.session_creation_end(),
        (_, KeyCode::Esc) => app.exit_session_creation_mode(),
        _ => {}
    }
    Ok(())
}
