//! Interactive dialog boxes — plan approval and user question prompts

use ratatui::{
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::tui::render_wrap::wrap_text;
use crate::tui::util::AZURE;

/// Render plan approval prompt when awaiting user response to ExitPlanMode
pub(super) fn render_plan_approval(lines: &mut Vec<Line<'static>>, width: usize) {
    let color = Color::Yellow;
    let box_width = 50.min(width.saturating_sub(4));

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Top border
    lines.push(
        Line::from(vec![Span::styled(
            format!("┌{}┐", "─".repeat(box_width.saturating_sub(2))),
            Style::default().fg(color),
        )])
        .alignment(Alignment::Center),
    );

    // Header
    let header = " ⏳ Awaiting Plan Approval ";
    let header_pad = box_width.saturating_sub(header.chars().count() + 2);
    lines.push(
        Line::from(vec![
            Span::styled("│", Style::default().fg(color)),
            Span::styled(
                header,
                Style::default()
                    .fg(Color::Black)
                    .bg(color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ".repeat(header_pad), Style::default().bg(color)),
            Span::styled("│", Style::default().fg(color)),
        ])
        .alignment(Alignment::Center),
    );

    // Separator
    lines.push(
        Line::from(vec![Span::styled(
            format!("├{}┤", "─".repeat(box_width.saturating_sub(2))),
            Style::default().fg(color),
        )])
        .alignment(Alignment::Center),
    );

    // Options
    let options = [
        "1. Yes, clear context and bypass permissions",
        "2. Yes, and manually approve edits",
        "3. Yes, and bypass permissions",
        "4. Yes, manually approve edits",
        "5. Type to tell Claude what to change",
    ];

    for opt in &options {
        let pad = box_width.saturating_sub(opt.chars().count() + 4);
        lines.push(
            Line::from(vec![
                Span::styled("│ ", Style::default().fg(color)),
                Span::styled(opt.to_string(), Style::default().fg(Color::White)),
                Span::styled(format!("{} │", " ".repeat(pad)), Style::default().fg(color)),
            ])
            .alignment(Alignment::Center),
        );
    }

    // Bottom border
    lines.push(
        Line::from(vec![Span::styled(
            format!("└{}┘", "─".repeat(box_width.saturating_sub(2))),
            Style::default().fg(color),
        )])
        .alignment(Alignment::Center),
    );
}

/// Render AskUserQuestion tool call as a numbered options box.
/// Input structure: { "questions": [{ "question": "...", "header": "...",
///   "options": [{ "label": "...", "description": "..." }], "multiSelect": bool }] }
pub(super) fn render_ask_user_question(
    lines: &mut Vec<Line<'static>>,
    input: &serde_json::Value,
    width: usize,
) {
    let color = Color::Magenta;
    let Some(questions) = input.get("questions").and_then(|v| v.as_array()) else {
        return;
    };

    for q in questions {
        let question = q.get("question").and_then(|v| v.as_str()).unwrap_or("?");
        let options = q.get("options").and_then(|v| v.as_array());
        let multi = q
            .get("multiSelect")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Box width: fit content or cap at panel width
        let box_width = 60.min(width.saturating_sub(4));

        lines.push(Line::from(""));
        lines.push(Line::from(""));

        // Top border
        lines.push(
            Line::from(vec![Span::styled(
                format!("┌{}┐", "─".repeat(box_width.saturating_sub(2))),
                Style::default().fg(color),
            )])
            .alignment(Alignment::Center),
        );

        // Header with question text (wrap if needed)
        let header_icon = if multi { "☑ " } else { "❓ " };
        let header_max = box_width.saturating_sub(4 + header_icon.len());
        for (i, chunk) in wrap_text(question, header_max).into_iter().enumerate() {
            let prefix = if i == 0 { header_icon } else { "   " };
            let text = format!("{}{}", prefix, chunk);
            let pad = box_width.saturating_sub(text.chars().count() + 2);
            lines.push(
                Line::from(vec![
                    Span::styled("│ ", Style::default().fg(color)),
                    Span::styled(
                        text,
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("{} │", " ".repeat(pad)), Style::default().fg(color)),
                ])
                .alignment(Alignment::Center),
            );
        }

        // Separator
        lines.push(
            Line::from(vec![Span::styled(
                format!("├{}┤", "─".repeat(box_width.saturating_sub(2))),
                Style::default().fg(color),
            )])
            .alignment(Alignment::Center),
        );

        // Numbered options
        if let Some(opts) = options {
            for (idx, opt) in opts.iter().enumerate() {
                let label = opt.get("label").and_then(|v| v.as_str()).unwrap_or("?");
                let desc = opt.get("description").and_then(|v| v.as_str());
                // Option label line
                let opt_text = format!("{}. {}", idx + 1, label);
                let pad = box_width.saturating_sub(opt_text.chars().count() + 4);
                lines.push(
                    Line::from(vec![
                        Span::styled("│ ", Style::default().fg(color)),
                        Span::styled(opt_text, Style::default().fg(AZURE)),
                        Span::styled(format!("{} │", " ".repeat(pad)), Style::default().fg(color)),
                    ])
                    .alignment(Alignment::Center),
                );
                // Option description (dimmer, indented)
                if let Some(d) = desc {
                    let indent = "   ";
                    let desc_max = box_width.saturating_sub(4 + indent.len());
                    for chunk in wrap_text(d, desc_max) {
                        let text = format!("{}{}", indent, chunk);
                        let pad = box_width.saturating_sub(text.chars().count() + 4);
                        lines.push(
                            Line::from(vec![
                                Span::styled("│ ", Style::default().fg(color)),
                                Span::styled(text, Style::default().fg(Color::DarkGray)),
                                Span::styled(
                                    format!("{} │", " ".repeat(pad)),
                                    Style::default().fg(color),
                                ),
                            ])
                            .alignment(Alignment::Center),
                        );
                    }
                }
            }
        }

        // "Other" note
        let other_text = format!(
            "{}. Other (type your answer)",
            options.map(|o| o.len() + 1).unwrap_or(1)
        );
        let pad = box_width.saturating_sub(other_text.chars().count() + 4);
        lines.push(
            Line::from(vec![
                Span::styled("│ ", Style::default().fg(color)),
                Span::styled(other_text, Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{} │", " ".repeat(pad)), Style::default().fg(color)),
            ])
            .alignment(Alignment::Center),
        );

        // Bottom border
        lines.push(
            Line::from(vec![Span::styled(
                format!("└{}┘", "─".repeat(box_width.saturating_sub(2))),
                Style::default().fg(color),
            )])
            .alignment(Alignment::Center),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

    // ── render_ask_user_question tests ───────────────────────────────────

    /// Verifies render_ask_user_question produces visible lines with
    /// box borders, question text, numbered options, and an "Other" entry.
    /// This test exists because the rendering is the user-facing presentation
    /// of AskUserQuestion — if box drawing or numbering is wrong, the user
    /// can't correctly select options.
    #[test]
    fn test_render_ask_user_question_basic_structure() {
        let input = json!({
            "questions": [{
                "question": "Which approach?",
                "header": "Approach",
                "options": [
                    {"label": "Option A", "description": "First choice"},
                    {"label": "Option B", "description": "Second choice"}
                ],
                "multiSelect": false
            }]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);

        // Flatten all span content into strings for assertion
        let text: Vec<String> = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect();

        // Box borders present
        assert!(
            text.iter().any(|l| l.contains('┌') && l.contains('┐')),
            "Missing top border"
        );
        assert!(
            text.iter().any(|l| l.contains('└') && l.contains('┘')),
            "Missing bottom border"
        );
        assert!(
            text.iter().any(|l| l.contains('├') && l.contains('┤')),
            "Missing separator"
        );

        // Question text visible
        assert!(
            text.iter().any(|l| l.contains("Which approach?")),
            "Missing question text"
        );

        // Numbered options
        assert!(
            text.iter().any(|l| l.contains("1. Option A")),
            "Missing option 1"
        );
        assert!(
            text.iter().any(|l| l.contains("2. Option B")),
            "Missing option 2"
        );

        // Descriptions visible
        assert!(
            text.iter().any(|l| l.contains("First choice")),
            "Missing option 1 description"
        );
        assert!(
            text.iter().any(|l| l.contains("Second choice")),
            "Missing option 2 description"
        );

        // Other option present
        assert!(
            text.iter().any(|l| l.contains("3. Other")),
            "Missing Other option"
        );
    }

    /// Verifies multi-select annotation appears in the header.
    #[test]
    fn test_render_ask_user_question_multi_select_icon() {
        let input = json!({
            "questions": [{
                "question": "Select features",
                "options": [{"label": "A", "description": ""}],
                "multiSelect": true
            }]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);
        let text: Vec<String> = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect();
        // Multi-select uses ☑ icon instead of ❓
        assert!(
            text.iter().any(|l| l.contains('☑')),
            "Multi-select should show checkbox icon"
        );
    }

    /// Verifies empty questions array produces no output (no panic).
    #[test]
    fn test_render_ask_user_question_empty() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &json!({}), 80);
        assert!(lines.is_empty(), "Empty input should produce no lines");
        render_ask_user_question(&mut lines, &json!({"questions": []}), 80);
        assert!(
            lines.is_empty(),
            "Empty questions array should produce no lines"
        );
    }

    /// Verifies narrow width doesn't panic or produce garbled output.
    /// This tests the wrapping logic with constrained box width.
    #[test]
    fn test_render_ask_user_question_narrow_width() {
        let input = json!({
            "questions": [{
                "question": "A very long question that should wrap within the narrow box width to test text wrapping behavior",
                "options": [{"label": "Short", "description": "Also a description"}],
                "multiSelect": false
            }]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        // Minimum usable width (box_width = 60.min(width-4) = 16)
        render_ask_user_question(&mut lines, &input, 20);
        assert!(
            !lines.is_empty(),
            "Should produce output even at narrow width"
        );
    }

    /// Multiple questions each get their own box.
    #[test]
    fn test_render_ask_multi_questions_separate_boxes() {
        let input = json!({
            "questions": [
                {"question": "First?", "options": [{"label": "A", "description": ""}], "multiSelect": false},
                {"question": "Second?", "options": [{"label": "B", "description": ""}], "multiSelect": false}
            ]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);
        let text = lines_to_text(&lines);
        // Should have two top borders (one per question)
        let top_borders = text
            .iter()
            .filter(|l| l.contains('┌') && l.contains('┐'))
            .count();
        assert_eq!(
            top_borders, 2,
            "Each question should have its own top border"
        );
    }

    /// Question with no description shows only label.
    #[test]
    fn test_render_ask_no_description() {
        let input = json!({
            "questions": [{
                "question": "Pick?",
                "options": [{"label": "Yes"}],
                "multiSelect": false
            }]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("1. Yes")));
    }

    /// Option with empty description is handled.
    #[test]
    fn test_render_ask_empty_description() {
        let input = json!({
            "questions": [{
                "question": "Q?",
                "options": [{"label": "Opt", "description": ""}],
                "multiSelect": false
            }]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("1. Opt")));
    }

    /// Questions with null options field produces no numbered options.
    #[test]
    fn test_render_ask_null_options() {
        let input = json!({
            "questions": [{"question": "Free form?", "options": null, "multiSelect": false}]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Free form?")));
        // "Other" should still be present with number 1
        assert!(text.iter().any(|l| l.contains("1. Other")));
    }

    /// "Other" option number is correct with 5 options.
    #[test]
    fn test_render_ask_other_number_five_options() {
        let options: Vec<serde_json::Value> = (1..=5)
            .map(|i| json!({"label": format!("Opt{}", i), "description": ""}))
            .collect();
        let input = json!({
            "questions": [{"question": "Q?", "options": options, "multiSelect": false}]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("6. Other")));
    }

    /// Very wide width doesn't cause issues.
    #[test]
    fn test_render_ask_wide_width() {
        let input = json!({
            "questions": [{"question": "Q?", "options": [{"label": "A", "description": "desc"}], "multiSelect": false}]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 500);
        assert!(!lines.is_empty());
    }

    /// Width of 4 (minimum before box_width = 0).
    #[test]
    fn test_render_ask_minimum_width() {
        let input = json!({
            "questions": [{"question": "Q?", "options": [], "multiSelect": false}]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 4);
        assert!(!lines.is_empty());
    }

    /// Width of 0 should not panic.
    #[test]
    fn test_render_ask_zero_width() {
        let input = json!({
            "questions": [{"question": "Q?", "options": [], "multiSelect": false}]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 0);
        assert!(!lines.is_empty());
    }

    /// Unicode in option labels.
    #[test]
    fn test_render_ask_unicode_labels() {
        let input = json!({
            "questions": [{
                "question": "言語?",
                "options": [{"label": "日本語", "description": "Japanese"}, {"label": "中文", "description": "Chinese"}],
                "multiSelect": false
            }]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("1. 日本語")));
        assert!(text.iter().any(|l| l.contains("2. 中文")));
    }

    /// Long description wraps within box.
    #[test]
    fn test_render_ask_long_description_wraps() {
        let long_desc = "This is a very long description that should definitely wrap across multiple lines within the constrained box width boundary.";
        let input = json!({
            "questions": [{
                "question": "Q?",
                "options": [{"label": "A", "description": long_desc}],
                "multiSelect": false
            }]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 60);
        let text = lines_to_text(&lines);
        // The description should be split across multiple lines
        let desc_lines: Vec<&String> = text
            .iter()
            .filter(|l| {
                l.contains("description") || l.contains("definitely") || l.contains("boundary")
            })
            .collect();
        assert!(!desc_lines.is_empty(), "Long description should be present");
    }

    /// Missing label falls back to "?".
    #[test]
    fn test_render_ask_missing_label() {
        let input = json!({
            "questions": [{
                "question": "Q?",
                "options": [{"description": "no label here"}],
                "multiSelect": false
            }]
        });
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_ask_user_question(&mut lines, &input, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("1. ?")));
    }

    // ── render_plan_approval tests ──────────────────────────────────────

    /// Plan approval renders all 5 options.
    #[test]
    fn test_render_plan_approval_all_options() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan_approval(&mut lines, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("1. Yes, clear context")));
        assert!(text.iter().any(|l| l.contains("2. Yes, and manually")));
        assert!(text.iter().any(|l| l.contains("3. Yes, and bypass")));
        assert!(text.iter().any(|l| l.contains("4. Yes, manually")));
        assert!(text.iter().any(|l| l.contains("5. Type to tell")));
    }

    /// Plan approval header present.
    #[test]
    fn test_render_plan_approval_header() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan_approval(&mut lines, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Awaiting Plan Approval")));
    }

    /// Plan approval has box borders.
    #[test]
    fn test_render_plan_approval_borders() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan_approval(&mut lines, 80);
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains('┌')));
        assert!(text.iter().any(|l| l.contains('└')));
    }

    /// Plan approval at narrow width doesn't panic.
    #[test]
    fn test_render_plan_approval_narrow() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan_approval(&mut lines, 10);
        assert!(!lines.is_empty());
    }

    /// Plan approval at zero width doesn't panic.
    #[test]
    fn test_render_plan_approval_zero_width() {
        let mut lines: Vec<Line<'static>> = Vec::new();
        render_plan_approval(&mut lines, 0);
        assert!(!lines.is_empty());
    }
}
