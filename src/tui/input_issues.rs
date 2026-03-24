//! Input handler for the GitHub Issues panel.
//! Full-screen modal overlay — consumes all input when active, dispatched via
//! the centralized keybinding system (lookup_issues_action in keybindings.rs).
//! Filter mode intercepts keys for text input when active.

use anyhow::Result;
use crossterm::event::{self, KeyCode};

use super::keybindings::{lookup_issues_action, Action};
use crate::app::App;

/// Handle keyboard input when the Issues panel is active.
/// Filter mode takes priority when active — raw text input.
/// Otherwise all keys resolved through keybindings.rs.
pub fn handle_issues_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    // Filter mode intercepts all input
    if let Some(ref panel) = app.issues_panel {
        if panel.filter_active {
            return handle_filter_input(key, app);
        }
    }

    // Raw key handlers (not in binding system)
    match key.code {
        KeyCode::Char('/') => {
            if let Some(ref mut panel) = app.issues_panel {
                panel.filter_active = true;
                panel.filter.clear();
                panel.filter_cursor = 0;
            }
            return Ok(());
        }
        KeyCode::Char('R') if key.modifiers == event::KeyModifiers::SHIFT => {
            app.open_issues_panel();
            return Ok(());
        }
        _ => {}
    }

    let Some(action) = lookup_issues_action(key.modifiers, key.code) else {
        return Ok(()); // Modal eats unrecognized keys
    };

    match action {
        Action::Escape => {
            app.close_issues_panel();
        }
        Action::Quit => {
            if !app.git_action_in_progress() {
                app.should_quit = true;
            }
        }
        Action::NavDown => {
            if let Some(ref mut panel) = app.issues_panel {
                if !panel.filtered_indices.is_empty()
                    && panel.selected + 1 < panel.filtered_indices.len()
                {
                    panel.selected += 1;
                }
            }
        }
        Action::NavUp => {
            if let Some(ref mut panel) = app.issues_panel {
                if panel.selected > 0 {
                    panel.selected -= 1;
                }
            }
        }
        Action::PageDown => {
            if let Some(ref mut panel) = app.issues_panel {
                let page = 20;
                let max = panel.filtered_indices.len().saturating_sub(1);
                panel.selected = (panel.selected + page).min(max);
            }
        }
        Action::PageUp => {
            if let Some(ref mut panel) = app.issues_panel {
                let page = 20;
                panel.selected = panel.selected.saturating_sub(page);
            }
        }
        Action::GoToTop => {
            if let Some(ref mut panel) = app.issues_panel {
                panel.selected = 0;
                panel.scroll = 0;
            }
        }
        Action::GoToBottom => {
            if let Some(ref mut panel) = app.issues_panel {
                if !panel.filtered_indices.is_empty() {
                    panel.selected = panel.filtered_indices.len() - 1;
                }
            }
        }
        Action::IssuesCreate => {
            // Cache the issues JSON from the panel before closing it
            let cached_json = app
                .issues_panel
                .as_ref()
                .map(|p| crate::app::state::issues::serialize_issues_for_prompt(&p.issues))
                .unwrap_or_default();
            // Close panel, enter prompt mode for issue creation
            // The actual agent spawn happens when the user submits the first prompt
            // (intercepted in send_staged_prompt)
            app.issues_panel = None;
            app.issue_session = Some(crate::app::types::IssueSession {
                slot_id: String::new(),
                session_id: None,
                approval_pending: false,
                worktree_path: app
                    .current_worktree()
                    .and_then(|w| w.worktree_path.clone())
                    .unwrap_or_default(),
                duplicate_detected: false,
                cached_issues_json: cached_json,
                store_session_id: None,
                saved_session_id: app.current_session_id,
            });
            // Clear session pane immediately so stale content doesn't show
            app.display_events.clear();
            app.invalidate_render_cache();
            app.rendered_events_count = 0;
            app.rendered_content_line_count = 0;
            app.rendered_events_start = 0;
            app.session_scroll = usize::MAX;
            app.focus = crate::app::types::Focus::Input;
            app.prompt_mode = true;
            app.title_session_name = "[NEW ISSUE]".to_string();
        }
        Action::Confirm => {
            // Open selected issue in browser
            if let Some(ref panel) = app.issues_panel {
                if let Some(issue) = panel.selected_issue() {
                    let url = issue.url.clone();
                    if !url.is_empty() {
                        let _ = open_url_in_browser(&url);
                        app.set_status(&format!("Opened issue #{} in browser", issue.number));
                    }
                }
            }
        }
        _ => {} // Modal eats all other actions
    }

    Ok(())
}

/// Handle text input while the filter is active.
fn handle_filter_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    let panel = match app.issues_panel.as_mut() {
        Some(p) => p,
        None => return Ok(()),
    };

    match key.code {
        KeyCode::Esc => {
            panel.filter_active = false;
            panel.filter.clear();
            panel.filter_cursor = 0;
            panel.refilter();
        }
        KeyCode::Enter => {
            panel.filter_active = false;
            // Keep the filter text applied
        }
        KeyCode::Backspace => {
            if panel.filter_cursor > 0 {
                let byte_idx = panel
                    .filter
                    .char_indices()
                    .nth(panel.filter_cursor - 1)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                panel.filter.remove(byte_idx);
                panel.filter_cursor -= 1;
                panel.refilter();
            }
        }
        KeyCode::Left => {
            if panel.filter_cursor > 0 {
                panel.filter_cursor -= 1;
            }
        }
        KeyCode::Right => {
            if panel.filter_cursor < panel.filter.chars().count() {
                panel.filter_cursor += 1;
            }
        }
        KeyCode::Char(c) => {
            let byte_idx = panel
                .filter
                .char_indices()
                .nth(panel.filter_cursor)
                .map(|(i, _)| i)
                .unwrap_or(panel.filter.len());
            panel.filter.insert(byte_idx, c);
            panel.filter_cursor += 1;
            panel.refilter();
        }
        _ => {}
    }

    Ok(())
}

/// Open a URL in the default browser.
fn open_url_in_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()?;
    }
    Ok(())
}
