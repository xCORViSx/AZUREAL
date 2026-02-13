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
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame, Terminal,
};
use std::io;

use crate::app::{App, Focus};
use crate::config::Config;

use super::event_loop;
use super::{draw_dialogs, draw_god_files, draw_input, draw_output, draw_projects, draw_sidebar, draw_status, draw_terminal, draw_viewer, draw_wizard};

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

    // Show splash screen immediately — visible while project/session loading runs.
    // Minimum 3s display so the branding registers even on fast machines.
    terminal.draw(draw_splash)?;
    let splash_start = std::time::Instant::now();

    let mut app = App::new();
    app.update_terminal_title();
    app.load()?;
    app.load_run_commands();
    app.load_preset_prompts();
    app.load_session_output();
    let config = Config::load().unwrap_or_default();

    // Hold splash for remainder of 3s minimum (loading time counts toward it)
    let elapsed = splash_start.elapsed();
    let min_splash = std::time::Duration::from_secs(3);
    if elapsed < min_splash {
        std::thread::sleep(min_splash - elapsed);
    }

    let result = event_loop::run_app(&mut terminal, &mut app, config).await;

    // Restore default terminal title on exit
    let _ = execute!(terminal.backend_mut(), crossterm::terminal::SetTitle(""));

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
    // Projects panel takes over the screen (shown on startup without git repo, or via 'P')
    if app.is_projects_panel_active() {
        draw_projects::draw_projects_panel(f, app);
        return;
    }

    // God File panel takes over the screen (shown via 'g' in Worktrees)
    if app.god_file_panel.is_some() {
        draw_god_files::draw_god_files_panel(f, app);
        return;
    }

    // Wizard modal takes over the screen
    if app.is_wizard_active() {
        draw_wizard::draw_wizard_modal(f, app);
        return;
    }

    // Layout: Convo gets full height, Input/Terminal spans Worktrees + Viewer
    //
    // ┌──────────┬───────────────────────┬─────────────┐
    // │Worktrees │       Viewer          │             │
    // │  (40)    │       (rem)           │  Convo (80) │
    // ├──────────┴───────────────────────┤             │
    // │     Input / Terminal             │             │
    // ├──────────────────────────────────┴─────────────┤
    // │                 Status Bar                     │
    // └───────────────────────────────────────────────┘
    // Worktrees pane shows FileTree overlay when 'f' is pressed.
    // Convo pane shows Session list overlay when 's' is pressed.

    // Step 1: Reserve status bar at bottom
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(1)])
        .split(f.area());
    let content_area = outer[0];
    let status_area = outer[1];

    // Step 2: Split content horizontally — Worktrees (40) | Viewer (remaining) | Convo (80 fixed)
    let h_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(40),  // Worktrees (or FileTree overlay)
            Constraint::Min(20),    // Viewer (remaining space)
            Constraint::Length(80), // Convo (fixed 80 cols)
        ])
        .split(content_area);

    // Left side = Worktrees + Viewer widths for the input area span
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

    // Step 4: Split top 2 panes horizontally (Worktrees + Viewer)
    let top_h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(40),  // Worktrees (or FileTree overlay)
            Constraint::Min(10),    // Viewer (all remaining left-side width)
        ])
        .split(top_panes_area);

    // Cache all pane rects for mouse click hit-testing and fast-path rendering
    app.input_area = input_area;
    app.pane_worktrees = top_h[0];
    app.pane_viewer = top_h[1];
    app.pane_convo = convo_area;

    // Draw panes — worktrees pane shows file tree overlay when toggled
    if app.show_file_tree {
        draw_sidebar::draw_file_tree_overlay(f, app, top_h[0]);
    } else {
        draw_sidebar::draw_sidebar(f, app, top_h[0]);
    }
    draw_viewer::draw_viewer(f, app, top_h[1]);
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
    // Preset prompt overlays (picker takes priority over dialog)
    if app.preset_prompt_picker.is_some() {
        draw_dialogs::draw_preset_prompt_picker(f, app, f.area());
    } else if app.preset_prompt_dialog.is_some() {
        draw_dialogs::draw_preset_prompt_dialog(f, app);
    }
}

