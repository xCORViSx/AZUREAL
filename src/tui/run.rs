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
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame, Terminal,
};
use std::io;

use crate::app::{App, Focus};
use crate::config::Config;

use super::event_loop;
use super::keybindings;
use super::util::{GIT_ORANGE, AZURE};
use super::{draw_azureal, draw_dialogs, draw_git_actions, draw_health, draw_input, draw_output, draw_projects, draw_sidebar, draw_status, draw_terminal, draw_viewer};

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

    // Step 1: Reserve status bar at bottom (shared by both layouts)
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(1)])
        .split(f.area());
    let content_area = outer[0];
    let status_area = outer[1];

    if app.git_actions_panel.is_some() {
        // ── Git mode layout ──────────────────────────────────────────────
        // Full-width status box at bottom, 3-column panes above.
        //
        // ┌──────────┬──────────────────────────┬──────────────┐
        // │ Actions  │                          │              │
        // │          │     Viewer (diff)        │   Commits    │
        // ├──────────┤                          │              │
        // │ Changed  │                          │              │
        // │ Files    │                          │              │
        // ├──────────┴──────────────────────────┴──────────────┤
        // │  Git Status Box (full width, hints in title)       │
        // ├────────────────────────────────────────────────────┤
        // │                  Status Bar                        │
        // └────────────────────────────────────────────────────┘

        let git_box_height = 3u16; // borders + 1 content line
        let git_v = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(git_box_height)])
            .split(content_area);
        let panes_area = git_v[0];
        let git_box_area = git_v[1];

        let git_h = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(40),
                Constraint::Min(10),
                Constraint::Percentage(35),
            ])
            .split(panes_area);

        app.input_area = git_box_area;
        app.pane_worktrees = git_h[0];
        app.pane_viewer = git_h[1];
        app.pane_convo = git_h[2];

        draw_sidebar::draw_sidebar(f, app, git_h[0]);
        draw_viewer::draw_viewer(f, app, git_h[1]);
        draw_output::draw_output(f, app, git_h[2]);
        draw_git_status_box(f, app, git_box_area);
    } else {
        // ── Normal mode layout ───────────────────────────────────────────
        // Convo gets full height, Input/Terminal spans Worktrees + Viewer.
        //
        // ┌──────────┬──────────────────────────┬──────────────┐
        // │Worktrees │         Viewer           │              │
        // │  (15%)   │         (50%)            │  Convo (35%) │
        // ├──────────┴──────────────────────────┤              │
        // │     Input / Terminal                │              │
        // ├─────────────────────────────────────┴──────────────┤
        // │                  Status Bar                        │
        // └────────────────────────────────────────────────────┘

        let h_split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(15),
                Constraint::Percentage(50),
                Constraint::Percentage(35),
            ])
            .split(content_area);
        let left_width = h_split[0].width + h_split[1].width;
        let convo_area = h_split[2];

        let input_height = if app.terminal_mode {
            app.terminal_height + 2
        } else {
            let input_inner_width = left_width.saturating_sub(2) as usize;
            let input_lines = if input_inner_width > 0 && !app.input.is_empty() {
                let mut rows = 1usize;
                let mut col = 0usize;
                for c in app.input.chars() {
                    if c == '\n' { rows += 1; col = 0; }
                    else {
                        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
                        if col + w > input_inner_width { rows += 1; col = w; }
                        else { col += w; }
                    }
                }
                rows
            } else { 1 };
            let max_input = (content_area.height * 3 / 4).max(3);
            (input_lines as u16 + 2).min(max_input)
        };

        let left_rect = Rect::new(content_area.x, content_area.y, left_width, content_area.height);
        let left_v = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(input_height)])
            .split(left_rect);
        let top_panes_area = left_v[0];
        let input_area = left_v[1];

        let top_h = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(40), Constraint::Min(10)])
            .split(top_panes_area);

        app.input_area = input_area;
        app.pane_worktrees = top_h[0];
        app.pane_viewer = top_h[1];
        app.pane_convo = convo_area;

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
    }

    // RCR approval dialog — rendered over convo pane
    if app.rcr_session.as_ref().is_some_and(|m| m.approval_pending) {
        draw_output::draw_rcr_approval(f, app.pane_convo);
    }
    // Post-merge dialog — rendered over convo pane
    if let Some(ref pmd) = app.post_merge_dialog {
        draw_output::draw_post_merge_dialog(f, app.pane_convo, pmd);
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
    // Git panel overlays — commit editor and conflict resolution render over viewer pane
    if let Some(ref panel) = app.git_actions_panel {
        if let Some(ref overlay) = panel.commit_overlay {
            draw_git_actions::draw_commit_editor(f, overlay, app.pane_viewer);
        }
        if let Some(ref ov) = panel.conflict_overlay {
            draw_git_actions::draw_conflict_inline(f, ov, app.pane_viewer);
        }
        if let Some(ref ov) = panel.auto_resolve_overlay {
            draw_git_actions::draw_auto_resolve_overlay(f, ov, app.pane_viewer);
        }
    }
    // AZUREAL++ developer hub panel (⌃a global)
    if app.azureal_panel.is_some() {
        draw_azureal::draw_azureal_panel(f, app);
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

/// Git status box — full-width bar reusing the input box area.
/// Title shows keybinding hints (formatted like the prompt box); content shows operation result messages.
fn draw_git_status_box(f: &mut Frame, app: &App, area: Rect) {
    let panel = match app.git_actions_panel {
        Some(ref p) => p,
        None => return,
    };

    let hints = keybindings::git_actions_footer();
    let label = format!(" GIT: {} ", panel.worktree_name);
    let max_w = area.width.saturating_sub(2) as usize;
    let (top_title, bottom_title) = draw_input::split_title_hints(&label, &hints, max_w);

    // Content: result message or empty
    let content = if let Some((ref msg, is_error)) = panel.result_message {
        let color = if is_error { Color::Red } else { Color::Green };
        vec![Line::from(Span::styled(format!(" {}", msg), Style::default().fg(color)))]
    } else {
        vec![]
    };

    let border_style = Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD);
    let mut block = Block::default()
        .title(Span::styled(top_title, border_style))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(border_style);

    if let Some(bottom) = bottom_title {
        block = block.title_bottom(Span::styled(bottom, border_style));
    }

    f.render_widget(Paragraph::new(content).block(block), area);
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
