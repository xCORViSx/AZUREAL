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
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;
use std::time::Duration;

use crate::app::{App, Focus, ViewMode};
use crate::claude::{ClaudeEvent, ClaudeProcess};
use crate::config::Config;
use crate::db::Database;
use crate::git::Git;
use crate::models::{OutputType, SessionStatus};
use crate::session::SessionManager;

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

        // Poll for Claude output - collect events first to avoid borrow issues
        let events: Vec<ClaudeEvent> = if let Some(ref receiver) = app.claude_receiver {
            let mut events = Vec::new();
            while let Ok(event) = receiver.try_recv() {
                events.push(event);
            }
            events
        } else {
            Vec::new()
        };

        let mut should_clear_receiver = false;
        for event in events {
            match event {
                ClaudeEvent::Output(output) => {
                    // Save to database first
                    if let Some(session) = app.current_session() {
                        let session_id = session.id.clone();
                        let _ = app.db.add_session_output(
                            &session_id,
                            output.output_type,
                            &output.data,
                        );
                    }
                    app.add_output(output.data);
                }
                ClaudeEvent::Started { pid } => {
                    if let Some(session) = app.current_session() {
                        let session_id = session.id.clone();
                        let _ = app.db.update_session_pid(&session_id, Some(pid));
                        let _ = app.db.update_session_status(&session_id, SessionStatus::Running);
                        app.update_session_status(&session_id, SessionStatus::Running);
                    }
                    app.set_status(format!("Claude started (PID: {})", pid));
                }
                ClaudeEvent::Exited { code } => {
                    if let Some(session) = app.current_session() {
                        let session_id = session.id.clone();
                        let status = if code == Some(0) {
                            SessionStatus::Completed
                        } else {
                            SessionStatus::Failed
                        };
                        let _ = app.db.update_session_status(&session_id, status);
                        app.update_session_status(&session_id, status);
                    }
                    app.set_status(format!("Claude exited with code: {:?}", code));
                    should_clear_receiver = true;
                }
                ClaudeEvent::Error(e) => {
                    app.add_output(format!("Error: {}", e));
                    app.set_status(format!("Error: {}", e));
                }
            }
        }
        if should_clear_receiver {
            app.claude_receiver = None;
        }

        // Handle input events with timeout
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Global keybindings
                match (key.modifiers, key.code) {
                    (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                        app.should_quit = true;
                    }
                    (KeyModifiers::CONTROL, KeyCode::Char('q')) => {
                        app.should_quit = true;
                    }
                    _ => {}
                }

                if app.should_quit {
                    break;
                }

                // Mode-specific keybindings
                match app.focus {
                    Focus::Sessions => match key.code {
                        KeyCode::Char('j') | KeyCode::Down => app.select_next_session(),
                        KeyCode::Char('k') | KeyCode::Up => app.select_prev_session(),
                        KeyCode::Char('J') => app.select_next_project(),
                        KeyCode::Char('K') => app.select_prev_project(),
                        KeyCode::Tab => app.focus = Focus::Output,
                        KeyCode::Char('n') => {
                            // Create new session
                            app.focus = Focus::Input;
                            app.set_status("Enter prompt for new session:");
                        }
                        KeyCode::Char('d') => {
                            // View diff
                            if let Some(session) = app.current_session() {
                                if let Some(project) = app.current_project() {
                                    match Git::get_diff(&session.worktree_path, &project.main_branch) {
                                        Ok(diff) => {
                                            app.diff_text = Some(diff.diff_text);
                                            app.view_mode = ViewMode::Diff;
                                            app.focus = Focus::Output;
                                        }
                                        Err(e) => {
                                            app.set_status(format!("Failed to get diff: {}", e));
                                        }
                                    }
                                }
                            }
                        }
                        KeyCode::Char('r') => {
                            // Rebase from main
                            if let Some(session) = app.current_session() {
                                if let Some(project) = app.current_project() {
                                    match Git::rebase_onto_main(&session.worktree_path, &project.main_branch) {
                                        Ok(()) => {
                                            app.set_status("Rebased successfully");
                                        }
                                        Err(e) => {
                                            app.set_status(format!("Rebase failed: {}", e));
                                        }
                                    }
                                }
                            }
                        }
                        KeyCode::Char('a') => {
                            // Archive session
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
                        KeyCode::Enter => {
                            // Start/resume session
                            if let Some(session) = app.current_session() {
                                if session.status == SessionStatus::Pending
                                    || session.status == SessionStatus::Stopped
                                    || session.status == SessionStatus::Completed
                                {
                                    // Start Claude
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
                    },
                    Focus::Output => match key.code {
                        KeyCode::Char('j') | KeyCode::Down => app.scroll_output_down(1),
                        KeyCode::Char('k') | KeyCode::Up => app.scroll_output_up(1),
                        KeyCode::Char('G') => app.scroll_output_to_bottom(),
                        KeyCode::Char('g') => app.output_scroll = 0,
                        KeyCode::PageDown => app.scroll_output_down(10),
                        KeyCode::PageUp => app.scroll_output_up(10),
                        KeyCode::Tab => app.focus = Focus::Input,
                        KeyCode::Char('o') => app.view_mode = ViewMode::Output,
                        KeyCode::Char('d') => {
                            if let Some(session) = app.current_session() {
                                if let Some(project) = app.current_project() {
                                    if let Ok(diff) = Git::get_diff(&session.worktree_path, &project.main_branch) {
                                        app.diff_text = Some(diff.diff_text);
                                        app.view_mode = ViewMode::Diff;
                                    }
                                }
                            }
                        }
                        KeyCode::Esc => app.focus = Focus::Sessions,
                        KeyCode::Char('q') => app.should_quit = true,
                        _ => {}
                    },
                    Focus::Input => match key.code {
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

                                // Create new session
                                if let Some(project) = app.current_project().cloned() {
                                    match SessionManager::new(&app.db).create_session(&project, &prompt) {
                                        Ok(session) => {
                                            app.set_status(format!("Created session: {}", session.name));
                                            let _ = app.refresh_sessions();

                                            // Start Claude immediately
                                            match claude_process.spawn(
                                                &session.worktree_path,
                                                &session.initial_prompt,
                                                None,
                                            ) {
                                                Ok(rx) => {
                                                    app.claude_receiver = Some(rx);
                                                    app.selected_session = Some(0);
                                                    app.load_session_output();
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
                    },
                }
            }
        }

        if app.should_quit {
            break;
        }
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
    let status_text = if let Some(ref msg) = app.status_message {
        msg.clone()
    } else {
        let help = match app.focus {
            Focus::Sessions => "n:new  d:diff  r:rebase  a:archive  Enter:start  Tab:switch  q:quit",
            Focus::Output => "j/k:scroll  o:output  d:diff  Tab:switch  Esc:back  q:quit",
            Focus::Input => "Enter:submit  Esc:cancel",
        };
        help.to_string()
    };

    let status = Paragraph::new(status_text)
        .style(Style::default().fg(Color::Gray));

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

