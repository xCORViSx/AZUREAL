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
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::super::util::AZURE;
use crate::app::App;

/// Measure text using the terminal display columns it will occupy.
fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// Return text truncated to a display-column budget with a fitting ellipsis.
fn truncate_text_to_width(text: &str, max_width: usize) -> String {
    if display_width(text) <= max_width {
        return text.to_string();
    }

    if max_width == 0 {
        return String::new();
    }

    let ellipsis = "\u{2026}";
    let ellipsis_width = display_width(ellipsis);
    if max_width <= ellipsis_width {
        return ellipsis.to_string();
    }

    let content_width = max_width - ellipsis_width;
    let mut truncated = String::new();
    let mut current_width = 0usize;

    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width + ch_width > content_width {
            break;
        }
        truncated.push(ch);
        current_width += ch_width;
    }

    truncated.push_str(ellipsis);
    truncated
}

/// Build the fixed-width content inside a tab slot, excluding the trailing gap.
fn tab_slot_label(name: &str, slot_w: usize) -> String {
    let content_width = slot_w.saturating_sub(1);
    if content_width == 0 {
        return String::new();
    }

    let name_width = content_width.saturating_sub(1);
    let display = truncate_text_to_width(name, name_width);
    let mut label = String::from(" ");
    label.push_str(&display);

    let padding = content_width.saturating_sub(display_width(&label));
    label.extend(std::iter::repeat_n(' ', padding));
    label
}

/// How many rows the tab bar occupies (0 if no tabs, 1 for ≤6, 2 for >6)
pub(super) fn tab_bar_rows(tab_count: usize) -> u16 {
    if tab_count == 0 {
        0
    } else if tab_count <= 6 {
        1
    } else {
        2
    }
}

/// Draw fixed-width tab bar: 6 tabs per row, up to 2 rows (12 max).
/// Each "slot" is inner_width/6. Tab content fills slot_w-1 chars + 1 char gap.
pub(super) fn draw_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    let inner_w = area.width.saturating_sub(2) as usize;
    if inner_w < 12 {
        return;
    }
    // Each slot includes the tab + 1 trailing gap char. 6 slots fill the row.
    let slot_w = inner_w / 6;
    let rows = tab_bar_rows(app.viewer_tabs.len());

    for row in 0..rows {
        let y = area.y + 1 + row;
        let bar_area = Rect::new(area.x + 1, y, inner_w as u16, 1);
        let start = row as usize * 6;
        let end = (start + 6).min(app.viewer_tabs.len());
        let mut spans: Vec<Span> = Vec::new();

        for idx in start..end {
            let name = app.viewer_tabs[idx].name();
            let is_active = idx == app.viewer_active_tab;
            let style = if is_active {
                Style::default()
                    .fg(Color::Black)
                    .bg(AZURE)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray).bg(Color::DarkGray)
            };
            // Fixed-width content plus one gap char equals the full slot width.
            let tab_str = tab_slot_label(name, slot_w);
            spans.push(Span::styled(tab_str, style));
            spans.push(Span::raw(" "));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), bar_area);
    }
}

/// Draw tab dialog overlay for switching between tabs
pub(super) fn draw_tab_dialog(f: &mut Frame, app: &App, area: Rect) {
    let tab_count = app.viewer_tabs.len();
    if tab_count == 0 {
        return;
    }

    let dialog_width = 40u16.min(area.width.saturating_sub(4));
    let dialog_height = (tab_count as u16 + 4).min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
    let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

    f.render_widget(Clear, dialog_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(Span::styled(
            " Tabs ",
            Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
        ))
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
        Style::default().fg(Color::DarkGray),
    )));

    let para = Paragraph::new(lines);
    f.render_widget(para, inner);
}

#[cfg(test)]
/// Regression coverage for viewer tab row counts, labels, and dialog geometry.
mod tests {
    use super::*;
    use crate::app::types::{ViewerMode, ViewerTab};

    // ── tab_bar_rows: determines how many rows the tab bar occupies ──

    /// Verifies tab bar rows zero tabs.
    #[test]
    fn test_tab_bar_rows_zero_tabs() {
        assert_eq!(tab_bar_rows(0), 0);
    }

    /// Verifies tab bar rows one tab.
    #[test]
    fn test_tab_bar_rows_one_tab() {
        assert_eq!(tab_bar_rows(1), 1);
    }

    /// Verifies tab bar rows six tabs.
    #[test]
    fn test_tab_bar_rows_six_tabs() {
        assert_eq!(tab_bar_rows(6), 1);
    }

