//! TUI entry point and main layout
//!
//! Contains the run() function to start the TUI and the ui() layout function.

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    Frame, Terminal,
};
use std::io;

use crate::app::{App, Focus};
use crate::config::Config;

use super::event_loop;
use super::{draw_dialogs, draw_file_tree, draw_input, draw_output, draw_sidebar, draw_status, draw_terminal, draw_viewer, draw_wizard};

/// Run the TUI application
pub async fn run() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    app.load()?;

    // Load output for the initially selected session
    app.load_session_output();

    let config = Config::load().unwrap_or_default();
    let result = event_loop::run_app(&mut terminal, &mut app, config).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    result
}

/// Main UI layout and rendering
pub fn ui(f: &mut Frame, app: &mut App) {
    // Wizard modal takes over the screen
    if app.is_wizard_active() {
        draw_wizard::draw_wizard_modal(f, app);
        return;
    }

    // Main layout - terminal mode replaces input with embedded PTY shell
    let chunks = if app.terminal_mode {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(10),
                Constraint::Length(app.terminal_height + 2),
                Constraint::Length(1),
            ])
            .split(f.area())
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(10),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(f.area())
    };

    // Split main content into 4 panes: Sessions, FileTree, Viewer, Convo
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(40),      // Sessions
            Constraint::Length(40),      // FileTree
            Constraint::Percentage(50),  // Viewer (50% of remaining)
            Constraint::Percentage(50),  // Convo (50% of remaining)
        ])
        .split(chunks[0]);

    // Draw main components
    draw_sidebar::draw_sidebar(f, app, main_chunks[0]);
    draw_file_tree::draw_file_tree(f, app, main_chunks[1]);
    draw_viewer::draw_viewer(f, app, main_chunks[2]);
    draw_output::draw_output(f, app, main_chunks[3]);

    // Draw either terminal or input
    if app.terminal_mode {
        draw_terminal::draw_terminal(f, app, chunks[1]);
    } else {
        draw_input::draw_input(f, app, chunks[1]);
    }
    draw_status::draw_status(f, app, chunks[2]);

    // Draw overlays
    if app.focus == Focus::SessionCreation {
        draw_dialogs::draw_session_creation_modal(f, app);
    }
    if let Some(ref dialog) = app.branch_dialog {
        draw_dialogs::draw_branch_dialog(f, dialog, f.area());
    }
    if app.show_help {
        draw_dialogs::draw_help_overlay(f);
    }
    if app.context_menu.is_some() {
        draw_dialogs::draw_context_menu(f, app, f.area());
    }
}
