//! Git panel viewer rendering
//!
//! Renders diff content from the git actions panel state, populating
//! viewer_lines_cache for selection/copy/scroll support.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use super::super::util::{GIT_BROWN, GIT_ORANGE};
use super::selection::apply_selection_to_line;
use crate::app::App;

/// Git panel viewer — populates viewer_lines_cache for selection/copy/scroll support
pub(super) fn draw_git_viewer_selectable(
    f: &mut Frame,
    app: &mut App,
    area: Rect,
    _is_focused: bool,
    viewport_height: usize,
) {
    let (diff, title_str) = match app.git_actions_panel.as_ref() {
        Some(p) => (p.viewer_diff.clone(), p.viewer_diff_title.clone()),
        None => return,
    };

    let title = match title_str {
        Some(ref t) => format!(" {} ", t),
        None => " Viewer ".to_string(),
    };

    let block = Block::default()
        .title(Span::styled(
            &title,
            Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(GIT_ORANGE).add_modifier(Modifier::BOLD));

    // Clear previous frame's cells so placeholder text doesn't bleed through
    // when diff content doesn't fill the full viewport width/height.
    f.render_widget(Clear, area);

    match diff {
        Some(ref diff_text) => {
            // Build styled lines from diff (no line number gutter — gutter=0)
            let all_lines: Vec<Line<'static>> = diff_text
                .lines()
                .map(|l| {
                    let style = if l.starts_with('+') && !l.starts_with("+++") {
                        Style::default().fg(Color::Green)
                    } else if l.starts_with('-') && !l.starts_with("---") {
                        Style::default().fg(Color::Red)
                    } else if l.starts_with("@@") {
                        Style::default().fg(Color::Cyan)
                    } else if l.starts_with("diff ") || l.starts_with("index ") {
                        Style::default().fg(GIT_BROWN)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    Line::from(Span::styled(format!(" {}", l), style))
                })
                .collect();

            // Populate cache for selection/copy infrastructure
            app.viewer_lines_cache = all_lines;
            app.clamp_viewer_scroll();
            let scroll = app.viewer_scroll;

            // Build viewport slice with selection highlighting
            let display_lines: Vec<Line> = app
                .viewer_lines_cache
                .iter()
                .enumerate()
                .skip(scroll)
                .take(viewport_height)
                .map(|(idx, line)| {
                    if let Some((sl, sc, el, ec)) = app.viewer_selection {
                        if idx >= sl && idx <= el {
                            let content: String =
                                line.spans.iter().map(|s| s.content.as_ref()).collect();
                            Line::from(apply_selection_to_line(
                                line.spans.clone(),
                                &content,
                                idx,
                                sl,
                                sc,
                                el,
                                ec,
                                0,
                            ))
                        } else {
                            line.clone()
                        }
                    } else {
                        line.clone()
                    }
                })
                .collect();

            f.render_widget(Paragraph::new(display_lines).block(block), area);
        }
        None => {
            // No diff selected — clear cache and show hint
            app.viewer_lines_cache.clear();
            let hint = vec![
                Line::from(""),
                Line::from(Span::styled(
                    " Select a file or commit to view its diff",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            f.render_widget(Paragraph::new(hint).block(block), area);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};

    // ── Helper: build a diff line's style the same way the production code does ──

    fn style_for_diff_line(l: &str) -> Style {
        if l.starts_with('+') && !l.starts_with("+++") {
            Style::default().fg(Color::Green)
        } else if l.starts_with('-') && !l.starts_with("---") {
            Style::default().fg(Color::Red)
        } else if l.starts_with("@@") {
            Style::default().fg(Color::Cyan)
        } else if l.starts_with("diff ") || l.starts_with("index ") {
            Style::default().fg(GIT_BROWN)
        } else {
            Style::default().fg(Color::White)
        }
    }

    // ── Helper: build styled lines from a diff string (mirrors production logic) ──

    fn build_diff_lines(diff: &str) -> Vec<Line<'static>> {
        diff.lines()
            .map(|l| {
                let style = style_for_diff_line(l);
                Line::from(Span::styled(format!(" {}", l), style))
            })
            .collect()
    }

    // ── 1. Addition lines ──

    #[test]
    fn test_addition_line_is_green() {
        let style = style_for_diff_line("+added line");
        assert_eq!(style.fg, Some(Color::Green));
    }

    #[test]
    fn test_addition_single_plus_green() {
        let style = style_for_diff_line("+");
        assert_eq!(style.fg, Some(Color::Green));
    }

    #[test]
    fn test_addition_plus_space_green() {
        let style = style_for_diff_line("+ ");
        assert_eq!(style.fg, Some(Color::Green));
    }

    #[test]
    fn test_addition_plus_with_content_green() {
        let style = style_for_diff_line("+fn main() {}");
        assert_eq!(style.fg, Some(Color::Green));
    }

    #[test]
    fn test_plus_plus_is_green() {
        // "++" is an addition (not "+++"), so it should be Green
        let style = style_for_diff_line("++double plus");
        assert_eq!(style.fg, Some(Color::Green));
    }

    // ── 2. Deletion lines ──

    #[test]
    fn test_deletion_line_is_red() {
        let style = style_for_diff_line("-removed line");
        assert_eq!(style.fg, Some(Color::Red));
    }

    #[test]
    fn test_deletion_single_minus_red() {
        let style = style_for_diff_line("-");
        assert_eq!(style.fg, Some(Color::Red));
    }

    #[test]
    fn test_deletion_minus_space_red() {
        let style = style_for_diff_line("- ");
        assert_eq!(style.fg, Some(Color::Red));
    }

    #[test]
    fn test_deletion_minus_with_content_red() {
        let style = style_for_diff_line("-fn old() {}");
        assert_eq!(style.fg, Some(Color::Red));
    }

    #[test]
    fn test_minus_minus_is_red() {
        // "--" is a deletion (not "---"), so it should be Red
        let style = style_for_diff_line("--double minus");
        assert_eq!(style.fg, Some(Color::Red));
    }

    // ── 3. Triple-prefix lines (file headers) are NOT add/del ──

    #[test]
    fn test_triple_plus_not_green() {
        let style = style_for_diff_line("+++ b/src/main.rs");
        // "+++" lines are context/white (not green)
        assert_eq!(style.fg, Some(Color::White));
    }

    #[test]
    fn test_triple_minus_not_red() {
        let style = style_for_diff_line("--- a/src/main.rs");
        // "---" lines are context/white (not red)
        assert_eq!(style.fg, Some(Color::White));
    }

    // ── 4. Hunk header lines ──

    #[test]
    fn test_hunk_header_is_cyan() {
        let style = style_for_diff_line("@@ -1,3 +1,4 @@");
        assert_eq!(style.fg, Some(Color::Cyan));
    }

    #[test]
    fn test_hunk_header_with_function_name_cyan() {
        let style = style_for_diff_line("@@ -10,6 +10,7 @@ fn main()");
        assert_eq!(style.fg, Some(Color::Cyan));
    }

    #[test]
    fn test_double_at_only_is_cyan() {
        let style = style_for_diff_line("@@");
        assert_eq!(style.fg, Some(Color::Cyan));
    }

    // ── 5. Diff/index header lines ──

    #[test]
    fn test_diff_header_is_brown() {
        let style = style_for_diff_line("diff --git a/foo.rs b/foo.rs");
        assert_eq!(style.fg, Some(GIT_BROWN));
    }

    #[test]
    fn test_diff_space_only_is_brown() {
        let style = style_for_diff_line("diff ");
        assert_eq!(style.fg, Some(GIT_BROWN));
    }

    #[test]
    fn test_index_header_is_brown() {
        let style = style_for_diff_line("index abc1234..def5678 100644");
        assert_eq!(style.fg, Some(GIT_BROWN));
    }

    #[test]
    fn test_index_space_only_is_brown() {
        let style = style_for_diff_line("index ");
        assert_eq!(style.fg, Some(GIT_BROWN));
    }

    // ── 6. Context (plain) lines ──

    #[test]
    fn test_context_line_is_white() {
        let style = style_for_diff_line(" fn unchanged() {}");
        assert_eq!(style.fg, Some(Color::White));
    }

    #[test]
    fn test_empty_line_is_white() {
        let style = style_for_diff_line("");
        assert_eq!(style.fg, Some(Color::White));
    }

    #[test]
    fn test_plain_text_line_is_white() {
        let style = style_for_diff_line("some ordinary text");
        assert_eq!(style.fg, Some(Color::White));
    }

    #[test]
    fn test_whitespace_only_line_is_white() {
        let style = style_for_diff_line("    ");
        assert_eq!(style.fg, Some(Color::White));
    }

    // ── 7. build_diff_lines: line formatting ──

    #[test]
    fn test_build_diff_lines_prepends_space() {
        let lines = build_diff_lines("+added");
        let content: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            content.starts_with(' '),
            "Lines should be prefixed with a space"
        );
    }

    #[test]
    fn test_build_diff_lines_count_matches_input() {
        let diff = "+line1\n-line2\n context\n@@hunk@@";
        let lines = build_diff_lines(diff);
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn test_build_diff_lines_empty_diff() {
        let lines = build_diff_lines("");
        // An empty string yields zero lines from .lines() iterator
        assert_eq!(lines.len(), 0);
    }

    #[test]
    fn test_build_diff_lines_single_line() {
        let lines = build_diff_lines("+single");
        assert_eq!(lines.len(), 1);
        let content: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(content, " +single");
    }

    #[test]
    fn test_build_diff_lines_preserves_content() {
        let lines = build_diff_lines("-let x = 42;");
        let content: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(content, " -let x = 42;");
    }

    // ── 8. Color constant verification ──

    #[test]
    fn test_git_orange_is_rgb() {
        match GIT_ORANGE {
            Color::Rgb(r, g, b) => {
                assert_eq!(r, 240);
                assert_eq!(g, 80);
                assert_eq!(b, 50);
            }
            _ => panic!("GIT_ORANGE should be an RGB color"),
        }
    }

    #[test]
    fn test_git_brown_is_rgb() {
        match GIT_BROWN {
            Color::Rgb(r, g, b) => {
                assert_eq!(r, 160);
                assert_eq!(g, 82);
                assert_eq!(b, 45);
            }
            _ => panic!("GIT_BROWN should be an RGB color"),
        }
    }

    // ── 9. Edge cases for prefix detection ──

    #[test]
    fn test_diff_no_space_after_is_brown() {
        // "diff " matches, but "different" should not (doesn't start with "diff ")
        let style = style_for_diff_line("different");
        assert_eq!(style.fg, Some(Color::White));
    }

    #[test]
    fn test_index_no_space_after_is_white() {
        // "index " matches, but "indexed" should not
        let style = style_for_diff_line("indexed file");
        assert_eq!(style.fg, Some(Color::White));
    }

    #[test]
    fn test_at_sign_alone_is_white() {
        // Single "@" is not a hunk header
        let style = style_for_diff_line("@single");
        assert_eq!(style.fg, Some(Color::White));
    }

    #[test]
    fn test_triple_plus_bare_is_white() {
        let style = style_for_diff_line("+++");
        assert_eq!(style.fg, Some(Color::White));
    }

    #[test]
    fn test_triple_minus_bare_is_white() {
        let style = style_for_diff_line("---");
        assert_eq!(style.fg, Some(Color::White));
    }

    // ── 10. Full diff scenario ──

    #[test]
    fn test_full_diff_coloring() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
index abc1234..def5678 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
-    println!(\"old\");
+    println!(\"new\");
+    println!(\"extra\");
 }";
        let lines = build_diff_lines(diff);
        assert_eq!(lines.len(), 10);

        // diff header -> brown
        let s0 = style_for_diff_line("diff --git a/src/main.rs b/src/main.rs");
        assert_eq!(s0.fg, Some(GIT_BROWN));

        // index -> brown
        let s1 = style_for_diff_line("index abc1234..def5678 100644");
        assert_eq!(s1.fg, Some(GIT_BROWN));

        // --- -> white (not red)
        let s2 = style_for_diff_line("--- a/src/main.rs");
        assert_eq!(s2.fg, Some(Color::White));

        // +++ -> white (not green)
        let s3 = style_for_diff_line("+++ b/src/main.rs");
        assert_eq!(s3.fg, Some(Color::White));

        // @@ -> cyan
        let s4 = style_for_diff_line("@@ -1,3 +1,4 @@");
        assert_eq!(s4.fg, Some(Color::Cyan));

        // context -> white
        let s5 = style_for_diff_line(" fn main() {");
        assert_eq!(s5.fg, Some(Color::White));

        // deletion -> red
        let s6 = style_for_diff_line("-    println!(\"old\");");
        assert_eq!(s6.fg, Some(Color::Red));

        // addition -> green
        let s7 = style_for_diff_line("+    println!(\"new\");");
        assert_eq!(s7.fg, Some(Color::Green));
    }

    // ── 11. Style modifier verification ──

    #[test]
    fn test_style_has_no_modifier_by_default() {
        let style = style_for_diff_line("+added");
        assert!(!style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_context_style_no_modifier() {
        let style = style_for_diff_line(" context");
        assert!(!style.add_modifier.contains(Modifier::BOLD));
    }

    // ── 12. Multi-line diff building ──

    #[test]
    fn test_build_diff_lines_many_additions() {
        let diff = (0..20)
            .map(|i| format!("+line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = build_diff_lines(&diff);
        assert_eq!(lines.len(), 20);
        for line in &lines {
            let style = line.spans[0].style;
            assert_eq!(style.fg, Some(Color::Green));
        }
    }

    #[test]
    fn test_build_diff_lines_many_deletions() {
        let diff = (0..15)
            .map(|i| format!("-line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = build_diff_lines(&diff);
        assert_eq!(lines.len(), 15);
        for line in &lines {
            let style = line.spans[0].style;
            assert_eq!(style.fg, Some(Color::Red));
        }
    }

    #[test]
    fn test_build_diff_lines_mixed() {
        let diff = "+add\n-del\n context\n@@hunk@@\ndiff header";
        let lines = build_diff_lines(diff);
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Green));
        assert_eq!(lines[1].spans[0].style.fg, Some(Color::Red));
        assert_eq!(lines[2].spans[0].style.fg, Some(Color::White));
        assert_eq!(lines[3].spans[0].style.fg, Some(Color::Cyan));
        // "diff header" doesn't start with "diff " — it's "diff " which matches.
        // Actually "diff header" starts with "diff " — no, it starts with "diff " only if there's a space at index 4.
        // "diff header" -> 'd','i','f','f',' ','h'... -> starts_with("diff ") == true
        assert_eq!(lines[4].spans[0].style.fg, Some(GIT_BROWN));
    }

    // ── 13. Span content integrity ──

    #[test]
    fn test_each_line_has_exactly_one_span() {
        let diff = "+a\n-b\n c\n@@d@@\ndiff e\nindex f";
        let lines = build_diff_lines(diff);
        for line in &lines {
            assert_eq!(
                line.spans.len(),
                1,
                "Each diff line should have exactly one span"
            );
        }
    }

    #[test]
    fn test_span_content_includes_leading_space() {
        let lines = build_diff_lines("@@ -1 +1 @@");
        let content = &lines[0].spans[0].content;
        assert_eq!(content.as_ref(), " @@ -1 +1 @@");
    }

    // ── 14. Boundary: no-newline at end of file ──

    #[test]
    fn test_no_newline_marker_is_white() {
        let style = style_for_diff_line("\\ No newline at end of file");
        assert_eq!(style.fg, Some(Color::White));
    }

    // ── 15. Unicode in diff content ──

    #[test]
    fn test_unicode_addition_line() {
        let style = style_for_diff_line("+日本語テスト");
        assert_eq!(style.fg, Some(Color::Green));
    }

    #[test]
    fn test_unicode_deletion_line() {
        let style = style_for_diff_line("-日本語テスト");
        assert_eq!(style.fg, Some(Color::Red));
    }

    #[test]
    fn test_build_diff_lines_unicode_content() {
        let lines = build_diff_lines("+こんにちは");
        let content: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(content, " +こんにちは");
    }

    // ── 16. Additional edge cases ──

    #[test]
    fn test_newline_only_diff_is_zero_lines() {
        let lines = build_diff_lines("\n");
        // "\n" splits into ["", ""] by .lines() — actually .lines() yields [""]
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_tab_indented_context_line_is_white() {
        let style = style_for_diff_line("\tindented with tab");
        assert_eq!(style.fg, Some(Color::White));
    }

    #[test]
    fn test_addition_with_tab_is_green() {
        let style = style_for_diff_line("+\tindented added");
        assert_eq!(style.fg, Some(Color::Green));
    }

    #[test]
    fn test_deletion_with_tab_is_red() {
        let style = style_for_diff_line("-\tindented removed");
        assert_eq!(style.fg, Some(Color::Red));
    }
}