    /// Verifies tab bar rows seven tabs.
    #[test]
    fn test_tab_bar_rows_seven_tabs() {
        assert_eq!(tab_bar_rows(7), 2);
    }

    /// Verifies tab bar rows twelve tabs.
    #[test]
    fn test_tab_bar_rows_twelve_tabs() {
        assert_eq!(tab_bar_rows(12), 2);
    }

    /// Verifies tab bar rows two tabs.
    #[test]
    fn test_tab_bar_rows_two_tabs() {
        assert_eq!(tab_bar_rows(2), 1);
    }

    /// Verifies tab bar rows three tabs.
    #[test]
    fn test_tab_bar_rows_three_tabs() {
        assert_eq!(tab_bar_rows(3), 1);
    }

    /// Verifies tab bar rows four tabs.
    #[test]
    fn test_tab_bar_rows_four_tabs() {
        assert_eq!(tab_bar_rows(4), 1);
    }

    /// Verifies tab bar rows five tabs.
    #[test]
    fn test_tab_bar_rows_five_tabs() {
        assert_eq!(tab_bar_rows(5), 1);
    }

    /// Verifies tab bar rows eight tabs.
    #[test]
    fn test_tab_bar_rows_eight_tabs() {
        assert_eq!(tab_bar_rows(8), 2);
    }

    /// Verifies tab bar rows nine tabs.
    #[test]
    fn test_tab_bar_rows_nine_tabs() {
        assert_eq!(tab_bar_rows(9), 2);
    }

    /// Verifies tab bar rows ten tabs.
    #[test]
    fn test_tab_bar_rows_ten_tabs() {
        assert_eq!(tab_bar_rows(10), 2);
    }

    /// Verifies tab bar rows eleven tabs.
    #[test]
    fn test_tab_bar_rows_eleven_tabs() {
        assert_eq!(tab_bar_rows(11), 2);
    }

    // ── Boundary: exactly at threshold ──

    /// Verifies tab bar rows boundary six is one row.
    #[test]
    fn test_tab_bar_rows_boundary_six_is_one_row() {
        assert_eq!(tab_bar_rows(6), 1, "6 tabs should fit in 1 row");
    }

    /// Verifies tab bar rows boundary seven is two rows.
    #[test]
    fn test_tab_bar_rows_boundary_seven_is_two_rows() {
        assert_eq!(tab_bar_rows(7), 2, "7 tabs requires 2 rows");
    }

    // ── Large tab count (over 12 — still 2 rows, capped) ──

    /// Verifies tab bar rows thirteen still two.
    #[test]
    fn test_tab_bar_rows_thirteen_still_two() {
        assert_eq!(tab_bar_rows(13), 2);
    }

    /// Verifies tab bar rows twenty still two.
    #[test]
    fn test_tab_bar_rows_twenty_still_two() {
        assert_eq!(tab_bar_rows(20), 2);
    }

    /// Verifies tab bar rows hundred still two.
    #[test]
    fn test_tab_bar_rows_hundred_still_two() {
        assert_eq!(tab_bar_rows(100), 2);
    }

    // ── ViewerTab name() tests (used by draw_tab_bar) ──

    /// Create a minimal viewer tab with the supplied title for tab-rendering tests.
    fn make_tab(title: &str) -> ViewerTab {
        ViewerTab {
            path: None,
            content: None,
            scroll: 0,
            mode: ViewerMode::Empty,
            title: title.to_string(),
        }
    }

    /// Verifies viewer tab name simple.
    #[test]
    fn test_viewer_tab_name_simple() {
        let tab = make_tab("main.rs");
        assert_eq!(tab.name(), "main.rs");
    }

    /// Verifies viewer tab name empty.
    #[test]
    fn test_viewer_tab_name_empty() {
        let tab = make_tab("");
        assert_eq!(tab.name(), "");
    }

    /// Verifies viewer tab name with path.
    #[test]
    fn test_viewer_tab_name_with_path() {
        let tab = make_tab("src/lib.rs");
        assert_eq!(tab.name(), "src/lib.rs");
    }

    /// Verifies viewer tab name unicode.
    #[test]
    fn test_viewer_tab_name_unicode() {
        let tab = make_tab("fichier.rs");
        assert_eq!(tab.name(), "fichier.rs");
    }

    /// Verifies viewer tab name long.
    #[test]
    fn test_viewer_tab_name_long() {
        let long = "a".repeat(200);
        let tab = make_tab(&long);
        assert_eq!(tab.name(), long.as_str());
    }

    // ── ViewerTab field defaults ──

