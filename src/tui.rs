use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;
use std::time::Duration;

use crate::app::{App, BranchDialog, Focus, ViewMode, SessionAction};
use crate::claude::{ClaudeEvent, ClaudeProcess};
use crate::config::Config;
use crate::db::Database;
use crate::git::Git;
use crate::models::{RebaseResult, RebaseState, SessionStatus};
use crate::session::SessionManager;

/// Events that can occur in the TUI
#[derive(Debug)]
pub enum TuiEvent {
    /// User keyboard input
    Input(event::KeyEvent),
    /// Claude process event
    Claude(ClaudeEvent),
    /// Tick for periodic updates
    Tick,
}

/// Run the TUI application
pub async fn run(db: Database) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new(db);
    app.load()?;

    // If no projects, prompt to add one
    if app.projects.is_empty() {
        let cwd = std::env::current_dir()?;
        if Git::is_git_repo(&cwd) {
            app.add_project(cwd)?;
        }
    }

    // Load config
    let config = Config::load().unwrap_or_default();

    // Main loop
    let result = run_app(&mut terminal, &mut app, config).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    config: Config,
) -> Result<()> {
    let claude_process = ClaudeProcess::new(config);

    loop {
        // Draw UI
        terminal.draw(|f| ui(f, app))?;

        // Collect all available events
        let events = collect_events(app)?;

        // Handle each event
        for event in events {
            handle_event(event, app, &claude_process)?;
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Collect all available events (keyboard input, Claude output, etc.)
fn collect_events(app: &App) -> Result<Vec<TuiEvent>> {
    let mut events = Vec::new();

    // Poll for Claude output
    if let Some(ref receiver) = app.claude_receiver {
        while let Ok(event) = receiver.try_recv() {
            events.push(TuiEvent::Claude(event));
        }
    }

    // Poll for keyboard input with timeout
    if event::poll(Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            events.push(TuiEvent::Input(key));
        }
    }

    // If no events, add a Tick
    if events.is_empty() {
        events.push(TuiEvent::Tick);
    }

    Ok(events)
}

/// Handle a single TUI event
fn handle_event(event: TuiEvent, app: &mut App, claude_process: &ClaudeProcess) -> Result<()> {
    match event {
        TuiEvent::Claude(claude_event) => {
            handle_claude_event(claude_event, app)?;
        }
        TuiEvent::Input(key_event) => {
            handle_key_event(key_event, app, claude_process)?;
        }
        TuiEvent::Tick => {
            // Periodic updates can go here
        }
    }
    Ok(())
}

/// Handle Claude process events
fn handle_claude_event(event: ClaudeEvent, app: &mut App) -> Result<()> {
    match event {
        ClaudeEvent::Output(output) => {
            app.handle_claude_output(output.output_type, output.data);
        }
        ClaudeEvent::Started { pid } => {
            app.handle_claude_started(pid);
        }
        ClaudeEvent::Exited { code } => {
            if app.handle_claude_exited(code) {
                app.claude_receiver = None;
            }
        }
        ClaudeEvent::Error(e) => {
            app.handle_claude_error(e);
        }
    }
    Ok(())
}

/// Handle keyboard input events
fn handle_key_event(
    key: event::KeyEvent,
    app: &mut App,
    claude_process: &ClaudeProcess,
) -> Result<()> {
    // Global keybindings
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('c')) | (KeyModifiers::CONTROL, KeyCode::Char('q')) => {
            app.should_quit = true;
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Char('?')) => {
            app.toggle_help();
            return Ok(());
        }
        (KeyModifiers::NONE, KeyCode::Tab) => {
            if !app.show_help {
                app.focus_next();
            }
            return Ok(());
        }
        (KeyModifiers::SHIFT, KeyCode::BackTab) => {
            if !app.show_help {
                app.focus_prev();
            }
            return Ok(());
        }
        _ => {}
    }

    // Help overlay is open, only handle keys for closing it
    if app.show_help {
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => {
                app.toggle_help();
            }
            _ => {}
        }
        return Ok(());
    }

    // Handle context menu navigation first if open
    if app.context_menu.is_some() {
        handle_context_menu_input(key, app, claude_process)?;
        return Ok(());
    }

    // Mode-specific keybindings
    match app.focus {
        Focus::Sessions => handle_sessions_input(key, app, claude_process)?,
        Focus::Output => handle_output_input(key, app)?,
        Focus::Input => handle_input_mode(key, app, claude_process)?,
        Focus::SessionCreation => handle_session_creation_input(key, app, claude_process)?,
        Focus::BranchDialog => handle_branch_dialog_input(key, app)?,
    }

    Ok(())
}

/// Handle keyboard input when context menu is open
fn handle_context_menu_input(
    key: event::KeyEvent,
    app: &mut App,
    claude_process: &ClaudeProcess,
) -> Result<()> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.context_menu_next(),
        KeyCode::Char('k') | KeyCode::Up => app.context_menu_prev(),
        KeyCode::Enter => {
            // Execute selected action
            if let Some(action) = app.selected_action() {
                execute_action(app, claude_process, action)?;
            }
            app.close_context_menu();
        }
        KeyCode::Esc | KeyCode::Char('q') => {
            app.close_context_menu();
        }
        _ => {}
    }
    Ok(())
}

