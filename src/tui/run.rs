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
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use std::io;

use crate::app::{App, Focus};
use crate::config::Config;

use super::event_loop;
use super::util::AZURE;
use super::{draw_dialogs, draw_git_actions, draw_health, draw_input, draw_output, draw_projects, draw_sidebar, draw_status, draw_terminal, draw_viewer};

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
    // Auto-detect Nerd Font support by probing a PUA glyph during splash
    app.nerd_fonts = super::file_icons::detect_nerd_font();
    if !app.nerd_fonts {
        app.set_status("Nerd Font not detected — using emoji icons. Install a Nerd Font for richer file tree icons");
    }

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

    // Git panel takes over the screen (Shift+G) — full-app 4-pane layout
    if app.git_actions_panel.is_some() {
        draw_git_actions::draw_git_layout(f, app);
        return;
    }



    // Layout: Convo gets full height, Input/Terminal spans Worktrees + Viewer
    //
    // ┌──────────┬──────────────────────────┬──────────────┐
    // │Worktrees │         Viewer           │              │
    // │  (15%)   │         (50%)            │  Convo (35%) │
    // ├──────────┴──────────────────────────┤              │
    // │     Input / Terminal                │              │
    // ├─────────────────────────────────────┴──────────────┤
    // │                  Status Bar                        │
    // └────────────────────────────────────────────────────┘
    // Worktrees pane shows FileTree overlay when 'f' is pressed.
    // Convo pane shows Session list overlay when 's' is pressed.

    // Step 1: Reserve status bar at bottom
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(1)])
        .split(f.area());
    let content_area = outer[0];
    let status_area = outer[1];

    // Step 2: Split content horizontally — Worktrees (15%) | Viewer (50%) | Convo (35%)
    let h_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(15), // Worktrees (or FileTree overlay)
            Constraint::Percentage(50), // Viewer
            Constraint::Percentage(35), // Convo
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

    // RCR approval dialog — rendered over convo pane after Claude exits during RCR
    if app.rcr_session.as_ref().is_some_and(|m| m.approval_pending) {
        draw_output::draw_rcr_approval(f, convo_area);
    }

    // Post-merge dialog — rendered over convo pane after successful squash merge or RCR accept
    if let Some(ref pmd) = app.post_merge_dialog {
        draw_output::draw_post_merge_dialog(f, convo_area, pmd);
    }

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
    // Run command overlays (picker takes priority over dialog)
    if app.run_command_picker.is_some() {
        draw_dialogs::draw_run_command_picker(f, app, f.area());
    } else if app.run_command_dialog.is_some() {
        draw_dialogs::draw_run_command_dialog(f, app);
    }
    // Preset prompt overlays — dialog draws on top of picker (spawned from picker via e/a)
    if app.preset_prompt_picker.is_some() {
        draw_dialogs::draw_preset_prompt_picker(f, app, f.area());
    }
    if app.preset_prompt_dialog.is_some() {
        draw_dialogs::draw_preset_prompt_dialog(f, app);
    }
    // Worktree Health panel overlay (Shift+H global) — hidden during scope mode (file tree is the UI)
    if app.health_panel.is_some() && !app.god_file_filter_mode {
        draw_health::draw_health_panel(f, app);
    }
    // Git panel now takes over the full screen (early return above)
    // Debug dump naming dialog (⌃d) — small centered input popup
    if let Some(ref name) = app.debug_dump_naming {
        draw_debug_dump_naming(f, name);
    }
    // Debug dump saving indicator — shown while the dump I/O runs on next frame
    if let Some(ref name) = app.debug_dump_saving {
        draw_debug_dump_saving(f, name);
    }
    // Auto-rebase success dialog — 2-second toast after successful auto-rebase
    if let Some((ref branch, _)) = app.auto_rebase_success_until {
        draw_auto_rebase_dialog(f, branch, true);
    }
    // Generic loading indicator — highest z-order, shown while a deferred action runs next frame
    if let Some(ref msg) = app.loading_indicator {
        draw_loading_indicator(f, msg);
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

/// Small centered dialog for naming a debug dump file.
/// Shows "debug-output_<name>" preview with text input. Enter confirms, Esc cancels.
fn draw_debug_dump_naming(f: &mut Frame, name: &str) {
    let area = f.area();
    // Preview what the filename will be
    let preview = if name.is_empty() { "debug-output".to_string() }
        else { format!("debug-output_{}", name) };
    let display = format!(" .azureal/{} ", preview);
    let hint = "Name this dump (Enter:save  Esc:cancel)";
    // Dialog width: fits whichever content line is widest + 2 for borders + 2 padding
    let content_w = (display.len()).max(hint.len()) as u16 + 4;
    let w = content_w.max(40).min(area.width.saturating_sub(4));
    let h = 5u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let rect = Rect::new(x, y, w, h);
    // Two lines: instruction + filename preview with cursor
    let content = vec![
        Line::from(Span::styled(hint, Style::default().fg(Color::White))),
        Line::from(vec![
            Span::styled(".azureal/debug-output".to_string(), Style::default().fg(Color::DarkGray)),
            Span::styled(
                if name.is_empty() { String::new() } else { format!("_{}", name) },
                Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
            ),
            Span::styled("▏".to_string(), Style::default().fg(AZURE)),
        ]),
    ];
    let dialog = Paragraph::new(content)
        .alignment(Alignment::Center)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(AZURE))
            .title(Span::styled(" Debug Dump ", Style::default().fg(AZURE).add_modifier(Modifier::BOLD))));
    // Clear the background behind the dialog
    f.render_widget(ratatui::widgets::Clear, rect);
    f.render_widget(dialog, rect);
}

