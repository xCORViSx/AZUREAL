//! Session pane border and block construction.
//!
//! Builds the decorated `Block` for the session pane including:
//! - Focus/RCR border styling
//! - Right-aligned title (context badge, PID/exit code)
//! - Centered session name
//! - Bottom-border hints (RCR keys, model indicator)

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders},
};

use crate::app::App;
use crate::tui::util::AZURE;

/// Build the fully-decorated session pane block.
///
/// Includes focus/RCR border styling, right-aligned context/PID badge,
/// centered session name, RCR hint, and model indicator on bottom border.
pub(super) fn build_session_block(app: &App, area: Rect, title: &str) -> Block<'static> {
    let is_focused = app.focus == crate::app::Focus::Session;
    let rcr_active = app.rcr_session.is_some();
    let border_style = if rcr_active {
        // RCR mode: green borders to visually indicate active conflict resolution
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else if is_focused {
        Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    // Build right-aligned title: context percentage + PID/exit code
    let branch = app.current_worktree().map(|s| s.branch_name.clone());
    let right_title: Option<Line<'static>> = {
        let mut spans: Vec<Span<'static>> = Vec::new();

        // Context usage badge — pre-computed in update_token_badge(), just read the cache
        if let Some((ref text, color)) = app.token_badge_cache {
            spans.push(Span::styled(
                text.clone(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
        }

        // PID while running (active slot's PID = its key), exit code after.
        // Suppress when viewing a historic (non-active) session file to prevent
        // showing another session's PID or exit code in the border.
        if let Some(b) = branch.as_deref() {
            if !app.viewing_historic_session {
                // The active slot's key IS the PID string
                let active_pid = app
                    .active_slot
                    .get(b)
                    .filter(|slot| app.running_sessions.contains(*slot))
                    .and_then(|slot| slot.parse::<u32>().ok());
                if let Some(pid) = active_pid {
                    spans.push(Span::styled(
                        format!(" PID:{} ", pid),
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else if let Some(&code) = app
                    .active_slot
                    .get(b)
                    .and_then(|slot| app.agent_exit_codes.get(slot))
                    .or_else(|| app.agent_exit_codes.get(b))
                {
                    let (text, color) = if code == 0 {
                        (" exit:0 ".to_string(), Color::Green)
                    } else {
                        (format!(" exit:{} ", code), Color::Red)
                    };
                    spans.push(Span::styled(text, Style::default().fg(color)));
                }
            }
        }

        if spans.is_empty() {
            None
        } else {
            Some(Line::from(spans).alignment(Alignment::Right))
        }
    };

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(if is_focused {
            BorderType::Double
        } else {
            BorderType::Plain
        })
        .title(Span::styled(title.to_string(), border_style))
        .border_style(border_style);

    // Centered session name in [brackets] on top border
    if !app.title_session_name.is_empty() {
        // Available space: total border width minus left title, right title, and some padding
        let right_len = right_title
            .as_ref()
            .map(|rt| rt.spans.iter().map(|s| s.content.len()).sum::<usize>())
            .unwrap_or(0);
        let avail = (area.width as usize).saturating_sub(title.len() + right_len + 4);
        let name = &app.title_session_name;
        let bracketed = if name.chars().count() + 2 <= avail {
            format!("[{}]", name)
        } else if avail > 5 {
            let trunc: String = name.chars().take(avail - 3).collect();
            format!("[{}\u{2026}]", trunc)
        } else {
            String::new()
        };
        if !bracketed.is_empty() {
            let title_color = if rcr_active {
                Color::Green
            } else {
                Color::White
            };
            block = block.title(
                Line::from(Span::styled(bracketed, Style::default().fg(title_color)))
                    .alignment(Alignment::Center),
            );
        }
    }

    // Add right-aligned PID/exit title — ratatui fills gap with border chars
    if let Some(rt) = right_title {
        block = block.title(rt);
    }

    // RCR review mode: show ⌃a hint on bottom border when dialog is dismissed
    if let Some(ref rcr) = app.rcr_session {
        if !rcr.approval_pending {
            block = block.title_bottom(
                Line::from(vec![
                    Span::styled(
                        " ⌃a ",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("Accept/Abort ", Style::default().fg(Color::DarkGray)),
                ])
                .alignment(Alignment::Center),
            );
        }
    }

    // Model indicator on bottom border (right-aligned)
    {
        let model_name = app.display_model_name();
        let model_color = match model_name {
            // Claude models
            "opus" => Color::Magenta,
            "sonnet" => Color::Cyan,
            "haiku" => Color::Yellow,
            // Codex models
            "gpt-5.4" => Color::Green,
            "gpt-5.3-codex" => Color::LightGreen,
            "gpt-5.2-codex" => Color::Rgb(0, 200, 200),
            "gpt-5.2" => Color::LightCyan,
            "gpt-5.1-codex-max" => Color::Blue,
            "gpt-5.1-codex-mini" => Color::LightBlue,
            _ => Color::DarkGray,
        };
        let model_key = crate::tui::keybindings::find_key_for_action(
            &crate::tui::keybindings::GLOBAL,
            crate::tui::keybindings::Action::CycleModel,
        )
        .unwrap_or_else(|| "Ctrl+m".into());
        block = block.title_bottom(
            Line::from(vec![
                Span::styled(
                    format!(" {}", model_key),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(":", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{} ", model_name),
                    Style::default()
                        .fg(model_color)
                        .add_modifier(Modifier::BOLD),
                ),
            ])
            .alignment(Alignment::Right),
        );
    }

    block
}