/// Execute a session action
fn execute_action(app: &mut App, claude_process: &ClaudeProcess, action: SessionAction) -> Result<()> {
    match action {
        SessionAction::Start => {
            if let Some(session) = app.current_session() {
                if session.status == SessionStatus::Pending
                    || session.status == SessionStatus::Stopped
                    || session.status == SessionStatus::Completed
                {
                    match claude_process.spawn(
                        &session.worktree_path,
                        &session.initial_prompt,
                        None,
                    ) {
                        Ok(rx) => {
                            app.claude_receiver = Some(rx);
                            app.set_status("Starting Claude...");
                        }
                        Err(e) => {
                            app.set_status(format!("Failed to start: {}", e));
                        }
                    }
                }
            }
        }
        SessionAction::Stop => {
            app.set_status("Stop action not yet implemented");
        }
        SessionAction::Archive => {
            if let Some(session) = app.current_session() {
                let session_id = session.id.clone();
                if let Err(e) = SessionManager::new(&app.db).archive_session(&session_id) {
                    app.set_status(format!("Failed to archive: {}", e));
                } else {
                    app.set_status("Session archived");
                    let _ = app.refresh_sessions();
                }
            }
        }
        SessionAction::Delete => {
            app.set_status("Delete action not yet implemented - use with caution");
        }
        SessionAction::ViewDiff => {
            if let Err(e) = app.load_diff() {
                app.set_status(format!("Failed to get diff: {}", e));
            }
        }
        SessionAction::RebaseFromMain => {
            if let Err(e) = app.rebase_current_session() {
                app.set_status(format!("Rebase failed: {}", e));
            }
        }
        SessionAction::OpenInEditor => {
            if let Some(session) = app.current_session() {
                let path = session.worktree_path.display().to_string();
                app.set_status(format!("Editor integration not implemented. Path: {}", path));
            }
        }
        SessionAction::CopyWorktreePath => {
            if let Some(session) = app.current_session() {
                let path = session.worktree_path.display().to_string();
                app.set_status(format!("Copied to clipboard (not implemented): {}", path));
            }
        }
    }
    Ok(())
}

/// Handle keyboard input when Sessions pane is focused
fn handle_sessions_input(
    key: event::KeyEvent,
    app: &mut App,
    claude_process: &ClaudeProcess,
) -> Result<()> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.select_next_session(),
        KeyCode::Char('k') | KeyCode::Up => app.select_prev_session(),
        KeyCode::Char('J') => app.select_next_project(),
        KeyCode::Char('K') => app.select_prev_project(),
        KeyCode::Tab => app.focus = Focus::Output,
        KeyCode::Char(' ') | KeyCode::Char('?') => {
            // Open context menu
            app.open_context_menu();
        }
        KeyCode::Char('n') => {
            app.enter_session_creation_mode();
        }
        KeyCode::Char('w') => {
            // Create worktree from existing branch
            if let Some(project) = app.current_project() {
                match Git::list_available_branches(&project.path) {
                    Ok(branches) => {
                        app.open_branch_dialog(branches);
                    }
                    Err(e) => {
                        app.set_status(format!("Failed to list branches: {}", e));
                    }
                }
            }
        }
        KeyCode::Char('d') => {
            if let Err(e) = app.load_diff() {
                app.set_status(format!("Failed to get diff: {}", e));
            }
        }
        KeyCode::Char('r') => {
            // Rebase from main with conflict detection
            if let Some(session) = app.current_session() {
                if let Some(project) = app.current_project() {
                    let worktree_path = session.worktree_path.clone();
                    let main_branch = project.main_branch.clone();
                    match Git::rebase_onto_main(&worktree_path, &main_branch) {
                        Ok(RebaseResult::Success) => {
                            app.set_status("Rebase completed successfully");
                            app.clear_rebase_status();
                        }
                        Ok(RebaseResult::UpToDate) => {
                            app.set_status("Already up to date with main branch");
                        }
                        Ok(RebaseResult::Conflicts(status)) => {
                            let conflict_count = status.conflicted_files.len();
                            app.set_rebase_status(status);
                            app.set_status(format!(
                                "Rebase conflicts: {} file(s) need resolution. Press 'R' for rebase menu.",
                                conflict_count
                            ));
                        }
                        Ok(RebaseResult::Aborted) => {
                            app.set_status("Rebase was aborted");
                            app.clear_rebase_status();
                        }
                        Ok(RebaseResult::Failed(e)) => {
                            app.set_status(format!("Rebase failed: {}", e));
                        }
                        Err(e) => {
                            app.set_status(format!("Rebase error: {}", e));
                        }
                    }
                }
            }
        }
        KeyCode::Char('R') => {
            // Show rebase status/menu
            if let Some(session) = app.current_session() {
                let worktree_path = session.worktree_path.clone();
                if Git::is_rebase_in_progress(&worktree_path) {
                    match Git::get_rebase_status(&worktree_path) {
                        Ok(status) => {
                            app.set_rebase_status(status);
                        }
                        Err(e) => {
                            app.set_status(format!("Failed to get rebase status: {}", e));
                        }
                    }
                } else {
                    app.set_status("No rebase in progress");
                }
            }
        }
        KeyCode::Char('a') => {
            if let Err(e) = app.archive_current_session() {
                app.set_status(format!("Failed to archive: {}", e));
            }
        }
        KeyCode::Enter => {
            if let Some(session) = app.current_session() {
                if session.status == SessionStatus::Pending
                    || session.status == SessionStatus::Stopped
                    || session.status == SessionStatus::Completed
                {
                    let session_id = session.id.clone();
                    match claude_process.spawn(
                        session_id.clone(),
                        &session.worktree_path,
                        &session.initial_prompt,
                        None,
                    ) {
                        Ok(rx) => {
                            app.claude_receiver = Some(rx);
                            app.running_session_id = Some(session_id);
                            app.set_status("Starting Claude...");
                        }
                        Err(e) => {
                            app.set_status(format!("Failed to start: {}", e));
                        }
                    }
                }
            }
        }
        KeyCode::Char('i') => {
            // Send input to running session
            if app.running_session_id.is_some() {
                app.focus = Focus::Input;
                app.set_status("Enter input to send to Claude:");
            } else {
                app.set_status("No running session");
            }
        }
        KeyCode::Char('s') => {
            // Stop running session
            if let Some(ref session_id) = app.running_session_id {
                if let Err(e) = claude_process.stop_session(session_id) {
                    app.set_status(format!("Failed to stop: {}", e));
                } else {
                    app.set_status("Session stopped");
                    app.running_session_id = None;
                    app.claude_receiver = None;
                }
            }
        }
        KeyCode::Char('q') => app.should_quit = true,
        _ => {}
    }
    Ok(())
}

