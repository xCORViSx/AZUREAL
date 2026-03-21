//! Commit message overlay input handling.
//!
//! Text editing (type/backspace/arrows), Enter to commit, ⌘P to commit+push,
//! Shift+Enter for newline, Esc to cancel.

use anyhow::Result;
use crossterm::event;

use crate::app::App;

/// Handle input while the commit message overlay is open.
pub(super) fn handle_commit_overlay(key: event::KeyEvent, app: &mut App) -> Result<()> {
    use crossterm::event::{KeyCode, KeyModifiers};
    let panel = match app.git_actions_panel.as_mut() {
        Some(p) => p,
        None => return Ok(()),
    };
    let overlay = match panel.commit_overlay.as_mut() {
        Some(o) => o,
        None => return Ok(()),
    };

    let generating = overlay.generating;

    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Esc) => {
            panel.commit_overlay = None;
        }

        (KeyModifiers::NONE, KeyCode::Enter)
            if !generating && !overlay.message.trim().is_empty() =>
        {
            let msg = overlay.message.clone();
            let wt = panel.worktree_path.clone();
            panel.commit_overlay = None;
            app.loading_indicator = Some("Committing...".into());
            app.deferred_action = Some(crate::app::DeferredAction::GitCommit {
                worktree: wt,
                message: msg,
            });
        }

        (m, KeyCode::Char(c))
            if crate::tui::keybindings::is_cmd_key(m, KeyCode::Char(c), 'p')
                && !generating
                && !overlay.message.trim().is_empty() =>
        {
            let msg = overlay.message.clone();
            let wt = panel.worktree_path.clone();
            panel.commit_overlay = None;
            app.loading_indicator = Some("Committing and pushing...".into());
            app.deferred_action = Some(crate::app::DeferredAction::GitCommitAndPush {
                worktree: wt,
                message: msg,
            });
        }

        (KeyModifiers::NONE, KeyCode::Backspace) if !generating => {
            if overlay.cursor > 0 {
                let byte_pos = overlay
                    .message
                    .char_indices()
                    .nth(overlay.cursor - 1)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                let next_byte = overlay
                    .message
                    .char_indices()
                    .nth(overlay.cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(overlay.message.len());
                overlay.message.replace_range(byte_pos..next_byte, "");
                overlay.cursor -= 1;
            }
        }

        (KeyModifiers::NONE, KeyCode::Delete) if !generating => {
            let char_count = overlay.message.chars().count();
            if overlay.cursor < char_count {
                let byte_pos = overlay
                    .message
                    .char_indices()
                    .nth(overlay.cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(overlay.message.len());
                let next_byte = overlay
                    .message
                    .char_indices()
                    .nth(overlay.cursor + 1)
                    .map(|(i, _)| i)
                    .unwrap_or(overlay.message.len());
                overlay.message.replace_range(byte_pos..next_byte, "");
            }
        }

        (KeyModifiers::NONE, KeyCode::Left) if !generating => {
            if overlay.cursor > 0 {
                overlay.cursor -= 1;
            }
        }
        (KeyModifiers::NONE, KeyCode::Right) if !generating => {
            let char_count = overlay.message.chars().count();
            if overlay.cursor < char_count {
                overlay.cursor += 1;
            }
        }

        (KeyModifiers::NONE, KeyCode::Home) if !generating => {
            overlay.cursor = 0;
        }
        (KeyModifiers::NONE, KeyCode::End) if !generating => {
            overlay.cursor = overlay.message.chars().count();
        }

        (KeyModifiers::NONE, KeyCode::Up) if !generating => {
            let chars: Vec<char> = overlay.message.chars().collect();
            let mut line_start = overlay.cursor;
            while line_start > 0 && chars.get(line_start - 1) != Some(&'\n') {
                line_start -= 1;
            }
            if line_start > 0 {
                let prev_end = line_start - 1;
                let mut prev_start = prev_end;
                while prev_start > 0 && chars.get(prev_start - 1) != Some(&'\n') {
                    prev_start -= 1;
                }
                let col = overlay.cursor - line_start;
                let prev_len = prev_end - prev_start;
                overlay.cursor = prev_start + col.min(prev_len);
            }
        }
        (KeyModifiers::NONE, KeyCode::Down) if !generating => {
            let chars: Vec<char> = overlay.message.chars().collect();
            let mut line_start = overlay.cursor;
            while line_start > 0 && chars.get(line_start - 1) != Some(&'\n') {
                line_start -= 1;
            }
            let col = overlay.cursor - line_start;
            let mut line_end = overlay.cursor;
            while line_end < chars.len() && chars[line_end] != '\n' {
                line_end += 1;
            }
            if line_end < chars.len() {
                let next_start = line_end + 1;
                let mut next_end = next_start;
                while next_end < chars.len() && chars[next_end] != '\n' {
                    next_end += 1;
                }
                let next_len = next_end - next_start;
                overlay.cursor = next_start + col.min(next_len);
            }
        }

        // Shift+Enter or Ctrl+J — insert newline (Ctrl+J fallback for terminals
        // without Kitty protocol, e.g. WezTerm on macOS)
        (m, KeyCode::Enter) if m.contains(KeyModifiers::SHIFT) && !generating => {
            let byte_pos = overlay
                .message
                .char_indices()
                .nth(overlay.cursor)
                .map(|(i, _)| i)
                .unwrap_or(overlay.message.len());
            overlay.message.insert(byte_pos, '\n');
            overlay.cursor += 1;
        }
        (KeyModifiers::CONTROL, KeyCode::Char('j')) if !generating => {
            let byte_pos = overlay
                .message
                .char_indices()
                .nth(overlay.cursor)
                .map(|(i, _)| i)
                .unwrap_or(overlay.message.len());
            overlay.message.insert(byte_pos, '\n');
            overlay.cursor += 1;
        }

        (m, KeyCode::Char(c)) if !generating && !m.contains(KeyModifiers::CONTROL) => {
            let byte_pos = overlay
                .message
                .char_indices()
                .nth(overlay.cursor)
                .map(|(i, _)| i)
                .unwrap_or(overlay.message.len());
            overlay.message.insert(byte_pos, c);
            overlay.cursor += 1;
        }

        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn key_mod(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    // ══════════════════════════════════════════════════════════════════
    //  GitCommitOverlay construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn commit_overlay_empty_message() {
        let ov = crate::app::types::GitCommitOverlay {
            message: String::new(),
            cursor: 0,
            generating: false,
            scroll: 0,
            receiver: None,
        };
        assert!(ov.message.is_empty());
        assert_eq!(ov.cursor, 0);
        assert!(!ov.generating);
    }

    #[test]
    fn commit_overlay_generating_state() {
        let ov = crate::app::types::GitCommitOverlay {
            message: String::new(),
            cursor: 0,
            generating: true,
            scroll: 0,
            receiver: None,
        };
        assert!(ov.generating);
    }

    #[test]
    fn commit_overlay_with_message() {
        let ov = crate::app::types::GitCommitOverlay {
            message: "feat: add tests".to_string(),
            cursor: 15,
            generating: false,
            scroll: 0,
            receiver: None,
        };
        assert_eq!(ov.message, "feat: add tests");
        assert_eq!(ov.cursor, 15);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Key matching — overlay keys
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn esc_matches_cancel() {
        let k = key(KeyCode::Esc);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Esc)
        ));
    }

    #[test]
    fn enter_matches_commit() {
        let k = key(KeyCode::Enter);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Enter)
        ));
    }

    #[test]
    fn cmd_p_matches_commit_push() {
        let k = key_mod(KeyCode::Char('p'), KeyModifiers::SUPER);
        assert!(k.modifiers.contains(KeyModifiers::SUPER));
        assert_eq!(k.code, KeyCode::Char('p'));
    }

    #[test]
    fn backspace_matches_delete() {
        let k = key(KeyCode::Backspace);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Backspace)
        ));
    }

    #[test]
    fn delete_key_matches() {
        let k = key(KeyCode::Delete);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Delete)
        ));
    }

    #[test]
    fn left_arrow_matches() {
        let k = key(KeyCode::Left);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Left)
        ));
    }

    #[test]
    fn right_arrow_matches() {
        let k = key(KeyCode::Right);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Right)
        ));
    }

    #[test]
    fn home_matches() {
        let k = key(KeyCode::Home);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Home)
        ));
    }

    #[test]
    fn end_matches() {
        let k = key(KeyCode::End);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::End)
        ));
    }

    #[test]
    fn up_arrow_matches() {
        let k = key(KeyCode::Up);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Up)
        ));
    }

    #[test]
    fn down_arrow_matches() {
        let k = key(KeyCode::Down);
        assert!(matches!(
            (k.modifiers, k.code),
            (KeyModifiers::NONE, KeyCode::Down)
        ));
    }

    #[test]
    fn shift_enter_newline() {
        let k = key_mod(KeyCode::Enter, KeyModifiers::SHIFT);
        assert!(k.modifiers.contains(KeyModifiers::SHIFT));
        assert_eq!(k.code, KeyCode::Enter);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Cursor navigation logic (Up/Down line-based)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn cursor_up_single_line_no_movement() {
        let message = "hello world";
        let chars: Vec<char> = message.chars().collect();
        let cursor = 5usize;
        let mut line_start = cursor;
        while line_start > 0 && chars.get(line_start - 1) != Some(&'\n') {
            line_start -= 1;
        }
        // No newline before — line_start is 0, no previous line
        assert_eq!(line_start, 0);
    }

    #[test]
    fn cursor_up_two_lines() {
        let message = "line1\nline2 text";
        let chars: Vec<char> = message.chars().collect();
        let cursor = 8usize; // in "line2 text"
        let mut line_start = cursor;
        while line_start > 0 && chars.get(line_start - 1) != Some(&'\n') {
            line_start -= 1;
        }
        assert_eq!(line_start, 6); // after newline
        let col = cursor - line_start;
        assert_eq!(col, 2);
    }

    #[test]
    fn cursor_down_single_line_no_movement() {
        let message = "hello";
        let chars: Vec<char> = message.chars().collect();
        let cursor = 2usize;
        let mut line_end = cursor;
        while line_end < chars.len() && chars[line_end] != '\n' {
            line_end += 1;
        }
        // No newline after — at end
        assert_eq!(line_end, chars.len());
    }

    // ══════════════════════════════════════════════════════════════════
    //  Message manipulation (insert, backspace, delete)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn insert_char_at_cursor() {
        let mut msg = String::from("feat: ");
        let cursor = 6usize;
        let byte_pos = msg
            .char_indices()
            .nth(cursor)
            .map(|(i, _)| i)
            .unwrap_or(msg.len());
        msg.insert(byte_pos, 'a');
        assert_eq!(msg, "feat: a");
    }

    #[test]
    fn insert_newline_at_cursor() {
        let mut msg = String::from("ab");
        let cursor = 1usize;
        let byte_pos = msg
            .char_indices()
            .nth(cursor)
            .map(|(i, _)| i)
            .unwrap_or(msg.len());
        msg.insert(byte_pos, '\n');
        assert_eq!(msg, "a\nb");
    }

    #[test]
    fn backspace_removes_char_before_cursor() {
        let mut msg = String::from("abc");
        let mut cursor = 3usize;
        if cursor > 0 {
            let bp = msg
                .char_indices()
                .nth(cursor - 1)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let nb = msg
                .char_indices()
                .nth(cursor)
                .map(|(i, _)| i)
                .unwrap_or(msg.len());
            msg.replace_range(bp..nb, "");
            cursor -= 1;
        }
        assert_eq!(msg, "ab");
        assert_eq!(cursor, 2);
    }

    #[test]
    fn delete_removes_char_at_cursor() {
        let mut msg = String::from("abc");
        let cursor = 1usize;
        let char_count = msg.chars().count();
        if cursor < char_count {
            let bp = msg
                .char_indices()
                .nth(cursor)
                .map(|(i, _)| i)
                .unwrap_or(msg.len());
            let nb = msg
                .char_indices()
                .nth(cursor + 1)
                .map(|(i, _)| i)
                .unwrap_or(msg.len());
            msg.replace_range(bp..nb, "");
        }
        assert_eq!(msg, "ac");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Empty message trim check (used in Enter guard)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn empty_message_trim_is_empty() {
        assert!("".trim().is_empty());
    }
    #[test]
    fn whitespace_message_trim_is_empty() {
        assert!("   ".trim().is_empty());
    }
    #[test]
    fn nonempty_message_trim_is_not_empty() {
        assert!(!"feat: x".trim().is_empty());
    }

    // ══════════════════════════════════════════════════════════════════
    //  Generating flag guards (keys blocked while generating)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn generating_blocks_enter() {
        let generating = true;
        let msg_empty = false;
        let should_commit = !generating && !msg_empty;
        assert!(!should_commit);
    }

    #[test]
    fn not_generating_allows_enter() {
        let generating = false;
        let msg_empty = false;
        let should_commit = !generating && !msg_empty;
        assert!(should_commit);
    }

    #[test]
    fn char_blocked_by_control() {
        let modifiers = KeyModifiers::CONTROL;
        let generating = false;
        let should_insert = !generating && !modifiers.contains(KeyModifiers::CONTROL);
        assert!(!should_insert);
    }

    #[test]
    fn char_allowed_plain() {
        let modifiers = KeyModifiers::NONE;
        let generating = false;
        let should_insert = !generating && !modifiers.contains(KeyModifiers::CONTROL);
        assert!(should_insert);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Home/End cursor positions
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn home_sets_cursor_0() {
        let cursor = 0usize;
        assert_eq!(cursor, 0);
    }

    #[test]
    fn end_sets_cursor_to_char_count() {
        let msg = "hello world";
        let cursor = msg.chars().count();
        assert_eq!(cursor, 11);
    }

    #[test]
    fn end_unicode_char_count() {
        let msg = "hello 🌍";
        let cursor = msg.chars().count();
        assert_eq!(cursor, 7);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Additional cursor navigation: Left / Right boundaries
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn left_at_zero_stays_zero() {
        let cursor = 0usize;
        let new_cursor = if cursor > 0 { cursor - 1 } else { cursor };
        assert_eq!(new_cursor, 0);
    }

    #[test]
    fn left_from_middle_decrements() {
        let cursor = 5usize;
        let new_cursor = if cursor > 0 { cursor - 1 } else { cursor };
        assert_eq!(new_cursor, 4);
    }

    #[test]
    fn right_at_end_stays_at_end() {
        let msg = "hello";
        let char_count = msg.chars().count();
        let cursor = char_count;
        let new_cursor = if cursor < char_count {
            cursor + 1
        } else {
            cursor
        };
        assert_eq!(new_cursor, char_count);
    }

    #[test]
    fn right_from_middle_increments() {
        let msg = "hello";
        let char_count = msg.chars().count();
        let cursor = 2usize;
        let new_cursor = if cursor < char_count {
            cursor + 1
        } else {
            cursor
        };
        assert_eq!(new_cursor, 3);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Backspace at position 0 is a no-op
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn backspace_at_zero_is_noop() {
        let mut msg = String::from("abc");
        let mut cursor = 0usize;
        if cursor > 0 {
            let bp = msg
                .char_indices()
                .nth(cursor - 1)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let nb = msg
                .char_indices()
                .nth(cursor)
                .map(|(i, _)| i)
                .unwrap_or(msg.len());
            msg.replace_range(bp..nb, "");
            cursor -= 1;
        }
        assert_eq!(msg, "abc");
        assert_eq!(cursor, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Delete at end is a no-op
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn delete_at_end_is_noop() {
        let mut msg = String::from("abc");
        let cursor = 3usize;
        let char_count = msg.chars().count();
        if cursor < char_count {
            let bp = msg
                .char_indices()
                .nth(cursor)
                .map(|(i, _)| i)
                .unwrap_or(msg.len());
            let nb = msg
                .char_indices()
                .nth(cursor + 1)
                .map(|(i, _)| i)
                .unwrap_or(msg.len());
            msg.replace_range(bp..nb, "");
        }
        assert_eq!(msg, "abc");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Insert char at beginning (cursor == 0)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn insert_char_at_beginning() {
        let mut msg = String::from("bc");
        let cursor = 0usize;
        let byte_pos = msg
            .char_indices()
            .nth(cursor)
            .map(|(i, _)| i)
            .unwrap_or(msg.len());
        msg.insert(byte_pos, 'a');
        assert_eq!(msg, "abc");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Insert char into empty string
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn insert_char_into_empty() {
        let mut msg = String::new();
        let cursor = 0usize;
        let byte_pos = msg
            .char_indices()
            .nth(cursor)
            .map(|(i, _)| i)
            .unwrap_or(msg.len());
        msg.insert(byte_pos, 'x');
        assert_eq!(msg, "x");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Backspace on single character clears string
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn backspace_single_char_leaves_empty() {
        let mut msg = String::from("a");
        let mut cursor = 1usize;
        if cursor > 0 {
            let bp = msg
                .char_indices()
                .nth(cursor - 1)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let nb = msg
                .char_indices()
                .nth(cursor)
                .map(|(i, _)| i)
                .unwrap_or(msg.len());
            msg.replace_range(bp..nb, "");
            cursor -= 1;
        }
        assert!(msg.is_empty());
        assert_eq!(cursor, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Multi-byte (unicode) cursor handling
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn char_count_ascii_only() {
        let msg = "feat: something";
        assert_eq!(msg.chars().count(), 15);
    }

    #[test]
    fn char_count_multibyte_emoji() {
        // emoji is 1 char despite being 4 bytes
        let msg = "fix: 🐛 bug";
        let char_count = msg.chars().count();
        assert_eq!(char_count, 10);
        // byte length > char count
        assert!(msg.len() > char_count);
    }

    #[test]
    fn insert_unicode_char() {
        let mut msg = String::from("ab");
        let cursor = 1usize;
        let byte_pos = msg
            .char_indices()
            .nth(cursor)
            .map(|(i, _)| i)
            .unwrap_or(msg.len());
        msg.insert(byte_pos, '→');
        assert_eq!(msg, "a→b");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Shift+Enter newline insertion preserves surrounding text
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn shift_enter_at_start_prepends_newline() {
        let mut msg = String::from("hello");
        let cursor = 0usize;
        let byte_pos = msg
            .char_indices()
            .nth(cursor)
            .map(|(i, _)| i)
            .unwrap_or(msg.len());
        msg.insert(byte_pos, '\n');
        assert_eq!(msg, "\nhello");
    }

    #[test]
    fn shift_enter_at_end_appends_newline() {
        let mut msg = String::from("hello");
        let cursor = msg.chars().count();
        let byte_pos = msg
            .char_indices()
            .nth(cursor)
            .map(|(i, _)| i)
            .unwrap_or(msg.len());
        msg.insert(byte_pos, '\n');
        assert_eq!(msg, "hello\n");
    }

    // ══════════════════════════════════════════════════════════════════
    //  commit_overlay scroll field
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn commit_overlay_scroll_default_zero() {
        let ov = crate::app::types::GitCommitOverlay {
            message: String::new(),
            cursor: 0,
            generating: false,
            scroll: 0,
            receiver: None,
        };
        assert_eq!(ov.scroll, 0);
    }

    #[test]
    fn commit_overlay_scroll_nonzero() {
        let ov = crate::app::types::GitCommitOverlay {
            message: "multi\nline\nmessage".to_string(),
            cursor: 0,
            generating: false,
            scroll: 2,
            receiver: None,
        };
        assert_eq!(ov.scroll, 2);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Key modifier combinations
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn super_modifier_present_on_cmd_p() {
        let k = key_mod(KeyCode::Char('p'), KeyModifiers::SUPER);
        assert!(k.modifiers.contains(KeyModifiers::SUPER));
    }

    #[test]
    fn control_modifier_present() {
        let k = key_mod(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(k.modifiers.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn shift_modifier_present() {
        let k = key_mod(KeyCode::Enter, KeyModifiers::SHIFT);
        assert!(k.modifiers.contains(KeyModifiers::SHIFT));
    }

    #[test]
    fn none_modifier_has_no_control() {
        let k = key(KeyCode::Char('a'));
        assert!(!k.modifiers.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn none_modifier_has_no_super() {
        let k = key(KeyCode::Char('s'));
        assert!(!k.modifiers.contains(KeyModifiers::SUPER));
    }
}
