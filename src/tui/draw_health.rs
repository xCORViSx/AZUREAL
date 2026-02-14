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

use crate::app::App;
use crate::app::types::{HealthTab, ModuleStyleDialog, RustModuleStyle, PythonModuleStyle};
use super::keybindings;

/// Health panel accent — bright green from the scope overlay
const GF_GREEN: Color = Color::Rgb(80, 200, 80);

/// Draw the Worktree Health panel as a centered modal overlay.
/// Renders a tab bar at the top and tab-specific content below it.
pub fn draw_health_panel(f: &mut Frame, app: &App) {
    let Some(ref panel) = app.health_panel else { return };
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

    // Render the modal block
    let title = Line::from(vec![
        Span::styled(" Worktree Health ", Style::default().fg(GF_GREEN).bold()),
    ]);
    let block = Block::default()
        .title(title)
        .title_alignment(ratatui::layout::Alignment::Center)
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
            footer, Style::default().fg(Color::DarkGray),
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
        format!("  {} files over 1000 LOC ({} checked)", panel.god_files.len(), checked_count),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    // Visible items: modal height minus chrome (tab bar + header area + footer)
    let visible_items = (modal_h as usize).saturating_sub(12);
    let scroll = if panel.god_selected < panel.god_scroll {
        panel.god_selected
    } else if panel.god_selected >= panel.god_scroll + visible_items {
        panel.god_selected.saturating_sub(visible_items.saturating_sub(1))
    } else {
        panel.god_scroll
    };

    // Render visible entries
    for (i, entry) in panel.god_files.iter().enumerate().skip(scroll).take(visible_items) {
        let is_selected = i == panel.god_selected;
        let checkbox = if entry.checked { "[x] " } else { "[ ] " };
        let checkbox_color = if entry.checked { GF_GREEN } else { Color::DarkGray };
        let line_count_str = format!(" {} lines", entry.line_count);
        let path_max = inner_w.saturating_sub(checkbox.len() + line_count_str.len() + 1);
        let path_display = if entry.rel_path.len() > path_max {
            format!("…{}", &entry.rel_path[entry.rel_path.len().saturating_sub(path_max - 1)..])
        } else {
            entry.rel_path.clone()
        };
        let padding = inner_w.saturating_sub(checkbox.len() + path_display.len() + line_count_str.len());
        let pad_str = " ".repeat(padding);
        let (path_style, count_style) = if is_selected {
            (Style::default().fg(GF_GREEN).add_modifier(Modifier::BOLD), Style::default().fg(GF_GREEN))
        } else {
            (Style::default().fg(Color::White), Style::default().fg(Color::DarkGray))
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
            format!("  {}-{} of {}", pos, (pos + visible_items - 1).min(total), total),
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
    let score_color = if panel.doc_score >= 80.0 { GF_GREEN }
        else if panel.doc_score >= 50.0 { Color::Yellow }
        else { Color::Red };
    lines.push(Line::from(vec![
        Span::styled("  Overall Documentation Score: ", Style::default().fg(Color::White)),
        Span::styled(format!("{:.1}%", panel.doc_score), Style::default().fg(score_color).add_modifier(Modifier::BOLD)),
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

    // Column header
    lines.push(Line::from(Span::styled(
        "  Sorted by coverage (worst first)",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    // Visible items
    let visible_items = (modal_h as usize).saturating_sub(12);
    let scroll = if panel.doc_selected < panel.doc_scroll {
        panel.doc_selected
    } else if panel.doc_selected >= panel.doc_scroll + visible_items {
        panel.doc_selected.saturating_sub(visible_items.saturating_sub(1))
    } else {
        panel.doc_scroll
    };

    // Bar width for the visual coverage indicator
    let bar_width = 10usize;

    for (i, entry) in panel.doc_entries.iter().enumerate().skip(scroll).take(visible_items) {
        let is_selected = i == panel.doc_selected;

        // Coverage percentage string and color
        let pct_str = format!("{:5.1}%", entry.coverage_pct);
        let pct_color = if entry.coverage_pct >= 80.0 { GF_GREEN }
            else if entry.coverage_pct >= 50.0 { Color::Yellow }
            else { Color::Red };

        // Visual coverage bar: filled blocks + empty blocks
        let filled = (entry.coverage_pct / 100.0 * bar_width as f32).round() as usize;
        let bar: String = "█".repeat(filled) + &"░".repeat(bar_width - filled);

        // Items ratio
        let ratio = format!(" {}/{}", entry.documented_items, entry.total_items);

        // Path — truncate if needed, leave room for pct + bar + ratio
        let fixed_width = pct_str.len() + 1 + bar_width + ratio.len() + 2;
        let path_max = inner_w.saturating_sub(fixed_width + 1);
        let path_display = if entry.rel_path.len() > path_max {
            format!("…{}", &entry.rel_path[entry.rel_path.len().saturating_sub(path_max - 1)..])
        } else {
            entry.rel_path.clone()
        };
        let padding = inner_w.saturating_sub(path_display.len() + fixed_width);
        let pad_str = " ".repeat(padding);

        let path_style = if is_selected {
            Style::default().fg(GF_GREEN).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        lines.push(Line::from(vec![
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
            format!("  {}-{} of {}", pos, (pos + visible_items - 1).min(total), total),
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
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
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
        lines.push(Line::from(Span::styled("  Rust (.rs):", Style::default().fg(Color::DarkGray))));
        let cursor = if is_selected { "▸ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(format!("    {}{}", cursor, label), style),
        ]));
        lines.push(Line::from(vec![
            Span::styled(format!("      {}", hint), dim),
        ]));
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
        lines.push(Line::from(Span::styled("  Python (.py):", Style::default().fg(Color::DarkGray))));
        let cursor = if is_selected { "▸ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(format!("    {}{}", cursor, label), style),
        ]));
        lines.push(Line::from(vec![
            Span::styled(format!("      {}", hint), dim),
        ]));
        lines.push(Line::from(""));
    }

    // Instruction hint
    lines.push(Line::from(Span::styled(
        "  Space to toggle  ·  Enter to confirm  ·  Esc to cancel",
        Style::default().fg(Color::DarkGray),
    )));
}