/// Handle keyboard input when Output pane is focused
fn handle_output_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    match app.view_mode {
        ViewMode::Rebase => handle_rebase_input(key, app)?,
        _ => {
            match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => app.scroll_output_down(1),
                (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => app.scroll_output_up(1),
                (KeyModifiers::NONE, KeyCode::Char('G')) => app.scroll_output_to_bottom(),
                (KeyModifiers::NONE, KeyCode::Char('g')) => app.output_scroll = 0,
                (KeyModifiers::NONE, KeyCode::PageDown) => app.scroll_output_down(10),
                (KeyModifiers::NONE, KeyCode::PageUp) => app.scroll_output_up(10),
                (KeyModifiers::CONTROL, KeyCode::Char('d')) => app.scroll_output_down(20),
                (KeyModifiers::CONTROL, KeyCode::Char('u')) => app.scroll_output_up(20),
                (KeyModifiers::CONTROL, KeyCode::Char('f')) => app.scroll_output_down(40),
                (KeyModifiers::CONTROL, KeyCode::Char('b')) => app.scroll_output_up(40),
                (KeyModifiers::NONE, KeyCode::Char('o')) => app.view_mode = ViewMode::Output,
                (KeyModifiers::NONE, KeyCode::Char('d')) => {
                    if let Err(e) = app.load_diff() {
                        app.set_status(format!("Failed to get diff: {}", e));
                    }
                }
                (KeyModifiers::SHIFT, KeyCode::Char('R')) => {
                    // Show rebase view if in progress
                    if let Some(session) = app.current_session() {
                        let worktree_path = session.worktree_path.clone();
                        if Git::is_rebase_in_progress(&worktree_path) {
                            if let Ok(status) = Git::get_rebase_status(&worktree_path) {
                                app.set_rebase_status(status);
                            }
                        }
                    }
                }
                (KeyModifiers::NONE, KeyCode::Esc) => app.focus = Focus::Sessions,
                (KeyModifiers::NONE, KeyCode::Char('q')) => app.should_quit = true,
                _ => {}
            }
        }
    }
    Ok(())
}

