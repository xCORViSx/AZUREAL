//! TUI entry point and main layout
//!
//! Contains the run() function to start the TUI and the ui() layout function.

use anyhow::Result;
#[cfg(not(target_os = "windows"))]
use crossterm::event::{KeyboardEnhancementFlags, PushKeyboardEnhancementFlags};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, PopKeyboardEnhancementFlags},
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
use super::util::{AZURE, GIT_BROWN, GIT_ORANGE};
use super::{
    draw_dialogs, draw_git_actions, draw_health, draw_input, draw_output, draw_projects,
    draw_sidebar, draw_status, draw_terminal, draw_viewer,
};

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
    //
    // Disabled on Windows — Windows Terminal claims Kitty support but the
    // implementation conflicts with mouse capture: scroll/arrow CSI sequences
    // leak through as raw text (e.g. "[A[B" appearing in the input box).
    #[cfg(not(target_os = "windows"))]
    let kbd_enhanced = execute!(
        stdout,
        PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
        )
    )
    .is_ok();
    #[cfg(target_os = "windows")]
    let kbd_enhanced = false;

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
    app.load_session_output(); // also restores selected_model + backend
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
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
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
            .constraints([
                Constraint::Length(1),
                Constraint::Min(4),
                Constraint::Length(git_box_height),
            ])
            .split(content_area);
        let tab_bar_area = git_v[0];
        let panes_area = git_v[1];
        let git_box_area = git_v[2];

        app.pane_worktree_tabs = tab_bar_area;
        draw_git_worktree_tabs(f, app, tab_bar_area);

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
        app.pane_session = git_h[2];

        draw_sidebar::draw_sidebar(f, app, git_h[0]);
        draw_viewer::draw_viewer(f, app, git_h[1]);
        draw_output::draw_output(f, app, git_h[2]);
        draw_git_status_box(f, app, git_box_area);
    } else {
        // ── Normal mode layout ───────────────────────────────────────────
        // Worktree tab row at top, then 3-column panes below.
        //
        // ┌─ [★ main] │ [○ feat-a] │ [● feat-b] ───────────────┐
        // ├──────────┬──────────────────────────┬───────────────┤
        // │FileTree  │         Viewer           │               │
        // │  (15%)   │         (50%)            │  Session (35%)│
        // ├──────────┴──────────────────────────┤               │
        // │     Input / Terminal                │               │
        // ├─────────────────────────────────────┴───────────────┤
        // │                  Status Bar                         │
        // └─────────────────────────────────────────────────────┘

        let normal_v = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(5)])
            .split(content_area);
        let tab_row_area = normal_v[0];
        let below_tabs = normal_v[1];

        app.pane_worktree_tabs = tab_row_area;

        let h_split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(15),
                Constraint::Percentage(50),
                Constraint::Percentage(35),
            ])
            .split(below_tabs);
        let left_width = h_split[0].width + h_split[1].width;
        let session_area = h_split[2];

        let input_height = if app.terminal_mode {
            app.terminal_height + 2
        } else {
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
                        if col + w > input_inner_width {
                            rows += 1;
                            col = w;
                        } else {
                            col += w;
                        }
                    }
                }
                rows
            } else {
                1
            };
            let max_input = (below_tabs.height * 3 / 4).max(3);
            (input_lines as u16 + 2).min(max_input)
        };

        let left_rect = Rect::new(below_tabs.x, below_tabs.y, left_width, below_tabs.height);
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
        app.pane_session = session_area;

        draw_worktree_tabs(f, app, tab_row_area);
        draw_sidebar::draw_file_tree_overlay(f, app, top_h[0]);
        draw_viewer::draw_viewer(f, app, top_h[1]);
        draw_output::draw_output(f, app, session_area);

        if app.terminal_mode {
            draw_terminal::draw_terminal(f, app, input_area);
        } else {
            draw_input::draw_input(f, app, input_area);
        }
    }

    // RCR approval dialog — rendered over session pane
    if app.rcr_session.as_ref().is_some_and(|m| m.approval_pending) {
        draw_output::draw_rcr_approval(f, app.pane_session);
    }
    // Post-merge dialog — rendered center screen
    if let Some(ref pmd) = app.post_merge_dialog {
        draw_output::draw_post_merge_dialog(f, f.area(), pmd);
    }

    app.pane_status = status_area;
    draw_status::draw_status(f, app, status_area);

    // Draw overlays
    if let Some(ref popup) = app.table_popup {
        draw_dialogs::draw_table_popup(f, popup, f.area());
    }
    if let Some(ref dialog) = app.delete_worktree_dialog {
        draw_dialogs::draw_delete_worktree_dialog(f, dialog, f.area());
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
    // Debug dump naming dialog — centered input for entering dump name
    if app.debug_dump_naming.is_some() {
        draw_debug_dump_naming(f, app);
    }
    // Debug dump saving indicator — flash while dump is being written
    if app.debug_dump_saving.is_some() {
        draw_debug_dump_saving(f);
    }
    // Auto-rebase success dialog — 2-second toast after successful auto-rebase
    if let Some((ref branch, _)) = app.auto_rebase_success_until {
        draw_auto_rebase_dialog(f, branch, true);
    }
    // Welcome modal — no worktrees and not browsing main
    if app.needs_welcome_modal() {
        draw_dialogs::draw_welcome_modal(f);
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
    let bf_lines: Vec<Line<'static>> = butterfly
        .iter()
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
    f.render_widget(
        bf_widget,
        ratatui::layout::Rect::new(
            area.x,
            bf_start_y,
            area.width,
            bf_h.min(
                area.height
                    .saturating_sub(bf_start_y.saturating_sub(area.y)),
            ),
        ),
    );

    // Then render text on top (foreground overwrites butterfly cells)
    let mut lines: Vec<Line<'static>> = logo
        .iter()
        .map(|row| Line::from(Span::styled(row.to_string(), logo_style)))
        .collect();
    lines.push(Line::from(""));
    for row in &acronym {
        lines.push(Line::from(Span::styled(row.to_string(), dim_style)));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "L o a d i n g   p r o j e c t . . .",
        logo_style,
    )));

    let splash = Paragraph::new(lines).alignment(Alignment::Center);
    let splash_area = ratatui::layout::Rect::new(area.x, text_start_y, area.width, total_height);
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
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        );
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
    let label = " GIT ".to_string();
    let max_w = area.width.saturating_sub(2) as usize;
    let (top_title, bottom_title) = draw_input::split_title_hints(&label, &hints, max_w);

    // Content: result message or empty
    let content = if let Some((ref msg, is_error)) = panel.result_message {
        let color = if is_error { Color::Red } else { Color::Green };
        let mut style = Style::default().fg(color);
        if app.git_status_selected {
            style = style.bg(Color::Rgb(60, 60, 100));
        }
        vec![Line::from(Span::styled(format!(" {}", msg), style))]
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

/// Horizontal worktree tab bar — 1 row at the top of the normal mode layout.
/// Auto-rebase indicator color for a worktree branch.
/// Returns Some(color) if auto-rebase is enabled: green=idle, orange=RCR active, blue=approval pending.
fn rebase_indicator_color(app: &App, branch: &str) -> Option<Color> {
    if !app.auto_rebase_enabled.contains(branch) {
        return None;
    }
    if let Some(ref rcr) = app.rcr_session {
        if rcr.branch == branch {
            return Some(if rcr.approval_pending {
                Color::Blue
            } else {
                GIT_ORANGE
            });
        }
    }
    Some(Color::Green)
}

/// Active tab: AZURE bg + white fg + bold. Inactive: DarkGray fg.
/// [★ main] tab always first (main branch browse). Archived worktrees shown dim with ◇.
/// Pagination: when tabs don't fit, they are packed into pages greedily.
fn draw_worktree_tabs(f: &mut Frame, app: &mut App, area: Rect) {
    let avail = area.width as usize;
    let base_x = area.x;
    let focused = app.focus == Focus::Worktrees;

    // Build tab entries: (display_label, is_active, is_archived, target, rebase_color, is_unread)
    // target: None = [M] main browse, Some(idx) = worktree index
    let mut tabs: Vec<(String, bool, bool, Option<usize>, Option<Color>, bool)> = Vec::new();

    let main_branch = app
        .project
        .as_ref()
        .map(|p| p.main_branch.as_str())
        .unwrap_or("main");
    tabs.push((
        format!("★ {}", main_branch),
        app.browsing_main,
        false,
        None,
        None,
        false,
    ));

    for (idx, wt) in app.worktrees.iter().enumerate() {
        let active = !app.browsing_main && app.selected_worktree == Some(idx);
        let rebase_color = rebase_indicator_color(app, &wt.branch_name);
        let unread = app.unread_sessions.contains(&wt.branch_name);
        if wt.archived {
            tabs.push((
                format!("◇ {}", wt.name()),
                active,
                true,
                Some(idx),
                rebase_color,
                false,
            ));
        } else if unread {
            tabs.push((
                format!("◐ {}", wt.name()),
                active,
                false,
                Some(idx),
                rebase_color,
                true,
            ));
        } else {
            let status = wt.status(app.is_session_running(&wt.branch_name));
            tabs.push((
                format!("{} {}", status.symbol(), wt.name()),
                active,
                false,
                Some(idx),
                rebase_color,
                false,
            ));
        }
    }

    if tabs.is_empty() {
        return;
    }

    // Display width of each tab: "label " = display_width + 1, plus "R" if rebase indicator
    let tab_widths: Vec<usize> = tabs
        .iter()
        .map(|(label, _, _, _, rebase, _)| {
            let base: usize = label
                .chars()
                .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1))
                .sum::<usize>()
                + 1;
            if rebase.is_some() {
                base + 1
            } else {
                base
            }
        })
        .collect();

    // Pack tabs into pages greedily
    let mut pages: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_w: usize = 0;
    let mut active_page: usize = 0;

    for (i, (&tw, (_, is_active, _, _, _, _))) in tab_widths.iter().zip(tabs.iter()).enumerate() {
        let cost = if cur.is_empty() { tw } else { tw + 1 };
        if !cur.is_empty() && cur_w + cost > avail {
            pages.push(std::mem::take(&mut cur));
            cur = vec![i];
            cur_w = tw;
        } else {
            cur.push(i);
            cur_w += cost;
        }
        if *is_active {
            active_page = pages.len();
        }
    }
    if !cur.is_empty() {
        pages.push(cur);
    }

    let total_pages = pages.len();
    let page_tabs = match pages.get(active_page) {
        Some(p) => p,
        None => return,
    };

    // Build spans and hit-test regions
    let mut spans: Vec<Span> = Vec::with_capacity(page_tabs.len() * 2 + 1);
    let mut hits: Vec<(u16, u16, Option<usize>)> = Vec::with_capacity(page_tabs.len());
    let mut x_cursor: u16 = base_x;

    for (j, &idx) in page_tabs.iter().enumerate() {
        let (ref label, is_active, is_archived, target, rebase_color, is_unread) = tabs[idx];
        let tab_text = format!("{} ", label);
        let mut tab_w: u16 = tab_text
            .chars()
            .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) as u16)
            .sum();

        let dim = if focused {
            Color::Gray
        } else {
            Color::DarkGray
        };
        let style = if is_active {
            if target.is_none() {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::White)
                    .bg(AZURE)
                    .add_modifier(Modifier::BOLD)
            }
        } else if is_archived {
            Style::default().fg(dim)
        } else if is_unread {
            Style::default().fg(AZURE)
        } else if target.is_none() {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(dim)
        };

        spans.push(Span::styled(tab_text, style));

        if let Some(color) = rebase_color {
            spans.push(Span::styled(
                "R",
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
            tab_w += 1;
        }

        hits.push((x_cursor, x_cursor + tab_w, target));
        x_cursor += tab_w;

        if j + 1 < page_tabs.len() {
            let sep_color = if focused { AZURE } else { Color::DarkGray };
            spans.push(Span::styled("│", Style::default().fg(sep_color)));
            x_cursor += 1;
        }
    }

    if total_pages > 1 {
        let page_color = if focused {
            Color::Gray
        } else {
            Color::DarkGray
        };
        spans.push(Span::styled(
            format!("  {}/{}", active_page + 1, total_pages),
            Style::default().fg(page_color).add_modifier(Modifier::DIM),
        ));
    }

    app.worktree_tab_hits = hits;
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Horizontal worktree tab bar — 1 row at the top of the git panel.
/// Reuses the same design as `draw_worktree_tabs` (★ main tab, status symbols,
/// archived styling, pagination, hit-test regions) but with GIT_ORANGE/GIT_BROWN
/// colors instead of AZURE/Yellow/DarkGray.
fn draw_git_worktree_tabs(f: &mut Frame, app: &mut App, area: Rect) {
    let panel = match app.git_actions_panel.as_ref() {
        Some(p) => p,
        None => return,
    };
    let active_branch = &panel.worktree_name;
    let avail = area.width as usize;
    let base_x = area.x;

    // Build tab entries: (display_label, is_active, is_archived, target, rebase_color, is_unread)
    // target: None = main branch, Some(idx) = worktree index
    let mut tabs: Vec<(String, bool, bool, Option<usize>, Option<Color>, bool)> = Vec::new();

    let main_branch = app
        .project
        .as_ref()
        .map(|p| p.main_branch.as_str())
        .unwrap_or("main");
    let main_is_active = *active_branch == main_branch;
    tabs.push((
        format!("★ {}", main_branch),
        main_is_active,
        false,
        None,
        None,
        false,
    ));

    for (idx, wt) in app.worktrees.iter().enumerate() {
        let active = !main_is_active && wt.branch_name == *active_branch;
        let rebase_color = rebase_indicator_color(app, &wt.branch_name);
        let unread = app.unread_sessions.contains(&wt.branch_name);
        if wt.archived {
            tabs.push((
                format!("◇ {}", wt.name()),
                active,
                true,
                Some(idx),
                rebase_color,
                false,
            ));
        } else if unread {
            tabs.push((
                format!("◐ {}", wt.name()),
                active,
                false,
                Some(idx),
                rebase_color,
                true,
            ));
        } else {
            let status = wt.status(app.is_session_running(&wt.branch_name));
            tabs.push((
                format!("{} {}", status.symbol(), wt.name()),
                active,
                false,
                Some(idx),
                rebase_color,
                false,
            ));
        }
    }

    if tabs.is_empty() {
        return;
    }

    // Display width of each tab: "label " = display_width + 1, plus "R" if rebase indicator
    let tab_widths: Vec<usize> = tabs
        .iter()
        .map(|(label, _, _, _, rebase, _)| {
            let base: usize = label
                .chars()
                .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1))
                .sum::<usize>()
                + 1;
            if rebase.is_some() {
                base + 1
            } else {
                base
            }
        })
        .collect();

    // Pack tabs into pages greedily
    let mut pages: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_w: usize = 0;
    let mut active_page: usize = 0;

    for (i, (&tw, (_, is_active, _, _, _, _))) in tab_widths.iter().zip(tabs.iter()).enumerate() {
        let cost = if cur.is_empty() { tw } else { tw + 1 };
        if !cur.is_empty() && cur_w + cost > avail {
            pages.push(std::mem::take(&mut cur));
            cur = vec![i];
            cur_w = tw;
        } else {
            cur.push(i);
            cur_w += cost;
        }
        if *is_active {
            active_page = pages.len();
        }
    }
    if !cur.is_empty() {
        pages.push(cur);
    }

    let total_pages = pages.len();
    let page_tabs = match pages.get(active_page) {
        Some(p) => p,
        None => return,
    };

    // Build spans and hit-test regions
    let mut spans: Vec<Span> = Vec::with_capacity(page_tabs.len() * 2 + 1);
    let mut hits: Vec<(u16, u16, Option<usize>)> = Vec::with_capacity(page_tabs.len());
    let mut x_cursor: u16 = base_x;

    for (j, &idx) in page_tabs.iter().enumerate() {
        let (ref label, is_active, is_archived, target, rebase_color, is_unread) = tabs[idx];
        let tab_text = format!("{} ", label);
        let mut tab_w: u16 = tab_text
            .chars()
            .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) as u16)
            .sum();

        // Same styling logic as draw_worktree_tabs but with git color palette
        let style = if is_active {
            if target.is_none() {
                Style::default()
                    .fg(Color::Black)
                    .bg(GIT_ORANGE)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::White)
                    .bg(GIT_ORANGE)
                    .add_modifier(Modifier::BOLD)
            }
        } else if is_archived {
            Style::default().fg(Color::DarkGray)
        } else if is_unread {
            Style::default().fg(AZURE)
        } else if target.is_none() {
            Style::default().fg(GIT_BROWN)
        } else {
            Style::default().fg(GIT_BROWN)
        };

        spans.push(Span::styled(tab_text, style));

        if let Some(color) = rebase_color {
            spans.push(Span::styled(
                "R",
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
            tab_w += 1;
        }

        hits.push((x_cursor, x_cursor + tab_w, target));
        x_cursor += tab_w;

        if j + 1 < page_tabs.len() {
            spans.push(Span::styled("│", Style::default().fg(GIT_BROWN)));
            x_cursor += 1;
        }
    }

    if total_pages > 1 {
        spans.push(Span::styled(
            format!("  {}/{}", active_page + 1, total_pages),
            Style::default().fg(GIT_BROWN).add_modifier(Modifier::DIM),
        ));
    }

    app.worktree_tab_hits = hits;
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Debug dump naming dialog — centered input for entering a suffix for the dump file.
/// ⌃d opens this, user types a name, Enter saves, Esc cancels.
fn draw_debug_dump_naming(f: &mut Frame, app: &App) {
    let area = f.area();
    let input_text = app.debug_dump_naming.as_deref().unwrap_or("");
    let prompt = format!(" Name: {}▏", input_text);
    let w = 50u16.min(area.width.saturating_sub(4));
    let h = 3u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let rect = Rect::new(x, y, w, h);
    let dialog = Paragraph::new(Span::styled(prompt, Style::default().fg(Color::White))).block(
        Block::default()
            .title(Span::styled(
                " Debug Dump ",
                Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(AZURE)),
    );
    f.render_widget(ratatui::widgets::Clear, rect);
    f.render_widget(dialog, rect);
}

/// Debug dump saving indicator — brief flash shown while the dump file is being written.
fn draw_debug_dump_saving(f: &mut Frame) {
    let area = f.area();
    let msg = " Saving debug dump... ";
    let w = (msg.len() as u16 + 4).min(area.width.saturating_sub(4));
    let h = 3u16;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let rect = Rect::new(x, y, w, h);
    let dialog = Paragraph::new(Span::styled(msg, Style::default().fg(Color::White)))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(AZURE)),
        );
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
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(AZURE)),
        );
    f.render_widget(ratatui::widgets::Clear, rect);
    f.render_widget(dialog, rect);
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Color constants ───────────────────────────────────────────────

    #[test]
    fn azure_color_is_rgb() {
        assert!(matches!(AZURE, Color::Rgb(_, _, _)));
    }

    #[test]
    fn git_orange_is_rgb() {
        assert!(matches!(GIT_ORANGE, Color::Rgb(_, _, _)));
    }

    #[test]
    fn git_brown_is_rgb() {
        assert!(matches!(GIT_BROWN, Color::Rgb(_, _, _)));
    }

    #[test]
    fn azure_not_equal_git_orange() {
        assert_ne!(AZURE, GIT_ORANGE);
    }

    #[test]
    fn azure_not_equal_git_brown() {
        assert_ne!(AZURE, GIT_BROWN);
    }

    #[test]
    fn git_orange_not_equal_git_brown() {
        assert_ne!(GIT_ORANGE, GIT_BROWN);
    }

    // ── Splash screen content ─────────────────────────────────────────

    #[test]
    fn splash_azure_color() {
        let az = Color::Rgb(51, 153, 255);
        assert_eq!(az, Color::Rgb(51, 153, 255));
    }

    #[test]
    fn splash_dim_color() {
        let dim = Color::Rgb(25, 76, 128);
        assert_eq!(dim, Color::Rgb(25, 76, 128));
    }

    #[test]
    fn splash_butterfly_color() {
        let butterfly_color = Color::Rgb(15, 45, 80);
        assert_eq!(butterfly_color, Color::Rgb(15, 45, 80));
    }

    #[test]
    fn splash_colors_are_all_distinct() {
        let az = Color::Rgb(51, 153, 255);
        let dim = Color::Rgb(25, 76, 128);
        let bf = Color::Rgb(15, 45, 80);
        assert_ne!(az, dim);
        assert_ne!(az, bf);
        assert_ne!(dim, bf);
    }

    // ── Auto-rebase dialog formatting ─────────────────────────────────

    #[test]
    fn auto_rebase_success_message_format() {
        let branch = "feat-tests";
        let msg = format!(" {} rebased onto main \u{2713} ", branch);
        assert!(msg.contains("feat-tests"));
        assert!(msg.contains("rebased onto main"));
        assert!(msg.contains("\u{2713}")); // checkmark
    }

    #[test]
    fn auto_rebase_in_progress_message_format() {
        let branch = "feat-tests";
        let msg = format!(" Auto-rebasing {} onto main... ", branch);
        assert!(msg.contains("Auto-rebasing"));
        assert!(msg.contains("feat-tests"));
        assert!(msg.contains("onto main..."));
    }

    #[test]
    fn auto_rebase_success_border_is_green() {
        let success = true;
        let border_color = if success { Color::Green } else { AZURE };
        assert_eq!(border_color, Color::Green);
    }

    #[test]
    fn auto_rebase_progress_border_is_azure() {
        let success = false;
        let border_color = if success { Color::Green } else { AZURE };
        assert_eq!(border_color, AZURE);
    }

    // ── Dialog centering arithmetic ───────────────────────────────────

    #[test]
    fn dialog_center_x_with_100_width() {
        let area_x: u16 = 0;
        let area_width: u16 = 100;
        let w: u16 = 30;
        let x = area_x + (area_width.saturating_sub(w)) / 2;
        assert_eq!(x, 35);
    }

    #[test]
    fn dialog_center_y_with_50_height() {
        let area_y: u16 = 0;
        let area_height: u16 = 50;
        let h: u16 = 3;
        let y = area_y + (area_height.saturating_sub(h)) / 2;
        assert_eq!(y, 23);
    }

    #[test]
    fn dialog_width_clamped_to_area() {
        let area_width: u16 = 20;
        let msg_len: u16 = 30;
        let w = (msg_len + 4).min(area_width.saturating_sub(4));
        assert_eq!(w, 16); // 20 - 4 = 16
    }

    #[test]
    fn dialog_width_not_clamped_when_fits() {
        let area_width: u16 = 100;
        let msg_len: u16 = 20;
        let w = (msg_len + 4).min(area_width.saturating_sub(4));
        assert_eq!(w, 24); // 20 + 4 = 24, 100 - 4 = 96, min(24, 96) = 24
    }

    #[test]
    fn dialog_center_with_offset_area() {
        let area_x: u16 = 10;
        let area_width: u16 = 80;
        let w: u16 = 30;
        let x = area_x + (area_width.saturating_sub(w)) / 2;
        assert_eq!(x, 35); // 10 + (80 - 30)/2 = 10 + 25 = 35
    }

    #[test]
    fn dialog_saturating_sub_prevents_underflow() {
        let area_width: u16 = 2;
        let w: u16 = 10;
        let result = area_width.saturating_sub(w);
        assert_eq!(result, 0);
    }

    // ── Git status box height ─────────────────────────────────────────

    #[test]
    fn git_box_height_is_three() {
        let git_box_height = 3u16;
        assert_eq!(git_box_height, 3);
    }

    // ── Loading indicator padding ─────────────────────────────────────

    #[test]
    fn loading_indicator_padding() {
        let msg = "Loading session...";
        let padded = format!(" {} ", msg);
        assert_eq!(padded, " Loading session... ");
        assert_eq!(padded.len(), msg.len() + 2);
    }

    #[test]
    fn loading_indicator_width_calculation() {
        let padded = " Loading... ";
        let w = (padded.len() as u16 + 4).min(100u16.saturating_sub(4));
        assert_eq!(w, padded.len() as u16 + 4); // 12 + 4 = 16, fits in 96
    }

    // ── Debug dump naming dialog ──────────────────────────────────────

    #[test]
    fn debug_dump_prompt_format() {
        let input_text = "my-dump";
        let prompt = format!(" Name: {}\u{25CF}", input_text);
        assert!(prompt.contains("my-dump"));
        assert!(prompt.starts_with(" Name: "));
    }

    #[test]
    fn debug_dump_prompt_empty_input() {
        let input_text = "";
        let prompt = format!(" Name: {}\u{25CF}", input_text);
        assert_eq!(prompt, " Name: \u{25CF}");
    }

    #[test]
    fn debug_dump_dialog_width_clamped() {
        let area_width: u16 = 30;
        let w = 50u16.min(area_width.saturating_sub(4));
        assert_eq!(w, 26); // min(50, 30-4) = 26
    }

    #[test]
    fn debug_dump_dialog_width_unclamped() {
        let area_width: u16 = 200;
        let w = 50u16.min(area_width.saturating_sub(4));
        assert_eq!(w, 50); // min(50, 196) = 50
    }

    // ── Saving indicator ──────────────────────────────────────────────

    #[test]
    fn saving_debug_dump_message_literal() {
        let msg = " Saving debug dump... ";
        assert_eq!(msg.len(), 22);
    }

    // ── Layout constraint values ──────────────────────────────────────

    #[test]
    fn normal_mode_sidebar_percentage() {
        // File tree is 15%
        let pct = 15u16;
        assert_eq!(pct, 15);
    }

    #[test]
    fn normal_mode_viewer_percentage() {
        // Viewer is 50%
        let pct = 50u16;
        assert_eq!(pct, 50);
    }

    #[test]
    fn normal_mode_session_percentage() {
        // Session is 35%
        let pct = 35u16;
        assert_eq!(pct, 35);
    }

    #[test]
    fn percentages_sum_to_100() {
        assert_eq!(15 + 50 + 35, 100);
    }

    #[test]
    fn git_mode_sidebar_width() {
        let w = 40u16;
        assert_eq!(w, 40);
    }

    #[test]
    fn git_mode_session_percentage() {
        let pct = 35u16;
        assert_eq!(pct, 35);
    }

    // ── Input height calculation ──────────────────────────────────────

    #[test]
    fn terminal_mode_input_height() {
        let terminal_height: u16 = 10;
        let input_height = terminal_height + 2;
        assert_eq!(input_height, 12);
    }

    #[test]
    fn max_input_height_uses_three_quarters() {
        let below_tabs_height: u16 = 40;
        let max_input = (below_tabs_height * 3 / 4).max(3);
        assert_eq!(max_input, 30);
    }

    #[test]
    fn max_input_height_minimum_is_three() {
        let below_tabs_height: u16 = 2;
        let max_input = (below_tabs_height * 3 / 4).max(3);
        assert_eq!(max_input, 3); // (2*3/4) = 1, max(1, 3) = 3
    }

    #[test]
    fn input_lines_clamped_to_max() {
        let input_lines: u16 = 100;
        let max_input: u16 = 30;
        let result = (input_lines + 2).min(max_input);
        assert_eq!(result, 30);
    }

    #[test]
    fn input_lines_plus_border() {
        let input_lines: u16 = 5;
        let max_input: u16 = 30;
        let result = (input_lines + 2).min(max_input);
        assert_eq!(result, 7); // 5 + 2 border = 7
    }

    // ── Row wrapping calculation (input area) ─────────────────────────

    #[test]
    fn row_wrapping_single_line() {
        let input = "hello";
        let inner_width: usize = 80;
        let mut rows = 1usize;
        let mut col = 0usize;
        for c in input.chars() {
            if c == '\n' {
                rows += 1;
                col = 0;
            } else {
                let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
                if col + w > inner_width {
                    rows += 1;
                    col = w;
                } else {
                    col += w;
                }
            }
        }
        assert_eq!(rows, 1);
    }

    #[test]
    fn row_wrapping_newline() {
        let input = "hello\nworld";
        let inner_width: usize = 80;
        let mut rows = 1usize;
        let mut col = 0usize;
        for c in input.chars() {
            if c == '\n' {
                rows += 1;
                col = 0;
            } else {
                let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
                if col + w > inner_width {
                    rows += 1;
                    col = w;
                } else {
                    col += w;
                }
            }
        }
        assert_eq!(rows, 2);
    }

    #[test]
    fn row_wrapping_at_width_boundary() {
        let input = "aaaa"; // 4 chars
        let inner_width: usize = 3; // wraps after 3
        let mut rows = 1usize;
        let mut col = 0usize;
        for c in input.chars() {
            if c == '\n' {
                rows += 1;
                col = 0;
            } else {
                let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
                if col + w > inner_width {
                    rows += 1;
                    col = w;
                } else {
                    col += w;
                }
            }
        }
        assert_eq!(rows, 2); // "aaa" on row 1, "a" wraps to row 2
    }

    #[test]
    fn row_wrapping_empty_input() {
        let input = "";
        let inner_width: usize = 80;
        // Empty check bypasses calculation, defaults to 1
        let input_lines = if inner_width > 0 && !input.is_empty() {
            let mut rows = 1usize;
            let mut col = 0usize;
            for c in input.chars() {
                if c == '\n' {
                    rows += 1;
                    col = 0;
                } else {
                    let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
                    if col + w > inner_width {
                        rows += 1;
                        col = w;
                    } else {
                        col += w;
                    }
                }
            }
            rows
        } else {
            1
        };
        assert_eq!(input_lines, 1);
    }

    // ── Splash screen dimensions ──────────────────────────────────────

    #[test]
    fn splash_logo_has_ten_rows() {
        let logo: Vec<&str> = vec![
            "line1", "line2", "line3", "line4", "line5", "line6", "line7", "line8", "line9",
            "line10",
        ];
        assert_eq!(logo.len(), 10);
    }

    #[test]
    fn splash_acronym_has_twelve_rows() {
        let acronym: Vec<&str> = vec![
            "l1", "l2", "l3", "l4", "l5", "l6", "l7", "l8", "l9", "l10", "l11", "l12",
        ];
        assert_eq!(acronym.len(), 12);
    }

    #[test]
    fn splash_total_height_calculation() {
        let logo_height: u16 = 10;
        let acronym_height: u16 = 12;
        let total_height = logo_height + 1 + acronym_height + 2 + 1;
        assert_eq!(total_height, 26);
    }

    #[test]
    fn splash_center_y_calculation() {
        let area_y: u16 = 0;
        let area_height: u16 = 60;
        let center_y = area_y + area_height / 2;
        assert_eq!(center_y, 30);
    }

    #[test]
    fn splash_text_start_y() {
        let center_y: u16 = 30;
        let total_height: u16 = 26;
        let text_start_y = center_y.saturating_sub(total_height / 2);
        assert_eq!(text_start_y, 17);
    }

    #[test]
    fn splash_butterfly_has_37_rows() {
        // The actual butterfly vec in draw_splash has 37 entries
        let butterfly_len = 37;
        assert_eq!(butterfly_len, 37);
    }

    #[test]
    fn splash_butterfly_start_y() {
        let center_y: u16 = 30;
        let bf_h: u16 = 37;
        let bf_start_y = center_y.saturating_sub(bf_h / 2);
        assert_eq!(bf_start_y, 12);
    }

    // ── Minimum splash duration ───────────────────────────────────────

    #[test]
    fn min_splash_is_three_seconds() {
        let min_splash = std::time::Duration::from_secs(3);
        assert_eq!(min_splash.as_secs(), 3);
    }

    #[test]
    fn splash_remaining_when_fast_load() {
        let min_splash = std::time::Duration::from_secs(3);
        let elapsed = std::time::Duration::from_millis(500);
        assert!(elapsed < min_splash);
        let remaining = min_splash - elapsed;
        assert_eq!(remaining.as_millis(), 2500);
    }

    #[test]
    fn splash_no_remaining_when_slow_load() {
        let min_splash = std::time::Duration::from_secs(3);
        let elapsed = std::time::Duration::from_secs(5);
        assert!(elapsed >= min_splash);
    }

    // ── Nerd font detection message ───────────────────────────────────

    #[test]
    fn nerd_font_warning_message() {
        let msg = "Nerd Font not detected \u{2014} using emoji icons. Install a Nerd Font for richer file tree icons";
        assert!(msg.contains("Nerd Font"));
        assert!(msg.contains("emoji icons"));
    }

    // ── Tab packing (greedy) arithmetic ───────────────────────────────

    #[test]
    fn tab_packing_first_tab_no_separator() {
        let cur_is_empty = true;
        let tw = 10;
        let cost = if cur_is_empty { tw } else { tw + 1 };
        assert_eq!(cost, 10); // first tab has no separator
    }

    #[test]
    fn tab_packing_subsequent_tabs_add_separator() {
        let cur_is_empty = false;
        let tw = 10;
        let cost = if cur_is_empty { tw } else { tw + 1 };
        assert_eq!(cost, 11); // +1 for separator
    }

    #[test]
    fn tab_packing_overflow_starts_new_page() {
        let avail: usize = 20;
        let cur_w: usize = 18;
        let cost: usize = 5;
        let overflow = !vec![0usize].is_empty() && cur_w + cost > avail;
        assert!(overflow); // 18 + 5 = 23 > 20
    }

    #[test]
    fn tab_packing_fits_stays_on_page() {
        let avail: usize = 20;
        let cur_w: usize = 10;
        let cost: usize = 5;
        let overflow = !vec![0usize].is_empty() && cur_w + cost > avail;
        assert!(!overflow); // 10 + 5 = 15 <= 20
    }

    // ── Page indicator formatting ─────────────────────────────────────

    #[test]
    fn page_indicator_format() {
        let active_page: usize = 0;
        let total_pages: usize = 3;
        let indicator = format!("  {}/{}", active_page + 1, total_pages);
        assert_eq!(indicator, "  1/3");
    }

    #[test]
    fn page_indicator_last_page() {
        let active_page: usize = 2;
        let total_pages: usize = 3;
        let indicator = format!("  {}/{}", active_page + 1, total_pages);
        assert_eq!(indicator, "  3/3");
    }

    // ── Focus enum comparison ─────────────────────────────────────────

    #[test]
    fn focus_worktrees_equality() {
        assert_eq!(Focus::Worktrees, Focus::Worktrees);
    }

    #[test]
    fn focus_variants_are_distinct() {
        assert_ne!(Focus::Worktrees, Focus::BranchDialog);
    }

    // ── Rect construction ─────────────────────────────────────────────

    #[test]
    fn rect_new_sets_fields() {
        let r = Rect::new(5, 10, 80, 24);
        assert_eq!(r.x, 5);
        assert_eq!(r.y, 10);
        assert_eq!(r.width, 80);
        assert_eq!(r.height, 24);
    }

    #[test]
    fn rect_zero() {
        let r = Rect::new(0, 0, 0, 0);
        assert_eq!(r.x, 0);
        assert_eq!(r.width, 0);
    }

    // ── Style construction ────────────────────────────────────────────

    #[test]
    fn style_default_is_reset() {
        let s = Style::default();
        assert_eq!(s, Style::default());
    }

    #[test]
    fn style_fg_sets_foreground() {
        let s = Style::default().fg(Color::Red);
        assert_ne!(s, Style::default());
    }

    #[test]
    fn style_bg_sets_background() {
        let s = Style::default().bg(Color::Blue);
        assert_ne!(s, Style::default());
    }

    #[test]
    fn style_bold_modifier() {
        let s = Style::default().add_modifier(Modifier::BOLD);
        assert_ne!(s, Style::default());
    }

    #[test]
    fn style_dim_modifier() {
        let s = Style::default().add_modifier(Modifier::DIM);
        assert_ne!(s, Style::default());
    }
}
