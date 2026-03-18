//! Input event dispatch
//!
//! Routes crossterm events (key, mouse, resize) to the appropriate handlers.

use anyhow::Result;
use crossterm::event::{Event, KeyCode, MouseButton, MouseEventKind};

use crate::app::App;
use crate::backend::AgentProcess;

use super::actions::handle_key_event;
use super::coords::{screen_to_cache_pos, screen_to_edit_pos, screen_to_input_char};
use super::mouse::{handle_mouse_click, handle_mouse_drag};

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
                handle_key_event(key, app, claude_process)?;
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
                    // Terminal pane: anchor in scrollback-adjusted row/col
                    let tr = mr.saturating_sub(app.input_area.y + 1) as usize
                        + app.terminal_scroll;
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
