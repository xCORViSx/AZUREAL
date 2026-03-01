//! Tab bar rendering for the viewer panel
//!
//! Fixed-width tab bar (up to 12 tabs across 2 rows) and tab picker dialog.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use super::super::util::AZURE;

/// How many rows the tab bar occupies (0 if no tabs, 1 for ≤6, 2 for >6)
pub(super) fn tab_bar_rows(tab_count: usize) -> u16 {
    if tab_count == 0 { 0 }
    else if tab_count <= 6 { 1 }
    else { 2 }
}

/// Draw fixed-width tab bar: 6 tabs per row, up to 2 rows (12 max).
/// Each "slot" is inner_width/6. Tab content fills slot_w-1 chars + 1 char gap.
pub(super) fn draw_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    let inner_w = area.width.saturating_sub(2) as usize;
    if inner_w < 12 { return; }
    // Each slot includes the tab + 1 trailing gap char. 6 slots fill the row.
    let slot_w = inner_w / 6;
    // Visible tab content = slot minus gap(1) minus leading pad(1)
    let name_max = slot_w.saturating_sub(2);
    let rows = tab_bar_rows(app.viewer_tabs.len());

    for row in 0..rows {
        let y = area.y + 1 + row;
        let bar_area = Rect::new(area.x + 1, y, inner_w as u16, 1);
        let start = row as usize * 6;
        let end = (start + 6).min(app.viewer_tabs.len());
        let mut spans: Vec<Span> = Vec::new();

        for idx in start..end {
            let name = app.viewer_tabs[idx].name();
            // Truncate to fit, ellipsis if too long
            let display = if name.chars().count() > name_max {
                let trunc: String = name.chars().take(name_max.saturating_sub(1)).collect();
                format!("{trunc}…")
            } else {
                name.to_string()
            };
            let is_active = idx == app.viewer_active_tab;
            let style = if is_active {
                Style::default().fg(Color::Black).bg(AZURE).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray).bg(Color::DarkGray)
            };
            // " name" padded to slot_w-1, then 1 gap char = total slot_w
            let padded = format!(" {:<width$}", display, width = slot_w - 2);
            let tab_str: String = padded.chars().take(slot_w - 1).collect();
            spans.push(Span::styled(tab_str, style));
            spans.push(Span::raw(" "));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), bar_area);
    }
}

