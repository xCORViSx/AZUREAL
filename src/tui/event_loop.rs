//! Core event loop and event handling

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use std::io;
use std::time::Duration;

use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::{App, Focus};
use crate::claude::{ClaudeEvent, ClaudeProcess};
use crate::config::Config;

use super::input_dialogs::{handle_branch_dialog_input, handle_context_menu_input};
use super::input_output::handle_output_input;
use super::input_sessions::handle_sessions_input;
use super::input_terminal::{handle_input_mode, handle_session_creation_input};
use super::input_wizard::handle_wizard_input;
use super::ui;

/// Events that can occur in the TUI
#[derive(Debug)]
pub enum TuiEvent {
    Input(event::KeyEvent),
    Claude(String, ClaudeEvent),
    Tick,
}

/// Main TUI event loop
pub async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    config: Config,
) -> Result<()> {
    let claude_process = ClaudeProcess::new(config);

    loop {
        app.poll_terminal();
        terminal.draw(|f| ui(f, app))?;

        let events = collect_events(app)?;
        for event in events {
            handle_event(event, app, &claude_process)?;
        }

        if app.should_quit { break; }
    }

    Ok(())
}

/// Collect all available events (keyboard input, Claude output, etc.)
fn collect_events(app: &App) -> Result<Vec<TuiEvent>> {
    let mut events = Vec::new();

    // Poll ALL Claude receivers (one per running session)
    for (session_id, receiver) in &app.claude_receivers {
        while let Ok(event) = receiver.try_recv() {
            events.push(TuiEvent::Claude(session_id.clone(), event));
        }
    }

    // Poll for keyboard input with timeout
    if event::poll(Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            events.push(TuiEvent::Input(key));
        }
    }

    if events.is_empty() { events.push(TuiEvent::Tick); }

    Ok(events)
}

/// Handle a single TUI event
fn handle_event(event: TuiEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    match event {
        TuiEvent::Claude(session_id, claude_event) => handle_claude_event(&session_id, claude_event, app)?,
        TuiEvent::Input(key_event) => handle_key_event(key_event, app, claude_process)?,
        TuiEvent::Tick => {}
    }
    Ok(())
}

/// Handle Claude process events for a specific session
fn handle_claude_event(session_id: &str, event: ClaudeEvent, app: &mut App) -> Result<()> {
    match event {
        ClaudeEvent::Output(output) => app.handle_claude_output(session_id, output.output_type, output.data),
        ClaudeEvent::Started { pid } => app.handle_claude_started(session_id, pid),
        ClaudeEvent::SessionId(claude_session_id) => app.set_claude_session_id(session_id, claude_session_id),
        ClaudeEvent::Exited { code } => app.handle_claude_exited(session_id, code),
        ClaudeEvent::Error(e) => app.handle_claude_error(session_id, e),
    }
    Ok(())
}

/// Handle keyboard input events
fn handle_key_event(key: event::KeyEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    // Global keybindings
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('c')) | (KeyModifiers::CONTROL, KeyCode::Char('q')) => {
            app.should_quit = true;
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Char('i')) if !app.insert_mode && !app.show_help && app.context_menu.is_none() && !app.is_wizard_active() => {
            app.focus = Focus::Input;
            app.insert_mode = true;
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Char('t')) if !app.insert_mode && !app.show_help && app.context_menu.is_none() && !app.is_wizard_active() => {
            app.toggle_terminal();
            app.focus = Focus::Input;
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Char('+')) | (KeyModifiers::SHIFT, KeyCode::Char('+')) if !app.insert_mode && app.terminal_mode => {
            app.adjust_terminal_height(2);
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Char('-')) if !app.insert_mode && app.terminal_mode => {
            app.adjust_terminal_height(-2);
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Char('?')) if !app.insert_mode => {
            app.toggle_help();
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Tab) => {
            // Don't cycle focus when in insert mode (typing in input/terminal)
            if !app.show_help && !app.insert_mode { app.focus_next(); }
            return Ok(());
        }
        (KeyModifiers::SHIFT, KeyCode::BackTab) => {
            // Don't cycle focus when in insert mode
            if !app.show_help && !app.insert_mode { app.focus_prev(); }
            return Ok(());
        }
        _ => {}
    }

    // Help overlay is open
    if app.show_help {
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc => app.toggle_help(),
            _ => {}
        }
        return Ok(());
    }

    // Context menu
    if app.context_menu.is_some() {
        handle_context_menu_input(key, app, claude_process)?;
        return Ok(());
    }

    // Wizard
    if app.is_wizard_active() {
        handle_wizard_input(app, key.code, claude_process);
        return Ok(());
    }

    // Mode-specific keybindings
    match app.focus {
        Focus::Sessions => handle_sessions_input(key, app)?,
        Focus::Output => handle_output_input(key, app)?,
        Focus::Input => handle_input_mode(key, app, claude_process)?,
        Focus::SessionCreation => handle_session_creation_input(key, app, claude_process)?,
        Focus::BranchDialog => handle_branch_dialog_input(key, app)?,
    }

    Ok(())
}
