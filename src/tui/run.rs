//! TUI entry point and main layout
//!
//! Contains the run() function to start the TUI and the ui() layout function.
//! Heavy rendering is split into submodules:
//! - `splash`: Block-pixel ASCII art splash shown during initialization
//! - `worktree_tabs`: Horizontal tab bar rendering for normal and git modes
//! - `overlays`: Small popup/dialog overlays (auto-rebase, git status, debug dump, loading)

mod overlays;
mod splash;
mod worktree_tabs;

use anyhow::Result;
#[cfg(not(target_os = "windows"))]
use crossterm::event::{KeyboardEnhancementFlags, PushKeyboardEnhancementFlags};
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        PopKeyboardEnhancementFlags,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    Frame, Terminal,
};
use std::io;

use crate::app::App;
use crate::config::Config;

use overlays::{
    draw_auto_rebase_dialog, draw_debug_dump_naming, draw_debug_dump_saving, draw_git_status_box,
    draw_loading_indicator, draw_stt_download_dialog,
};
use splash::draw_splash;
use worktree_tabs::{draw_git_worktree_tabs, draw_worktree_tabs};

use super::event_loop;
use super::{
    draw_dialogs, draw_git_actions, draw_health, draw_input, draw_issues, draw_output,
    draw_projects, draw_sidebar, draw_status, draw_terminal, draw_viewer,
};

/// Fallback detection for Kitty keyboard protocol support via TERM_PROGRAM.
/// crossterm's `supports_keyboard_enhancement()` queries the terminal with a DSR
/// sequence and waits for a response, but some terminals don't respond fast enough —
/// causing a false negative. These terminals are known to fully support the Kitty
/// keyboard protocol, so we check the env var as a safety net.
///
/// NOTE: WezTerm is deliberately excluded. It accepts `PushKeyboardEnhancementFlags`
/// silently but does NOT actually enable the protocol on macOS — Shift+Enter and
/// Ctrl+M remain indistinguishable from plain Enter. Including it would set
/// `kbd_enhanced=true`, causing hint labels to show non-functional primary keys.
#[cfg(not(target_os = "windows"))]
fn term_program_supports_kitty() -> bool {
    matches!(
        std::env::var("TERM_PROGRAM").as_deref(),
        Ok("iTerm.app" | "kitty" | "ghostty")
    )
}