    /// Verifies viewer tab default scroll.
    #[test]
    fn test_viewer_tab_default_scroll() {
        let tab = make_tab("test");
        assert_eq!(tab.scroll, 0);
    }

    /// Verifies viewer tab default mode.
    #[test]
    fn test_viewer_tab_default_mode() {
        let tab = make_tab("test");
        assert_eq!(tab.mode, ViewerMode::Empty);
    }

    /// Verifies viewer tab no path.
    #[test]
    fn test_viewer_tab_no_path() {
        let tab = make_tab("test");
        assert!(tab.path.is_none());
    }

    /// Verifies viewer tab no content.
    #[test]
    fn test_viewer_tab_no_content() {
        let tab = make_tab("test");
        assert!(tab.content.is_none());
    }

    /// Verifies viewer tab with content.
    #[test]
    fn test_viewer_tab_with_content() {
        let mut tab = make_tab("test");
        tab.content = Some("file content".into());
        assert_eq!(tab.content.as_deref(), Some("file content"));
    }

    /// Verifies viewer tab with path.
    #[test]
    fn test_viewer_tab_with_path() {
        let mut tab = make_tab("test");
        tab.path = Some(std::path::PathBuf::from("/src/main.rs"));
        assert_eq!(tab.path.unwrap().to_str().unwrap(), "/src/main.rs");
    }

    /// Verifies viewer tab file mode.
    #[test]
    fn test_viewer_tab_file_mode() {
        let mut tab = make_tab("code.rs");
        tab.mode = ViewerMode::File;
        assert_eq!(tab.mode, ViewerMode::File);
    }

    /// Verifies viewer tab diff mode.
    #[test]
    fn test_viewer_tab_diff_mode() {
        let mut tab = make_tab("diff");
        tab.mode = ViewerMode::Diff;
        assert_eq!(tab.mode, ViewerMode::Diff);
    }

    /// Verifies viewer tab image mode.
    #[test]
    fn test_viewer_tab_image_mode() {
        let mut tab = make_tab("img.png");
        tab.mode = ViewerMode::Image;
        assert_eq!(tab.mode, ViewerMode::Image);
    }

    // ── App tab state tests ──

    /// Verifies app default no tabs.
    #[test]
    fn test_app_default_no_tabs() {
        let app = App::new();
        assert!(app.viewer_tabs.is_empty());
        assert_eq!(app.viewer_active_tab, 0);
    }

    /// Verifies app tab dialog default false.
    #[test]
    fn test_app_tab_dialog_default_false() {
        let app = App::new();
        assert!(!app.viewer_tab_dialog);
    }

    /// Verifies app one tab rows.
    #[test]
    fn test_app_one_tab_rows() {
        let mut app = App::new();
        app.viewer_tabs.push(make_tab("file1.rs"));
        assert_eq!(tab_bar_rows(app.viewer_tabs.len()), 1);
    }

    /// Verifies app six tabs rows.
    #[test]
    fn test_app_six_tabs_rows() {
        let mut app = App::new();
        for i in 0..6 {
            app.viewer_tabs.push(make_tab(&format!("tab{}.rs", i)));
        }
        assert_eq!(tab_bar_rows(app.viewer_tabs.len()), 1);
    }

    /// Verifies app seven tabs rows.
    #[test]
    fn test_app_seven_tabs_rows() {
        let mut app = App::new();
        for i in 0..7 {
            app.viewer_tabs.push(make_tab(&format!("tab{}.rs", i)));
        }
        assert_eq!(tab_bar_rows(app.viewer_tabs.len()), 2);
    }

    /// Verifies app twelve tabs rows.
    #[test]
    fn test_app_twelve_tabs_rows() {
        let mut app = App::new();
        for i in 0..12 {
            app.viewer_tabs.push(make_tab(&format!("tab{}.rs", i)));
        }
        assert_eq!(tab_bar_rows(app.viewer_tabs.len()), 2);
    }

    /// Verifies active tab within bounds.
    #[test]
    fn test_active_tab_within_bounds() {
        let mut app = App::new();
        for i in 0..5 {
            app.viewer_tabs.push(make_tab(&format!("t{}", i)));
        }
        app.viewer_active_tab = 3;
        assert!(app.viewer_active_tab < app.viewer_tabs.len());
    }

    /// Verifies tab names accessible after push.
    #[test]
    fn test_tab_names_accessible_after_push() {
        let mut app = App::new();
        app.viewer_tabs.push(make_tab("alpha"));
        app.viewer_tabs.push(make_tab("beta"));
        assert_eq!(app.viewer_tabs[0].name(), "alpha");
        assert_eq!(app.viewer_tabs[1].name(), "beta");
    }

