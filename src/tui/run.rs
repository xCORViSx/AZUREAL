//! TUI entry point and main layout
//!
//! Contains the run() function to start the TUI and the ui() layout function.

use anyhow::Result;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture,
        KeyboardEnhancementFlags, PushKeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame, Terminal,
};
use std::io;

use crate::app::{App, Focus};
use crate::config::Config;
use super::util::AZURE;

use super::event_loop;
use super::{draw_dialogs, draw_file_tree, draw_input, draw_output, draw_sidebar, draw_status, draw_terminal, draw_viewer, draw_wizard};

/// Run the TUI application
pub async fn run() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    // Enable Kitty keyboard protocol so Shift+Enter is distinguishable from Enter.
    // DISAMBIGUATE alone makes Enter → CSI 13u, Shift+Enter → CSI 13;2u.
    // REPORT_EVENT_TYPES adds Press/Release/Repeat — only Press is processed.
    // We intentionally omit REPORT_ALL_KEYS because it makes Shift+letter
    // arrive as (SHIFT, Char('1')) instead of (NONE, Char('!')), breaking
    // secondary character input (!, @, #, etc.).
    let kbd_enhanced = execute!(
        stdout,
        PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
        )
    ).is_ok();

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    app.load()?;
    app.load_run_commands();

    // Load output for the initially selected session
    app.load_session_output();

    let config = Config::load().unwrap_or_default();
    let result = event_loop::run_app(&mut terminal, &mut app, config).await;

    // Pop keyboard enhancement before leaving (only if we pushed it)
    if kbd_enhanced {
        let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    }
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

    // Layout: Convo gets full height, Input/Terminal spans first 3 panes only
    //
    // ┌──────────┬──────────┬─────────────┬─────────────┐
    // │ Sessions │ FileTree │   Viewer    │             │
    // ├──────────┴──────────┴─────────────┤    Convo    │
    // │     Input / Terminal              │             │
    // ├───────────────────────────────────┴─────────────┤
    // │                 Status Bar                      │
    // └────────────────────────────────────────────────┘

    // Step 1: Reserve title bar at top and status bar at bottom
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(5), Constraint::Length(1)])
        .split(f.area());
    let title_area = outer[0];
    let content_area = outer[1];
    let status_area = outer[2];

    // Step 2: Split content horizontally — left side (3 panes + input) vs Convo (full height)
    let h_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(40 + 40), // Sessions + FileTree base width
            Constraint::Percentage(50),  // Viewer (50% of remaining after 80 cols)
            Constraint::Percentage(50),  // Convo (50% of remaining after 80 cols)
        ])
        .split(content_area);

    // Merge first two chunks into "left side" for the vertical split
    let left_width = h_split[0].width + h_split[1].width;
    let convo_area = h_split[2];

    // Step 3: Split left side vertically — top 3 panes + input/terminal at bottom
    let input_height = if app.terminal_mode {
        app.terminal_height + 2
    } else {
        // Dynamic input height: count visual lines from newlines + word-wrapping
        let input_inner_width = left_width.saturating_sub(2) as usize;
        let input_lines = if input_inner_width > 0 && !app.input.is_empty() {
            let mut rows = 1usize;
            let mut col = 0usize;
            for c in app.input.chars() {
                if c == '\n' {
                    rows += 1;
                    col = 0;
                } else {
                    let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
                    if col + w > input_inner_width { rows += 1; col = w; }
                    else { col += w; }
                }
            }
            rows
        } else {
            1
        };
        // Cap at 3/4 of available height so top panes stay visible
        let max_input = (content_area.height * 3 / 4).max(3);
        (input_lines as u16 + 2).min(max_input) // +2 for borders
    };

    // Build a Rect for the left side manually (covers Sessions + FileTree + Viewer)
    let left_rect = ratatui::layout::Rect::new(
        content_area.x, content_area.y, left_width, content_area.height,
    );
    let left_v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(input_height)])
        .split(left_rect);
    let top_panes_area = left_v[0];
    let input_area = left_v[1];

    // Step 4: Split top 3 panes horizontally
    let top_h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(40),  // Sessions
            Constraint::Length(40),  // FileTree
            Constraint::Min(10),    // Viewer (all remaining left-side width)
        ])
        .split(top_panes_area);

    // Cache all pane rects for mouse click hit-testing and fast-path rendering
    app.input_area = input_area;
    app.pane_sessions = top_h[0];
    app.pane_file_tree = top_h[1];
    app.pane_viewer = top_h[2];
    app.pane_convo = convo_area;

    // Draw title bar
    draw_title_bar(f, app, title_area);

    // Draw panes
    draw_sidebar::draw_sidebar(f, app, top_h[0]);
    draw_file_tree::draw_file_tree(f, app, top_h[1]);
    draw_viewer::draw_viewer(f, app, top_h[2]);
    draw_output::draw_output(f, app, convo_area);

    if app.terminal_mode {
        draw_terminal::draw_terminal(f, app, input_area);
    } else {
        draw_input::draw_input(f, app, input_area);
    }
    draw_status::draw_status(f, app, status_area);

    // Draw overlays
    if app.focus == Focus::WorktreeCreation {
        draw_dialogs::draw_worktree_creation_modal(f, app);
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
    // Run command overlays (picker takes priority over dialog)
    if app.run_command_picker.is_some() {
        draw_dialogs::draw_run_command_picker(f, app, f.area());
    } else if app.run_command_dialog.is_some() {
        draw_dialogs::draw_run_command_dialog(f, app);
    }
}

