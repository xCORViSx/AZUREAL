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
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;
use std::time::Duration;

use crate::app::{App, Focus, ViewMode};
use crate::claude::{ClaudeEvent, ClaudeProcess};
use crate::config::Config;
use crate::db::Database;
use crate::git::Git;
use crate::models::SessionStatus;

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
        _ => {}
    }

    // Mode-specific keybindings
    match app.focus {
        Focus::Sessions => handle_sessions_input(key, app, claude_process)?,
        Focus::Output => handle_output_input(key, app)?,
        Focus::Input => handle_input_mode(key, app, claude_process)?,
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
        KeyCode::Char('n') => {
            app.focus = Focus::Input;
            app.set_status("Enter prompt for new session:");
        }
        KeyCode::Char('d') => {
            if let Err(e) = app.load_diff() {
                app.set_status(format!("Failed to get diff: {}", e));
            }
        }
        KeyCode::Char('r') => {
            if let Err(e) = app.rebase_current_session() {
                app.set_status(format!("Rebase failed: {}", e));
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
        KeyCode::Char('q') => app.should_quit = true,
        _ => {}
    }
    Ok(())
}

/// Handle keyboard input when Output pane is focused
fn handle_output_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.scroll_output_down(1),
        KeyCode::Char('k') | KeyCode::Up => app.scroll_output_up(1),
        KeyCode::Char('G') => app.scroll_output_to_bottom(),
        KeyCode::Char('g') => app.output_scroll = 0,
        KeyCode::PageDown => app.scroll_output_down(10),
        KeyCode::PageUp => app.scroll_output_up(10),
        KeyCode::Tab => app.focus = Focus::Input,
        KeyCode::Char('o') => app.view_mode = ViewMode::Output,
        KeyCode::Char('d') => {
            if let Err(e) = app.load_diff() {
                app.set_status(format!("Failed to get diff: {}", e));
            }
        }
        KeyCode::Esc => app.focus = Focus::Sessions,
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
                let prompt = app.input.clone();
                app.clear_input();

                match app.create_new_session(prompt) {
                    Ok(session) => {
                        app.set_status(format!("Created session: {}", session.name));

                        // Start Claude immediately
                        match claude_process.spawn(
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
    };

    let content = match app.view_mode {
        ViewMode::Output => {
            let lines: Vec<Line> = app
                .output_lines
                .iter()
                .skip(app.output_scroll)
                .map(|line| Line::from(colorize_output(line)))
                .collect();
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
        match app.focus {
            Focus::Sessions => "n:new  d:diff  r:rebase  a:archive  Enter:start  q:quit",
            Focus::Output => "j/k:scroll  o:output  d:diff  Esc:back  q:quit",
            Focus::Input => "Enter:submit  Esc:cancel",
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

