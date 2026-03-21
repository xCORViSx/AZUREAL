//! Help overlay with auto-sized multi-column layout from centralized keybindings

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::tui::keybindings;
use crate::tui::util::AZURE;

/// Draw help overlay with auto-sized columns from centralized keybindings
pub fn draw_help_overlay(f: &mut Frame, kbd_enhanced: bool, alt_enter_stolen: bool) {
    let area = f.area();
    let sections = keybindings::help_sections();

    // Each display row is either a single binding or two paired bindings merged.
    // Paired: "j/↓ down · k/↑ up" — both key+desc on one line separated by dim ·
    enum HelpRow {
        Single {
            keys: String,
            desc: &'static str,
        },
        Paired {
            keys1: String,
            desc1: &'static str,
            keys2: String,
            desc2: &'static str,
        },
    }

    // Build display rows per section, merging pair_with_next bindings
    let mut section_rows: Vec<(&str, Vec<HelpRow>)> = Vec::new();
    for section in &sections {
        let mut rows = Vec::new();
        let bindings = section.bindings;
        let mut i = 0;
        while i < bindings.len() {
            if bindings[i].pair_with_next && i + 1 < bindings.len() {
                rows.push(HelpRow::Paired {
                    keys1: bindings[i].display_keys_adaptive(kbd_enhanced, alt_enter_stolen),
                    desc1: bindings[i].description,
                    keys2: bindings[i + 1].display_keys_adaptive(kbd_enhanced, alt_enter_stolen),
                    desc2: bindings[i + 1].description,
                });
                i += 2;
            } else {
                rows.push(HelpRow::Single {
                    keys: bindings[i].display_keys_adaptive(kbd_enhanced, alt_enter_stolen),
                    desc: bindings[i].description,
                });
                i += 1;
            }
        }
        section_rows.push((section.title, rows));
    }

    // Max key width across all single + paired entries (for the first key column)
    let key_width = section_rows
        .iter()
        .flat_map(|(_, rows)| rows.iter())
        .map(|row| match row {
            HelpRow::Single { keys, .. } => keys.len(),
            HelpRow::Paired { keys1, keys2, .. } => keys1.len().max(keys2.len()),
        })
        .max()
        .unwrap_or(10)
        + 2;

    // Max single-entry desc width (used for column sizing)
    let desc_width = section_rows
        .iter()
        .flat_map(|(_, rows)| rows.iter())
        .map(|row| match row {
            HelpRow::Single { desc, .. } => desc.len(),
            // Paired rows: key1+desc1 + separator + key2+desc2 — we size off single rows
            HelpRow::Paired { desc1, desc2, .. } => desc1.len().max(desc2.len()),
        })
        .max()
        .unwrap_or(20);

    // Paired rows need extra space: key + desc + " · " + key + desc
    // Column width = max(single_width, paired_width)
    let single_width = key_width + 1 + desc_width + 2;
    let paired_width = key_width + 1 + desc_width + 3 + key_width + 1 + desc_width + 2;
    let col_width = single_width.max(paired_width);

    // Calculate how many columns fit (min 1, max 3)
    let available_width = area.width.saturating_sub(4) as usize;
    let num_cols = (available_width / col_width).clamp(1, 3);
    let actual_col_width = available_width / num_cols;

    // Distribute sections across columns to minimize max column height.
    // Try all possible partition points and pick the split with the smallest
    // tallest column — guarantees optimal packing for ≤3 columns.
    let section_heights: Vec<usize> = section_rows
        .iter()
        .map(|(_, rows)| rows.len() + 2)
        .collect();
    let n = section_heights.len();

    // Prefix sums for O(1) range height queries
    let mut prefix = vec![0usize; n + 1];
    for i in 0..n {
        prefix[i + 1] = prefix[i] + section_heights[i];
    }
    let range_height = |from: usize, to: usize| prefix[to] - prefix[from];

    // Find optimal partition: which sections go in which column
    let best_splits: Vec<usize> = if num_cols == 1 || n <= 1 {
        // Everything in one column
        vec![n]
    } else if num_cols == 2 {
        // Try all split points, pick the one minimizing max(col0, col1)
        let mut best = (usize::MAX, vec![n, n]);
        for s in 1..n {
            let max_h = range_height(0, s).max(range_height(s, n));
            if max_h < best.0 {
                best = (max_h, vec![s, n]);
            }
        }
        best.1
    } else {
        // 3 columns: try all pairs of split points
        let mut best = (usize::MAX, vec![n, n, n]);
        for s1 in 1..n {
            for s2 in (s1 + 1)..n {
                let max_h = range_height(0, s1)
                    .max(range_height(s1, s2))
                    .max(range_height(s2, n));
                if max_h < best.0 {
                    best = (max_h, vec![s1, s2, n]);
                }
            }
        }
        // Also try 2-column packing in case 3 columns wastes space
        for s in 1..n {
            let max_h = range_height(0, s).max(range_height(s, n));
            if max_h < best.0 {
                best = (max_h, vec![s, n]);
            }
        }
        best.1
    };

    let actual_num_cols = best_splits.len();
    let mut columns: Vec<Vec<Line>> = vec![Vec::new(); actual_num_cols];

    let dim_style = Style::default().fg(Color::DarkGray);

    let mut col_idx = 0;
    for (idx, (title, rows)) in section_rows.iter().enumerate() {
        // Advance to next column when we've passed the split point
        if col_idx < actual_num_cols - 1 && idx >= best_splits[col_idx] {
            col_idx += 1;
        }

        columns[col_idx].push(Line::from(vec![Span::styled(
            *title,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));

        for row in rows {
            let line = match row {
                HelpRow::Single { keys, desc } => {
                    let key_span = Span::styled(
                        format!("{:>width$}", keys, width = key_width),
                        Style::default().fg(AZURE),
                    );
                    let desc_span = Span::raw(format!(" {}", desc));
                    Line::from(vec![key_span, desc_span])
                }
                HelpRow::Paired {
                    keys1,
                    desc1,
                    keys2,
                    desc2,
                } => {
                    // "  keys1 desc1 · keys2 desc2"
                    let k1 = Span::styled(
                        format!("{:>width$}", keys1, width = key_width),
                        Style::default().fg(AZURE),
                    );
                    let d1 = Span::raw(format!(" {} ", desc1));
                    let sep = Span::styled("· ", dim_style);
                    let k2 = Span::styled(keys2.clone(), Style::default().fg(AZURE));
                    let d2 = Span::raw(format!(" {}", desc2));
                    Line::from(vec![k1, d1, sep, k2, d2])
                }
            };
            columns[col_idx].push(line);
        }

        columns[col_idx].push(Line::from(""));
    }

    // Calculate actual height needed (max column height + title + footer + borders)
    let max_col_height = columns.iter().map(|c| c.len()).max().unwrap_or(0);
    let help_height = (max_col_height as u16 + 4).min(area.height.saturating_sub(4));

    // Calculate actual width needed
    let help_width =
        ((actual_col_width * actual_num_cols) as u16 + 4).min(area.width.saturating_sub(4));

    let help_area = Rect {
        x: (area.width.saturating_sub(help_width)) / 2,
        y: (area.height.saturating_sub(help_height)) / 2,
        width: help_width,
        height: help_height,
    };

    // Clear background
    f.render_widget(Clear, help_area);

    // Create inner area for content
    let inner = Rect {
        x: help_area.x + 1,
        y: help_area.y + 1,
        width: help_area.width.saturating_sub(2),
        height: help_area.height.saturating_sub(2),
    };

    // Split into columns
    let col_constraints: Vec<Constraint> = (0..actual_num_cols)
        .map(|_| Constraint::Ratio(1, actual_num_cols as u32))
        .collect();
    let col_areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(col_constraints)
        .split(inner);

    // Render border
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(" Help (? to close) ")
        .border_style(Style::default().fg(AZURE))
        .style(Style::default().bg(Color::Reset));
    f.render_widget(block, help_area);

    // Render each column
    for (i, col_lines) in columns.iter().enumerate() {
        if i < col_areas.len() {
            let para = Paragraph::new(col_lines.clone());
            f.render_widget(para, col_areas[i]);
        }
    }
}