/// Draw the title bar: "azureal" left, session title centered, branch right
fn draw_title_bar(f: &mut Frame, app: &App, area: Rect) {
    let width = area.width as usize;

    // Left: app name
    let left = " azureal ";
    let left_len = left.len();

    // Right: branch name (if session selected)
    let right_text = app.current_session().map(|s| format!(" {} ", s.name())).unwrap_or_default();
    let right_len = right_text.len();

    // Center: session title wrapped in brackets, ellipsied to fit between left and right
    // Uses cached title_session_name (updated on session switch, zero I/O here)
    let center_text = &app.title_session_name;

    // Wrap center text in brackets and ellipsis to fit available space
    let max_center = width.saturating_sub(left_len + right_len + 2);
    let bracketed = if center_text.is_empty() {
        String::new()
    } else {
        let inner_max = max_center.saturating_sub(2); // room for [ ]
        if center_text.chars().count() <= inner_max {
            format!("[{}]", center_text)
        } else if inner_max > 3 {
            // Ellipsis the center text
            let truncated: String = center_text.chars().take(inner_max - 1).collect();
            format!("[{}…]", truncated)
        } else {
            String::new()
        }
    };

    // Build the full line: left-pad center to be visually centered, fill rest with spaces
    let center_len = bracketed.chars().count();
    // Target center position (middle of total width)
    let center_start = if center_len > 0 {
        (width / 2).saturating_sub(center_len / 2).max(left_len)
    } else {
        0
    };
    // Clamp so center doesn't overlap right
    let center_start = center_start.min(width.saturating_sub(right_len + center_len));
    let gap_left = center_start.saturating_sub(left_len);
    let gap_right = width.saturating_sub(center_start + center_len + right_len);

    let mut spans = vec![
        Span::styled(left, Style::default().fg(AZURE).add_modifier(Modifier::BOLD)),
    ];
    if center_len > 0 {
        spans.push(Span::raw(" ".repeat(gap_left)));
        spans.push(Span::styled(bracketed, Style::default().fg(Color::White)));
    } else {
        spans.push(Span::raw(" ".repeat(gap_left)));
    }
    spans.push(Span::raw(" ".repeat(gap_right)));
    if !right_text.is_empty() {
        spans.push(Span::styled(right_text.clone(), Style::default().fg(Color::DarkGray)));
    }

    let bar = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(Color::Rgb(20, 20, 30)))
        .alignment(Alignment::Left);
    f.render_widget(bar, area);
}
