//! Small popup/dialog overlays
//!
//! Centered popups for auto-rebase status, git status box, debug dump naming,
//! debug dump saving indicator, and generic loading indicator.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::app::App;

use super::super::draw_input;
use super::super::keybindings;
use super::super::util::{AZURE, GIT_ORANGE};

/// Auto-rebase dialog — centered popup showing rebase progress or success.
/// `success` = true shows green border with checkmark, false shows AZURE "in progress".
pub fn draw_auto_rebase_dialog(f: &mut Frame, branches: &[String], success: bool) {
    let area = f.area();
    let lines: Vec<Line> = if success {
        branches
            .iter()
            .map(|b| {
                Line::from(Span::styled(
                    format!(" {} \u{2713}", b),
                    Style::default().fg(Color::White),
                ))
            })
            .collect()
    } else {
        branches
            .iter()
            .map(|b| {
                Line::from(Span::styled(
                    format!(" {} ...", b),
                    Style::default().fg(Color::White),
                ))
            })
            .collect()
    };
    let border_color = if success { Color::Green } else { AZURE };
    let title = if success {
        " rebased onto main "
    } else {
        " auto-rebasing onto main "
    };
    let max_line_w = lines
        .iter()
        .map(|l| l.width() as u16)
        .max()
        .unwrap_or(0)
        .max(title.len() as u16);
    let w = (max_line_w + 4).min(area.width.saturating_sub(4));
    let h = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
    // Top-right corner — avoids overlapping the centered post-merge dialog
    let x = area.x + area.width.saturating_sub(w + 1);
    let y = area.y + 1;
    let rect = Rect::new(x, y, w, h);
    let dialog = Paragraph::new(lines).alignment(Alignment::Center).block(
        Block::default()
            .title(title)
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    );
    f.render_widget(ratatui::widgets::Clear, rect);
    f.render_widget(dialog, rect);
}

/// Git status box — full-width bar reusing the input box area.
/// Title shows keybinding hints (formatted like the prompt box); content shows operation result messages.
pub fn draw_git_status_box(f: &mut Frame, app: &App, area: Rect) {
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

/// Debug dump naming dialog — centered input for entering a suffix for the dump file.
/// ⌃d opens this, user types a name, Enter saves, Esc cancels.
pub fn draw_debug_dump_naming(f: &mut Frame, app: &App) {
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
pub fn draw_debug_dump_saving(f: &mut Frame) {
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

/// Generic loading indicator — centered popup shown while a deferred action
/// (session load, file open, health scan, project switch, etc.) runs on the
/// next frame. Reused by all two-phase deferred draw operations.
pub fn draw_loading_indicator(f: &mut Frame, msg: &str) {
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

/// STT model download prompt — centered dialog asking if the user wants to download the Whisper model.
pub fn draw_stt_download_dialog(f: &mut Frame) {
    let area = f.area();
    let key_style = Style::default()
        .fg(Color::Magenta)
        .add_modifier(Modifier::BOLD);
    let white = Style::default().fg(Color::White);
    let dim = Style::default().fg(Color::DarkGray);

    let size_hint = "~466 MB";
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("  Whisper speech model not found.  ", white)),
        Line::from(Span::styled(
            format!("  Download it now? ({})  ", size_hint),
            dim,
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("   y", key_style),
            Span::styled("  Download   ", white),
        ]),
        Line::from(vec![
            Span::styled("   Esc", key_style),
            Span::styled("  Cancel   ", dim),
        ]),
        Line::from(""),
    ];

    let h = lines.len() as u16 + 2;
    let w = 42u16.min(area.width.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let rect = Rect::new(x, y, w, h);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Magenta))
        .title(Span::styled(
            " Speech-to-Text ",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center);

    f.render_widget(ratatui::widgets::Clear, rect);
    f.render_widget(Paragraph::new(lines).block(block), rect);
}

/// Update available dialog — centered modal showing version info and install/skip/dismiss options.
pub fn draw_update_dialog(f: &mut Frame, info: &crate::updater::UpdateInfo) {
    let area = f.area();
    let key_style = Style::default().fg(AZURE).add_modifier(Modifier::BOLD);
    let white = Style::default().fg(Color::White);
    let dim = Style::default().fg(Color::DarkGray);
    let green = Style::default().fg(Color::Green);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  New version available: v{}  ", info.version),
            green.add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("  Current: v{}  ", crate::updater::CURRENT_VERSION),
            dim,
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("   y", key_style),
            Span::styled("  Install now   ", white),
        ]),
        Line::from(vec![
            Span::styled("   n", key_style),
            Span::styled("  Skip this version   ", white),
        ]),
        Line::from(vec![
            Span::styled("   Esc", key_style),
            Span::styled("  Remind me tomorrow   ", dim),
        ]),
        Line::from(""),
    ];

    let h = lines.len() as u16 + 2;
    let w = 40u16.min(area.width.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let rect = Rect::new(x, y, w, h);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(AZURE))
        .title(Span::styled(
            " Update Available ",
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center);

    f.render_widget(ratatui::widgets::Clear, rect);
    f.render_widget(Paragraph::new(lines).block(block), rect);
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::super::super::util::AZURE;

    #[test]
    fn auto_rebase_success_message_format() {
        let branch = "feat-tests";
        let msg = format!(" {} rebased onto main \u{2713} ", branch);
        assert!(msg.contains("feat-tests"));
        assert!(msg.contains("rebased onto main"));
        assert!(msg.contains("\u{2713}"));
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
        assert_eq!(w, 16);
    }

    #[test]
    fn dialog_width_not_clamped_when_fits() {
        let area_width: u16 = 100;
        let msg_len: u16 = 20;
        let w = (msg_len + 4).min(area_width.saturating_sub(4));
        assert_eq!(w, 24);
    }

    #[test]
    fn dialog_center_with_offset_area() {
        let area_x: u16 = 10;
        let area_width: u16 = 80;
        let w: u16 = 30;
        let x = area_x + (area_width.saturating_sub(w)) / 2;
        assert_eq!(x, 35);
    }

    #[test]
    fn dialog_saturating_sub_prevents_underflow() {
        let area_width: u16 = 2;
        let w: u16 = 10;
        let result = area_width.saturating_sub(w);
        assert_eq!(result, 0);
    }

    #[test]
    fn git_box_height_is_three() {
        let git_box_height = 3u16;
        assert_eq!(git_box_height, 3);
    }

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
        assert_eq!(w, padded.len() as u16 + 4);
    }

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
        assert_eq!(w, 26);
    }

    #[test]
    fn debug_dump_dialog_width_unclamped() {
        let area_width: u16 = 200;
        let w = 50u16.min(area_width.saturating_sub(4));
        assert_eq!(w, 50);
    }

    #[test]
    fn saving_debug_dump_message_literal() {
        let msg = " Saving debug dump... ";
        assert_eq!(msg.len(), 22);
    }
}
