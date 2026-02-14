//! Input handler for the God File System panel.
//! This is a full-screen modal overlay — consumes all input when active,
//! bypasses the centralized keybinding system (same pattern as Projects panel).

use anyhow::Result;
use crossterm::event::{self, KeyCode, KeyModifiers};

use crate::app::App;
use crate::claude::ClaudeProcess;

/// Handle keyboard input when the God File panel is active.
/// j/k navigate, Space toggles check, a toggles all, Enter modularizes, Esc closes.
pub fn handle_god_files_input(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    let panel = match app.god_file_panel {
        Some(ref mut p) => p,
        None => return Ok(()),
    };

    match (key.modifiers, key.code) {
        // Navigate down
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            if !panel.entries.is_empty() && panel.selected + 1 < panel.entries.len() {
                panel.selected += 1;
            }
        }
        // Navigate up
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            if panel.selected > 0 {
                panel.selected -= 1;
            }
        }
        // Jump to top
        (KeyModifiers::ALT, KeyCode::Up) => {
            panel.selected = 0;
        }
        // Jump to bottom
        (KeyModifiers::ALT, KeyCode::Down) => {
            if !panel.entries.is_empty() {
                panel.selected = panel.entries.len() - 1;
            }
        }
        // Toggle check on selected entry
        (KeyModifiers::NONE, KeyCode::Char(' ')) => {
            app.god_file_toggle_check();
        }
        // Toggle all checks
        (KeyModifiers::NONE, KeyCode::Char('a')) => {
            app.god_file_toggle_all();
        }
        // View checked files in viewer tabs (up to 12)
        (KeyModifiers::NONE, KeyCode::Char('v')) => {
            app.god_file_view_checked();
        }
        // Filter mode — open FileTree overlay to see/edit scan scope
        (KeyModifiers::NONE, KeyCode::Char('f')) => {
            app.enter_god_file_filter_mode();
        }
        // Modularize checked files (Enter or 'm')
        (KeyModifiers::NONE, KeyCode::Enter) | (KeyModifiers::NONE, KeyCode::Char('m')) => {
            app.god_file_modularize(claude_process);
        }
        // Close panel
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.close_god_file_panel();
        }
        _ => {}
    }
    Ok(())
}
