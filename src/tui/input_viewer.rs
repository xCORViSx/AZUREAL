//! Viewer input handling
//!
//! Handles keyboard input when the Viewer panel is focused.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Focus};

/// Handle keyboard input for the Viewer panel
pub fn handle_viewer_input(key: KeyEvent, app: &mut App) -> Result<()> {
    match (key.modifiers, key.code) {
        // Scroll: j/k or arrow keys
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            app.scroll_viewer_down(1);
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            app.scroll_viewer_up(1);
        }

        // Half-page scroll (uses cached viewport height)
        (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
            app.scroll_viewer_down(app.viewer_viewport_height / 2);
        }
        (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
            app.scroll_viewer_up(app.viewer_viewport_height / 2);
        }

        // Full-page scroll (uses cached viewport height)
        (KeyModifiers::CONTROL, KeyCode::Char('f')) | (KeyModifiers::NONE, KeyCode::PageDown) => {
            app.scroll_viewer_down(app.viewer_viewport_height.saturating_sub(2));
        }
        (KeyModifiers::CONTROL, KeyCode::Char('b')) | (KeyModifiers::NONE, KeyCode::PageUp) => {
            app.scroll_viewer_up(app.viewer_viewport_height.saturating_sub(2));
        }

        // Home/End
        (KeyModifiers::NONE, KeyCode::Home) | (KeyModifiers::NONE, KeyCode::Char('g')) => {
            app.viewer_scroll = 0;
        }
        (KeyModifiers::SHIFT, KeyCode::Char('G')) | (KeyModifiers::NONE, KeyCode::End) => {
            app.scroll_viewer_to_bottom();
        }

        // Escape: clear viewer and return to file tree
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.clear_viewer();
            app.focus = Focus::FileTree;
        }

        // q: close viewer without clearing
        (KeyModifiers::NONE, KeyCode::Char('q')) => {
            app.focus = Focus::FileTree;
        }

        _ => {}
    }

    Ok(())
}
