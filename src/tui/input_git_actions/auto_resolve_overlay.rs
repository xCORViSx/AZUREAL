//! Auto-resolve settings overlay input handling.
//!
//! Manages the list of files that should be auto-resolved via union merge
//! during rebase conflicts. j/k navigate, Space toggles, a adds, d removes,
//! Esc saves and closes.

use anyhow::Result;
use crossterm::event;

use crate::app::App;

/// Handle input while the auto-resolve settings overlay is open.
pub(super) fn handle_auto_resolve_overlay(key: event::KeyEvent, app: &mut App) -> Result<()> {
    use crossterm::event::{KeyCode, KeyModifiers};

    let panel = match app.git_actions_panel.as_mut() {
        Some(p) => p,
        None => return Ok(()),
    };
    let overlay = match panel.auto_resolve_overlay.as_mut() {
        Some(o) => o,
        None => return Ok(()),
    };

    // Add mode: typing a new filename
    if overlay.adding {
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Esc) => {
                overlay.adding = false;
                overlay.input_buffer.clear();
                overlay.input_cursor = 0;
            }
            (KeyModifiers::NONE, KeyCode::Enter) => {
                let name = overlay.input_buffer.trim().to_string();
                if !name.is_empty() && !overlay.files.iter().any(|(f, _)| f == &name) {
                    overlay.files.push((name, true));
                    overlay.selected = overlay.files.len() - 1;
                }
                overlay.adding = false;
                overlay.input_buffer.clear();
                overlay.input_cursor = 0;
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                if overlay.input_cursor > 0 {
                    let idx = overlay
                        .input_buffer
                        .char_indices()
                        .nth(overlay.input_cursor - 1)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    let end = overlay
                        .input_buffer
                        .char_indices()
                        .nth(overlay.input_cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(overlay.input_buffer.len());
                    overlay.input_buffer.replace_range(idx..end, "");
                    overlay.input_cursor -= 1;
                }
            }
            (KeyModifiers::NONE, KeyCode::Left) => {
                if overlay.input_cursor > 0 {
                    overlay.input_cursor -= 1;
                }
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                let len = overlay.input_buffer.chars().count();
                if overlay.input_cursor < len {
                    overlay.input_cursor += 1;
                }
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                let byte_idx = overlay
                    .input_buffer
                    .char_indices()
                    .nth(overlay.input_cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(overlay.input_buffer.len());
                overlay.input_buffer.insert(byte_idx, c);
                overlay.input_cursor += 1;
            }
            _ => {}
        }
        return Ok(());
    }

    // Normal mode: navigate/toggle/add/remove
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Esc) => {
            // Save to azufig, update panel cache, close overlay
            let enabled: Vec<String> = overlay
                .files
                .iter()
                .filter(|(_, on)| *on)
                .map(|(f, _)| f.clone())
                .collect();
            let repo_root = panel.repo_root.clone();
            crate::azufig::save_auto_resolve_files(&repo_root, &enabled);
            panel.auto_resolve_files = enabled;
            panel.auto_resolve_overlay = None;
        }
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            if !overlay.files.is_empty() && overlay.selected + 1 < overlay.files.len() {
                overlay.selected += 1;
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            if overlay.selected > 0 {
                overlay.selected -= 1;
            }
        }
        (KeyModifiers::NONE, KeyCode::Char(' ')) => {
            if let Some(entry) = overlay.files.get_mut(overlay.selected) {
                entry.1 = !entry.1;
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('a')) => {
            overlay.adding = true;
            overlay.input_buffer.clear();
            overlay.input_cursor = 0;
        }
        (KeyModifiers::NONE, KeyCode::Char('d')) => {
            if !overlay.files.is_empty() {
                overlay.files.remove(overlay.selected);
                if overlay.selected >= overlay.files.len() && overlay.selected > 0 {
                    overlay.selected -= 1;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::{AutoResolveOverlay, GitActionsPanel};
    use crate::app::App;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// Create a KeyEvent from a code and modifiers
    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    /// Create an App with a GitActionsPanel and AutoResolveOverlay pre-populated
    fn app_with_overlay(files: Vec<(String, bool)>, adding: bool) -> App {
        let mut app = App::new();
        app.git_actions_panel = Some(GitActionsPanel {
            worktree_name: "test".into(),
            worktree_path: std::path::PathBuf::from("/tmp/test-wt"),
            repo_root: std::path::PathBuf::from("/tmp/test-repo"),
            main_branch: "main".into(),
            is_on_main: false,
            changed_files: Vec::new(),
            selected_file: 0,
            file_scroll: 0,
            focused_pane: 0,
            selected_action: 0,
            result_message: None,
            commit_overlay: None,
            conflict_overlay: None,
            commits: Vec::new(),
            selected_commit: 0,
            commit_scroll: 0,
            viewer_diff: None,
            viewer_diff_title: None,
            commits_behind_main: 0,
            commits_ahead_main: 0,
            commits_behind_remote: 0,
            commits_ahead_remote: 0,
            auto_resolve_files: Vec::new(),
            auto_resolve_overlay: Some(AutoResolveOverlay {
                files,
                selected: 0,
                adding,
                input_buffer: String::new(),
                input_cursor: 0,
            }),
            squash_merge_receiver: None,
            discard_confirm: None,
        });
        app
    }

    // ── 1. Add mode: Esc cancels adding ──

    #[test]
    fn test_add_mode_esc_cancels_adding() {
        let mut app = app_with_overlay(vec![], true);
        handle_auto_resolve_overlay(key(KeyCode::Esc, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(!ov.adding);
    }

    #[test]
    fn test_add_mode_esc_clears_buffer() {
        let mut app = app_with_overlay(vec![], true);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.input_buffer = "partial".into();
            ov.input_cursor = 7;
        }
        handle_auto_resolve_overlay(key(KeyCode::Esc, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(ov.input_buffer.is_empty());
        assert_eq!(ov.input_cursor, 0);
    }

    // ── 2. Add mode: Enter adds non-empty name ──

    #[test]
    fn test_add_mode_enter_adds_file() {
        let mut app = app_with_overlay(vec![], true);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.input_buffer = "Cargo.lock".into();
            ov.input_cursor = 10;
        }
        handle_auto_resolve_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(!ov.adding);
        assert_eq!(ov.files.len(), 1);
        assert_eq!(ov.files[0].0, "Cargo.lock");
        assert!(ov.files[0].1); // enabled by default
        assert_eq!(ov.selected, 0);
    }

    #[test]
    fn test_add_mode_enter_empty_does_not_add() {
        let mut app = app_with_overlay(vec![], true);
        handle_auto_resolve_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(ov.files.is_empty());
    }

    #[test]
    fn test_add_mode_enter_whitespace_only_does_not_add() {
        let mut app = app_with_overlay(vec![], true);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.input_buffer = "   ".into();
            ov.input_cursor = 3;
        }
        handle_auto_resolve_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(ov.files.is_empty());
    }

    #[test]
    fn test_add_mode_enter_duplicate_does_not_add() {
        let mut app = app_with_overlay(vec![("Cargo.lock".into(), true)], true);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.input_buffer = "Cargo.lock".into();
            ov.input_cursor = 10;
        }
        handle_auto_resolve_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.files.len(), 1); // no duplicate
    }

    // ── 3. Add mode: Backspace deletes character ──

    #[test]
    fn test_add_mode_backspace_deletes_char() {
        let mut app = app_with_overlay(vec![], true);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.input_buffer = "abc".into();
            ov.input_cursor = 3;
        }
        handle_auto_resolve_overlay(key(KeyCode::Backspace, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.input_buffer, "ab");
        assert_eq!(ov.input_cursor, 2);
    }

    #[test]
    fn test_add_mode_backspace_at_start_noop() {
        let mut app = app_with_overlay(vec![], true);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.input_buffer = "abc".into();
            ov.input_cursor = 0;
        }
        handle_auto_resolve_overlay(key(KeyCode::Backspace, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.input_buffer, "abc");
        assert_eq!(ov.input_cursor, 0);
    }

    // ── 4. Add mode: Left/Right arrow keys ──

    #[test]
    fn test_add_mode_left_arrow() {
        let mut app = app_with_overlay(vec![], true);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.input_buffer = "abc".into();
            ov.input_cursor = 2;
        }
        handle_auto_resolve_overlay(key(KeyCode::Left, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.input_cursor, 1);
    }

    #[test]
    fn test_add_mode_left_arrow_at_zero_noop() {
        let mut app = app_with_overlay(vec![], true);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.input_buffer = "abc".into();
            ov.input_cursor = 0;
        }
        handle_auto_resolve_overlay(key(KeyCode::Left, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.input_cursor, 0);
    }

    #[test]
    fn test_add_mode_right_arrow() {
        let mut app = app_with_overlay(vec![], true);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.input_buffer = "abc".into();
            ov.input_cursor = 1;
        }
        handle_auto_resolve_overlay(key(KeyCode::Right, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.input_cursor, 2);
    }

    #[test]
    fn test_add_mode_right_arrow_at_end_noop() {
        let mut app = app_with_overlay(vec![], true);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.input_buffer = "abc".into();
            ov.input_cursor = 3;
        }
        handle_auto_resolve_overlay(key(KeyCode::Right, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.input_cursor, 3);
    }

    // ── 5. Add mode: Char input ──

    #[test]
    fn test_add_mode_char_input() {
        let mut app = app_with_overlay(vec![], true);
        handle_auto_resolve_overlay(key(KeyCode::Char('a'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.input_buffer, "a");
        assert_eq!(ov.input_cursor, 1);
    }

    #[test]
    fn test_add_mode_char_input_at_middle() {
        let mut app = app_with_overlay(vec![], true);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.input_buffer = "ac".into();
            ov.input_cursor = 1;
        }
        handle_auto_resolve_overlay(key(KeyCode::Char('b'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.input_buffer, "abc");
        assert_eq!(ov.input_cursor, 2);
    }

    #[test]
    fn test_add_mode_shift_char_input() {
        let mut app = app_with_overlay(vec![], true);
        handle_auto_resolve_overlay(key(KeyCode::Char('A'), KeyModifiers::SHIFT), &mut app)
            .unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.input_buffer, "A");
    }

    // ── 6. Normal mode: j/Down navigates down ──

    #[test]
    fn test_normal_j_moves_down() {
        let mut app = app_with_overlay(
            vec![("a".into(), true), ("b".into(), true), ("c".into(), true)],
            false,
        );
        handle_auto_resolve_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.selected, 1);
    }

    #[test]
    fn test_normal_down_arrow_moves_down() {
        let mut app = app_with_overlay(vec![("a".into(), true), ("b".into(), true)], false);
        handle_auto_resolve_overlay(key(KeyCode::Down, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.selected, 1);
    }

    #[test]
    fn test_normal_j_at_end_stays() {
        let mut app = app_with_overlay(vec![("a".into(), true), ("b".into(), true)], false);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.selected = 1;
        }
        handle_auto_resolve_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.selected, 1);
    }

    #[test]
    fn test_normal_j_empty_list_noop() {
        let mut app = app_with_overlay(vec![], false);
        handle_auto_resolve_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.selected, 0);
    }

    // ── 7. Normal mode: k/Up navigates up ──

    #[test]
    fn test_normal_k_moves_up() {
        let mut app = app_with_overlay(vec![("a".into(), true), ("b".into(), true)], false);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.selected = 1;
        }
        handle_auto_resolve_overlay(key(KeyCode::Char('k'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.selected, 0);
    }

    #[test]
    fn test_normal_up_arrow_moves_up() {
        let mut app = app_with_overlay(vec![("a".into(), true), ("b".into(), true)], false);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.selected = 1;
        }
        handle_auto_resolve_overlay(key(KeyCode::Up, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.selected, 0);
    }

    #[test]
    fn test_normal_k_at_top_stays() {
        let mut app = app_with_overlay(vec![("a".into(), true)], false);
        handle_auto_resolve_overlay(key(KeyCode::Char('k'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.selected, 0);
    }

    // ── 8. Normal mode: Space toggles ──

    #[test]
    fn test_normal_space_toggles_on_to_off() {
        let mut app = app_with_overlay(vec![("f".into(), true)], false);
        handle_auto_resolve_overlay(key(KeyCode::Char(' '), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(!ov.files[0].1);
    }

    #[test]
    fn test_normal_space_toggles_off_to_on() {
        let mut app = app_with_overlay(vec![("f".into(), false)], false);
        handle_auto_resolve_overlay(key(KeyCode::Char(' '), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(ov.files[0].1);
    }

    #[test]
    fn test_normal_space_empty_list_noop() {
        let mut app = app_with_overlay(vec![], false);
        handle_auto_resolve_overlay(key(KeyCode::Char(' '), KeyModifiers::NONE), &mut app).unwrap();
        // No panic
    }

    // ── 9. Normal mode: 'a' enters add mode ──

    #[test]
    fn test_normal_a_enters_add_mode() {
        let mut app = app_with_overlay(vec![], false);
        handle_auto_resolve_overlay(key(KeyCode::Char('a'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(ov.adding);
        assert!(ov.input_buffer.is_empty());
        assert_eq!(ov.input_cursor, 0);
    }

    // ── 10. Normal mode: 'd' removes entry ──

    #[test]
    fn test_normal_d_removes_entry() {
        let mut app = app_with_overlay(vec![("a".into(), true), ("b".into(), true)], false);
        handle_auto_resolve_overlay(key(KeyCode::Char('d'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.files.len(), 1);
        assert_eq!(ov.files[0].0, "b");
    }

    #[test]
    fn test_normal_d_removes_last_adjusts_selected() {
        let mut app = app_with_overlay(vec![("a".into(), true), ("b".into(), true)], false);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.selected = 1;
        }
        handle_auto_resolve_overlay(key(KeyCode::Char('d'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.files.len(), 1);
        assert_eq!(ov.selected, 0); // adjusted down
    }

    #[test]
    fn test_normal_d_empty_list_noop() {
        let mut app = app_with_overlay(vec![], false);
        handle_auto_resolve_overlay(key(KeyCode::Char('d'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(ov.files.is_empty());
    }

    #[test]
    fn test_normal_d_single_item_clears_list() {
        let mut app = app_with_overlay(vec![("only".into(), true)], false);
        handle_auto_resolve_overlay(key(KeyCode::Char('d'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(ov.files.is_empty());
        assert_eq!(ov.selected, 0);
    }

    // ── 11. No panel: early return ──

    #[test]
    fn test_no_panel_returns_ok() {
        let mut app = App::new();
        let result = handle_auto_resolve_overlay(key(KeyCode::Esc, KeyModifiers::NONE), &mut app);
        assert!(result.is_ok());
    }

    // ── 12. No overlay: early return ──

    #[test]
    fn test_no_overlay_returns_ok() {
        let mut app = app_with_overlay(vec![], false);
        app.git_actions_panel.as_mut().unwrap().auto_resolve_overlay = None;
        let result = handle_auto_resolve_overlay(key(KeyCode::Esc, KeyModifiers::NONE), &mut app);
        assert!(result.is_ok());
    }

    // ── 13. Add mode: typing a full filename ──

    #[test]
    fn test_add_mode_type_full_name_and_submit() {
        let mut app = app_with_overlay(vec![], true);
        for c in "test.rs".chars() {
            handle_auto_resolve_overlay(key(KeyCode::Char(c), KeyModifiers::NONE), &mut app)
                .unwrap();
        }
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.input_buffer, "test.rs");
        assert_eq!(ov.input_cursor, 7);

        handle_auto_resolve_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.files.len(), 1);
        assert_eq!(ov.files[0].0, "test.rs");
    }

    // ── 14. Normal mode: navigate then toggle ──

    #[test]
    fn test_navigate_then_toggle() {
        let mut app = app_with_overlay(vec![("a".into(), true), ("b".into(), false)], false);
        handle_auto_resolve_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app).unwrap();
        handle_auto_resolve_overlay(key(KeyCode::Char(' '), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(ov.files[1].1); // toggled from false to true
        assert!(ov.files[0].1); // unchanged
    }

    // ── 15. Add mode: backspace mid-string ──

    #[test]
    fn test_add_mode_backspace_mid_string() {
        let mut app = app_with_overlay(vec![], true);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.input_buffer = "abcd".into();
            ov.input_cursor = 2; // cursor after 'b'
        }
        handle_auto_resolve_overlay(key(KeyCode::Backspace, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.input_buffer, "acd");
        assert_eq!(ov.input_cursor, 1);
    }

    // ── 16. Normal mode: unknown key is no-op ──

    #[test]
    fn test_normal_unknown_key_noop() {
        let mut app = app_with_overlay(vec![("f".into(), true)], false);
        handle_auto_resolve_overlay(key(KeyCode::Char('x'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.files.len(), 1);
        assert_eq!(ov.selected, 0);
    }

    // ── 17. Add mode: unknown key in add mode is no-op ──

    #[test]
    fn test_add_mode_ctrl_key_noop() {
        let mut app = app_with_overlay(vec![], true);
        handle_auto_resolve_overlay(key(KeyCode::Char('a'), KeyModifiers::CONTROL), &mut app)
            .unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(ov.input_buffer.is_empty());
    }

    // ── 18. Enter after adding positions selected at new entry ──

    #[test]
    fn test_add_selects_new_entry() {
        let mut app = app_with_overlay(vec![("a".into(), true), ("b".into(), true)], true);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.input_buffer = "c".into();
            ov.input_cursor = 1;
        }
        handle_auto_resolve_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.selected, 2); // new entry at index 2
    }

    // ── 19. Delete middle entry adjusts selection ──

    #[test]
    fn test_delete_middle_entry() {
        let mut app = app_with_overlay(
            vec![("a".into(), true), ("b".into(), true), ("c".into(), true)],
            false,
        );
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.selected = 1;
        }
        handle_auto_resolve_overlay(key(KeyCode::Char('d'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.files.len(), 2);
        // "b" removed, now ["a", "c"], selected stays at 1
        assert_eq!(ov.files[1].0, "c");
    }

    // ── 20. Add mode: enter clears buffer and cursor ──

    #[test]
    fn test_add_mode_enter_clears_buffer() {
        let mut app = app_with_overlay(vec![], true);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.input_buffer = "file.txt".into();
            ov.input_cursor = 8;
        }
        handle_auto_resolve_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(ov.input_buffer.is_empty());
        assert_eq!(ov.input_cursor, 0);
    }

    // ── 21. Multiple add/delete cycles ──

    #[test]
    fn test_add_delete_cycle() {
        let mut app = app_with_overlay(vec![], false);
        // Enter add mode, add "x"
        handle_auto_resolve_overlay(key(KeyCode::Char('a'), KeyModifiers::NONE), &mut app).unwrap();
        handle_auto_resolve_overlay(key(KeyCode::Char('x'), KeyModifiers::NONE), &mut app).unwrap();
        handle_auto_resolve_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.files.len(), 1);

        // Delete it
        handle_auto_resolve_overlay(key(KeyCode::Char('d'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(ov.files.is_empty());
    }

    // ── 22. Toggle second item in multi-item list ──

    #[test]
    fn test_toggle_second_item() {
        let mut app = app_with_overlay(
            vec![("a".into(), true), ("b".into(), true), ("c".into(), false)],
            false,
        );
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.selected = 2;
        }
        handle_auto_resolve_overlay(key(KeyCode::Char(' '), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(ov.files[2].1);
        assert!(ov.files[0].1); // unchanged
    }

    // ── 23. Add mode: unicode char input ──

    #[test]
    fn test_add_mode_unicode_char() {
        let mut app = app_with_overlay(vec![], true);
        handle_auto_resolve_overlay(key(KeyCode::Char('é'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.input_buffer, "é");
        assert_eq!(ov.input_cursor, 1);
    }

    // ── 24. Add mode: multiple chars then backspace ──

    #[test]
    fn test_add_mode_multiple_chars_then_backspace() {
        let mut app = app_with_overlay(vec![], true);
        for c in "hello".chars() {
            handle_auto_resolve_overlay(key(KeyCode::Char(c), KeyModifiers::NONE), &mut app)
                .unwrap();
        }
        handle_auto_resolve_overlay(key(KeyCode::Backspace, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.input_buffer, "hell");
        assert_eq!(ov.input_cursor, 4);
    }

    // ── 25. Navigate full list with j/k ──

    #[test]
    fn test_navigate_full_list() {
        let mut app = app_with_overlay(
            vec![("a".into(), true), ("b".into(), true), ("c".into(), true)],
            false,
        );
        // Move to end
        handle_auto_resolve_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app).unwrap();
        handle_auto_resolve_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.selected, 2);
        // Move back to start
        handle_auto_resolve_overlay(key(KeyCode::Char('k'), KeyModifiers::NONE), &mut app).unwrap();
        handle_auto_resolve_overlay(key(KeyCode::Char('k'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.selected, 0);
    }

    // ── 26. Add mode: insert at beginning of buffer ──

    #[test]
    fn test_add_mode_insert_at_beginning() {
        let mut app = app_with_overlay(vec![], true);
        {
            let ov = app
                .git_actions_panel
                .as_mut()
                .unwrap()
                .auto_resolve_overlay
                .as_mut()
                .unwrap();
            ov.input_buffer = "bc".into();
            ov.input_cursor = 0;
        }
        handle_auto_resolve_overlay(key(KeyCode::Char('a'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.input_buffer, "abc");
        assert_eq!(ov.input_cursor, 1);
    }

    // ── 27. Delete all items one by one ──

    #[test]
    fn test_delete_all_items() {
        let mut app = app_with_overlay(
            vec![("a".into(), true), ("b".into(), true), ("c".into(), true)],
            false,
        );
        handle_auto_resolve_overlay(key(KeyCode::Char('d'), KeyModifiers::NONE), &mut app).unwrap();
        handle_auto_resolve_overlay(key(KeyCode::Char('d'), KeyModifiers::NONE), &mut app).unwrap();
        handle_auto_resolve_overlay(key(KeyCode::Char('d'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(ov.files.is_empty());
    }

    // ── 28. Toggle all items off then on ──

    #[test]
    fn test_toggle_all_off_then_on() {
        let mut app = app_with_overlay(vec![("a".into(), true), ("b".into(), true)], false);
        // Toggle first off
        handle_auto_resolve_overlay(key(KeyCode::Char(' '), KeyModifiers::NONE), &mut app).unwrap();
        // Move down and toggle second off
        handle_auto_resolve_overlay(key(KeyCode::Char('j'), KeyModifiers::NONE), &mut app).unwrap();
        handle_auto_resolve_overlay(key(KeyCode::Char(' '), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(!ov.files[0].1);
        assert!(!ov.files[1].1);

        // Toggle second back on
        handle_auto_resolve_overlay(key(KeyCode::Char(' '), KeyModifiers::NONE), &mut app).unwrap();
        // Move up and toggle first back on
        handle_auto_resolve_overlay(key(KeyCode::Char('k'), KeyModifiers::NONE), &mut app).unwrap();
        handle_auto_resolve_overlay(key(KeyCode::Char(' '), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(ov.files[0].1);
        assert!(ov.files[1].1);
    }

    // ── 29. Add mode: Tab key is no-op ──

    #[test]
    fn test_add_mode_tab_noop() {
        let mut app = app_with_overlay(vec![], true);
        handle_auto_resolve_overlay(key(KeyCode::Tab, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(ov.input_buffer.is_empty());
    }

    // ── 30. Normal mode: 'a' then Esc returns to normal mode ──

    #[test]
    fn test_a_then_esc_returns_to_normal() {
        let mut app = app_with_overlay(vec![], false);
        handle_auto_resolve_overlay(key(KeyCode::Char('a'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(ov.adding);
        handle_auto_resolve_overlay(key(KeyCode::Esc, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert!(!ov.adding);
    }

    // ── 31. Add then navigate and delete ──

    #[test]
    fn test_add_then_navigate_delete() {
        let mut app = app_with_overlay(vec![], false);
        // Add "x"
        handle_auto_resolve_overlay(key(KeyCode::Char('a'), KeyModifiers::NONE), &mut app).unwrap();
        handle_auto_resolve_overlay(key(KeyCode::Char('x'), KeyModifiers::NONE), &mut app).unwrap();
        handle_auto_resolve_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app).unwrap();
        // Add "y"
        handle_auto_resolve_overlay(key(KeyCode::Char('a'), KeyModifiers::NONE), &mut app).unwrap();
        handle_auto_resolve_overlay(key(KeyCode::Char('y'), KeyModifiers::NONE), &mut app).unwrap();
        handle_auto_resolve_overlay(key(KeyCode::Enter, KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.files.len(), 2);
        // Navigate to first and delete
        handle_auto_resolve_overlay(key(KeyCode::Char('k'), KeyModifiers::NONE), &mut app).unwrap();
        handle_auto_resolve_overlay(key(KeyCode::Char('d'), KeyModifiers::NONE), &mut app).unwrap();
        let ov = app
            .git_actions_panel
            .as_ref()
            .unwrap()
            .auto_resolve_overlay
            .as_ref()
            .unwrap();
        assert_eq!(ov.files.len(), 1);
    }
}
