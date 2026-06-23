//! Input event dispatch
//!
//! Routes crossterm events (key, mouse, resize) to the appropriate handlers.

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind};

use crate::app::{App, Focus};
use crate::backend::AgentProcess;

use super::super::input_dialogs::paste_into_run_command_dialog;
use super::super::input_projects::handle_projects_paste;
use super::actions::handle_key_event;
use super::coords::{screen_to_cache_pos, screen_to_edit_pos, screen_to_input_char};
use super::mouse::{handle_mouse_click, handle_mouse_drag};

/// Return true when a key event is the auto-prompt toggle shortcut.
fn is_auto_prompt_toggle_key(key: crossterm::event::KeyEvent) -> bool {
    let allowed_modifiers = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
    key.modifiers.contains(KeyModifiers::CONTROL)
        && key.modifiers.difference(allowed_modifiers).is_empty()
        && matches!(key.code, KeyCode::Char(c) if c.eq_ignore_ascii_case(&'a'))
}

/// Return true when an active mode already owns Ctrl+A semantics.
fn ctrl_a_belongs_to_existing_mode(app: &App) -> bool {
    app.rcr_session.is_some()
        || app.issue_session.is_some()
        || (app.terminal_mode && app.prompt_mode && app.focus == Focus::Input)
}

/// Toggle auto prompt and update the status bar with the new state.
fn toggle_auto_prompt(app: &mut App) {
    let enabled = app.auto_prompt.toggle();
    let status = if enabled {
        "Auto prompt ON - next prompt will repeat"
    } else {
        "Auto prompt OFF"
    };
    app.set_status(status);
}