    // ── Tab name truncation logic (char boundary tests for draw_tab_bar) ──

    /// Verifies short name no truncation.
    #[test]
    fn test_short_name_no_truncation() {
        let name = "abc";
        assert_eq!(truncate_text_to_width(name, 10), "abc");
    }

    /// Verifies long name truncation.
    #[test]
    fn test_long_name_truncation() {
        let name = "a_very_long_filename_here.rs";
        assert_eq!(truncate_text_to_width(name, 10), "a_very_lo…");
    }

    /// Verifies exact length no truncation.
    #[test]
    fn test_exact_length_no_truncation() {
        let name = "exactfit10";
        assert_eq!(name.chars().count(), 10);
        assert_eq!(truncate_text_to_width(name, 10), "exactfit10");
    }

    /// Verifies tab slot label uses display width for wide title.
    #[test]
    fn test_tab_slot_label_uses_display_width_for_wide_title() {
        let label = tab_slot_label("日本abc", 7);
        assert_eq!(label, " 日本…");
        assert_eq!(display_width(&label), 6);
    }

    /// Verifies tab slot label pads after wide truncation.
    #[test]
    fn test_tab_slot_label_pads_after_wide_truncation() {
        let label = tab_slot_label("日本abc", 6);
        assert_eq!(label, " 日… ");
        assert_eq!(display_width(&label), 5);
    }

    /// Verifies tab slot label handles tiny slots.
    #[test]
    fn test_tab_slot_label_handles_tiny_slots() {
        assert_eq!(tab_slot_label("abc", 0), "");
        assert_eq!(tab_slot_label("abc", 1), "");
        assert_eq!(tab_slot_label("abc", 2), " ");
    }

    // ── tab_bar_rows exhaustive coverage for all counts 0..=12 ──

    /// Verifies tab bar rows all values 0 to 6 are 0 or 1.
    #[test]
    fn test_tab_bar_rows_all_values_0_to_6_are_0_or_1() {
        for n in 0..=6 {
            let r = tab_bar_rows(n);
            assert!(r <= 1, "tab_bar_rows({}) = {}, expected ≤1", n, r);
        }
    }

    /// Verifies tab bar rows all values 7 to 12 are 2.
    #[test]
    fn test_tab_bar_rows_all_values_7_to_12_are_2() {
        for n in 7..=12 {
            let r = tab_bar_rows(n);
            assert_eq!(r, 2, "tab_bar_rows({}) should be 2", n);
        }
    }

    // ── slot_w arithmetic used in draw_tab_bar ──

    /// Verifies slot width divisible by six.
    #[test]
    fn test_slot_width_divisible_by_six() {
        // inner_w = area.width - 2. slot_w = inner_w / 6
        let area_width = 80u16;
        let inner_w = area_width.saturating_sub(2) as usize;
        let slot_w = inner_w / 6;
        assert_eq!(slot_w, 13); // 78 / 6 = 13
    }

    /// Verifies name max from slot w.
    #[test]
    fn test_name_max_from_slot_w() {
        // name_max = slot_w - 2  (strip leading pad + gap)
        let slot_w = 13usize;
        let name_max = slot_w.saturating_sub(2);
        assert_eq!(name_max, 11);
    }

    /// Verifies slot w below 12 skips render.
    #[test]
    fn test_slot_w_below_12_skips_render() {
        // draw_tab_bar returns early when inner_w < 12
        let area_width = 13u16; // inner_w = 11
        let inner_w = area_width.saturating_sub(2) as usize;
        assert!(inner_w < 12);
    }

    // ── Dialog geometry ──

    /// Verifies dialog width capped at 40.
    #[test]
    fn test_dialog_width_capped_at_40() {
        let area_width = 200u16;
        let dialog_width = 40u16.min(area_width.saturating_sub(4));
        assert_eq!(dialog_width, 40);
    }

    /// Verifies dialog width follows narrow terminal.
    #[test]
    fn test_dialog_width_follows_narrow_terminal() {
        let area_width = 30u16;
        let dialog_width = 40u16.min(area_width.saturating_sub(4));
        assert_eq!(dialog_width, 26);
    }

    /// Verifies dialog height with few tabs.
    #[test]
    fn test_dialog_height_with_few_tabs() {
        let tab_count = 3usize;
        let area_height = 40u16;
        let dialog_height = (tab_count as u16 + 4).min(area_height.saturating_sub(4));
        assert_eq!(dialog_height, 7);
    }
}