/// "Saving..." indicator shown while the debug dump I/O runs on the next frame.
/// Prevents the app from looking frozen during large dumps.
fn draw_debug_dump_saving(f: &mut Frame, name: &str) {
    let area = f.area();
    let filename = if name.is_empty() { "debug-output".to_string() }
        else { format!("debug-output_{}", name) };
    let msg = format!(" Saving {}… ", filename);
    let w = (msg.len() as u16 + 4).min(area.width.saturating_sub(4));
    let h = 3u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let rect = Rect::new(x, y, w, h);
    let dialog = Paragraph::new(Span::styled(msg, Style::default().fg(Color::White)))
        .alignment(Alignment::Center)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(AZURE))
            .title(Span::styled(" Debug Dump ", Style::default().fg(AZURE).add_modifier(Modifier::BOLD))));
    f.render_widget(ratatui::widgets::Clear, rect);
    f.render_widget(dialog, rect);
}

/// Generic loading indicator — centered popup shown while a deferred action
/// (session load, file open, health scan, project switch, etc.) runs on the
/// next frame. Reused by all two-phase deferred draw operations.
/// Auto-rebase dialog — centered popup showing rebase progress or success.
/// `success` = true shows green border with checkmark, false shows AZURE "in progress".
fn draw_auto_rebase_dialog(f: &mut Frame, branch: &str, success: bool) {
    let area = f.area();
    let msg = if success {
        format!(" {} rebased onto main \u{2713} ", branch)
    } else {
        format!(" Auto-rebasing {} onto main... ", branch)
    };
    let border_color = if success { Color::Green } else { AZURE };
    let w = (msg.len() as u16 + 4).min(area.width.saturating_sub(4));
    let h = 3u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let rect = Rect::new(x, y, w, h);
    let dialog = Paragraph::new(Span::styled(msg, Style::default().fg(Color::White)))
        .alignment(Alignment::Center)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)));
    f.render_widget(ratatui::widgets::Clear, rect);
    f.render_widget(dialog, rect);
}

fn draw_loading_indicator(f: &mut Frame, msg: &str) {
    let area = f.area();
    let padded = format!(" {} ", msg);
    let w = (padded.len() as u16 + 4).min(area.width.saturating_sub(4));
    let h = 3u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let rect = Rect::new(x, y, w, h);
    let dialog = Paragraph::new(Span::styled(padded, Style::default().fg(Color::White)))
        .alignment(Alignment::Center)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(AZURE)));
    f.render_widget(ratatui::widgets::Clear, rect);
    f.render_widget(dialog, rect);
}