/// Handle keyboard input when in Rebase view mode
fn handle_rebase_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.select_next_conflict(),
        KeyCode::Char('k') | KeyCode::Up => app.select_prev_conflict(),
        KeyCode::Char('c') => {
            // Continue rebase
            if let Some(session) = app.current_session() {
                let worktree_path = session.worktree_path.clone();
                match Git::rebase_continue(&worktree_path) {
                    Ok(RebaseResult::Success) => {
                        app.set_status("Rebase completed successfully");
                        app.clear_rebase_status();
                    }
                    Ok(RebaseResult::Conflicts(status)) => {
                        let conflict_count = status.conflicted_files.len();
                        app.set_rebase_status(status);
                        app.set_status(format!(
                            "More conflicts: {} file(s) need resolution",
                            conflict_count
                        ));
                    }
                    Ok(RebaseResult::Failed(e)) => {
                        app.set_status(format!("Continue failed: {}", e));
                    }
                    Err(e) => {
                        app.set_status(format!("Error: {}", e));
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Char('A') => {
            // Abort rebase
            if let Some(session) = app.current_session() {
                let worktree_path = session.worktree_path.clone();
                match Git::rebase_abort(&worktree_path) {
                    Ok(RebaseResult::Aborted) => {
                        app.set_status("Rebase aborted");
                        app.clear_rebase_status();
                    }
                    Ok(RebaseResult::Failed(e)) => {
                        app.set_status(format!("Abort failed: {}", e));
                    }
                    Err(e) => {
                        app.set_status(format!("Error: {}", e));
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Char('s') => {
            // Skip current commit
            if let Some(session) = app.current_session() {
                let worktree_path = session.worktree_path.clone();
                match Git::rebase_skip(&worktree_path) {
                    Ok(RebaseResult::Success) => {
                        app.set_status("Rebase completed successfully");
                        app.clear_rebase_status();
                    }
                    Ok(RebaseResult::Conflicts(status)) => {
                        let conflict_count = status.conflicted_files.len();
                        app.set_rebase_status(status);
                        app.set_status(format!(
                            "More conflicts: {} file(s) need resolution",
                            conflict_count
                        ));
                    }
                    Ok(RebaseResult::Failed(e)) => {
                        app.set_status(format!("Skip failed: {}", e));
                    }
                    Err(e) => {
                        app.set_status(format!("Error: {}", e));
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Char('o') => {
            // Resolve using ours (keep our changes)
            if let Some(session) = app.current_session() {
                if let Some(file) = app.current_conflict_file() {
                    let worktree_path = session.worktree_path.clone();
                    let file = file.to_string();
                    match Git::resolve_using_ours(&worktree_path, &file) {
                        Ok(()) => {
                            app.set_status(format!("Resolved {} using ours", file));
                            // Refresh rebase status
                            if let Ok(status) = Git::get_rebase_status(&worktree_path) {
                                if status.conflicted_files.is_empty() {
                                    app.set_status("All conflicts resolved. Press 'c' to continue rebase.");
                                }
                                app.set_rebase_status(status);
                            }
                        }
                        Err(e) => {
                            app.set_status(format!("Failed to resolve: {}", e));
                        }
                    }
                }
            }
        }
        KeyCode::Char('t') => {
            // Resolve using theirs (accept incoming changes)
            if let Some(session) = app.current_session() {
                if let Some(file) = app.current_conflict_file() {
                    let worktree_path = session.worktree_path.clone();
                    let file = file.to_string();
                    match Git::resolve_using_theirs(&worktree_path, &file) {
                        Ok(()) => {
                            app.set_status(format!("Resolved {} using theirs", file));
                            // Refresh rebase status
                            if let Ok(status) = Git::get_rebase_status(&worktree_path) {
                                if status.conflicted_files.is_empty() {
                                    app.set_status("All conflicts resolved. Press 'c' to continue rebase.");
                                }
                                app.set_rebase_status(status);
                            }
                        }
                        Err(e) => {
                            app.set_status(format!("Failed to resolve: {}", e));
                        }
                    }
                }
            }
        }
        KeyCode::Enter => {
            // View conflict diff for selected file
            if let Some(session) = app.current_session() {
                if let Some(file) = app.current_conflict_file() {
                    let worktree_path = session.worktree_path.clone();
                    let file = file.to_string();
                    match Git::get_conflict_diff(&worktree_path, &file) {
                        Ok(diff) => {
                            app.diff_text = Some(diff);
                            app.view_mode = ViewMode::Diff;
                        }
                        Err(e) => {
                            app.set_status(format!("Failed to get diff: {}", e));
                        }
                    }
                }
            }
        }
        KeyCode::Esc => {
            app.view_mode = ViewMode::Output;
            app.focus = Focus::Sessions;
        }
        KeyCode::Char('q') => app.should_quit = true,
        _ => {}
    }
    Ok(())
}

/// Handle keyboard input when Input field is focused
fn handle_input_mode(
    key: event::KeyEvent,
    app: &mut App,
    claude_process: &ClaudeProcess,
) -> Result<()> {
    match key.code {
        KeyCode::Char(c) => app.input_char(c),
        KeyCode::Backspace => app.input_backspace(),
        KeyCode::Delete => app.input_delete(),
        KeyCode::Left => app.input_left(),
        KeyCode::Right => app.input_right(),
        KeyCode::Home => app.input_home(),
        KeyCode::End => app.input_end(),
        KeyCode::Enter => {
            if !app.input.is_empty() {
                let input = app.input.clone();
                app.clear_input();

                // Check if we're sending input to a running session or creating a new one
                if let Some(ref session_id) = app.running_session_id {
                    // Send input to running session
                    match claude_process.send_input(session_id, &input) {
                        Ok(()) => {
                            app.set_status("Input sent");
                            // Also display the input in output
                            app.add_output(format!("> {}", input));
                        }
                        Err(e) => {
                            app.set_status(format!("Failed to send input: {}", e));
                        }
                    }
                } else {
                    // Create new session
                    match app.create_new_session(input) {
                        Ok(session) => {
                            app.set_status(format!("Created session: {}", session.name));

                            // Start Claude immediately
                            let session_id = session.id.clone();
                            match claude_process.spawn(
                                session_id.clone(),
                                &session.worktree_path,
                                &session.initial_prompt,
                                None,
                            ) {
                                Ok(rx) => {
                                    app.claude_receiver = Some(rx);
                                    app.running_session_id = Some(session_id);
                                }
                                Err(e) => {
                                    app.set_status(format!("Failed to start: {}", e));
                                }
                            }
                        }
                        Err(e) => {
                            app.set_status(format!("Failed to create session: {}", e));
                        }
                    }
                }
                app.focus = Focus::Sessions;
            }
        }
        KeyCode::Esc => {
            app.clear_input();
            app.clear_status();
            app.focus = Focus::Sessions;
        }
        KeyCode::Tab => app.focus = Focus::Sessions,
        _ => {}
    }
    Ok(())
}

/// Handle keyboard input when session creation modal is focused
fn handle_session_creation_input(
    key: event::KeyEvent,
    app: &mut App,
    claude_process: &ClaudeProcess,
) -> Result<()> {
    match (key.modifiers, key.code) {
        // Ctrl+Enter to submit
        (KeyModifiers::CONTROL, KeyCode::Enter) => {
            if !app.session_creation_input.is_empty() {
                let prompt = app.session_creation_input.clone();
                app.exit_session_creation_mode();

                match app.create_new_session(prompt) {
                    Ok(session) => {
                        app.set_status(format!("Created session: {}", session.name));

                        // Start Claude immediately
                        match claude_process.spawn(
                            session.id.clone(),
                            &session.worktree_path,
                            &session.initial_prompt,
                            None,
                        ) {
                            Ok(rx) => {
                                app.claude_receiver = Some(rx);
                            }
                            Err(e) => {
                                app.set_status(format!("Failed to start: {}", e));
                            }
                        }
                    }
                    Err(e) => {
                        app.set_status(format!("Failed to create session: {}", e));
                    }
                }
            }
        }
        // Regular Enter adds newline
        (KeyModifiers::NONE, KeyCode::Enter) => {
            app.session_creation_char('\n');
        }
        // Character input
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
            app.session_creation_char(c);
        }
        (_, KeyCode::Backspace) => app.session_creation_backspace(),
        (_, KeyCode::Delete) => app.session_creation_delete(),
        (_, KeyCode::Left) => app.session_creation_left(),
        (_, KeyCode::Right) => app.session_creation_right(),
        (_, KeyCode::Home) => app.session_creation_home(),
        (_, KeyCode::End) => app.session_creation_end(),
        (_, KeyCode::Esc) => {
            app.exit_session_creation_mode();
        }
        _ => {}
    }
    Ok(())
}

/// Handle keyboard input when Branch dialog is focused
fn handle_branch_dialog_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    if let Some(ref mut dialog) = app.branch_dialog {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => dialog.select_next(),
            KeyCode::Up | KeyCode::Char('k') => dialog.select_prev(),
            KeyCode::Backspace => dialog.filter_backspace(),
            KeyCode::Enter => {
                if let Some(branch) = dialog.selected_branch().cloned() {
                    if let Some(project) = app.current_project().cloned() {
                        match SessionManager::new(&app.db)
                            .create_session_from_branch(&project, &branch)
                        {
                            Ok(session) => {
                                app.set_status(format!("Created worktree: {}", session.name));
                                let _ = app.refresh_sessions();
                            }
                            Err(e) => {
                                app.set_status(format!("Failed to create worktree: {}", e));
                            }
                        }
                    }
                    app.close_branch_dialog();
                }
            }
            KeyCode::Esc => {
                app.close_branch_dialog();
            }
            KeyCode::Char(c) => dialog.filter_char(c),
            _ => {}
        }
    } else {
        app.focus = Focus::Sessions;
    }
    Ok(())
}

fn ui(f: &mut Frame, app: &App) {
    // Main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),    // Main content
            Constraint::Length(3), // Input
            Constraint::Length(1), // Status bar
        ])
        .split(f.area());

    // Split main content into sidebar and output
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(30), // Sidebar
            Constraint::Min(40),    // Output
        ])
        .split(chunks[0]);

    // Draw sidebar (projects and sessions)
    draw_sidebar(f, app, main_chunks[0]);

    // Draw output/diff panel
    draw_output(f, app, main_chunks[1]);

    // Draw input
    draw_input(f, app, chunks[1]);

    // Draw status bar
    draw_status(f, app, chunks[2]);

    // Draw session creation modal if in session creation mode
    if app.focus == Focus::SessionCreation {
        draw_session_creation_modal(f, app);
    }

    // Draw branch dialog if active
    if let Some(ref dialog) = app.branch_dialog {
        draw_branch_dialog(f, dialog, f.area());
    }

    // Draw help overlay if active
    if app.show_help {
        draw_help_overlay(f);
    }

    // Draw context menu overlay if open
    if app.context_menu.is_some() {
        draw_context_menu(f, app, f.area());
    }
}

fn draw_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let mut items: Vec<ListItem> = Vec::new();

    // Add projects and their sessions
    for (proj_idx, project) in app.projects.iter().enumerate() {
        let is_selected_proj = proj_idx == app.selected_project;
        let proj_style = if is_selected_proj {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };

        items.push(ListItem::new(Line::from(vec![
            Span::styled("▸ ", proj_style),
            Span::styled(&project.name, proj_style),
        ])));

        // Show sessions for selected project
        if is_selected_proj {
            for (sess_idx, session) in app.sessions.iter().enumerate() {
                let is_selected = app.selected_session == Some(sess_idx);
                let status_color = session.status.color();

                let style = if is_selected && app.focus == Focus::Sessions {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    Style::default()
                };

                items.push(ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(session.status.symbol(), Style::default().fg(status_color)),
                    Span::raw(" "),
                    Span::styled(truncate(&session.name, 22), style),
                ])));
            }
        }
    }

    let sidebar = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Sessions ")
                .border_style(if app.focus == Focus::Sessions {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::Gray)
                }),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_widget(sidebar, area);
}