/// Process a single input event from the reader thread channel.
/// Dispatches key, mouse, and resize events to the appropriate handlers.
#[allow(clippy::too_many_arguments)]
pub fn process_input_event(
    evt: Event,
    app: &mut App,
    claude_process: &AgentProcess,
    needs_redraw: &mut bool,
    scroll_delta: &mut i32,
    scroll_col: &mut u16,
    scroll_row: &mut u16,
    had_key_event: &mut bool,
    cached_width: &mut u16,
    cached_height: &mut u16,
) -> Result<()> {
    match evt {
        Event::Key(key) => {
            // Input thread already filters to Press/Repeat only
            if !matches!(key.code, KeyCode::Modifier(_)) {
                if is_auto_prompt_toggle_key(key) && !ctrl_a_belongs_to_existing_mode(app) {
                    toggle_auto_prompt(app);
                    *needs_redraw = true;
                } else {
                    handle_key_event(key, app, claude_process)?;
                }
                *had_key_event = true;
            }
        }
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollDown => {
                *scroll_delta += 3;
                *scroll_col = mouse.column;
                *scroll_row = mouse.row;
            }
            MouseEventKind::ScrollUp => {
                *scroll_delta -= 3;
                *scroll_col = mouse.column;
                *scroll_row = mouse.row;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                app.viewer_selection = None;
                app.session_selection = None;
                app.terminal_selection = None;
                let (mc, mr) = (mouse.column, mouse.row);
                use ratatui::layout::Position;
                let mpos = Position::new(mc, mr);
                if app.pane_viewer.contains(mpos) {
                    if app.viewer_edit_mode {
                        if let Some((src_line, src_col)) = screen_to_edit_pos(app, mc, mr) {
                            app.mouse_drag_start = Some((src_line, src_col, 3));
                        }
                    } else if let Some((cl, cc)) = screen_to_cache_pos(
                        mc,
                        mr,
                        app.pane_viewer,
                        app.viewer_scroll,
                        app.viewer_lines_cache.len(),
                    ) {
                        app.mouse_drag_start = Some((cl, cc, 0));
                    }
                } else if app.pane_session.contains(mpos) {
                    app.clamp_session_scroll();
                    if let Some((cl, cc)) = screen_to_cache_pos(
                        mc,
                        mr,
                        app.pane_session,
                        app.session_scroll,
                        app.rendered_lines_cache.len(),
                    ) {
                        app.mouse_drag_start = Some((cl, cc, 1));
                    }
                } else if app.input_area.contains(mpos) && app.terminal_mode {
                    // Terminal pane: anchor in "distance from bottom" coordinates.
                    // from_bottom = (inner_height - 1 - vis_row) + scroll
                    // This is stable across scroll changes (doesn't drift).
                    let vis_row = mr.saturating_sub(app.input_area.y + 1) as usize;
                    let inner_h = app.terminal_rows as usize;
                    let tr =
                        (inner_h.saturating_sub(1).saturating_sub(vis_row)) + app.terminal_scroll;
                    let tc = mc.saturating_sub(app.input_area.x + 1) as usize;
                    app.mouse_drag_start = Some((tr, tc, 4));
                } else if app.input_area.contains(mpos) && app.prompt_mode && !app.terminal_mode {
                    let ci = screen_to_input_char(app, mc, mr);
                    app.mouse_drag_start = Some((ci, 0, 2));
                } else {
                    app.mouse_drag_start = None;
                }
                if handle_mouse_click(app, mc, mr) {
                    *needs_redraw = true;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if handle_mouse_drag(app, mouse.column, mouse.row) {
                    *needs_redraw = true;
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                app.mouse_drag_start = None;
            }
            _ => {}
        },
        Event::Paste(text) => {
            if app.run_command_dialog.is_some() {
                if paste_into_run_command_dialog(app, &text) {
                    *had_key_event = true;
                }
                *needs_redraw = true;
                return Ok(());
            }

            if app.is_projects_panel_active() {
                handle_projects_paste(&text, app)?;
                *had_key_event = true;
                *needs_redraw = true;
                return Ok(());
            }

            // Bracketed paste: terminal wraps pasted content so we receive it
            // as a single event instead of individual keystrokes. This prevents
            // newlines in pasted text from triggering Enter (which submits the prompt).
            //
            // Auto-enter prompt mode on paste: if the user pastes in command mode
            // (focus on Input but prompt_mode=false), activate prompt mode so the
            // pasted text lands in the input field instead of being silently dropped.
            if !app.prompt_mode
                && !app.terminal_mode
                && !app.viewer_edit_mode
                && matches!(
                    app.focus,
                    Focus::Input
                        | Focus::Session
                        | Focus::Viewer
                        | Focus::FileTree
                        | Focus::Worktrees
                )
            {
                app.prompt_mode = true;
                app.focus = Focus::Input;
            }
            if app.prompt_mode && !app.terminal_mode {
                if app.has_input_selection() {
                    app.input_delete_selection();
                }
                // Insert pasted text at cursor, preserving newlines for multi-line input
                let chars: Vec<char> = app.input.chars().collect();
                let before: String = chars[..app.input_cursor.min(chars.len())].iter().collect();
                let after: String = chars[app.input_cursor.min(chars.len())..].iter().collect();
                app.input = before + &text + &after;
                app.input_cursor += text.chars().count();
                *had_key_event = true;
            } else if app.terminal_mode {
                // In terminal type mode, forward with bracketed paste wrapping
                // so shells (PowerShell PSReadLine, bash, zsh) buffer the entire
                // block instead of executing each line individually.
                app.paste_to_terminal(&text);
                *had_key_event = true;
            } else if app.viewer_edit_mode {
                // In edit mode, insert pasted text at cursor
                if app.viewer_edit_selection.is_some() {
                    app.viewer_edit_delete_selection();
                }
                for c in text.chars() {
                    if c == '\n' {
                        app.viewer_edit_enter();
                    } else if c != '\r' {
                        app.viewer_edit_char(c);
                    }
                }
                *had_key_event = true;
            }
            *needs_redraw = true;
        }
        Event::Resize(w, h) => {
            *cached_width = w;
            *cached_height = h;
            app.screen_height = h;
            *needs_redraw = true;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
/// Tests for input event dispatch behavior.
mod tests {
    use super::*;
    use crate::app::types::{ProjectsPanel, RunCommandDialog};
    use crate::backend::AgentProcess;
    use crate::config::Config;
    use crossterm::event::{Event, KeyEvent, KeyModifiers};

    /// Dispatch one synthetic input event and return redraw/key flags.
    fn dispatch_event(app: &mut App, event: Event) -> (bool, bool) {
        let claude_process = AgentProcess::new(Config::default());
        let mut needs_redraw = false;
        let mut scroll_delta = 0;
        let mut scroll_col = 0;
        let mut scroll_row = 0;
        let mut had_key_event = false;
        let mut cached_width = 80;
        let mut cached_height = 24;

        process_input_event(
            event,
            app,
            &claude_process,
            &mut needs_redraw,
            &mut scroll_delta,
            &mut scroll_col,
            &mut scroll_row,
            &mut had_key_event,
            &mut cached_width,
            &mut cached_height,
        )
        .unwrap();

        (needs_redraw, had_key_event)
    }

    /// Dispatch a synthetic paste event and return redraw/key flags.
    fn dispatch_paste(app: &mut App, text: &str) -> (bool, bool) {
        dispatch_event(app, Event::Paste(text.to_string()))
    }

    /// Paste events route to the active Projects add input.
    #[test]
    fn paste_goes_to_projects_panel_input() {
        let mut app = App::new();
        app.focus = Focus::Input;
        app.projects_panel = Some(ProjectsPanel::new(vec![]));
        app.projects_panel.as_mut().unwrap().start_add();

        let (needs_redraw, had_key_event) = dispatch_paste(&mut app, "/tmp/repo");

        let panel = app.projects_panel.as_ref().unwrap();
        assert_eq!(panel.input, "/tmp/repo");
        assert_eq!(panel.input_cursor, 9);
        assert!(app.input.is_empty());
        assert!(!app.prompt_mode);
        assert!(needs_redraw);
        assert!(had_key_event);
    }

    /// Paste events route to the run-command dialog command field.
    #[test]
    fn paste_goes_to_run_command_dialog() {
        let mut app = App::new();
        app.focus = Focus::Input;
        app.run_command_dialog = Some(RunCommandDialog::new());
        app.run_command_dialog.as_mut().unwrap().editing_name = false;

        let (needs_redraw, had_key_event) = dispatch_paste(&mut app, "cargo test\ncargo fmt");

        let dialog = app.run_command_dialog.as_ref().unwrap();
        assert_eq!(dialog.command, "cargo test\ncargo fmt");
        assert_eq!(dialog.command_cursor, 20);
        assert!(app.input.is_empty());
        assert!(!app.prompt_mode);
        assert!(needs_redraw);
        assert!(had_key_event);
    }

    /// Paste in Projects browse mode is consumed without mutating prompt input.
    #[test]
    fn paste_in_projects_browse_mode_is_consumed() {
        let mut app = App::new();
        app.focus = Focus::Input;
        app.projects_panel = Some(ProjectsPanel::new(vec![]));

        let (needs_redraw, had_key_event) = dispatch_paste(&mut app, "/tmp/repo");

        assert!(app.input.is_empty());
        assert!(!app.prompt_mode);
        assert!(app.projects_panel.as_ref().unwrap().input.is_empty());
        assert!(needs_redraw);
        assert!(had_key_event);
    }

    /// Ctrl+A toggles auto prompt outside modes that own the shortcut.
    #[test]
    fn ctrl_a_toggles_auto_prompt() {
        let mut app = App::new();

        let (needs_redraw, had_key_event) = dispatch_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
        );

        assert!(app.auto_prompt.is_enabled());
        assert_eq!(
            app.status_message.as_deref(),
            Some("Auto prompt ON - next prompt will repeat")
        );
        assert!(needs_redraw);
        assert!(had_key_event);
    }

    /// Ctrl+A stays available to the embedded terminal in type mode.
    #[test]
    fn ctrl_a_terminal_type_mode_does_not_toggle_auto_prompt() {
        let mut app = App::new();
        app.focus = Focus::Input;
        app.terminal_mode = true;
        app.prompt_mode = true;

        let (_needs_redraw, had_key_event) = dispatch_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
        );

        assert!(!app.auto_prompt.is_enabled());
        assert!(had_key_event);
    }
}
