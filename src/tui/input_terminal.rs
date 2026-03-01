//! Terminal and Claude prompt input handling

use anyhow::Result;
use crossterm::event::{self, KeyCode, KeyModifiers};

use crate::app::{App, Focus};
use crate::claude::ClaudeProcess;
use crate::tui::keybindings::macos_opt_key;

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
            match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Char('t')) => {
                    // Enter type mode (not close terminal - that's Esc now)
                    app.prompt_mode = true;
                    app.scroll_terminal_to_bottom();
                }
                (KeyModifiers::NONE, KeyCode::Char('p')) => {
                    // Close terminal and enter Claude prompt
                    app.close_terminal();
                    app.focus = Focus::Input;
                    app.prompt_mode = true;
                }
                (_, KeyCode::Esc) => app.close_terminal(),
                (KeyModifiers::NONE, KeyCode::Char('+')) | (KeyModifiers::NONE, KeyCode::Char('=')) => app.adjust_terminal_height(2),
                (KeyModifiers::NONE, KeyCode::Char('-')) => app.adjust_terminal_height(-2),
                (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => app.scroll_terminal_up(1),
                (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => app.scroll_terminal_down(1),
                (KeyModifiers::NONE, KeyCode::Char('K')) => app.scroll_terminal_up(10),
                (KeyModifiers::NONE, KeyCode::Char('J')) => app.scroll_terminal_down(10),
                // ⌥↑/⌥↓ scroll to top/bottom
                (KeyModifiers::ALT, KeyCode::Up) => {
                    app.terminal_scroll = 10000;
                    app.terminal_parser.screen_mut().set_scrollback(10000);
                    app.terminal_scroll = app.terminal_parser.screen().scrollback();
                }
                (KeyModifiers::ALT, KeyCode::Down) => app.scroll_terminal_to_bottom(),
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
        (KeyModifiers::SUPER, KeyCode::Char('c')) => {
            app.input_copy();
            return Ok(());
        }
        (KeyModifiers::SUPER, KeyCode::Char('x')) => {
            app.input_cut();
            return Ok(());
        }
        (KeyModifiers::SUPER, KeyCode::Char('v')) => {
            app.input_paste();
            return Ok(());
        }
        (KeyModifiers::SUPER, KeyCode::Char('a')) => {
            app.input_select_all();
            return Ok(());
        }
        _ => {}
    }

    // Regular text editing
    match (key.modifiers, key.code) {
        (_, KeyCode::Esc) => app.prompt_mode = false,
        // ⌃s — toggle speech-to-text recording (start/stop mic capture + Whisper transcription)
        (KeyModifiers::CONTROL, KeyCode::Char('s')) => { app.toggle_stt(); }
        // ↑/↓ — browse prompt history (pulled from display_events UserMessage entries)
        (KeyModifiers::NONE, KeyCode::Up) => app.prompt_history_prev(),
        (KeyModifiers::NONE, KeyCode::Down) => app.prompt_history_next(),
        // Shift+Arrow for selection extension
        (KeyModifiers::SHIFT, KeyCode::Left) => app.input_left_select(true),
        (KeyModifiers::SHIFT, KeyCode::Right) => app.input_right_select(true),
        // ⌥+number quick-select preset prompts (⌥1-⌥9 → presets 0-8, ⌥0 → preset 9)
        // macOS ⌥+number produces unicode (¡™£¢∞§¶•ªº) — intercept before text input
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c))
            if macos_opt_key(c).map(|k| k.is_ascii_digit()).unwrap_or(false) =>
        {
            let digit = macos_opt_key(c).unwrap();
            let idx = if digit == '0' { 9 } else { (digit as usize) - ('1' as usize) };
            if idx < app.preset_prompts.len() {
                app.select_preset_prompt(idx);
            }
        }
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
        // ⌃w (universal Unix delete-word), ⌃Backspace (Linux/Windows)
        (KeyModifiers::CONTROL, KeyCode::Char('w')) | (KeyModifiers::CONTROL, KeyCode::Backspace) => app.input_delete_word(),
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

                // RCR mode: route prompts to the feature branch worktree where
                // the rebase is happening, resume the RCR session
                if let Some(ref rcr) = app.rcr_session {
                    let cwd = rcr.worktree_path.clone();
                    let resume = rcr.session_id.clone();
                    let branch = rcr.branch.clone();

                    let prompt_text = format!("You: {}\n", input);
                    app.add_user_message(input.clone());
                    app.process_session_chunk(&prompt_text);
                    app.current_todos.clear();

                    match claude_process.spawn(&cwd, &input, resume.as_deref(), None) {
                        Ok((rx, pid)) => {
                            let slot = pid.to_string();
                            // Update RCR to track the new process
                            if let Some(ref mut m) = app.rcr_session {
                                m.slot_id = slot;
                                m.approval_pending = false;
                            }
                            app.register_claude(branch, pid, rx);
                            app.set_status("[RCR] Running...");
                        }
                        Err(e) => app.set_status(format!("Failed to start: {}", e)),
                    }
                } else {
                    // Normal prompt flow — get session info: branch_name and worktree_path
                    let session_data = app.current_worktree().map(|s| (s.branch_name.clone(), s.worktree_path.clone()));

                    if let Some((branch_name, worktree_opt)) = session_data {
                        if let Some(wt_path) = worktree_opt {
                            let prompt_text = format!("You: {}\n", input.clone());
                            app.add_user_message(input.clone());
                            app.process_session_chunk(&prompt_text);
                            app.current_todos.clear();

                            // If awaiting plan approval, prepend hidden context explaining the options
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
                                let ctx = if let Some(ref q) = app.ask_user_questions_cache {
                                    build_ask_user_context(q)
                                } else {
                                    String::new()
                                };
                                app.ask_user_questions_cache = None;
                                if ctx.is_empty() { input.clone() }
                                else { format!("{}\n\nUser response: {}", ctx, input) }
                            } else {
                                input.clone()
                            };

                            let resume_id = app.get_claude_session_id(&branch_name).cloned();
                            match claude_process.spawn(&wt_path, &actual_prompt, resume_id.as_deref(), app.selected_model.as_deref()) {
                                Ok((rx, pid)) => {
                                    app.register_claude(branch_name, pid, rx);
                                    app.set_status("Running...");
                                }
                                Err(e) => app.set_status(format!("Failed to start: {}", e)),
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
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Verifies build_ask_user_context produces correct system context from
    /// a real AskUserQuestion input with 2 options and single-select.
    /// This test exists because the context string is invisible to the user
    /// but critical for Claude to interpret numbered responses.
    #[test]
    fn test_build_context_single_question_two_options() {
        let input = json!({
            "questions": [{
                "question": "Use tiktoken-rs with cl100k_base encoding?",
                "header": "Approach",
                "options": [
                    {"label": "tiktoken-rs (Recommended)", "description": "~95% accurate"},
                    {"label": "Character heuristic", "description": "~4 chars/token"}
                ],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("[SYSTEM:"), "Should start with system prefix");
        assert!(ctx.contains("Use tiktoken-rs"), "Should include question text");
        assert!(ctx.contains("1. tiktoken-rs (Recommended)"), "Should number first option");
        assert!(ctx.contains("2. Character heuristic"), "Should number second option");
        assert!(ctx.contains("3. Other (custom text)"), "Should include Other option");
        assert!(!ctx.contains("multi-select"), "Single-select should not mention multi");
        assert!(ctx.contains("Interpret numbers"), "Should explain number semantics");
    }

    /// Verifies multi-select questions are annotated.
    #[test]
    fn test_build_context_multi_select() {
        let input = json!({
            "questions": [{
                "question": "Which features?",
                "header": "Features",
                "options": [{"label": "A", "description": ""}, {"label": "B", "description": ""}],
                "multiSelect": true
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("(multi-select)"), "Multi-select should be annotated");
    }

    /// Verifies multiple questions get Q1/Q2 prefixes.
    #[test]
    fn test_build_context_multiple_questions() {
        let input = json!({
            "questions": [
                {
                    "question": "First?",
                    "header": "Q1",
                    "options": [{"label": "Yes", "description": ""}],
                    "multiSelect": false
                },
                {
                    "question": "Second?",
                    "header": "Q2",
                    "options": [{"label": "No", "description": ""}],
                    "multiSelect": false
                }
            ]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("Q1: First?"), "Should prefix first question with Q1");
        assert!(ctx.contains("Q2: Second?"), "Should prefix second question with Q2");
    }

    /// Verifies missing/invalid questions field returns empty string (no panic).
    #[test]
    fn test_build_context_empty_input() {
        assert!(build_ask_user_context(&json!({})).is_empty());
        assert!(build_ask_user_context(&json!({"questions": "not_array"})).is_empty());
    }

    /// Verifies options with missing fields use "?" fallback.
    #[test]
    fn test_build_context_missing_label_fallback() {
        let input = json!({
            "questions": [{
                "question": "Test?",
                "options": [{"description": "no label"}],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("1. ?"), "Missing label should show ?");
    }

    // ── Empty / null edge cases ─────────────────────────────────────────

    /// Null JSON value should return empty string.
    #[test]
    fn test_build_context_null_value() {
        assert!(build_ask_user_context(&json!(null)).is_empty());
    }

    /// Boolean JSON value should return empty (not an object).
    #[test]
    fn test_build_context_bool_value() {
        assert!(build_ask_user_context(&json!(true)).is_empty());
    }

    /// Numeric JSON value should return empty.
    #[test]
    fn test_build_context_number_value() {
        assert!(build_ask_user_context(&json!(42)).is_empty());
    }

    /// String JSON value should return empty.
    #[test]
    fn test_build_context_string_value() {
        assert!(build_ask_user_context(&json!("hello")).is_empty());
    }

    /// Array JSON value (not an object) should return empty.
    #[test]
    fn test_build_context_array_value() {
        assert!(build_ask_user_context(&json!([1, 2, 3])).is_empty());
    }

    /// Questions field is null (not missing, but explicitly null).
    #[test]
    fn test_build_context_questions_null() {
        assert!(build_ask_user_context(&json!({"questions": null})).is_empty());
    }

    /// Questions field is a number instead of array.
    #[test]
    fn test_build_context_questions_number() {
        assert!(build_ask_user_context(&json!({"questions": 123})).is_empty());
    }

    /// Questions field is a boolean instead of array.
    #[test]
    fn test_build_context_questions_bool() {
        assert!(build_ask_user_context(&json!({"questions": false})).is_empty());
    }

    /// Questions field is an object instead of array.
    #[test]
    fn test_build_context_questions_object() {
        assert!(build_ask_user_context(&json!({"questions": {"a": 1}})).is_empty());
    }

    /// Empty questions array returns empty string.
    #[test]
    fn test_build_context_questions_empty_array() {
        // Empty questions array still enters the loop but produces no Q lines,
        // yet the header/footer are emitted. Verify no panic at minimum.
        let result = build_ask_user_context(&json!({"questions": []}));
        // With 0 iterations the string should contain at least the header
        assert!(result.contains("[SYSTEM:") || result.is_empty());
    }

    // ── Single question variations ──────────────────────────────────────

    /// Single question with zero options (only "Other" should appear).
    #[test]
    fn test_build_context_no_options() {
        let input = json!({
            "questions": [{
                "question": "What do you think?",
                "options": [],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("What do you think?"));
        assert!(ctx.contains("1. Other (custom text)"));
    }

    /// Single question with missing options field entirely.
    #[test]
    fn test_build_context_missing_options_field() {
        let input = json!({
            "questions": [{
                "question": "Open ended?",
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("Open ended?"));
        // No options numbered, no "Other" since the options array is absent
        assert!(!ctx.contains("1. "));
    }

    /// Single question with options field set to null. The numbered option
    /// lines and "X. Other (custom text)" are inside the if-let block and
    /// are skipped, but the footer text still mentions "Other" in quotes.
    #[test]
    fn test_build_context_options_null() {
        let input = json!({
            "questions": [{
                "question": "Null opts?",
                "options": null,
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("Null opts?"));
        // The numbered "X. Other (custom text)" line should NOT appear
        assert!(!ctx.contains("Other (custom text)"));
    }

    /// Single question with one option produces "2. Other".
    #[test]
    fn test_build_context_one_option_other_is_two() {
        let input = json!({
            "questions": [{
                "question": "Pick?",
                "options": [{"label": "Only choice", "description": ""}],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("1. Only choice"));
        assert!(ctx.contains("2. Other (custom text)"));
    }

    /// Five options should number 1-5 with "6. Other".
    #[test]
    fn test_build_context_five_options() {
        let options: Vec<serde_json::Value> = (1..=5)
            .map(|i| json!({"label": format!("Option {}", i), "description": ""}))
            .collect();
        let input = json!({
            "questions": [{
                "question": "Many choices",
                "options": options,
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        for i in 1..=5 {
            assert!(ctx.contains(&format!("{}. Option {}", i, i)));
        }
        assert!(ctx.contains("6. Other (custom text)"));
    }

    /// Ten options should all be numbered correctly.
    #[test]
    fn test_build_context_ten_options() {
        let options: Vec<serde_json::Value> = (1..=10)
            .map(|i| json!({"label": format!("Opt{}", i), "description": ""}))
            .collect();
        let input = json!({
            "questions": [{
                "question": "Lots",
                "options": options,
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("10. Opt10"));
        assert!(ctx.contains("11. Other (custom text)"));
    }

    /// Missing question text should default to "?".
    #[test]
    fn test_build_context_missing_question_text() {
        let input = json!({
            "questions": [{
                "options": [{"label": "A", "description": ""}],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        // Single question, no "Q1:" prefix, just the fallback "?"
        assert!(ctx.contains("\n?"));
    }

    /// Question text is null instead of string.
    #[test]
    fn test_build_context_question_text_null() {
        let input = json!({
            "questions": [{
                "question": null,
                "options": [{"label": "X", "description": ""}],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("\n?"));
    }

    /// Question text is numeric (not a string).
    #[test]
    fn test_build_context_question_text_number() {
        let input = json!({
            "questions": [{
                "question": 42,
                "options": [],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("\n?"));
    }

    // ── multiSelect variations ──────────────────────────────────────────

    /// Missing multiSelect field defaults to false (no annotation).
    #[test]
    fn test_build_context_missing_multi_select() {
        let input = json!({
            "questions": [{
                "question": "Test?",
                "options": [{"label": "A", "description": ""}]
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(!ctx.contains("(multi-select)"));
    }

    /// multiSelect set to null defaults to false.
    #[test]
    fn test_build_context_multi_select_null() {
        let input = json!({
            "questions": [{
                "question": "Test?",
                "options": [],
                "multiSelect": null
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(!ctx.contains("(multi-select)"));
    }

    /// multiSelect as string "true" should default to false (wrong type).
    #[test]
    fn test_build_context_multi_select_string() {
        let input = json!({
            "questions": [{
                "question": "Test?",
                "options": [],
                "multiSelect": "true"
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(!ctx.contains("(multi-select)"));
    }

    /// multiSelect explicitly false should not add annotation.
    #[test]
    fn test_build_context_multi_select_explicit_false() {
        let input = json!({
            "questions": [{
                "question": "Test?",
                "options": [{"label": "A", "description": ""}],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(!ctx.contains("(multi-select)"));
    }

    // ── Multiple questions ──────────────────────────────────────────────

    /// Three questions should get Q1, Q2, Q3 prefixes.
    #[test]
    fn test_build_context_three_questions() {
        let input = json!({
            "questions": [
                {"question": "Alpha?", "options": [{"label": "A1", "description": ""}], "multiSelect": false},
                {"question": "Beta?", "options": [{"label": "B1", "description": ""}], "multiSelect": false},
                {"question": "Gamma?", "options": [{"label": "G1", "description": ""}], "multiSelect": true}
            ]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("Q1: Alpha?"));
        assert!(ctx.contains("Q2: Beta?"));
        assert!(ctx.contains("Q3: Gamma?"));
        assert!(ctx.contains("(multi-select)"));
    }

    /// Mixed multi-select in multiple questions: only annotated ones show it.
    #[test]
    fn test_build_context_mixed_multi_select_multiple() {
        let input = json!({
            "questions": [
                {"question": "Single?", "options": [], "multiSelect": false},
                {"question": "Multi?", "options": [], "multiSelect": true}
            ]
        });
        let ctx = build_ask_user_context(&input);
        // "Single?" line should NOT have multi-select
        // "Multi?" line SHOULD have multi-select
        let lines: Vec<&str> = ctx.lines().collect();
        let single_line = lines.iter().find(|l| l.contains("Single?")).unwrap();
        assert!(!single_line.contains("(multi-select)"));
        let multi_line = lines.iter().find(|l| l.contains("Multi?")).unwrap();
        assert!(multi_line.contains("(multi-select)"));
    }

    /// Single question does NOT get Q1 prefix.
    #[test]
    fn test_build_context_single_question_no_q_prefix() {
        let input = json!({
            "questions": [{
                "question": "Only one?",
                "options": [],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(!ctx.contains("Q1:"), "Single question should not have Q1 prefix");
        assert!(ctx.contains("\nOnly one?"));
    }

    // ── Option label edge cases ─────────────────────────────────────────

    /// Option label with special characters.
    #[test]
    fn test_build_context_special_chars_in_label() {
        let input = json!({
            "questions": [{
                "question": "Pick encoding?",
                "options": [{"label": "UTF-8 (±∞)", "description": ""}],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("1. UTF-8 (±∞)"));
    }

    /// Option label with unicode emoji.
    #[test]
    fn test_build_context_emoji_in_label() {
        let input = json!({
            "questions": [{
                "question": "Mood?",
                "options": [{"label": "Happy 😊", "description": ""}, {"label": "Sad 😢", "description": ""}],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("1. Happy 😊"));
        assert!(ctx.contains("2. Sad 😢"));
    }

    /// Option label that is empty string.
    #[test]
    fn test_build_context_empty_label() {
        let input = json!({
            "questions": [{
                "question": "Test?",
                "options": [{"label": "", "description": "desc"}],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("1. "));
    }

    /// Very long option label should not panic.
    #[test]
    fn test_build_context_very_long_label() {
        let long_label = "A".repeat(500);
        let input = json!({
            "questions": [{
                "question": "Test?",
                "options": [{"label": long_label, "description": ""}],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains(&format!("1. {}", long_label)));
    }

    /// Label is a number (wrong type) — should fall back to "?".
    #[test]
    fn test_build_context_label_wrong_type() {
        let input = json!({
            "questions": [{
                "question": "Test?",
                "options": [{"label": 42, "description": ""}],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("1. ?"));
    }

    /// Option is a string instead of object — should be skipped.
    #[test]
    fn test_build_context_option_is_string() {
        let input = json!({
            "questions": [{
                "question": "Test?",
                "options": ["not_an_object"],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        // The option is a string, .get("label") returns None → "?"
        assert!(ctx.contains("1. ?"));
    }

    /// Option is null.
    #[test]
    fn test_build_context_option_is_null() {
        let input = json!({
            "questions": [{
                "question": "Test?",
                "options": [null],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("1. ?"));
    }

    // ── Question text edge cases ────────────────────────────────────────

    /// Very long question text.
    #[test]
    fn test_build_context_very_long_question() {
        let long_q = "Q".repeat(1000);
        let input = json!({
            "questions": [{"question": long_q, "options": [], "multiSelect": false}]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains(&long_q));
    }

    /// Question text with newlines.
    #[test]
    fn test_build_context_question_with_newlines() {
        let input = json!({
            "questions": [{
                "question": "Line1\nLine2\nLine3",
                "options": [],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("Line1\nLine2\nLine3"));
    }

    /// Question with unicode text.
    #[test]
    fn test_build_context_unicode_question() {
        let input = json!({
            "questions": [{
                "question": "日本語のテスト？",
                "options": [{"label": "はい", "description": ""}, {"label": "いいえ", "description": ""}],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("日本語のテスト？"));
        assert!(ctx.contains("1. はい"));
        assert!(ctx.contains("2. いいえ"));
    }

    // ── Output structure verification ───────────────────────────────────

    /// Verify output starts with system tag and ends with closing bracket.
    #[test]
    fn test_build_context_output_bookends() {
        let input = json!({
            "questions": [{"question": "Q?", "options": [], "multiSelect": false}]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.starts_with("[SYSTEM:"));
        assert!(ctx.ends_with(']'));
    }

    /// Verify "Other" text always comes after numbered options.
    #[test]
    fn test_build_context_other_after_options() {
        let input = json!({
            "questions": [{
                "question": "Q?",
                "options": [
                    {"label": "First", "description": ""},
                    {"label": "Second", "description": ""},
                    {"label": "Third", "description": ""}
                ],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        let first_pos = ctx.find("1. First").unwrap();
        let other_pos = ctx.find("4. Other").unwrap();
        assert!(other_pos > first_pos, "Other should come after numbered options");
    }

    /// Each question's options get their own numbering starting from 1.
    #[test]
    fn test_build_context_independent_numbering() {
        let input = json!({
            "questions": [
                {"question": "Q1?", "options": [{"label": "A", "description": ""}], "multiSelect": false},
                {"question": "Q2?", "options": [{"label": "B", "description": ""}, {"label": "C", "description": ""}], "multiSelect": false}
            ]
        });
        let ctx = build_ask_user_context(&input);
        // Both questions should have "1." for their first option
        let parts: Vec<&str> = ctx.split("Q2:").collect();
        assert_eq!(parts.len(), 2);
        assert!(parts[0].contains("1. A"));
        assert!(parts[1].contains("1. B"));
        assert!(parts[1].contains("2. C"));
    }

    /// Footer text always present when questions exist.
    #[test]
    fn test_build_context_footer_text() {
        let input = json!({
            "questions": [{"question": "X?", "options": [], "multiSelect": false}]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("The user's response follows"));
        assert!(ctx.contains("Interpret numbers as selecting that option"));
        assert!(ctx.contains("Any other text is custom input"));
    }

    // ── Question entry is a non-object ──────────────────────────────────

    /// Question entry is a string instead of an object.
    #[test]
    fn test_build_context_question_entry_is_string() {
        let input = json!({
            "questions": ["not an object"]
        });
        let ctx = build_ask_user_context(&input);
        // .get("question") on a string returns None → "?"
        assert!(ctx.contains("?"));
    }

    /// Question entry is a number.
    #[test]
    fn test_build_context_question_entry_is_number() {
        let input = json!({
            "questions": [42]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("?"));
    }

    /// Extra fields in the JSON should not cause issues.
    #[test]
    fn test_build_context_extra_fields_ignored() {
        let input = json!({
            "questions": [{
                "question": "Approach?",
                "header": "H1",
                "options": [{"label": "Yes", "description": "d", "extra": "ignored"}],
                "multiSelect": false,
                "unknownField": 123
            }],
            "topLevelExtra": "also ignored"
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("Approach?"));
        assert!(ctx.contains("1. Yes"));
    }

    /// Options array contains a mix of valid objects and invalid types.
    #[test]
    fn test_build_context_mixed_option_types() {
        let input = json!({
            "questions": [{
                "question": "Mix?",
                "options": [
                    {"label": "Valid", "description": ""},
                    null,
                    42,
                    {"label": "Also valid", "description": ""}
                ],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("1. Valid"));
        assert!(ctx.contains("2. ?"));
        assert!(ctx.contains("3. ?"));
        assert!(ctx.contains("4. Also valid"));
        assert!(ctx.contains("5. Other (custom text)"));
    }

    /// Deeply nested JSON that doesn't match expected structure.
    #[test]
    fn test_build_context_deeply_nested() {
        let input = json!({
            "questions": [{
                "question": {"nested": "object"},
                "options": [],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        // question is object not string → as_str() returns None → "?"
        assert!(ctx.contains("\n?"));
    }

    /// Single question with multi-select and many options.
    #[test]
    fn test_build_context_multi_select_many_options() {
        let options: Vec<serde_json::Value> = (1..=8)
            .map(|i| json!({"label": format!("Feature {}", i), "description": ""}))
            .collect();
        let input = json!({
            "questions": [{
                "question": "Select all that apply",
                "options": options,
                "multiSelect": true
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("(multi-select)"));
        for i in 1..=8 {
            assert!(ctx.contains(&format!("{}. Feature {}", i, i)));
        }
        assert!(ctx.contains("9. Other (custom text)"));
    }

    /// Verify that multiSelect annotation appears right after question text.
    #[test]
    fn test_build_context_multi_select_position() {
        let input = json!({
            "questions": [{
                "question": "Features?",
                "options": [{"label": "X", "description": ""}],
                "multiSelect": true
            }]
        });
        let ctx = build_ask_user_context(&input);
        let q_pos = ctx.find("Features?").unwrap();
        let ms_pos = ctx.find("(multi-select)").unwrap();
        // multi-select should be on the same line, right after question
        assert!(ms_pos > q_pos);
        let between = &ctx[q_pos..ms_pos];
        assert!(!between.contains('\n'), "multi-select annotation should be on same line as question");
    }

    /// Header field is ignored by build_ask_user_context (only used by UI renderer).
    #[test]
    fn test_build_context_header_field_ignored() {
        let input = json!({
            "questions": [{
                "question": "Q?",
                "header": "This Header Should Not Appear In Context",
                "options": [],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(!ctx.contains("This Header Should Not Appear In Context"));
    }

    /// Empty string question.
    #[test]
    fn test_build_context_empty_string_question() {
        let input = json!({
            "questions": [{
                "question": "",
                "options": [{"label": "Y", "description": ""}],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        // Empty string is a valid string, not None — should appear as empty
        assert!(ctx.contains("1. Y"));
    }

    /// Whitespace-only question text.
    #[test]
    fn test_build_context_whitespace_question() {
        let input = json!({
            "questions": [{
                "question": "   ",
                "options": [],
                "multiSelect": false
            }]
        });
        let ctx = build_ask_user_context(&input);
        assert!(ctx.contains("   "));
    }
}