fn draw_output(f: &mut Frame, app: &App, area: Rect) {
    let title = match app.view_mode {
        ViewMode::Output => " Output ",
        ViewMode::Diff => " Diff (Syntax Highlighted) ",
        ViewMode::Messages => " Messages ",
        ViewMode::Rebase => " Rebase ",
    };

    let content = match app.view_mode {
        ViewMode::Output => {
            let mut lines: Vec<Line> = app
                .output_lines
                .iter()
                .skip(app.output_scroll)
                .map(|line| Line::from(colorize_output(line)))
                .collect();

            // Add the partial line buffer if it's not empty
            if !app.output_buffer.is_empty() {
                lines.push(Line::from(colorize_output(&app.output_buffer)));
            }

            lines
        }
        ViewMode::Diff => {
            if let Some(ref diff) = app.diff_text {
                // Use syntax highlighter for diff view
                let highlighted = app.diff_highlighter.colorize_diff(diff);
                highlighted
                    .into_iter()
                    .skip(app.diff_scroll)
                    .map(|spans| Line::from(spans))
                    .collect()
            } else {
                vec![Line::from("No diff available")]
            }
        }
        ViewMode::Messages => {
            vec![Line::from("Messages view not implemented")]
        }
        ViewMode::Rebase => {
            draw_rebase_content(app)
        }
    };

    let output = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(if app.focus == Focus::Output {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::Gray)
                }),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(output, area);
}