/// Block-pixel ASCII art splash screen shown during app initialization.
/// Renders "AZUREAL" logo centered on screen with acronym subtitle and
/// a dim spring azure butterfly outline in the background as the app mascot.
fn draw_splash(f: &mut Frame) {
    let az = Color::Rgb(51, 153, 255);
    let dim = Color::Rgb(25, 76, 128);
    // Very dim butterfly color — just barely visible behind text
    let butterfly_color = Color::Rgb(15, 45, 80);
    let logo_style = Style::default().fg(az);
    let dim_style = Style::default().fg(dim);
    let bf_style = Style::default().fg(butterfly_color);

    let area = f.area();

    // ── Spring azure butterfly (background layer) ──
    // Pure ░ fill, no box-drawing. Two wide upper wings, two smaller lower wings,
    // narrow body gap (2 spaces) down the center, antennae at top.
    // 37 rows tall so it extends well above/below the 26-row text block.
    let butterfly: Vec<&str> = vec![
        "                         ░                          ░",
        "                          ░░                      ░░",
        "                            ░░                  ░░",
        "                              ░░              ░░",
        "                      ░░░░░░░░░░░░░░░░░░░                    ░░░░░░░░░░░░░░░░░░░",
        "                  ░░░░░░░░░░░░░░░░░░░░░░░░                    ░░░░░░░░░░░░░░░░░░░░░░░░",
        "               ░░░░░░░░░░░░░░░░░░░░░░░░░░░░                  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "             ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░                ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "           ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░              ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "          ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░            ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "         ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░          ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "        ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░        ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "        ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░      ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "        ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░    ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "        ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "         ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "          ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "           ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░    ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "            ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░    ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "              ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░      ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "                ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░        ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "                  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░          ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "                     ░░░░░░░░░░░░░░░░░░░░░░░░░░            ░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "                        ░░░░░░░░░░░░░░░░░░░░░░░░          ░░░░░░░░░░░░░░░░░░░░░░░░",
        "                       ░░░░░░░░░░░░░░░░░░░░░░░░░░        ░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "                     ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░      ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "                    ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░    ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "                   ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "                   ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "                    ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "                     ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "                       ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "                         ░░░░░░░░░░░░░░░░░░░░░░░░░░    ░░░░░░░░░░░░░░░░░░░░░░░░░░",
        "                           ░░░░░░░░░░░░░░░░░░░░░░░      ░░░░░░░░░░░░░░░░░░░░░░░",
        "                              ░░░░░░░░░░░░░░░░░░          ░░░░░░░░░░░░░░░░░░",
        "                                 ░░░░░░░░░░░░░              ░░░░░░░░░░░░░",
        "                                     ░░░░░░░                    ░░░░░░░",
    ];

    // Center butterfly on the SAME vertical origin as the text content
    // so wings extend equally above and below. Text is 26 rows, butterfly
    // is taller — offset by the difference so they share the same center.
    let bf_h = butterfly.len() as u16;
    let bf_lines: Vec<Line<'static>> = butterfly.iter()
        .map(|row| Line::from(Span::styled(row.to_string(), bf_style)))
        .collect();
    let bf_widget = Paragraph::new(bf_lines).alignment(Alignment::Center);

    // ── Text content (foreground layer — overwrites butterfly where they overlap) ──
    let logo: Vec<&str> = vec![
        "  ████████      ████████████    ████    ████    ██████████      ████████████      ████████      ████          ",
        "  ████████      ████████████    ████    ████    ██████████      ████████████      ████████      ████          ",
        "████    ████          ████      ████    ████    ████    ████    ████            ████    ████    ████          ",
        "████    ████          ████      ████    ████    ████    ████    ████            ████    ████    ████          ",
        "████████████        ████        ████    ████    ██████████      ████████        ████████████    ████          ",
        "████████████        ████        ████    ████    ██████████      ████████        ████████████    ████          ",
        "████    ████      ████          ████    ████    ████    ████    ████            ████    ████    ████          ",
        "████    ████      ████          ████    ████    ████    ████    ████            ████    ████    ████          ",
        "████    ████    ████████████      ██████████    ████    ████    ████████████    ████    ████    ████████████  ",
        "████    ████    ████████████      ██████████    ████    ████    ████████████    ████    ████    ████████████  ",
    ];
    let acronym: Vec<&str> = vec![
        "▄▀▀▄ ▄▀▀▀ ▀▄ ▄▀ █▄  █ ▄▀▀▀ █  █ █▀▀▄ ▄▀▀▄ █▄  █ ▄▀▀▄ █  █ ▄▀▀▀   ▀▀▀█▀ ▄▀▀▄ █▄  █ █▀▀▀ █▀▀▄",
        "█▄▄█  ▀▀▄   █   █ ▀▄█ █    █▀▀█ █▄▄▀ █  █ █ ▀▄█ █  █ █  █  ▀▀▄    ▄▀   █  █ █ ▀▄█ █▀▀  █  █",
        "█  █ ▄▄▄▀   █   █   █ ▀▄▄▄ █  █ █ ▀▄ ▀▄▄▀ █   █ ▀▄▄▀ ▀▄▄▀ ▄▄▄▀   █▄▄▄▄ ▀▄▄▀ █   █ █▄▄▄ █▄▄▀",
        "█  █ █▄  █ ▀█▀ █▀▀▀ ▀█▀ █▀▀▀ █▀▀▄   █▀▀▄ █  █ █▄  █ ▀▀█▀▀ ▀█▀ █▄ ▄█ █▀▀▀",
        "█  █ █ ▀▄█  █  █▀▀   █  █▀▀  █  █   █▄▄▀ █  █ █ ▀▄█   █    █  █ ▀ █ █▀▀ ",
        "▀▄▄▀ █   █ ▄█▄ █    ▄█▄ █▄▄▄ █▄▄▀   █ ▀▄ ▀▄▄▀ █   █   █   ▄█▄ █   █ █▄▄▄",
        "█▀▀▀ █▄  █ █   █ ▀█▀ █▀▀▄ ▄▀▀▄ █▄  █ █▄ ▄█ █▀▀▀ █▄  █ ▀▀█▀▀",
        "█▀▀  █ ▀▄█ ▀▄ ▄▀  █  █▄▄▀ █  █ █ ▀▄█ █ ▀ █ █▀▀  █ ▀▄█   █  ",
        "█▄▄▄ █   █  ▀▄▀  ▄█▄ █ ▀▄ ▀▄▄▀ █   █ █   █ █▄▄▄ █   █   █  ",
        "█  █   ▄▀▀▄ ▄▀▀▀ █▀▀▀ █▄  █ ▀▀█▀▀ ▀█▀ ▄▀▀▀   █    █    █▄ ▄█ ▄▀▀▀",
        "▀▀▀█   █▄▄█ █ ▄▄ █▀▀  █ ▀▄█   █    █  █      █    █    █ ▀ █  ▀▀▄",
        "   █   █  █ ▀▄▄█ █▄▄▄ █   █   █   ▄█▄ ▀▄▄▄   █▄▄▄ █▄▄▄ █   █ ▄▄▄▀",
    ];

    let logo_height = logo.len() as u16;
    let acronym_height = acronym.len() as u16;
    let total_height = logo_height + 1 + acronym_height + 2 + 1;
    // Center point for all content — both butterfly and text share this
    let center_y = area.y + area.height / 2;
    let text_start_y = center_y.saturating_sub(total_height / 2);

    // Render butterfly first (background), centered on same point as text
    let bf_start_y = center_y.saturating_sub(bf_h / 2);
    f.render_widget(bf_widget, ratatui::layout::Rect::new(
        area.x, bf_start_y, area.width, bf_h.min(area.height.saturating_sub(bf_start_y.saturating_sub(area.y))),
    ));

    // Then render text on top (foreground overwrites butterfly cells)
    let mut lines: Vec<Line<'static>> = logo.iter()
        .map(|row| Line::from(Span::styled(row.to_string(), logo_style)))
        .collect();
    lines.push(Line::from(""));
    for row in &acronym {
        lines.push(Line::from(Span::styled(row.to_string(), dim_style)));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("L o a d i n g   p r o j e c t . . .", logo_style)));

    let splash = Paragraph::new(lines).alignment(Alignment::Center);
    let splash_area = ratatui::layout::Rect::new(
        area.x, text_start_y, area.width, total_height,
    );
    f.render_widget(splash, splash_area);
}