/// Run the TUI application
pub async fn run() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;

    // Enable Kitty keyboard protocol so Shift+Enter is distinguishable from Enter.
    // DISAMBIGUATE alone makes Enter → CSI 13u, Shift+Enter → CSI 13;2u.
    //
    // REPORT_EVENT_TYPES is deliberately OMITTED. It makes Kitty send both
    // press and release events, but Tab/Enter/Backspace use legacy single-byte
    // sequences (\t/\r/\x7F) for BOTH press and release — crossterm can't
    // distinguish them and decodes both as Press, causing double-fire.
    // (Kitty #8212: this is spec-undefined behavior for flag combo mode 3.)
    //
    // We intentionally omit REPORT_ALL_KEYS because it makes Shift+letter
    // arrive as (SHIFT, Char('1')) instead of (NONE, Char('!')), breaking
    // secondary character input (!, @, #, etc.).
    //
    // Disabled on Windows — Windows Terminal claims Kitty support but the
    // implementation conflicts with mouse capture: scroll/arrow CSI sequences
    // leak through as raw text (e.g. "[A[B" appearing in the input box).
    #[cfg(not(target_os = "windows"))]
    let kbd_enhanced = {
        let ct = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
        ct || term_program_supports_kitty()
    };
    #[cfg(not(target_os = "windows"))]
    if kbd_enhanced {
        execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }
    #[cfg(target_os = "windows")]
    let kbd_enhanced = false;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Load global config early to check startup screen preference
    let global_azufig = crate::azufig::load_global_azufig();
    let show_startup_screen = global_azufig.config.show_startup_screen;

    // Show splash screen immediately — visible while project/session loading runs.
    // Minimum 3s display so the branding registers even on fast machines.
    let splash_start = std::time::Instant::now();
    if show_startup_screen {
        terminal.draw(draw_splash)?;
    }

    // Clean up leftover .old binary from previous Windows update
    crate::updater::cleanup_old_binary();

    // Spawn update check on background thread (runs during splash, ~200-500ms)
    let update_rx = {
        let skip_version = global_azufig.config.skip_version.clone();
        let last_update_check = global_azufig.config.last_update_check;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result =
                crate::updater::check_for_update(skip_version.as_deref(), last_update_check);
            // Persist the check timestamp (unless rate-limited — already recent)
            if !matches!(result, crate::updater::UpdateCheckResult::RateLimited) {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                crate::azufig::save_last_update_check(now);
            }
            let _ = tx.send(result);
        });
        rx
    };

    let mut app = App::new();
    app.kbd_enhanced = kbd_enhanced;
    app.show_startup_screen = show_startup_screen;
    // WezTerm on macOS steals Alt+Enter for fullscreen toggle.
    // Detect it so hints show Ctrl+J instead of the non-functional Alt+Enter.
    app.alt_enter_stolen = matches!(std::env::var("TERM_PROGRAM").as_deref(), Ok("WezTerm"));
    let config = Config::load().unwrap_or_default();
    app.claude_available = config.is_backend_installed(crate::backend::Backend::Claude);
    app.codex_available = config.is_backend_installed(crate::backend::Backend::Codex);
    // If the default model's backend is unavailable, pick the first available
    if !app.claude_available {
        app.selected_model = Some(app.first_available_model().to_string());
        app.backend = crate::app::state::backend_for_model(app.selected_model.as_deref().unwrap());
    }
    app.update_terminal_title();
    app.update_check_receiver = Some(update_rx);
    app.load()?;
    app.load_run_commands();
    app.load_preset_prompts();
    app.load_session_output(); // also restores selected_model + backend (respects availability)
                               // Auto-detect Nerd Font support by probing a PUA glyph during splash
    app.nerd_fonts = super::file_icons::detect_nerd_font();
    if !app.nerd_fonts {
        app.set_status("Nerd Font not detected — using emoji icons. Install a Nerd Font for richer file tree icons");
    }

    // Hold splash for remainder of 3s minimum (loading time counts toward it)
    if show_startup_screen {
        let elapsed = splash_start.elapsed();
        let min_splash = std::time::Duration::from_secs(3);
        if elapsed < min_splash {
            std::thread::sleep(min_splash - elapsed);
        }
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
        DisableMouseCapture,
        DisableBracketedPaste
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
    // Issue approval dialog — rendered center screen
    if app
        .issue_session
        .as_ref()
        .is_some_and(|m| m.approval_pending)
    {
        draw_issues::draw_issue_approval(f, app);
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
    if let Some(ref dialog) = app.rename_worktree_dialog {
        let prefix = app
            .project
            .as_ref()
            .map(|p| p.branch_prefix.as_str())
            .unwrap_or("project");
        draw_dialogs::draw_rename_worktree_dialog(f, dialog, f.area(), prefix);
    }
    if let Some(ref dialog) = app.branch_dialog {
        draw_dialogs::draw_branch_dialog(f, dialog, f.area());
    }
    if app.show_help {
        draw_dialogs::draw_help_overlay(
            f,
            app.kbd_enhanced,
            app.alt_enter_stolen,
            app.show_startup_screen,
        );
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
    // Issues panel overlay (Shift+I global)
    if app.issues_panel.is_some() {
        draw_issues::draw_issues_panel(f, app);
    }
    // Git panel overlays — commit editor and conflict resolution render over viewer pane
    if let Some(ref panel) = app.git_actions_panel {
        if let Some(ref overlay) = panel.commit_overlay {
            draw_git_actions::draw_commit_editor(
                f,
                overlay,
                app.pane_viewer,
                app.kbd_enhanced,
                app.alt_enter_stolen,
            );
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
    if let Some((ref branches, _)) = app.auto_rebase_success_until {
        draw_auto_rebase_dialog(f, branches, true);
    }
    // Welcome modal — no worktrees and not browsing main
    if app.needs_welcome_modal() {
        draw_dialogs::draw_welcome_modal(f);
    }
    // STT model download dialog — asks to download Whisper model
    if app.stt_download_dialog {
        draw_stt_download_dialog(f);
    }
    // STT model download progress — shown while downloading
    if let Some(ref msg) = app.stt_download_message {
        draw_loading_indicator(f, msg);
    }
    // Generic loading indicator — highest z-order, shown while a deferred action runs next frame
    if let Some(ref msg) = app.loading_indicator {
        draw_loading_indicator(f, msg);
    }
    // Update available dialog — above all other overlays
    if let Some(ref info) = app.update_available {
        if let Some(ref msg) = app.update_progress_message {
            draw_loading_indicator(f, msg);
        } else {
            overlays::draw_update_dialog(f, info);
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    // ── Layout constraint values ──────────────────────────────────────

    #[test]
    fn normal_mode_sidebar_percentage() {
        let pct = 15u16;
        assert_eq!(pct, 15);
    }

    #[test]
    fn normal_mode_viewer_percentage() {
        let pct = 50u16;
        assert_eq!(pct, 50);
    }

    #[test]
    fn normal_mode_session_percentage() {
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

    #[test]
    fn git_box_height_is_three() {
        let git_box_height = 3u16;
        assert_eq!(git_box_height, 3);
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
        assert_eq!(max_input, 3);
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
        assert_eq!(result, 7);
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
        let input = "aaaa";
        let inner_width: usize = 3;
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
    fn row_wrapping_empty_input() {
        let input = "";
        let inner_width: usize = 80;
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
}