fn draw_rebase_content(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    if let Some(ref status) = app.rebase_status {
        // Header with rebase state
        let state_color = status.state.color();
        lines.push(Line::from(vec![
            Span::styled("State: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{} {}", status.state.symbol(), status.state.as_str()),
                Style::default().fg(state_color),
            ),
        ]));

        // Progress info
        if let (Some(current), Some(total)) = (status.current_step, status.total_steps) {
            lines.push(Line::from(vec![
                Span::styled("Progress: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(format!("{}/{}", current, total)),
            ]));
        }

        // Branch info
        if let Some(ref head) = status.head_name {
            lines.push(Line::from(vec![
                Span::styled("Rebasing: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(head.clone(), Style::default().fg(Color::Green)),
            ]));
        }

        if let Some(ref onto) = status.onto_branch {
            lines.push(Line::from(vec![
                Span::styled("Onto: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(onto.clone(), Style::default().fg(Color::Cyan)),
            ]));
        }

        // Current commit being applied
        if let Some(ref commit) = status.current_commit {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Current commit: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(commit.clone(), Style::default().fg(Color::Yellow)),
            ]));
            if let Some(ref msg) = status.current_commit_message {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::raw(msg.clone()),
                ]));
            }
        }

        // Conflicted files
        if !status.conflicted_files.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(
                    format!("Conflicts ({}):", status.conflicted_files.len()),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            ]));

            for (idx, file) in status.conflicted_files.iter().enumerate() {
                let is_selected = app.selected_conflict == Some(idx);
                let style = if is_selected {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    Style::default().fg(Color::Red)
                };
                let prefix = if is_selected { "▸ " } else { "  " };
                lines.push(Line::from(vec![
                    Span::raw(prefix),
                    Span::styled(file.clone(), style),
                ]));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Commands: ", Style::default().add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from("  o: use ours (keep our changes)"));
            lines.push(Line::from("  t: use theirs (accept incoming)"));
            lines.push(Line::from("  Enter: view conflict diff"));
            lines.push(Line::from("  c: continue rebase"));
            lines.push(Line::from("  s: skip this commit"));
            lines.push(Line::from("  A: abort rebase"));
        } else if status.state == RebaseState::InProgress {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("No conflicts. ", Style::default().fg(Color::Green)),
                Span::raw("Press 'c' to continue."),
            ]));
        }
    } else {
        lines.push(Line::from("No rebase in progress"));
    }

    lines
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let input = Paragraph::new(app.input.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Input (Enter to submit, Esc to cancel) ")
                .border_style(if app.focus == Focus::Input {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::Gray)
                }),
        );

    f.render_widget(input, area);

    // Show cursor in input mode
    if app.focus == Focus::Input {
        f.set_cursor_position((area.x + 1 + app.input_cursor as u16, area.y + 1));
    }
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    // Build status line with session info
    let mut status_spans = Vec::new();

    // Session info (left side)
    if let Some(session) = app.current_session() {
        // Session status with color
        let status_color = session.status.color();
        status_spans.push(Span::styled(
            format!("{} ", session.status.symbol()),
            Style::default().fg(status_color),
        ));

        // Session name
        status_spans.push(Span::styled(
            truncate(&session.name, 25),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));

        // PID if running
        if let Some(pid) = session.pid {
            status_spans.push(Span::raw(" "));
            status_spans.push(Span::styled(
                format!("[PID: {}]", pid),
                Style::default().fg(Color::Green),
            ));
        }

        // Branch name
        status_spans.push(Span::raw(" "));
        status_spans.push(Span::styled(
            format!("({})", session.branch_name),
            Style::default().fg(Color::Cyan),
        ));
    } else {
        status_spans.push(Span::styled(
            "No session selected",
            Style::default().fg(Color::Gray),
        ));
    }

    // Separator
    status_spans.push(Span::raw(" │ "));

    // View mode indicator
    let view_text = match app.view_mode {
        ViewMode::Output => "Output",
        ViewMode::Diff => "Diff",
        ViewMode::Messages => "Messages",
        ViewMode::Rebase => "Rebase",
    };
    status_spans.push(Span::styled(
        view_text,
        Style::default().fg(Color::Yellow),
    ));

    // Separator
    status_spans.push(Span::raw(" │ "));

    // Help text or status message
    let help_text = if let Some(ref msg) = app.status_message {
        msg.clone()
    } else {
        match (app.focus, app.view_mode) {
            (Focus::Sessions, _) => {
                if app.running_session_id.is_some() {
                    "?:help  Space:actions  n:new  w:worktree  i:input  s:stop  d:diff  r:rebase  R:status  a:archive  Tab/Shift+Tab:switch  q:quit"
                } else {
                    "?:help  Space:actions  n:new  w:worktree  d:diff  r:rebase  R:status  a:archive  Enter:start  Tab/Shift+Tab:switch  q:quit"
                }
            }
            (Focus::Output, ViewMode::Diff) => "?:help  j/k:scroll  s:save  o:output  Esc:back",
            (Focus::Output, ViewMode::Rebase) => "?:help  j/k:select  o:ours  t:theirs  c:continue  s:skip  A:abort  Enter:diff  Esc:back",
            (Focus::Output, _) => "?:help  j/k:scroll  Ctrl+d/u:half-page  Ctrl+f/b:full-page  o:output  d:diff  R:rebase  Esc:back  q:quit",
            (Focus::Input, _) => "?:help  Enter:submit  Esc:cancel  Tab/Shift+Tab:switch",
            (Focus::SessionCreation, _) => "Ctrl+Enter:submit  Esc:cancel  Enter:newline",
            (Focus::BranchDialog, _) => "j/k:select  Enter:confirm  Esc:cancel  type to filter",
        }.to_string()
    };
    status_spans.push(Span::styled(help_text, Style::default().fg(Color::Gray)));

    let status = Paragraph::new(Line::from(status_spans))
        .style(Style::default().bg(Color::Black));

    f.render_widget(status, area);
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

