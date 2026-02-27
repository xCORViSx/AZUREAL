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
                    let idx = overlay.input_buffer.char_indices()
                        .nth(overlay.input_cursor - 1)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    let end = overlay.input_buffer.char_indices()
                        .nth(overlay.input_cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(overlay.input_buffer.len());
                    overlay.input_buffer.replace_range(idx..end, "");
                    overlay.input_cursor -= 1;
                }
            }
            (KeyModifiers::NONE, KeyCode::Left) => {
                if overlay.input_cursor > 0 { overlay.input_cursor -= 1; }
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                let len = overlay.input_buffer.chars().count();
                if overlay.input_cursor < len { overlay.input_cursor += 1; }
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                let byte_idx = overlay.input_buffer.char_indices()
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
            let enabled: Vec<String> = overlay.files.iter()
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
            if overlay.selected > 0 { overlay.selected -= 1; }
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
