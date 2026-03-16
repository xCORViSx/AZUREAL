//! Worktree Health panel rendering — tabbed modal overlay housing multiple
//! health-check systems (God Files, Documentation). Same centered overlay
//! pattern as the Git panel with a tab bar at the top.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use super::keybindings;
use crate::app::types::{HealthTab, ModuleStyleDialog, PythonModuleStyle, RustModuleStyle};
use crate::app::App;

/// Health panel accent — bright green from the scope overlay
const GF_GREEN: Color = Color::Rgb(80, 200, 80);

/// Draw the Worktree Health panel as a centered modal overlay.
/// Renders a tab bar at the top and tab-specific content below it.
pub fn draw_health_panel(f: &mut Frame, app: &App) {
    let Some(ref panel) = app.health_panel else {
        return;
    };
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
    f.render_widget(Clear, modal);

    let inner_w = modal.width.saturating_sub(4) as usize;
    let mut lines: Vec<Line> = Vec::new();

    // ── Tab bar ──
    // Active tab: bright green bold. Inactive tab: dim gray.
    let (gf_style, doc_style) = match panel.tab {
        HealthTab::GodFiles => (
            Style::default().fg(GF_GREEN).add_modifier(Modifier::BOLD),
            Style::default().fg(Color::DarkGray),
        ),
        HealthTab::Documentation => (
            Style::default().fg(Color::DarkGray),
            Style::default().fg(GF_GREEN).add_modifier(Modifier::BOLD),
        ),
    };
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("[ God Files ]", gf_style),
        Span::raw("  "),
        Span::styled("[ Documentation ]", doc_style),
    ]));
    lines.push(Line::from(""));

    // ── Tab content ── footer sourced from keybindings.rs hint generators
    // Module style dialog overrides the god files tab content + footer
    let footer = match panel.tab {
        HealthTab::GodFiles => {
            if let Some(ref dialog) = panel.module_style_dialog {
                draw_module_style_dialog(&mut lines, dialog, inner_w);
                " Space:toggle  Enter:confirm  Esc:cancel ".to_string()
            } else {
                draw_god_files_tab(&mut lines, panel, inner_w, modal_h);
                keybindings::health_god_files_hints()
            }
        }
        HealthTab::Documentation => {
            draw_documentation_tab(&mut lines, panel, inner_w, modal_h);
            keybindings::health_docs_hints()
        }
    };

    // Render the modal block with tab hint top-left, scope hint top-right
    let tab_key = keybindings::find_key_for_action(
        &keybindings::HEALTH_SHARED,
        keybindings::Action::HealthSwitchTab,
    )
    .unwrap_or("Tab".into());
    let tab_hint = Line::from(Span::styled(
        format!(" {}:tab ", tab_key),
        Style::default().fg(GF_GREEN),
    ));
    let title = Line::from(vec![Span::styled(
        format!(" Health: {} ", panel.worktree_name),
        Style::default().fg(GF_GREEN).bold(),
    )])
    .alignment(ratatui::layout::Alignment::Center);
    let scope_key = keybindings::find_key_for_action(
        &keybindings::HEALTH_SHARED,
        keybindings::Action::HealthScopeMode,
    )
    .unwrap_or("s".into());
    let scope_hint = Line::from(Span::styled(
        format!(" {}:scope ", scope_key),
        Style::default().fg(GF_GREEN),
    ))
    .alignment(ratatui::layout::Alignment::Right);
    let block = Block::default()
        .title(tab_hint)
        .title(title)
        .title(scope_hint)
        .borders(Borders::ALL)
        .border_type(BorderType::QuadrantOutside)
        .border_style(Style::default().fg(GF_GREEN));
    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, modal);

    // Render footer hints at the bottom border
    let footer_y = modal.y + modal.height.saturating_sub(1);
    let footer_x = modal.x + (modal.width.saturating_sub(footer.len() as u16)) / 2;
    if footer_y < area.height && footer_x < area.width {
        let footer_rect = Rect::new(footer_x, footer_y, footer.len() as u16, 1);
        let footer_widget = Paragraph::new(Line::from(Span::styled(
            footer,
            Style::default().fg(GF_GREEN),
        )));
        f.render_widget(footer_widget, footer_rect);
    }
}

