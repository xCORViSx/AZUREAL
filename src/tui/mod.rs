//! Terminal User Interface module
//!
//! Split into focused submodules:
//! - `event_loop`: Core event loop and event handling
//! - `input_*`: Input handlers for different UI modes
//! - `draw_*`: Rendering functions for different UI components
//! - `util`: Utility functions (truncate, colorize, etc.)

mod draw_dialogs;
mod draw_input;
mod draw_output;
mod draw_sidebar;
mod draw_status;
mod draw_terminal;
mod draw_wizard;
mod event_loop;
mod input_dialogs;
mod input_output;
mod input_rebase;
mod input_sessions;
mod input_terminal;
mod input_wizard;
mod util;

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
use crate::db::Database;
use crate::git::Git;


/// Run the TUI application
pub async fn run(db: Database) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(db);
    app.load()?;

    // If no projects, add current directory if it's a git repo
    if app.projects.is_empty() {
        let cwd = std::env::current_dir()?;
        if Git::is_git_repo(&cwd) {
            app.add_project(cwd)?;
        }
    }

    let config = Config::load().unwrap_or_default();
    let result = event_loop::run_app(&mut terminal, &mut app, config).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    result
}

/// Main UI layout and rendering
fn ui(f: &mut Frame, app: &mut App) {
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

    // Split main content into sidebar and output
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(30), Constraint::Min(40)])
        .split(chunks[0]);

    // Draw main components
    draw_sidebar::draw_sidebar(f, app, main_chunks[0]);
    draw_output::draw_output(f, app, main_chunks[1]);

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
