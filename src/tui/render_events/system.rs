//! System event rendering — session init, hooks, and commands

use ratatui::{
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::tui::render_wrap::wrap_text;
use crate::tui::util::AZURE;

pub(super) fn render_init(lines: &mut Vec<Line<'static>>, model: &str, cwd: &str) {
    lines.push(Line::from(""));
    lines.push(
        Line::from(vec![Span::styled(
            " Session Started ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )])
        .alignment(Alignment::Center),
    );
    lines.push(
        Line::from(vec![
            Span::styled("Model: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                model.to_string(),
                Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
            ),
        ])
        .alignment(Alignment::Center),
    );
    lines.push(
        Line::from(vec![
            Span::styled("Path: ", Style::default().fg(Color::DarkGray)),
            Span::styled(cwd.to_string(), Style::default().fg(Color::White)),
        ])
        .alignment(Alignment::Center),
    );
    lines.push(Line::from(""));
}

pub(super) fn render_hook(
    lines: &mut Vec<Line<'static>>,
    name: &str,
    output: &str,
    hook_max: usize,
) {
    let hook_max = hook_max + 10;
    if !output.trim().is_empty() {
        let prefix_len = 2 + name.len() + 2;
        let output_max = hook_max.saturating_sub(prefix_len);
        let first_line = output.lines().next().unwrap_or("");
        for (i, wrapped) in wrap_text(first_line, output_max).into_iter().enumerate() {
            if i == 0 {
                lines.push(Line::from(vec![
                    Span::styled("› ", Style::default().fg(Color::DarkGray)),
                    Span::styled(name.to_string(), Style::default().fg(Color::DarkGray)),
                    Span::styled(": ", Style::default().fg(Color::DarkGray)),
                    Span::styled(wrapped, Style::default().fg(Color::DarkGray)),
                ]));
            } else {
                let indent = " ".repeat(prefix_len);
                lines.push(Line::from(vec![
                    Span::styled(indent, Style::default()),
                    Span::styled(wrapped, Style::default().fg(Color::DarkGray)),
                ]));
            }
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled("› ", Style::default().fg(Color::DarkGray)),
            Span::styled(name.to_string(), Style::default().fg(Color::DarkGray)),
        ]));
    }
}

pub(super) fn render_command(lines: &mut Vec<Line<'static>>, name: &str) {
    let cmd_style = Style::default()
        .fg(Color::Magenta)
        .add_modifier(Modifier::BOLD);
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("/ ", cmd_style),
        Span::styled(name.to_string(), cmd_style),
    ]));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_to_text(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    // ── render_init tests ───────────────────────────────────────────────

    /// render_init produces expected structure with model and cwd.
    #[test]
    fn test_render_init_basic() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_init(&mut lines, "claude-opus-4-20250514", "/home/user/project");
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Session Started")));
        assert!(text.iter().any(|l| l.contains("claude-opus-4-20250514")));
        assert!(text.iter().any(|l| l.contains("/home/user/project")));
    }

    /// render_init with empty model string.
    #[test]
    fn test_render_init_empty_model() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_init(&mut lines, "", "/path");
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Model:")));
    }

    /// render_init with empty cwd.
    #[test]
    fn test_render_init_empty_cwd() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_init(&mut lines, "model", "");
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Path:")));
    }

    /// render_init produces exactly 5 lines (empty, header, model, path, empty).
    #[test]
    fn test_render_init_line_count() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_init(&mut lines, "model", "/cwd");
        assert_eq!(lines.len(), 5);
    }

    /// render_init with unicode model name.
    #[test]
    fn test_render_init_unicode_model() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_init(&mut lines, "テスト-model", "/path");
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("テスト-model")));
    }

    // ── render_hook tests ───────────────────────────────────────────────

    /// Hook with output renders name and output.
    #[test]
    fn test_render_hook_with_output() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_hook(&mut lines, "pre-commit", "All checks passed", 80);
        let text = lines_to_text(&lines);
        assert!(text
            .iter()
            .any(|l| l.contains("pre-commit") && l.contains("All checks passed")));
    }

    /// Hook with empty output renders only name.
    #[test]
    fn test_render_hook_empty_output() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_hook(&mut lines, "post-checkout", "", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("post-checkout")));
    }

    /// Hook with whitespace-only output renders only name.
    #[test]
    fn test_render_hook_whitespace_output() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_hook(&mut lines, "hook-name", "   \t  ", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("hook-name")));
        // Whitespace-only is trimmed, so output is treated as empty
    }

    /// Hook with multiline output only shows first line.
    #[test]
    fn test_render_hook_multiline_output() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_hook(&mut lines, "lint", "Line 1\nLine 2\nLine 3", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Line 1")));
    }

    /// Hook with narrow width wraps output.
    #[test]
    fn test_render_hook_narrow_wraps() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_hook(&mut lines, "h", "A long output string that should wrap", 20);
        assert!(lines.len() >= 1);
    }

    /// Hook name contains special chars.
    #[test]
    fn test_render_hook_special_name() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_hook(&mut lines, "pre-push/main", "ok", 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("pre-push/main")));
    }

    // ── render_command tests ────────────────────────────────────────────

    /// Command renders with / prefix.
    #[test]
    fn test_render_command_basic() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_command(&mut lines, "compact");
        let text = lines_to_text(&lines);
        assert!(text
            .iter()
            .any(|l| l.contains("/ ") && l.contains("compact")));
    }

    /// Command produces exactly 2 lines (empty + command).
    #[test]
    fn test_render_command_line_count() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_command(&mut lines, "test");
        assert_eq!(lines.len(), 2);
    }

    /// Command with empty name.
    #[test]
    fn test_render_command_empty_name() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_command(&mut lines, "");
        assert_eq!(lines.len(), 2);
    }
}
