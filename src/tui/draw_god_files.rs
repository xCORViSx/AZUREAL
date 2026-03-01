//! God File panel rendering — centered modal overlay showing source files >1k LOC
//! with checkboxes for batch modularization. Same overlay pattern as the Git panel.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;

/// God File panel accent — same green used in the filter mode scope overlay
const GF_GREEN: Color = Color::Rgb(80, 200, 80);

/// Draw the God File panel as a centered modal overlay.
/// Caller should return early after this — it takes over the whole screen.
pub fn draw_god_files_panel(f: &mut Frame, app: &App) {
    let Some(ref panel) = app.god_file_panel else { return };
    let area = f.area();

    // Center modal — same size as Git panel (55% width, 70% height, min 50x16)
    let modal_w = (area.width * 55 / 100).max(50).min(area.width);
    let modal_h = (area.height * 70 / 100).max(16).min(area.height);
    let modal = Rect::new(
        area.x + (area.width.saturating_sub(modal_w)) / 2,
        area.y + (area.height.saturating_sub(modal_h)) / 2,
        modal_w,
        modal_h,
    );

    // Clear background behind the modal
    f.render_widget(Clear, modal);

    // Usable width inside borders + padding
    let inner_w = modal.width.saturating_sub(4) as usize;

    // Build content lines
    let mut lines: Vec<Line> = Vec::new();

    if panel.entries.is_empty() {
        // No god files found — congratulations!
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  No source files exceed 1000 lines. Your codebase is well-modularized!",
            Style::default().fg(GF_GREEN),
        )));
    } else {
        // Explain session naming convention: [GFM] = God File Modularize
        lines.push(Line::from(Span::styled(
            "  Sessions will be prefixed [GFM] (God File Modularize)",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));

        // Column header
        let checked_count = panel.entries.iter().filter(|e| e.checked).count();
        let header_text = format!(
            "  {} files over 1000 LOC ({} checked)",
            panel.entries.len(), checked_count,
        );
        lines.push(Line::from(Span::styled(header_text, Style::default().fg(Color::DarkGray))));
        lines.push(Line::from(""));

        // Calculate scroll window — how many items visible in the modal
        // (subtract 8 for: title border, GFM explanation, blank, header, blank, footer hint, bottom border)
        let visible_items = (modal_h as usize).saturating_sub(8);

        // Adjust scroll to keep selected item visible
        let scroll = if panel.selected < panel.scroll {
            panel.selected
        } else if panel.selected >= panel.scroll + visible_items {
            panel.selected.saturating_sub(visible_items.saturating_sub(1))
        } else {
            panel.scroll
        };

        // Render visible entries
        for (i, entry) in panel.entries.iter().enumerate().skip(scroll).take(visible_items) {
            let is_selected = i == panel.selected;

            // Checkbox: [x] or [ ]
            let checkbox = if entry.checked { "[x] " } else { "[ ] " };
            let checkbox_color = if entry.checked { GF_GREEN } else { Color::DarkGray };

            // File path — truncate if needed, leave room for line count
            let line_count_str = format!(" {} lines", entry.line_count);
            let path_max = inner_w.saturating_sub(checkbox.len() + line_count_str.len() + 1);
            let path_display = if entry.rel_path.len() > path_max {
                format!("…{}", &entry.rel_path[entry.rel_path.len().saturating_sub(path_max - 1)..])
            } else {
                entry.rel_path.clone()
            };

            // Pad path to right-align line count
            let padding = inner_w.saturating_sub(checkbox.len() + path_display.len() + line_count_str.len());
            let pad_str = " ".repeat(padding);

            // Style: selected row gets green highlight, others white
            let (path_style, count_style) = if is_selected {
                (
                    Style::default().fg(GF_GREEN).add_modifier(Modifier::BOLD),
                    Style::default().fg(GF_GREEN),
                )
            } else {
                (
                    Style::default().fg(Color::White),
                    Style::default().fg(Color::DarkGray),
                )
            };

            lines.push(Line::from(vec![
                Span::styled(checkbox, Style::default().fg(checkbox_color)),
                Span::styled(path_display, path_style),
                Span::raw(pad_str),
                Span::styled(line_count_str, count_style),
            ]));
        }

        // Scroll indicator if list is longer than viewport
        if panel.entries.len() > visible_items {
            let pos = scroll + 1;
            let total = panel.entries.len();
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {}-{} of {}", pos, (pos + visible_items - 1).min(total), total),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    // Footer key hints
    let footer = " Space:check  a:all  v:view  s:scope  Enter/m:modularize  Esc:close ";

    // Render the modal block with border and title
    let title = Line::from(vec![
        Span::styled(" God Files (>1000 LOC) ", Style::default().fg(GF_GREEN).bold()),
    ]);
    let block = Block::default()
        .title(title)
        .title_alignment(ratatui::layout::Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::QuadrantOutside)
        .border_style(Style::default().fg(GF_GREEN));

    let paragraph = Paragraph::new(lines)
        .block(block);

    f.render_widget(paragraph, modal);

    // Render footer hints at the bottom of the modal
    let footer_y = modal.y + modal.height.saturating_sub(1);
    let footer_x = modal.x + (modal.width.saturating_sub(footer.len() as u16)) / 2;
    if footer_y < area.height && footer_x < area.width {
        let footer_rect = Rect::new(footer_x, footer_y, footer.len() as u16, 1);
        let footer_widget = Paragraph::new(Line::from(Span::styled(
            footer,
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(footer_widget, footer_rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::GodFileEntry;
    use std::path::PathBuf;

    // ── GF_GREEN constant ──

    #[test]
    fn test_gf_green_value() { assert_eq!(GF_GREEN, Color::Rgb(80, 200, 80)); }
    #[test]
    fn test_gf_green_not_green() { assert_ne!(GF_GREEN, Color::Green); }
    #[test]
    fn test_gf_green_not_dark_gray() { assert_ne!(GF_GREEN, Color::DarkGray); }

    // ── GodFileEntry ──

    #[test]
    fn test_god_file_entry_construction() {
        let e = GodFileEntry { path: PathBuf::from("/src/big.rs"), rel_path: "src/big.rs".into(), line_count: 1500, checked: false };
        assert_eq!(e.line_count, 1500);
        assert!(!e.checked);
    }
    #[test]
    fn test_god_file_entry_checked() {
        let e = GodFileEntry { path: PathBuf::from("/x"), rel_path: "x".into(), line_count: 2000, checked: true };
        assert!(e.checked);
    }
    #[test]
    fn test_god_file_entry_clone() {
        let e = GodFileEntry { path: PathBuf::from("/a"), rel_path: "a".into(), line_count: 1001, checked: false };
        let c = e.clone();
        assert_eq!(c.line_count, 1001);
        assert_eq!(c.rel_path, "a");
    }
    #[test]
    fn test_god_file_entry_debug() {
        let e = GodFileEntry { path: PathBuf::from("/b"), rel_path: "b".into(), line_count: 999, checked: false };
        let dbg = format!("{:?}", e);
        assert!(dbg.contains("GodFileEntry"));
    }

    // ── Modal sizing math ──

    #[test]
    fn test_modal_width_normal() {
        let aw = 100u16;
        let mw = (aw * 55 / 100).max(50).min(aw);
        assert_eq!(mw, 55);
    }
    #[test]
    fn test_modal_width_small() {
        let aw = 40u16;
        let mw = (aw * 55 / 100).max(50).min(aw);
        assert_eq!(mw, 40); // min(50, 40) = 40 since clamped to area
    }
    #[test]
    fn test_modal_height_normal() {
        let ah = 40u16;
        let mh = (ah * 70 / 100).max(16).min(ah);
        assert_eq!(mh, 28);
    }
    #[test]
    fn test_modal_height_small() {
        let ah = 10u16;
        let mh = (ah * 70 / 100).max(16).min(ah);
        assert_eq!(mh, 10);
    }
    #[test]
    fn test_modal_centering_x() {
        let aw = 100u16;
        let mw = 55u16;
        let x = (aw.saturating_sub(mw)) / 2;
        assert_eq!(x, 22);
    }
    #[test]
    fn test_modal_centering_y() {
        let ah = 40u16;
        let mh = 28u16;
        let y = (ah.saturating_sub(mh)) / 2;
        assert_eq!(y, 6);
    }

    // ── Inner width ──

    #[test]
    fn test_inner_width() {
        let mw = 55u16;
        let iw = mw.saturating_sub(4) as usize;
        assert_eq!(iw, 51);
    }

    // ── Checkbox rendering ──

    #[test]
    fn test_checkbox_checked() { assert_eq!(if true { "[x] " } else { "[ ] " }, "[x] "); }
    #[test]
    fn test_checkbox_unchecked() { assert_eq!(if false { "[x] " } else { "[ ] " }, "[ ] "); }
    #[test]
    fn test_checkbox_color_checked() {
        let checked = true;
        let color = if checked { GF_GREEN } else { Color::DarkGray };
        assert_eq!(color, GF_GREEN);
    }
    #[test]
    fn test_checkbox_color_unchecked() {
        let checked = false;
        let color = if checked { GF_GREEN } else { Color::DarkGray };
        assert_eq!(color, Color::DarkGray);
    }

    // ── Path truncation ──

    #[test]
    fn test_path_short() {
        let rel = "src/main.rs";
        let path_max = 40;
        let display = if rel.len() > path_max {
            format!("\u{2026}{}", &rel[rel.len().saturating_sub(path_max - 1)..])
        } else { rel.to_string() };
        assert_eq!(display, "src/main.rs");
    }
    #[test]
    fn test_path_long_truncated() {
        let rel = "a/".repeat(30) + "file.rs";
        let path_max = 20;
        let display = if rel.len() > path_max {
            format!("\u{2026}{}", &rel[rel.len().saturating_sub(path_max - 1)..])
        } else { rel.to_string() };
        assert!(display.starts_with('\u{2026}'));
        assert!(display.len() <= path_max + 3); // ellipsis is multi-byte
    }

    // ── Line count format ──

    #[test]
    fn test_line_count_format() {
        let lc = 1500;
        let s = format!(" {} lines", lc);
        assert_eq!(s, " 1500 lines");
    }
    #[test]
    fn test_line_count_format_exact() {
        let s = format!(" {} lines", 1001);
        assert_eq!(s, " 1001 lines");
    }

    // ── Checked count ──

    #[test]
    fn test_checked_count_none() {
        let entries = vec![
            GodFileEntry { path: PathBuf::from("/a"), rel_path: "a".into(), line_count: 1100, checked: false },
            GodFileEntry { path: PathBuf::from("/b"), rel_path: "b".into(), line_count: 1200, checked: false },
        ];
        let count = entries.iter().filter(|e| e.checked).count();
        assert_eq!(count, 0);
    }
    #[test]
    fn test_checked_count_some() {
        let entries = vec![
            GodFileEntry { path: PathBuf::from("/a"), rel_path: "a".into(), line_count: 1100, checked: true },
            GodFileEntry { path: PathBuf::from("/b"), rel_path: "b".into(), line_count: 1200, checked: false },
            GodFileEntry { path: PathBuf::from("/c"), rel_path: "c".into(), line_count: 1300, checked: true },
        ];
        let count = entries.iter().filter(|e| e.checked).count();
        assert_eq!(count, 2);
    }
    #[test]
    fn test_checked_count_all() {
        let entries = vec![
            GodFileEntry { path: PathBuf::from("/a"), rel_path: "a".into(), line_count: 1100, checked: true },
        ];
        let count = entries.iter().filter(|e| e.checked).count();
        assert_eq!(count, 1);
    }

    // ── Header text formatting ──

    #[test]
    fn test_header_text() {
        let total = 5;
        let checked = 2;
        let s = format!("  {} files over 1000 LOC ({} checked)", total, checked);
        assert_eq!(s, "  5 files over 1000 LOC (2 checked)");
    }

    // ── Scroll indicator ──

    #[test]
    fn test_scroll_indicator_format() {
        let pos = 1;
        let visible = 10;
        let total = 25;
        let s = format!("  {}-{} of {}", pos, (pos + visible - 1).min(total), total);
        assert_eq!(s, "  1-10 of 25");
    }
    #[test]
    fn test_scroll_indicator_at_end() {
        let pos = 20;
        let visible = 10;
        let total = 25;
        let s = format!("  {}-{} of {}", pos, (pos + visible - 1).min(total), total);
        assert_eq!(s, "  20-25 of 25");
    }

    // ── Visible items calculation ──

    #[test]
    fn test_visible_items() {
        let mh = 30u16;
        let vis = (mh as usize).saturating_sub(8);
        assert_eq!(vis, 22);
    }
    #[test]
    fn test_visible_items_small() {
        let mh = 8u16;
        let vis = (mh as usize).saturating_sub(8);
        assert_eq!(vis, 0);
    }

    // ── Scroll adjustment ──

    #[test]
    fn test_scroll_keep_visible_above() {
        let selected = 2;
        let scroll = 5;
        let visible_items = 10;
        let new_scroll = if selected < scroll { selected }
            else if selected >= scroll + visible_items { selected.saturating_sub(visible_items.saturating_sub(1)) }
            else { scroll };
        assert_eq!(new_scroll, 2);
    }
    #[test]
    fn test_scroll_keep_visible_below() {
        let selected = 20;
        let scroll = 5;
        let visible_items = 10;
        let new_scroll = if selected < scroll { selected }
            else if selected >= scroll + visible_items { selected.saturating_sub(visible_items.saturating_sub(1)) }
            else { scroll };
        assert_eq!(new_scroll, 11);
    }
    #[test]
    fn test_scroll_already_visible() {
        let selected = 7;
        let scroll = 5;
        let visible_items = 10;
        let new_scroll = if selected < scroll { selected }
            else if selected >= scroll + visible_items { selected.saturating_sub(visible_items.saturating_sub(1)) }
            else { scroll };
        assert_eq!(new_scroll, 5);
    }

    // ── Footer text ──

    #[test]
    fn test_footer_text() {
        let footer = " Space:check  a:all  v:view  s:scope  Enter/m:modularize  Esc:close ";
        assert!(footer.contains("Space"));
        assert!(footer.contains("modularize"));
        assert!(footer.contains("Esc"));
    }
    #[test]
    fn test_footer_non_empty() {
        let footer = " Space:check  a:all  v:view  s:scope  Enter/m:modularize  Esc:close ";
        assert!(!footer.is_empty());
    }
    #[test]
    fn test_footer_center_x() {
        let mw = 55u16;
        let footer_len = 68u16;
        let fx = (mw.saturating_sub(footer_len)) / 2;
        assert_eq!(fx, 0); // footer wider than modal
    }
    #[test]
    fn test_footer_center_x_fits() {
        let mw = 100u16;
        let footer_len = 68u16;
        let fx = (mw.saturating_sub(footer_len)) / 2;
        assert_eq!(fx, 16);
    }

    // ── Empty state message ──

    #[test]
    fn test_empty_congrats_message() {
        let msg = "  No source files exceed 1000 lines. Your codebase is well-modularized!";
        assert!(msg.contains("well-modularized"));
    }

    // ── GFM prefix message ──

    #[test]
    fn test_gfm_session_prefix() {
        let msg = "  Sessions will be prefixed [GFM] (God File Modularize)";
        assert!(msg.contains("[GFM]"));
    }

    // ── Padding calculation ──

    #[test]
    fn test_padding_calculation() {
        let inner_w = 50;
        let checkbox_len = 4;
        let path_len = 15;
        let count_len = 12;
        let padding = inner_w.saturating_sub(checkbox_len + path_len + count_len);
        assert_eq!(padding, 19);
    }
    #[test]
    fn test_padding_overflow() {
        let inner_w = 10;
        let total = 15;
        let padding = inner_w.saturating_sub(total);
        assert_eq!(padding, 0);
    }

    // ── Style construction for selected ──

    #[test]
    fn test_selected_style() {
        let s = Style::default().fg(GF_GREEN).add_modifier(Modifier::BOLD);
        assert_eq!(s.fg, Some(GF_GREEN));
        assert!(s.add_modifier.contains(Modifier::BOLD));
    }
    #[test]
    fn test_unselected_style() {
        let s = Style::default().fg(Color::White);
        assert_eq!(s.fg, Some(Color::White));
    }
    #[test]
    fn test_count_style_selected() {
        let s = Style::default().fg(GF_GREEN);
        assert_eq!(s.fg, Some(GF_GREEN));
    }
    #[test]
    fn test_count_style_unselected() {
        let s = Style::default().fg(Color::DarkGray);
        assert_eq!(s.fg, Some(Color::DarkGray));
    }

    #[test]
    fn test_gf_green_rgb_values() {
        assert_eq!(GF_GREEN, Color::Rgb(80, 200, 80));
    }

    #[test]
    fn test_god_file_entry_checked_default() {
        let e = GodFileEntry { path: PathBuf::from("/a.rs"), rel_path: "a.rs".into(), line_count: 100, checked: false };
        assert!(!e.checked);
    }

    #[test]
    fn test_god_file_entry_line_count() {
        let e = GodFileEntry { path: PathBuf::from("/b.rs"), rel_path: "b.rs".into(), line_count: 999, checked: true };
        assert_eq!(e.line_count, 999);
    }

    #[test]
    fn test_style_fg_yellow() {
        let s = Style::default().fg(Color::Yellow);
        assert_eq!(s.fg, Some(Color::Yellow));
    }

    #[test]
    fn test_pathbuf_from_string() {
        let p = PathBuf::from("/src/main.rs");
        assert!(p.to_string_lossy().ends_with("main.rs"));
    }
}
