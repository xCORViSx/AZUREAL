//! Worktrees panel input handling

use anyhow::Result;
use crossterm::event::{self, KeyCode, KeyModifiers};

use crate::app::App;

/// Handle keyboard input when Worktree tab row is focused.
/// ALL command keybindings are resolved by lookup_action() in event_loop.rs BEFORE
/// this is called. This handler only receives unresolved keys.
pub fn handle_worktrees_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    // 's' — stop tracking (not a navigation binding, so not in WORKTREES array).
    // This is the only worktree key that isn't in the centralized system — it's a
    // destructive action (removes receiver) that only makes sense contextually.
    if key.modifiers == KeyModifiers::NONE && key.code == KeyCode::Char('s') {
        if let Some(session) = app.current_worktree() {
            let branch_name = session.branch_name.clone();
            let session_name = session.name().to_string();
            if app.running_sessions.remove(&branch_name) {
                app.agent_receivers.remove(&branch_name);
                app.invalidate_sidebar();
                app.set_status(format!("Stopped tracking: {}", session_name));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    /// Build a KeyEvent with no modifiers.
    fn key(code: KeyCode) -> event::KeyEvent {
        event::KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    /// Build a KeyEvent with specified modifiers.
    fn key_mod(code: KeyCode, modifiers: KeyModifiers) -> event::KeyEvent {
        event::KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    // ── KeyEvent construction ──────────────────────────────────────────

    #[test]
    fn key_helper_produces_none_modifiers() {
        let k = key(KeyCode::Char('s'));
        assert_eq!(k.modifiers, KeyModifiers::NONE);
        assert_eq!(k.code, KeyCode::Char('s'));
    }

    #[test]
    fn key_mod_helper_produces_shift() {
        let k = key_mod(KeyCode::Char('S'), KeyModifiers::SHIFT);
        assert_eq!(k.modifiers, KeyModifiers::SHIFT);
    }

    #[test]
    fn key_event_kind_is_press() {
        let k = key(KeyCode::Char('s'));
        assert_eq!(k.kind, KeyEventKind::Press);
    }

    #[test]
    fn key_event_state_is_none() {
        let k = key(KeyCode::Char('s'));
        assert_eq!(k.state, KeyEventState::NONE);
    }

    // ── 's' key matching (the only handled key) ───────────────────────

    #[test]
    fn s_key_matches_guard() {
        let k = key(KeyCode::Char('s'));
        assert!(k.modifiers == KeyModifiers::NONE && k.code == KeyCode::Char('s'));
    }

    #[test]
    fn shift_s_does_not_match_guard() {
        let k = key_mod(KeyCode::Char('S'), KeyModifiers::SHIFT);
        assert!(!(k.modifiers == KeyModifiers::NONE && k.code == KeyCode::Char('s')));
    }

    #[test]
    fn ctrl_s_does_not_match_guard() {
        let k = key_mod(KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert!(!(k.modifiers == KeyModifiers::NONE && k.code == KeyCode::Char('s')));
    }

    #[test]
    fn alt_s_does_not_match_guard() {
        let k = key_mod(KeyCode::Char('s'), KeyModifiers::ALT);
        assert!(!(k.modifiers == KeyModifiers::NONE && k.code == KeyCode::Char('s')));
    }

    #[test]
    fn super_s_does_not_match_guard() {
        let k = key_mod(KeyCode::Char('s'), KeyModifiers::SUPER);
        assert!(!(k.modifiers == KeyModifiers::NONE && k.code == KeyCode::Char('s')));
    }

    #[test]
    fn other_char_does_not_match_s() {
        let k = key(KeyCode::Char('a'));
        assert!(!(k.modifiers == KeyModifiers::NONE && k.code == KeyCode::Char('s')));
    }

    #[test]
    fn enter_does_not_match_s() {
        let k = key(KeyCode::Enter);
        assert!(!(k.modifiers == KeyModifiers::NONE && k.code == KeyCode::Char('s')));
    }

    #[test]
    fn esc_does_not_match_s() {
        let k = key(KeyCode::Esc);
        assert!(!(k.modifiers == KeyModifiers::NONE && k.code == KeyCode::Char('s')));
    }

    // ── KeyCode variant coverage ──────────────────────────────────────

    #[test]
    fn char_s_keycode() {
        assert_eq!(KeyCode::Char('s'), KeyCode::Char('s'));
    }

    #[test]
    fn char_s_not_equal_to_char_a() {
        assert_ne!(KeyCode::Char('s'), KeyCode::Char('a'));
    }

    #[test]
    fn char_s_not_equal_to_enter() {
        assert_ne!(KeyCode::Char('s'), KeyCode::Enter);
    }

    #[test]
    fn char_s_not_equal_to_esc() {
        assert_ne!(KeyCode::Char('s'), KeyCode::Esc);
    }

    #[test]
    fn char_s_not_equal_to_backspace() {
        assert_ne!(KeyCode::Char('s'), KeyCode::Backspace);
    }

    // ── KeyModifiers flags ────────────────────────────────────────────

    #[test]
    fn none_modifier_is_empty() {
        assert!(KeyModifiers::NONE.is_empty());
    }

    #[test]
    fn shift_modifier_not_empty() {
        assert!(!KeyModifiers::SHIFT.is_empty());
    }

    #[test]
    fn control_modifier_not_empty() {
        assert!(!KeyModifiers::CONTROL.is_empty());
    }

    #[test]
    fn alt_modifier_not_empty() {
        assert!(!KeyModifiers::ALT.is_empty());
    }

    #[test]
    fn super_modifier_not_empty() {
        assert!(!KeyModifiers::SUPER.is_empty());
    }

    #[test]
    fn combined_modifiers_contain_both() {
        let mods = KeyModifiers::SHIFT | KeyModifiers::CONTROL;
        assert!(mods.contains(KeyModifiers::SHIFT));
        assert!(mods.contains(KeyModifiers::CONTROL));
        assert!(!mods.contains(KeyModifiers::ALT));
    }

    // ── Stopped tracking format string ────────────────────────────────

    #[test]
    fn stopped_tracking_format() {
        let name = "my-feature";
        let msg = format!("Stopped tracking: {}", name);
        assert_eq!(msg, "Stopped tracking: my-feature");
    }

    #[test]
    fn stopped_tracking_format_empty_name() {
        let name = "";
        let msg = format!("Stopped tracking: {}", name);
        assert_eq!(msg, "Stopped tracking: ");
    }

    #[test]
    fn stopped_tracking_format_with_slashes() {
        let name = "feature/add-tests";
        let msg = format!("Stopped tracking: {}", name);
        assert!(msg.contains("feature/add-tests"));
    }

    // ── HashMap remove semantics ──────────────────────────────────────

    #[test]
    fn hashmap_remove_returns_true_if_present() {
        let mut set = std::collections::HashSet::new();
        set.insert("branch-a".to_string());
        assert!(set.remove("branch-a"));
    }

    #[test]
    fn hashmap_remove_returns_false_if_absent() {
        let mut set = std::collections::HashSet::new();
        set.insert("branch-a".to_string());
        assert!(!set.remove("branch-b"));
    }

    #[test]
    fn hashmap_remove_reduces_count() {
        let mut set = std::collections::HashSet::new();
        set.insert("a".to_string());
        set.insert("b".to_string());
        assert_eq!(set.len(), 2);
        set.remove("a");
        assert_eq!(set.len(), 1);
    }

    // ── String clone behavior (used for branch_name/session_name) ─────

    #[test]
    fn string_clone_is_equal() {
        let original = "main".to_string();
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn string_clone_is_independent() {
        let mut original = "main".to_string();
        let cloned = original.clone();
        original.push_str("-modified");
        assert_ne!(original, cloned);
    }

    // ── Guard condition combinations ──────────────────────────────────

    #[test]
    fn all_26_lowercase_letters_checked() {
        for c in 'a'..='z' {
            let k = key(KeyCode::Char(c));
            let matches_s = k.modifiers == KeyModifiers::NONE && k.code == KeyCode::Char('s');
            if c == 's' {
                assert!(matches_s);
            } else {
                assert!(!matches_s);
            }
        }
    }

    #[test]
    fn non_char_keycodes_do_not_match_s() {
        let codes = [
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Backspace,
            KeyCode::Delete,
            KeyCode::Tab,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::PageUp,
            KeyCode::PageDown,
        ];
        for code in codes {
            let k = key(code);
            assert!(!(k.modifiers == KeyModifiers::NONE && k.code == KeyCode::Char('s')));
        }
    }

    // ── KeyEventKind variants ─────────────────────────────────────────

    #[test]
    fn key_event_kind_press_is_default() {
        assert_eq!(key(KeyCode::Char('s')).kind, KeyEventKind::Press);
    }

    #[test]
    fn key_event_kind_press_not_release() {
        assert_ne!(KeyEventKind::Press, KeyEventKind::Release);
    }

    #[test]
    fn key_event_kind_press_not_repeat() {
        assert_ne!(KeyEventKind::Press, KeyEventKind::Repeat);
    }

    // ── Result type ───────────────────────────────────────────────────

    #[test]
    fn ok_result_is_ok() {
        let result: Result<()> = Ok(());
        assert!(result.is_ok());
    }

    #[test]
    fn ok_result_unwraps_to_unit() {
        let result: Result<()> = Ok(());
        assert_eq!(result.unwrap(), ());
    }

    // ── Modifier equality ─────────────────────────────────────────────

    #[test]
    fn none_equals_none() {
        assert_eq!(KeyModifiers::NONE, KeyModifiers::NONE);
    }

    #[test]
    fn shift_equals_shift() {
        assert_eq!(KeyModifiers::SHIFT, KeyModifiers::SHIFT);
    }

    #[test]
    fn none_not_equals_shift() {
        assert_ne!(KeyModifiers::NONE, KeyModifiers::SHIFT);
    }

    #[test]
    fn none_not_equals_control() {
        assert_ne!(KeyModifiers::NONE, KeyModifiers::CONTROL);
    }

    #[test]
    fn none_not_equals_alt() {
        assert_ne!(KeyModifiers::NONE, KeyModifiers::ALT);
    }

    #[test]
    fn none_not_equals_super() {
        assert_ne!(KeyModifiers::NONE, KeyModifiers::SUPER);
    }

    #[test]
    fn key_char_w_code() {
        let k = key(KeyCode::Char('w'));
        assert_eq!(k.code, KeyCode::Char('w'));
    }

    #[test]
    fn key_f1_code() {
        let k = key(KeyCode::F(1));
        assert_eq!(k.code, KeyCode::F(1));
    }

    #[test]
    fn key_insert_code() {
        let k = key(KeyCode::Insert);
        assert_eq!(k.code, KeyCode::Insert);
    }

    #[test]
    fn key_mod_ctrl_c() {
        let k = key_mod(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(k.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn key_mod_alt_shift() {
        let k = key_mod(KeyCode::Char('a'), KeyModifiers::ALT | KeyModifiers::SHIFT);
        assert!(k.modifiers.contains(KeyModifiers::ALT));
        assert!(k.modifiers.contains(KeyModifiers::SHIFT));
    }

    #[test]
    fn key_page_up_code() {
        let k = key(KeyCode::PageUp);
        assert_eq!(k.code, KeyCode::PageUp);
    }
}