fn colorize_output(line: &str) -> Vec<Span<'_>> {
    // Basic colorization for Claude output
    if line.starts_with("Error") || line.starts_with("error") {
        vec![Span::styled(line, Style::default().fg(Color::Red))]
    } else if line.starts_with('>') || line.contains("Tool:") {
        vec![Span::styled(line, Style::default().fg(Color::Yellow))]
    } else if line.starts_with('{') {
        vec![Span::styled(line, Style::default().fg(Color::Cyan))]
    } else {
        vec![Span::raw(line)]
    }
}

fn draw_branch_dialog(f: &mut Frame, dialog: &BranchDialog, area: Rect) {
    // Calculate dialog size - center it on screen
    let dialog_width = 60.min(area.width.saturating_sub(4));
    let dialog_height = 20.min(area.height.saturating_sub(4));
    let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;

    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    // Clear the background
    f.render_widget(Clear, dialog_area);

    // Split dialog into filter input and branch list
    let dialog_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Filter input
            Constraint::Min(5),    // Branch list
        ])
        .split(dialog_area);

    // Draw filter input
    let filter_title = if dialog.filter.is_empty() {
        " Filter (type to search) "
    } else {
        " Filter "
    };
    let filter_text = if dialog.filter.is_empty() {
        String::new()
    } else {
        dialog.filter.clone()
    };
    let filter = Paragraph::new(filter_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(filter_title),
        );
    f.render_widget(filter, dialog_chunks[0]);

    // Draw branch list
    let items: Vec<ListItem> = dialog
        .filtered_indices
        .iter()
        .enumerate()
        .map(|(display_idx, &branch_idx)| {
            let branch = &dialog.branches[branch_idx];
            let is_selected = display_idx == dialog.selected;

            let style = if is_selected {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else if branch.contains('/') {
                // Remote branch - show in different color
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };

            let prefix = if is_selected { "▸ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::raw(prefix),
                Span::styled(truncate(branch, dialog_width as usize - 4), style),
            ]))
        })
        .collect();

    let title = format!(
        " Select Branch ({}/{}) ",
        dialog.filtered_indices.len(),
        dialog.branches.len()
    );
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(title),
    );

    f.render_widget(list, dialog_chunks[1]);
}