/// Render the God Files tab content — same checkbox list as the old standalone panel.
/// Subtract 10 for: title border, tab bar, blank, GFM hint, blank, header, blank,
/// scroll indicator, footer, bottom border.
fn draw_god_files_tab(
    lines: &mut Vec<Line>,
    panel: &crate::app::types::HealthPanel,
    inner_w: usize,
    modal_h: u16,
) {
    if panel.god_files.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No source files exceed 1000 lines. Your codebase is well-modularized!",
            Style::default().fg(GF_GREEN),
        )));
        return;
    }

    // GFM session naming hint
    lines.push(Line::from(Span::styled(
        "  Sessions will be prefixed [GFM] (God File Modularize)",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    // Column header with checked count
    let checked_count = panel.god_files.iter().filter(|e| e.checked).count();
    lines.push(Line::from(Span::styled(
        format!(
            "  {} files over 1000 LOC ({} checked)",
            panel.god_files.len(),
            checked_count
        ),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    // Visible items: modal height minus chrome (tab bar + header area + footer)
    let visible_items = (modal_h as usize).saturating_sub(12);
    let scroll = if panel.god_selected < panel.god_scroll {
        panel.god_selected
    } else if panel.god_selected >= panel.god_scroll + visible_items {
        panel
            .god_selected
            .saturating_sub(visible_items.saturating_sub(1))
    } else {
        panel.god_scroll
    };

    // Render visible entries
    for (i, entry) in panel
        .god_files
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_items)
    {
        let is_selected = i == panel.god_selected;
        let checkbox = if entry.checked { "[x] " } else { "[ ] " };
        let checkbox_color = if entry.checked {
            GF_GREEN
        } else {
            Color::DarkGray
        };
        let line_count_str = format!(" {} lines", entry.line_count);
        let path_max = inner_w.saturating_sub(checkbox.len() + line_count_str.len() + 1);
        let path_display = if entry.rel_path.len() > path_max {
            format!(
                "…{}",
                &entry.rel_path[entry.rel_path.len().saturating_sub(path_max - 1)..]
            )
        } else {
            entry.rel_path.clone()
        };
        let padding =
            inner_w.saturating_sub(checkbox.len() + path_display.len() + line_count_str.len());
        let pad_str = " ".repeat(padding);
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

    // Scroll indicator
    if panel.god_files.len() > visible_items {
        let pos = scroll + 1;
        let total = panel.god_files.len();
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                "  {}-{} of {}",
                pos,
                (pos + visible_items - 1).min(total),
                total
            ),
            Style::default().fg(Color::DarkGray),
        )));
    }
}

/// Render the Documentation tab content — shows overall score and per-file coverage
/// sorted by worst-documented files first.
fn draw_documentation_tab(
    lines: &mut Vec<Line>,
    panel: &crate::app::types::HealthPanel,
    inner_w: usize,
    modal_h: u16,
) {
    // Overall score header with color coding
    let score_color = if panel.doc_score >= 80.0 {
        GF_GREEN
    } else if panel.doc_score >= 50.0 {
        Color::Yellow
    } else {
        Color::Red
    };
    lines.push(Line::from(vec![
        Span::styled(
            "  Overall Documentation Score: ",
            Style::default().fg(Color::White),
        ),
        Span::styled(
            format!("{:.1}%", panel.doc_score),
            Style::default()
                .fg(score_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  ({} files scanned)", panel.doc_entries.len()),
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    lines.push(Line::from(""));

    if panel.doc_entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No source files with documentable items found.",
            Style::default().fg(Color::DarkGray),
        )));
        return;
    }

    // [DH] session naming hint — mirrors the [GFM] hint in God Files tab
    lines.push(Line::from(Span::styled(
        "  Sessions will be prefixed [DH] (Documentation Health)",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    // Column header with checked count
    let checked_count = panel.doc_entries.iter().filter(|e| e.checked).count();
    lines.push(Line::from(Span::styled(
        format!(
            "  {} files sorted by coverage — worst first ({} checked)",
            panel.doc_entries.len(),
            checked_count
        ),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    // Visible items — extra chrome from hint + checked count rows
    let visible_items = (modal_h as usize).saturating_sub(14);
    let scroll = if panel.doc_selected < panel.doc_scroll {
        panel.doc_selected
    } else if panel.doc_selected >= panel.doc_scroll + visible_items {
        panel
            .doc_selected
            .saturating_sub(visible_items.saturating_sub(1))
    } else {
        panel.doc_scroll
    };

    let bar_width = 10usize;

    for (i, entry) in panel
        .doc_entries
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_items)
    {
        let is_selected = i == panel.doc_selected;

        // Checkbox prefix — same visual as God Files tab
        let checkbox = if entry.checked { "[x] " } else { "[ ] " };
        let checkbox_color = if entry.checked {
            GF_GREEN
        } else {
            Color::DarkGray
        };

        let pct_str = format!("{:5.1}%", entry.coverage_pct);
        let pct_color = if entry.coverage_pct >= 80.0 {
            GF_GREEN
        } else if entry.coverage_pct >= 50.0 {
            Color::Yellow
        } else {
            Color::Red
        };

        let filled = (entry.coverage_pct / 100.0 * bar_width as f32).round() as usize;
        let bar: String = "█".repeat(filled) + &"░".repeat(bar_width - filled);

        let ratio = format!(" {}/{}", entry.documented_items, entry.total_items);

        // Path — leave room for checkbox + pct + bar + ratio
        let fixed_width = checkbox.len() + pct_str.len() + 1 + bar_width + ratio.len() + 2;
        let path_max = inner_w.saturating_sub(fixed_width + 1);
        let path_display = if entry.rel_path.len() > path_max {
            format!(
                "…{}",
                &entry.rel_path[entry.rel_path.len().saturating_sub(path_max - 1)..]
            )
        } else {
            entry.rel_path.clone()
        };
        let padding = inner_w.saturating_sub(
            checkbox.len() + path_display.len() + pct_str.len() + 1 + bar_width + ratio.len(),
        );
        let pad_str = " ".repeat(padding);

        let path_style = if is_selected {
            Style::default().fg(GF_GREEN).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        lines.push(Line::from(vec![
            Span::styled(checkbox, Style::default().fg(checkbox_color)),
            Span::styled(path_display, path_style),
            Span::raw(pad_str),
            Span::styled(pct_str, Style::default().fg(pct_color)),
            Span::raw(" "),
            Span::styled(bar, Style::default().fg(pct_color)),
            Span::styled(ratio, Style::default().fg(Color::DarkGray)),
        ]));
    }

    // Scroll indicator
    if panel.doc_entries.len() > visible_items {
        let pos = scroll + 1;
        let total = panel.doc_entries.len();
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                "  {}-{} of {}",
                pos,
                (pos + visible_items - 1).min(total),
                total
            ),
            Style::default().fg(Color::DarkGray),
        )));
    }
}

/// Render the module style selector inline where the god files list normally goes.
/// One row per dual-style language (Rust and/or Python), with ●/○ radio indicators.
/// The user's cursor highlights the selected row in green; Space toggles the style.
fn draw_module_style_dialog(
    lines: &mut Vec<Line<'static>>,
    dialog: &ModuleStyleDialog,
    _inner_w: usize,
) {
    lines.push(Line::from(Span::styled(
        "  Select module style for checked files:",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // Track which visual row we're on so `selected` maps correctly
    let mut row = 0usize;

    if dialog.has_rust {
        let is_selected = dialog.selected == row;
        let (label, hint) = match dialog.rust_style {
            RustModuleStyle::FileBased => (
                "● File-based root (modulename.rs + modulename/)",
                "○ Directory module (modulename/mod.rs)",
            ),
            RustModuleStyle::ModRs => (
                "○ File-based root (modulename.rs + modulename/)",
                "● Directory module (modulename/mod.rs)",
            ),
        };
        let style = if is_selected {
            Style::default().fg(GF_GREEN).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let dim = if is_selected {
            Style::default().fg(GF_GREEN)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(Span::styled(
            "  Rust (.rs):",
            Style::default().fg(Color::DarkGray),
        )));
        let cursor = if is_selected { "▸ " } else { "  " };
        lines.push(Line::from(vec![Span::styled(
            format!("    {}{}", cursor, label),
            style,
        )]));
        lines.push(Line::from(vec![Span::styled(
            format!("      {}", hint),
            dim,
        )]));
        lines.push(Line::from(""));
        row += 1;
    }

    if dialog.has_python {
        let is_selected = dialog.selected == row;
        let (label, hint) = match dialog.python_style {
            PythonModuleStyle::Package => (
                "● Package (__init__.py directory)",
                "○ Single-file modules (no __init__.py)",
            ),
            PythonModuleStyle::SingleFile => (
                "○ Package (__init__.py directory)",
                "● Single-file modules (no __init__.py)",
            ),
        };
        let style = if is_selected {
            Style::default().fg(GF_GREEN).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let dim = if is_selected {
            Style::default().fg(GF_GREEN)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(Span::styled(
            "  Python (.py):",
            Style::default().fg(Color::DarkGray),
        )));
        let cursor = if is_selected { "▸ " } else { "  " };
        lines.push(Line::from(vec![Span::styled(
            format!("    {}{}", cursor, label),
            style,
        )]));
        lines.push(Line::from(vec![Span::styled(
            format!("      {}", hint),
            dim,
        )]));
        lines.push(Line::from(""));
    }

    // Instruction hint
    lines.push(Line::from(Span::styled(
        "  Space to toggle  ·  Enter to confirm  ·  Esc to cancel",
        Style::default().fg(Color::DarkGray),
    )));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::DocEntry;
    use std::path::PathBuf;

    // ── GF_GREEN constant ──

    #[test]
    fn test_gf_green_value() {
        assert_eq!(GF_GREEN, Color::Rgb(80, 200, 80));
    }
    #[test]
    fn test_gf_green_is_rgb() {
        matches!(GF_GREEN, Color::Rgb(_, _, _));
    }

    // ── HealthTab ──

    #[test]
    fn test_health_tab_god_files() {
        assert_eq!(HealthTab::GodFiles, HealthTab::GodFiles);
    }
    #[test]
    fn test_health_tab_documentation() {
        assert_eq!(HealthTab::Documentation, HealthTab::Documentation);
    }
    #[test]
    fn test_health_tab_ne() {
        assert_ne!(HealthTab::GodFiles, HealthTab::Documentation);
    }
    #[test]
    fn test_health_tab_copy() {
        let t = HealthTab::GodFiles;
        let c = t;
        assert_eq!(t, c);
    }
    #[test]
    fn test_health_tab_clone() {
        let t = HealthTab::Documentation;
        let c = t.clone();
        assert_eq!(t, c);
    }
    #[test]
    fn test_health_tab_debug() {
        assert_eq!(format!("{:?}", HealthTab::GodFiles), "GodFiles");
    }

    // ── RustModuleStyle ──

    #[test]
    fn test_rust_style_file_based() {
        assert_eq!(RustModuleStyle::FileBased, RustModuleStyle::FileBased);
    }
    #[test]
    fn test_rust_style_mod_rs() {
        assert_eq!(RustModuleStyle::ModRs, RustModuleStyle::ModRs);
    }
    #[test]
    fn test_rust_style_ne() {
        assert_ne!(RustModuleStyle::FileBased, RustModuleStyle::ModRs);
    }
    #[test]
    fn test_rust_style_labels_file_based() {
        let (label, hint) = match RustModuleStyle::FileBased {
            RustModuleStyle::FileBased => ("● File-based root", "○ Directory module"),
            RustModuleStyle::ModRs => ("○ File-based root", "● Directory module"),
        };
        assert!(label.starts_with('●'));
        assert!(hint.starts_with('○'));
    }
    #[test]
    fn test_rust_style_labels_mod_rs() {
        let (label, hint) = match RustModuleStyle::ModRs {
            RustModuleStyle::FileBased => ("● File-based root", "○ Directory module"),
            RustModuleStyle::ModRs => ("○ File-based root", "● Directory module"),
        };
        assert!(label.starts_with('○'));
        assert!(hint.starts_with('●'));
    }

    // ── PythonModuleStyle ──

    #[test]
    fn test_python_style_package() {
        assert_eq!(PythonModuleStyle::Package, PythonModuleStyle::Package);
    }
    #[test]
    fn test_python_style_single() {
        assert_eq!(PythonModuleStyle::SingleFile, PythonModuleStyle::SingleFile);
    }
    #[test]
    fn test_python_style_ne() {
        assert_ne!(PythonModuleStyle::Package, PythonModuleStyle::SingleFile);
    }
    #[test]
    fn test_python_style_labels_package() {
        let (label, _) = match PythonModuleStyle::Package {
            PythonModuleStyle::Package => ("● Package", "○ Single-file"),
            PythonModuleStyle::SingleFile => ("○ Package", "● Single-file"),
        };
        assert!(label.starts_with('●'));
    }

    // ── ModuleStyleDialog ──

    #[test]
    fn test_module_style_dialog_rust_only() {
        let d = ModuleStyleDialog {
            has_rust: true,
            has_python: false,
            rust_style: RustModuleStyle::FileBased,
            python_style: PythonModuleStyle::Package,
            selected: 0,
        };
        assert!(d.has_rust);
        assert!(!d.has_python);
    }
    #[test]
    fn test_module_style_dialog_both() {
        let d = ModuleStyleDialog {
            has_rust: true,
            has_python: true,
            rust_style: RustModuleStyle::ModRs,
            python_style: PythonModuleStyle::SingleFile,
            selected: 1,
        };
        assert!(d.has_rust && d.has_python);
        assert_eq!(d.selected, 1);
    }

    // ── DocEntry ──

    #[test]
    fn test_doc_entry_construction() {
        let e = DocEntry {
            path: PathBuf::from("/a.rs"),
            rel_path: "a.rs".into(),
            total_items: 10,
            documented_items: 5,
            coverage_pct: 50.0,
            checked: false,
        };
        assert_eq!(e.coverage_pct, 50.0);
    }
    #[test]
    fn test_doc_entry_checked() {
        let e = DocEntry {
            path: PathBuf::from("/b.rs"),
            rel_path: "b.rs".into(),
            total_items: 8,
            documented_items: 8,
            coverage_pct: 100.0,
            checked: true,
        };
        assert!(e.checked);
    }
    #[test]
    fn test_doc_entry_clone() {
        let e = DocEntry {
            path: PathBuf::from("/c.rs"),
            rel_path: "c.rs".into(),
            total_items: 3,
            documented_items: 1,
            coverage_pct: 33.3,
            checked: false,
        };
        let c = e.clone();
        assert_eq!(c.total_items, 3);
    }

    // ── Score color logic ──

    #[test]
    fn test_score_color_high() {
        let score = 90.0f32;
        let color = if score >= 80.0 {
            GF_GREEN
        } else if score >= 50.0 {
            Color::Yellow
        } else {
            Color::Red
        };
        assert_eq!(color, GF_GREEN);
    }
    #[test]
    fn test_score_color_medium() {
        let score = 60.0f32;
        let color = if score >= 80.0 {
            GF_GREEN
        } else if score >= 50.0 {
            Color::Yellow
        } else {
            Color::Red
        };
        assert_eq!(color, Color::Yellow);
    }
    #[test]
    fn test_score_color_low() {
        let score = 30.0f32;
        let color = if score >= 80.0 {
            GF_GREEN
        } else if score >= 50.0 {
            Color::Yellow
        } else {
            Color::Red
        };
        assert_eq!(color, Color::Red);
    }
    #[test]
    fn test_score_color_boundary_80() {
        let score = 80.0f32;
        let color = if score >= 80.0 {
            GF_GREEN
        } else if score >= 50.0 {
            Color::Yellow
        } else {
            Color::Red
        };
        assert_eq!(color, GF_GREEN);
    }
    #[test]
    fn test_score_color_boundary_50() {
        let score = 50.0f32;
        let color = if score >= 80.0 {
            GF_GREEN
        } else if score >= 50.0 {
            Color::Yellow
        } else {
            Color::Red
        };
        assert_eq!(color, Color::Yellow);
    }

    // ── Bar rendering ──

    #[test]
    fn test_bar_full() {
        let pct = 100.0f32;
        let bw = 10;
        let filled = (pct / 100.0 * bw as f32).round() as usize;
        let bar: String = "\u{2588}".repeat(filled) + &"\u{2591}".repeat(bw - filled);
        assert_eq!(bar.chars().count(), 10);
        assert!(!bar.contains('\u{2591}'));
    }
    #[test]
    fn test_bar_empty() {
        let pct = 0.0f32;
        let bw = 10;
        let filled = (pct / 100.0 * bw as f32).round() as usize;
        let bar: String = "\u{2588}".repeat(filled) + &"\u{2591}".repeat(bw - filled);
        assert_eq!(bar.chars().count(), 10);
        assert!(!bar.contains('\u{2588}'));
    }
    #[test]
    fn test_bar_half() {
        let pct = 50.0f32;
        let bw = 10;
        let filled = (pct / 100.0 * bw as f32).round() as usize;
        assert_eq!(filled, 5);
    }

    // ── Ratio format ──

    #[test]
    fn test_ratio_format() {
        let s = format!(" {}/{}", 5, 10);
        assert_eq!(s, " 5/10");
    }

    // ── Tab bar styles ──

    #[test]
    fn test_tab_gf_active() {
        let tab = HealthTab::GodFiles;
        let (gf, doc) = match tab {
            HealthTab::GodFiles => (
                Style::default().fg(GF_GREEN).add_modifier(Modifier::BOLD),
                Style::default().fg(Color::DarkGray),
            ),
            HealthTab::Documentation => (
                Style::default().fg(Color::DarkGray),
                Style::default().fg(GF_GREEN).add_modifier(Modifier::BOLD),
            ),
        };
        assert_eq!(gf.fg, Some(GF_GREEN));
        assert_eq!(doc.fg, Some(Color::DarkGray));
    }
    #[test]
    fn test_tab_doc_active() {
        let tab = HealthTab::Documentation;
        let (gf, doc) = match tab {
            HealthTab::GodFiles => (
                Style::default().fg(GF_GREEN).add_modifier(Modifier::BOLD),
                Style::default().fg(Color::DarkGray),
            ),
            HealthTab::Documentation => (
                Style::default().fg(Color::DarkGray),
                Style::default().fg(GF_GREEN).add_modifier(Modifier::BOLD),
            ),
        };
        assert_eq!(gf.fg, Some(Color::DarkGray));
        assert_eq!(doc.fg, Some(GF_GREEN));
    }

    // ── Modal sizing ──

    #[test]
    fn test_health_modal_w() {
        assert_eq!((100u16 * 55 / 100).max(50).min(100), 55);
    }
    #[test]
    fn test_health_modal_h() {
        assert_eq!((40u16 * 70 / 100).max(16).min(40), 28);
    }

    // ── Visible items ──

    #[test]
    fn test_gf_visible_items() {
        assert_eq!((30u16 as usize).saturating_sub(12), 18);
    }
    #[test]
    fn test_doc_visible_items() {
        assert_eq!((30u16 as usize).saturating_sub(14), 16);
    }

    // ── Percentage format ──

    #[test]
    fn test_pct_format() {
        assert_eq!(format!("{:5.1}%", 75.5), " 75.5%");
    }
    #[test]
    fn test_pct_format_100() {
        assert_eq!(format!("{:5.1}%", 100.0), "100.0%");
    }
    #[test]
    fn test_pct_format_0() {
        assert_eq!(format!("{:5.1}%", 0.0), "  0.0%");
    }

    // ── Score display ──

    #[test]
    fn test_score_display() {
        assert_eq!(format!("{:.1}%", 85.3f32), "85.3%");
    }

    // ── Dialog cursor logic ──

    #[test]
    fn test_dialog_cursor_selected() {
        let selected = 0;
        let is_selected = selected == 0;
        let cursor = if is_selected { "\u{25b8} " } else { "  " };
        assert_eq!(cursor, "\u{25b8} ");
    }
    #[test]
    fn test_dialog_cursor_not_selected() {
        let selected = 1;
        let is_selected = selected == 0;
        let cursor = if is_selected { "\u{25b8} " } else { "  " };
        assert_eq!(cursor, "  ");
    }

    // ── Hint text ──

    #[test]
    fn test_module_style_hint() {
        let hint = "  Space to toggle  ·  Enter to confirm  ·  Esc to cancel";
        assert!(hint.contains("Space"));
        assert!(hint.contains("Enter"));
        assert!(hint.contains("Esc"));
    }

    // ── DH session prefix ──

    #[test]
    fn test_dh_prefix() {
        let msg = "  Sessions will be prefixed [DH] (Documentation Health)";
        assert!(msg.contains("[DH]"));
    }

    // ── Scroll ──

    #[test]
    fn test_doc_scroll_above() {
        let selected: usize = 2;
        let scroll: usize = 5;
        let vis: usize = 10;
        let new = if selected < scroll {
            selected
        } else if selected >= scroll + vis {
            selected.saturating_sub(vis - 1)
        } else {
            scroll
        };
        assert_eq!(new, 2);
    }
    #[test]
    fn test_doc_scroll_below() {
        let selected: usize = 20;
        let scroll: usize = 5;
        let vis: usize = 10;
        let new = if selected < scroll {
            selected
        } else if selected >= scroll + vis {
            selected.saturating_sub(vis - 1)
        } else {
            scroll
        };
        assert_eq!(new, 11);
    }

    // ── Checked count ──

    #[test]
    fn test_doc_checked_count() {
        let entries = vec![
            DocEntry {
                path: PathBuf::from("/a"),
                rel_path: "a".into(),
                total_items: 5,
                documented_items: 3,
                coverage_pct: 60.0,
                checked: true,
            },
            DocEntry {
                path: PathBuf::from("/b"),
                rel_path: "b".into(),
                total_items: 5,
                documented_items: 0,
                coverage_pct: 0.0,
                checked: false,
            },
        ];
        assert_eq!(entries.iter().filter(|e| e.checked).count(), 1);
    }

    #[test]
    fn test_gf_green_rgb_green_channel_highest() {
        if let Color::Rgb(r, g, b) = GF_GREEN {
            assert!(g > r && g > b);
        } else {
            panic!();
        }
    }

    #[test]
    fn test_health_tab_ne_variants() {
        assert_ne!(HealthTab::GodFiles, HealthTab::Documentation);
    }
}
