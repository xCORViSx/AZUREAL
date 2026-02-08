//! Terminal and Claude prompt input handling

use anyhow::Result;
use crossterm::event::{self, KeyCode, KeyModifiers};

use crate::app::{App, Focus};
use crate::claude::ClaudeProcess;

/// Handle keyboard input when Input field is focused (terminal mode or Claude prompt)
pub fn handle_input_mode(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    // PTY Terminal mode - forward keys directly to shell
    if app.terminal_mode {
        if app.prompt_mode {
            // Type mode: send keystrokes to PTY
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    app.prompt_mode = false;
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
                KeyCode::Char('t') => {
                    // Enter type mode (not close terminal - that's Esc now)
                    app.prompt_mode = true;
                    app.scroll_terminal_to_bottom();
                }
                KeyCode::Char('p') => {
                    // Close terminal and enter Claude prompt
                    app.close_terminal();
                    app.focus = Focus::Input;
                    app.prompt_mode = true;
                }
                KeyCode::Esc => app.close_terminal(),
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

    // Non-terminal: vim-style prompt mode check
    if !app.prompt_mode {
        match key.code {
            KeyCode::Char('p') => app.prompt_mode = true,
            KeyCode::Esc => app.focus = Focus::Worktrees,
            _ => {}
        }
        return Ok(());
    }

    // Claude prompt mode - handle text editing
    // Clipboard operations (Cmd/Ctrl+C/X/V/A) - handle BEFORE character input
    match (key.modifiers, key.code) {
        (KeyModifiers::SUPER, KeyCode::Char('c')) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            app.input_copy();
            return Ok(());
        }
        (KeyModifiers::SUPER, KeyCode::Char('x')) | (KeyModifiers::CONTROL, KeyCode::Char('x')) => {
            app.input_cut();
            return Ok(());
        }
        (KeyModifiers::SUPER, KeyCode::Char('v')) | (KeyModifiers::CONTROL, KeyCode::Char('v')) => {
            app.input_paste();
            return Ok(());
        }
        (KeyModifiers::SUPER, KeyCode::Char('a')) | (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
            app.input_select_all();
            return Ok(());
        }
        _ => {}
    }

    // Regular text editing
    match (key.modifiers, key.code) {
        (_, KeyCode::Esc) => app.prompt_mode = false,
        // ⌃u — clear entire input (standard Unix kill-line; ⌥+letter won't work on macOS)
        (KeyModifiers::CONTROL, KeyCode::Char('u')) => { app.clear_input(); }
        // ↑/↓ — browse prompt history (pulled from display_events UserMessage entries)
        (KeyModifiers::NONE, KeyCode::Up) => app.prompt_history_prev(),
        (KeyModifiers::NONE, KeyCode::Down) => app.prompt_history_next(),
        // Shift+Arrow for selection extension
        (KeyModifiers::SHIFT, KeyCode::Left) => app.input_left_select(true),
        (KeyModifiers::SHIFT, KeyCode::Right) => app.input_right_select(true),
        // Regular character input - clears selection first if typing replaces it
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
            if app.has_input_selection() { app.input_delete_selection(); }
            app.input_char(c);
        }
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            if app.has_input_selection() { app.input_delete_selection(); }
            else { app.input_backspace(); }
        }
        (KeyModifiers::NONE, KeyCode::Delete) => {
            if app.has_input_selection() { app.input_delete_selection(); }
            else { app.input_delete(); }
        }
        (KeyModifiers::NONE, KeyCode::Left) => app.input_left_select(false),
        (KeyModifiers::NONE, KeyCode::Right) => app.input_right_select(false),
        (KeyModifiers::NONE, KeyCode::Home) => { app.input_clear_selection(); app.input_home(); }
        (KeyModifiers::NONE, KeyCode::End) => { app.input_clear_selection(); app.input_end(); }
        (KeyModifiers::CONTROL, KeyCode::Left) | (KeyModifiers::ALT, KeyCode::Left) => { app.input_clear_selection(); app.input_word_left(); }
        (KeyModifiers::CONTROL, KeyCode::Right) | (KeyModifiers::ALT, KeyCode::Right) => { app.input_clear_selection(); app.input_word_right(); }
        (KeyModifiers::CONTROL, KeyCode::Backspace) | (KeyModifiers::CONTROL, KeyCode::Char('w')) => app.input_delete_word(),
        // Shift+Enter — insert newline (Enter alone submits)
        // With DISAMBIGUATE_ESCAPE_CODES, Shift+Enter sends CSI 13;2u → (SHIFT, Enter).
        // ALT+Enter arm kept as safety net for Kitty-macOS edge cases.
        (KeyModifiers::SHIFT, KeyCode::Enter)
        | (KeyModifiers::ALT, KeyCode::Enter) => {
            if app.has_input_selection() { app.input_delete_selection(); }
            app.input_char('\n');
        }
        (KeyModifiers::NONE, KeyCode::Enter) => {
            if !app.input.is_empty() {
                let input = app.input.clone();
                app.clear_input();

                // Get session info: branch_name and worktree_path
                let session_data = app.current_session().map(|s| (s.branch_name.clone(), s.worktree_path.clone()));

                if let Some((branch_name, worktree_opt)) = session_data {
                    if let Some(wt_path) = worktree_opt {
                        // If Claude is already running, cancel it and stage the new prompt
                        if app.is_session_running(&branch_name) {
                            app.cancel_current_claude();
                            app.staged_prompt = Some(input);
                            app.set_status("Cancelling... prompt staged");
                        } else {
                            // Display user prompt (Claude's session files store the actual messages)
                            let prompt_text = format!("You: {}\n", input.clone());
                            app.add_user_message(input.clone());
                            app.process_output_chunk(&prompt_text);

                            // If awaiting plan approval, prepend hidden context explaining the options
                            // User only sees their input; Claude receives the context + input
                            let actual_prompt = if app.awaiting_plan_approval {
                                app.awaiting_plan_approval = false;
                                format!(
                                    "[SYSTEM: You just called ExitPlanMode. The user is viewing the plan approval prompt with these options:\n\
                                    1. Yes, clear context and bypass permissions\n\
                                    2. Yes, and manually approve edits\n\
                                    3. Yes, and bypass permissions\n\
                                    4. Yes, manually approve edits\n\
                                    5. Custom feedback - user will type what to change\n\n\
                                    The user's response follows. Interpret numbers 1-5 as selecting that option. Any other text is custom feedback (option 5).]\n\n\
                                    User response: {}",
                                    input
                                )
                            } else if app.awaiting_ask_user_question {
                                app.awaiting_ask_user_question = false;
                                // Build context from cached questions so Claude knows which options were shown
                                let ctx = if let Some(ref q) = app.ask_user_questions_cache {
                                    build_ask_user_context(q)
                                } else {
                                    String::new()
                                };
                                app.ask_user_questions_cache = None;
                                if ctx.is_empty() {
                                    input.clone()
                                } else {
                                    format!("{}\n\nUser response: {}", ctx, input)
                                }
                            } else {
                                input.clone()
                            };

                            let resume_id = app.get_claude_session_id(&branch_name).cloned();

                            match claude_process.spawn(&wt_path, &actual_prompt, resume_id.as_deref()) {
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
                        app.input_cursor = app.input.chars().count();
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

/// Handle keyboard input when worktree creation modal is focused
pub fn handle_worktree_creation_input(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Enter) => {
            if !app.worktree_creation_input.is_empty() {
                let prompt = app.worktree_creation_input.clone();
                app.exit_worktree_creation_mode();

                match app.create_new_worktree(prompt.clone()) {
                    Ok(worktree) => {
                        let branch_name = worktree.branch_name.clone();
                        app.set_status(format!("Created worktree: {}", worktree.name()));

                        if let Some(ref wt_path) = worktree.worktree_path {
                            match claude_process.spawn(wt_path, &prompt, None) {
                                Ok(rx) => app.register_claude(branch_name, rx),
                                Err(e) => app.set_status(format!("Failed to start: {}", e)),
                            }
                        }
                    }
                    Err(e) => app.set_status(format!("Failed to create worktree: {}", e)),
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Enter) => app.worktree_creation_char('\n'),
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => app.worktree_creation_char(c),
        (_, KeyCode::Backspace) => app.worktree_creation_backspace(),
        (_, KeyCode::Delete) => app.worktree_creation_delete(),
        (_, KeyCode::Left) => app.worktree_creation_left(),
        (_, KeyCode::Right) => app.worktree_creation_right(),
        (_, KeyCode::Home) => app.worktree_creation_home(),
        (_, KeyCode::End) => app.worktree_creation_end(),
        (_, KeyCode::Esc) => app.exit_worktree_creation_mode(),
        _ => {}
    }
    Ok(())
}

/// Build a hidden system context string from the cached AskUserQuestion JSON.
/// This gets prepended to the user's response so Claude knows which numbered
/// options were displayed and can interpret "1", "2", etc. correctly.
/// Input shape: { "questions": [{ "question": "...", "options": [{ "label": "...", "description": "..." }], "multiSelect": bool }] }
fn build_ask_user_context(input: &serde_json::Value) -> String {
    let Some(questions) = input.get("questions").and_then(|v| v.as_array()) else {
        return String::new();
    };
    let mut ctx = String::from("[SYSTEM: You just called AskUserQuestion. The user was shown these options:\n");
    for (qi, q) in questions.iter().enumerate() {
        let text = q.get("question").and_then(|v| v.as_str()).unwrap_or("?");
        let multi = q.get("multiSelect").and_then(|v| v.as_bool()).unwrap_or(false);
        if questions.len() > 1 {
            ctx.push_str(&format!("\nQ{}: {}", qi + 1, text));
        } else {
            ctx.push_str(&format!("\n{}", text));
        }
        if multi { ctx.push_str(" (multi-select)"); }
        if let Some(opts) = q.get("options").and_then(|v| v.as_array()) {
            for (i, opt) in opts.iter().enumerate() {
                let label = opt.get("label").and_then(|v| v.as_str()).unwrap_or("?");
                ctx.push_str(&format!("\n  {}. {}", i + 1, label));
            }
            // "Other" is always implicitly available in AskUserQuestion
            ctx.push_str(&format!("\n  {}. Other (custom text)", opts.len() + 1));
        }
    }
    ctx.push_str("\n\nThe user's response follows. Interpret numbers as selecting that option. Any other text is custom input (\"Other\").]");
    ctx
}