fn draw_help_overlay(f: &mut Frame) {
    // Create centered area for help
    let area = f.area();
    let help_width = 80.min(area.width - 4);
    let help_height = 30.min(area.height - 4);

    let help_area = Rect {
        x: (area.width.saturating_sub(help_width)) / 2,
        y: (area.height.saturating_sub(help_height)) / 2,
        width: help_width,
        height: help_height,
    };

    let help_text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("Crystal - Keyboard Navigation Help", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Global", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        ]),
        Line::from("  ?                Toggle this help"),
        Line::from("  Tab              Cycle focus forward (Sessions → Output → Input)"),
        Line::from("  Shift+Tab        Cycle focus backward"),
        Line::from("  Ctrl+c / Ctrl+q  Quit application"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Sessions Panel", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        ]),
        Line::from("  j / Down         Select next session"),
        Line::from("  k / Up           Select previous session"),
        Line::from("  J                Select next project"),
        Line::from("  K                Select previous project"),
        Line::from("  Space            Open context menu for session actions"),
        Line::from("  Enter            Start/resume selected session"),
        Line::from("  n                Create new session (enter prompt)"),
        Line::from("  w                Create worktree from existing branch"),
        Line::from("  d                View diff for selected session"),
        Line::from("  r                Rebase session onto main branch"),
        Line::from("  a                Archive selected session"),
        Line::from("  q                Quit application"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Output Panel", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        ]),
        Line::from("  j / Down         Scroll down one line"),
        Line::from("  k / Up           Scroll up one line"),
        Line::from("  Ctrl+d           Scroll down half page (20 lines)"),
        Line::from("  Ctrl+u           Scroll up half page (20 lines)"),
        Line::from("  Ctrl+f           Scroll down full page (40 lines)"),
        Line::from("  Ctrl+b           Scroll up full page (40 lines)"),
        Line::from("  PageDown         Scroll down 10 lines"),
        Line::from("  PageUp           Scroll up 10 lines"),
        Line::from("  g                Jump to top"),
        Line::from("  G                Jump to bottom"),
        Line::from("  o                Switch to output view"),
        Line::from("  d                Switch to diff view"),
        Line::from("  Esc              Return to Sessions panel"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Input Panel", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        ]),
        Line::from("  Enter            Submit prompt and create session"),
        Line::from("  Esc              Cancel and return to Sessions panel"),
        Line::from("  Arrow keys       Navigate input text"),
        Line::from("  Home / End       Jump to start/end of input"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Branch Dialog", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        ]),
        Line::from("  j / Down         Select next branch"),
        Line::from("  k / Up           Select previous branch"),
        Line::from("  Enter            Confirm selection"),
        Line::from("  Esc              Cancel"),
        Line::from("  Type             Filter branches"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Press ? or q or Esc to close this help", Style::default().fg(Color::Gray).add_modifier(Modifier::ITALIC))
        ]),
    ];

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help ")
                .border_style(Style::default().fg(Color::Cyan))
                .style(Style::default().bg(Color::Black))
        )
        .wrap(Wrap { trim: false });

    f.render_widget(help, help_area);
}

fn draw_session_creation_modal(f: &mut Frame, app: &App) {
    use ratatui::layout::Alignment;

    // Calculate centered modal area (80% width, 60% height)
    let area = f.area();
    let modal_width = (area.width * 4) / 5;
    let modal_height = (area.height * 3) / 5;
    let modal_x = (area.width - modal_width) / 2;
    let modal_y = (area.height - modal_height) / 2;

    let modal_area = Rect {
        x: modal_x,
        y: modal_y,
        width: modal_width,
        height: modal_height,
    };

    // Clear the background with a semi-transparent effect
    let bg_block = Block::default()
        .style(Style::default().bg(Color::Black));
    f.render_widget(bg_block, modal_area);

    // Split modal into sections
    let modal_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Min(10),   // Input area
            Constraint::Length(3), // Info/stats
        ])
        .split(modal_area);

    // Draw title
    let title = Paragraph::new("Create New Session")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::TOP | Borders::LEFT | Borders::RIGHT));
    f.render_widget(title, modal_chunks[0]);

    // Draw input area with content
    let input_text = &app.session_creation_input;
    let lines: Vec<Line> = input_text
        .split('\n')
        .map(|line| Line::from(line))
        .collect();

    let input_widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .style(Style::default().fg(Color::Yellow))
        )
        .wrap(Wrap { trim: false });

    f.render_widget(input_widget, modal_chunks[1]);

    // Calculate cursor position for rendering
    if let Some((cursor_x, cursor_y)) = calculate_cursor_position(
        input_text,
        app.session_creation_cursor,
        modal_chunks[1].width.saturating_sub(2) as usize,
    ) {
        f.set_cursor_position((
            modal_chunks[1].x + 1 + cursor_x as u16,
            modal_chunks[1].y + cursor_y as u16,
        ));
    }

    // Draw info bar
    let char_count = input_text.len();
    let line_count = input_text.lines().count().max(1);
    let info_text = format!(
        " {} chars | {} lines | Ctrl+Enter: Submit | Esc: Cancel ",
        char_count, line_count
    );

    let info = Paragraph::new(info_text)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray))
        .block(Block::default().borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT));

    f.render_widget(info, modal_chunks[2]);
}

/// Calculate the visual cursor position in a multi-line text area
fn calculate_cursor_position(text: &str, cursor: usize, width: usize) -> Option<(usize, usize)> {
    let mut x = 0;
    let mut y = 0;
    let mut pos = 0;

    for ch in text.chars() {
        if pos >= cursor {
            break;
        }

        if ch == '\n' {
            y += 1;
            x = 0;
        } else {
            x += 1;
            if x >= width {
                y += 1;
                x = 0;
            }
        }

        pos += ch.len_utf8();
    }

    Some((x, y))
}

fn draw_context_menu(f: &mut Frame, app: &App, area: Rect) {
    if let Some(ref menu) = app.context_menu {
        // Calculate menu dimensions
        let menu_width = 50;
        let menu_height = menu.actions.len() as u16 + 4; // +4 for title and borders

        // Center the menu
        let menu_x = (area.width.saturating_sub(menu_width)) / 2;
        let menu_y = (area.height.saturating_sub(menu_height)) / 2;

        let menu_area = Rect {
            x: menu_x,
            y: menu_y,
            width: menu_width,
            height: menu_height,
        };

        // Create menu items
        let items: Vec<ListItem> = menu
            .actions
            .iter()
            .enumerate()
            .map(|(idx, action)| {
                let is_selected = idx == menu.selected;
                let style = if is_selected {
                    Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let key_style = if is_selected {
                    Style::default().bg(Color::Cyan).fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::Yellow)
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!(" [{:>5}] ", action.key_hint()), key_style),
                    Span::styled(action.label(), style),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(" Session Actions (↑↓ to navigate, Enter to select, Esc to close) ")
                    .style(Style::default().bg(Color::Black)),
            );

        // Clear the area first (creates overlay effect)
        f.render_widget(
            Block::default()
                .style(Style::default().bg(Color::Black)),
            menu_area,
        );

        f.render_widget(list, menu_area);
    }
}