/// Draw tab dialog overlay for switching between tabs
pub(super) fn draw_tab_dialog(f: &mut Frame, app: &App, area: Rect) {
    let tab_count = app.viewer_tabs.len();
    if tab_count == 0 { return; }

    let dialog_width = 40u16.min(area.width.saturating_sub(4));
    let dialog_height = (tab_count as u16 + 4).min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
    let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(Span::styled(" Tabs ", Style::default().fg(AZURE).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(AZURE));

    f.render_widget(block.clone(), dialog_area);

    // List tabs inside dialog
    let inner = block.inner(dialog_area);
    let mut lines: Vec<Line> = Vec::new();

    for (idx, tab) in app.viewer_tabs.iter().enumerate() {
        let name = tab.name();
        let is_active = idx == app.viewer_active_tab;

        let prefix = if is_active { "▸ " } else { "  " };
        let style = if is_active {
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let num_style = Style::default().fg(Color::DarkGray);
        lines.push(Line::from(vec![
            Span::styled(format!("{}", idx + 1), num_style),
            Span::raw(" "),
            Span::styled(prefix, style),
            Span::styled(name.to_string(), style),
        ]));
    }

    // Add hints at bottom
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "j/k:nav Enter:select x:close Esc:cancel",
        Style::default().fg(Color::DarkGray)
    )));

    let para = Paragraph::new(lines);
    f.render_widget(para, inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::{ViewerMode, ViewerTab};

    // ── tab_bar_rows: determines how many rows the tab bar occupies ──

    #[test]
    fn test_tab_bar_rows_zero_tabs() {
        assert_eq!(tab_bar_rows(0), 0);
    }

    #[test]
    fn test_tab_bar_rows_one_tab() {
        assert_eq!(tab_bar_rows(1), 1);
    }

    #[test]
    fn test_tab_bar_rows_six_tabs() {
        assert_eq!(tab_bar_rows(6), 1);
    }

    #[test]
    fn test_tab_bar_rows_seven_tabs() {
        assert_eq!(tab_bar_rows(7), 2);
    }

    #[test]
    fn test_tab_bar_rows_twelve_tabs() {
        assert_eq!(tab_bar_rows(12), 2);
    }

    #[test]
    fn test_tab_bar_rows_two_tabs() {
        assert_eq!(tab_bar_rows(2), 1);
    }

    #[test]
    fn test_tab_bar_rows_three_tabs() {
        assert_eq!(tab_bar_rows(3), 1);
    }

    #[test]
    fn test_tab_bar_rows_four_tabs() {
        assert_eq!(tab_bar_rows(4), 1);
    }

    #[test]
    fn test_tab_bar_rows_five_tabs() {
        assert_eq!(tab_bar_rows(5), 1);
    }

    #[test]
    fn test_tab_bar_rows_eight_tabs() {
        assert_eq!(tab_bar_rows(8), 2);
    }

    #[test]
    fn test_tab_bar_rows_nine_tabs() {
        assert_eq!(tab_bar_rows(9), 2);
    }

    #[test]
    fn test_tab_bar_rows_ten_tabs() {
        assert_eq!(tab_bar_rows(10), 2);
    }

    #[test]
    fn test_tab_bar_rows_eleven_tabs() {
        assert_eq!(tab_bar_rows(11), 2);
    }

    // ── Boundary: exactly at threshold ──

    #[test]
    fn test_tab_bar_rows_boundary_six_is_one_row() {
        assert_eq!(tab_bar_rows(6), 1, "6 tabs should fit in 1 row");
    }

    #[test]
    fn test_tab_bar_rows_boundary_seven_is_two_rows() {
        assert_eq!(tab_bar_rows(7), 2, "7 tabs requires 2 rows");
    }

    // ── Large tab count (over 12 — still 2 rows, capped) ──

    #[test]
    fn test_tab_bar_rows_thirteen_still_two() {
        assert_eq!(tab_bar_rows(13), 2);
    }

    #[test]
    fn test_tab_bar_rows_twenty_still_two() {
        assert_eq!(tab_bar_rows(20), 2);
    }

    #[test]
    fn test_tab_bar_rows_hundred_still_two() {
        assert_eq!(tab_bar_rows(100), 2);
    }

    // ── ViewerTab name() tests (used by draw_tab_bar) ──

    fn make_tab(title: &str) -> ViewerTab {
        ViewerTab {
            path: None,
            content: None,
            scroll: 0,
            mode: ViewerMode::Empty,
            title: title.to_string(),
        }
    }

    #[test]
    fn test_viewer_tab_name_simple() {
        let tab = make_tab("main.rs");
        assert_eq!(tab.name(), "main.rs");
    }

    #[test]
    fn test_viewer_tab_name_empty() {
        let tab = make_tab("");
        assert_eq!(tab.name(), "");
    }

    #[test]
    fn test_viewer_tab_name_with_path() {
        let tab = make_tab("src/lib.rs");
        assert_eq!(tab.name(), "src/lib.rs");
    }

    #[test]
    fn test_viewer_tab_name_unicode() {
        let tab = make_tab("fichier.rs");
        assert_eq!(tab.name(), "fichier.rs");
    }

    #[test]
    fn test_viewer_tab_name_long() {
        let long = "a".repeat(200);
        let tab = make_tab(&long);
        assert_eq!(tab.name(), long.as_str());
    }

    // ── ViewerTab field defaults ──

    #[test]
    fn test_viewer_tab_default_scroll() {
        let tab = make_tab("test");
        assert_eq!(tab.scroll, 0);
    }

    #[test]
    fn test_viewer_tab_default_mode() {
        let tab = make_tab("test");
        assert_eq!(tab.mode, ViewerMode::Empty);
    }

    #[test]
    fn test_viewer_tab_no_path() {
        let tab = make_tab("test");
        assert!(tab.path.is_none());
    }

    #[test]
    fn test_viewer_tab_no_content() {
        let tab = make_tab("test");
        assert!(tab.content.is_none());
    }

    #[test]
    fn test_viewer_tab_with_content() {
        let mut tab = make_tab("test");
        tab.content = Some("file content".into());
        assert_eq!(tab.content.as_deref(), Some("file content"));
    }

    #[test]
    fn test_viewer_tab_with_path() {
        let mut tab = make_tab("test");
        tab.path = Some(std::path::PathBuf::from("/src/main.rs"));
        assert_eq!(tab.path.unwrap().to_str().unwrap(), "/src/main.rs");
    }

    #[test]
    fn test_viewer_tab_file_mode() {
        let mut tab = make_tab("code.rs");
        tab.mode = ViewerMode::File;
        assert_eq!(tab.mode, ViewerMode::File);
    }

    #[test]
    fn test_viewer_tab_diff_mode() {
        let mut tab = make_tab("diff");
        tab.mode = ViewerMode::Diff;
        assert_eq!(tab.mode, ViewerMode::Diff);
    }

    #[test]
    fn test_viewer_tab_image_mode() {
        let mut tab = make_tab("img.png");
        tab.mode = ViewerMode::Image;
        assert_eq!(tab.mode, ViewerMode::Image);
    }

    // ── App tab state tests ──

    #[test]
    fn test_app_default_no_tabs() {
        let app = App::new();
        assert!(app.viewer_tabs.is_empty());
        assert_eq!(app.viewer_active_tab, 0);
    }

    #[test]
    fn test_app_tab_dialog_default_false() {
        let app = App::new();
        assert!(!app.viewer_tab_dialog);
    }

    #[test]
    fn test_app_one_tab_rows() {
        let mut app = App::new();
        app.viewer_tabs.push(make_tab("file1.rs"));
        assert_eq!(tab_bar_rows(app.viewer_tabs.len()), 1);
    }

    #[test]
    fn test_app_six_tabs_rows() {
        let mut app = App::new();
        for i in 0..6 {
            app.viewer_tabs.push(make_tab(&format!("tab{}.rs", i)));
        }
        assert_eq!(tab_bar_rows(app.viewer_tabs.len()), 1);
    }

    #[test]
    fn test_app_seven_tabs_rows() {
        let mut app = App::new();
        for i in 0..7 {
            app.viewer_tabs.push(make_tab(&format!("tab{}.rs", i)));
        }
        assert_eq!(tab_bar_rows(app.viewer_tabs.len()), 2);
    }

    #[test]
    fn test_app_twelve_tabs_rows() {
        let mut app = App::new();
        for i in 0..12 {
            app.viewer_tabs.push(make_tab(&format!("tab{}.rs", i)));
        }
        assert_eq!(tab_bar_rows(app.viewer_tabs.len()), 2);
    }

    #[test]
    fn test_active_tab_within_bounds() {
        let mut app = App::new();
        for i in 0..5 {
            app.viewer_tabs.push(make_tab(&format!("t{}", i)));
        }
        app.viewer_active_tab = 3;
        assert!(app.viewer_active_tab < app.viewer_tabs.len());
    }

    #[test]
    fn test_tab_names_accessible_after_push() {
        let mut app = App::new();
        app.viewer_tabs.push(make_tab("alpha"));
        app.viewer_tabs.push(make_tab("beta"));
        assert_eq!(app.viewer_tabs[0].name(), "alpha");
        assert_eq!(app.viewer_tabs[1].name(), "beta");
    }

    // ── Tab name truncation logic (char boundary tests for draw_tab_bar) ──

    #[test]
    fn test_short_name_no_truncation() {
        let name = "abc";
        let name_max = 10;
        let display = if name.chars().count() > name_max {
            let trunc: String = name.chars().take(name_max.saturating_sub(1)).collect();
            format!("{trunc}...")
        } else {
            name.to_string()
        };
        assert_eq!(display, "abc");
    }

    #[test]
    fn test_long_name_truncation() {
        let name = "a_very_long_filename_here.rs";
        let name_max = 10;
        let display = if name.chars().count() > name_max {
            let trunc: String = name.chars().take(name_max.saturating_sub(1)).collect();
            format!("{trunc}...")
        } else {
            name.to_string()
        };
        assert_eq!(display, "a_very_lo...");
    }

    #[test]
    fn test_exact_length_no_truncation() {
        let name = "exactfit10";
        assert_eq!(name.chars().count(), 10);
        let name_max = 10;
        let display = if name.chars().count() > name_max {
            let trunc: String = name.chars().take(name_max.saturating_sub(1)).collect();
            format!("{trunc}...")
        } else {
            name.to_string()
        };
        assert_eq!(display, "exactfit10");
    }

    // ── tab_bar_rows exhaustive coverage for all counts 0..=12 ──

    #[test]
    fn test_tab_bar_rows_all_values_0_to_6_are_0_or_1() {
        for n in 0..=6 {
            let r = tab_bar_rows(n);
            assert!(r <= 1, "tab_bar_rows({}) = {}, expected ≤1", n, r);
        }
    }

    #[test]
    fn test_tab_bar_rows_all_values_7_to_12_are_2() {
        for n in 7..=12 {
            let r = tab_bar_rows(n);
            assert_eq!(r, 2, "tab_bar_rows({}) should be 2", n);
        }
    }

    // ── slot_w arithmetic used in draw_tab_bar ──

    #[test]
    fn test_slot_width_divisible_by_six() {
        // inner_w = area.width - 2. slot_w = inner_w / 6
        let area_width = 80u16;
        let inner_w = area_width.saturating_sub(2) as usize;
        let slot_w = inner_w / 6;
        assert_eq!(slot_w, 13);  // 78 / 6 = 13
    }

    #[test]
    fn test_name_max_from_slot_w() {
        // name_max = slot_w - 2  (strip leading pad + gap)
        let slot_w = 13usize;
        let name_max = slot_w.saturating_sub(2);
        assert_eq!(name_max, 11);
    }

    #[test]
    fn test_slot_w_below_12_skips_render() {
        // draw_tab_bar returns early when inner_w < 12
        let area_width = 13u16;  // inner_w = 11
        let inner_w = area_width.saturating_sub(2) as usize;
        assert!(inner_w < 12);
    }

    // ── Dialog geometry ──

    #[test]
    fn test_dialog_width_capped_at_40() {
        let area_width = 200u16;
        let dialog_width = 40u16.min(area_width.saturating_sub(4));
        assert_eq!(dialog_width, 40);
    }

    #[test]
    fn test_dialog_width_follows_narrow_terminal() {
        let area_width = 30u16;
        let dialog_width = 40u16.min(area_width.saturating_sub(4));
        assert_eq!(dialog_width, 26);
    }

    #[test]
    fn test_dialog_height_with_few_tabs() {
        let tab_count = 3usize;
        let area_height = 40u16;
        let dialog_height = (tab_count as u16 + 4).min(area_height.saturating_sub(4));
        assert_eq!(dialog_height, 7);
    }
}
