//! Display event rendering for TUI
//!
//! Thin orchestrator that dispatches to specialized renderers:
//! - `bubbles`: User/assistant message bubbles and completion banners
//! - `system`: Session init, hooks, and commands
//! - `tool_call`: Tool invocation rendering with status indicators
//! - `dialogs`: Plan approval and user question prompts
//! - `plan`: Plan mode full-width display

mod bubbles;
mod dialogs;
mod plan;
mod system;
mod tool_call;

use ratatui::{
    layout::Alignment,
    style::{Color, Style},
    text::{Line, Span},
};
use std::collections::HashSet;

use super::render_markdown::render_assistant_text_with_paths_colored;
use super::render_tools::render_tool_result;
use crate::events::DisplayEvent;
use crate::syntax::SyntaxHighlighter;

use bubbles::{assistant_identity, render_assistant_header_line, render_complete, render_user_message};
use dialogs::{render_ask_user_question, render_plan_approval};
use plan::render_plan;
use system::{render_command, render_hook, render_init};
use tool_call::render_tool_call;

/// Clickable path entry: (line_idx, start_col, end_col, file_path, old_string, new_string)
/// (line_idx, start_col, end_col, file_path, old_string, new_string, wrap_line_count)
pub type ClickablePath = (usize, usize, usize, String, String, String, usize);

/// Clickable table entry: (cache_line_start, cache_line_end, raw_markdown_text)
/// Identifies rendered table regions so mouse clicks can open a full-width popup.
pub type ClickableTable = (usize, usize, String);

/// Render DisplayEvents into Lines for the session pane with iMessage-style layout
/// Returns (lines, animation_indices, bubble_positions, clickable_paths, clickable_tables) where:
/// - animation_indices are (line_idx, span_idx, tool_use_id) for ALL tool indicators
/// - bubble_positions are (line_idx, is_user) pairs marking where message bubbles start
/// - clickable_paths are file path link regions for mouse click handling
/// - clickable_tables are table regions for click-to-expand popup
pub fn render_display_events(
    events: &[DisplayEvent],
    width: u16,
    pending_tools: &HashSet<String>,
    failed_tools: &HashSet<String>,
    syntax_highlighter: &mut SyntaxHighlighter,
    pending_user_message: Option<&str>,
) -> (
    Vec<Line<'static>>,
    Vec<(usize, usize, String)>,
    Vec<(usize, bool)>,
    Vec<ClickablePath>,
    Vec<ClickableTable>,
) {
    render_display_events_with_state(
        events,
        width,
        pending_tools,
        failed_tools,
        syntax_highlighter,
        pending_user_message,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Default::default(),
    )
}

/// Render only new events, appending to existing cache data.
/// `events` contains ONLY the new events (from start_idx onwards in the original array).
/// `pre_scan` contains pre-computed state flags from events before start_idx,
/// so we don't need the old events at all (eliminates the mega-clone).
pub fn render_display_events_incremental(
    events: &[DisplayEvent],
    width: u16,
    pending_tools: &HashSet<String>,
    failed_tools: &HashSet<String>,
    syntax_highlighter: &mut SyntaxHighlighter,
    pending_user_message: Option<&str>,
    pre_scan: super::render_thread::PreScanState,
) -> (
    Vec<Line<'static>>,
    Vec<(usize, usize, String)>,
    Vec<(usize, bool)>,
    Vec<ClickablePath>,
    Vec<ClickableTable>,
) {
    // Render only new events into fresh accumulators. Indices are relative to 0 —
    // the main thread offsets them by existing_line_count when extending its cache.
    render_display_events_with_state(
        events,
        width,
        pending_tools,
        failed_tools,
        syntax_highlighter,
        pending_user_message,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        pre_scan,
    )
}

/// Core renderer: iterates events from `start_idx`, appending to provided vectors.
/// Pre-scan state from earlier events is passed in (not re-scanned), so callers
/// can send only new events + pre-computed flags (eliminates mega-clone).
fn render_display_events_with_state(
    events: &[DisplayEvent],
    width: u16,
    pending_tools: &HashSet<String>,
    failed_tools: &HashSet<String>,
    syntax_highlighter: &mut SyntaxHighlighter,
    pending_user_message: Option<&str>,
    mut lines: Vec<Line<'static>>,
    mut animation_indices: Vec<(usize, usize, String)>,
    mut bubble_positions: Vec<(usize, bool)>,
    mut clickable_paths: Vec<ClickablePath>,
    mut clickable_tables: Vec<ClickableTable>,
    pre_scan: super::render_thread::PreScanState,
) -> (
    Vec<Line<'static>>,
    Vec<(usize, usize, String)>,
    Vec<(usize, bool)>,
    Vec<ClickablePath>,
    Vec<ClickableTable>,
) {
    let w = width as usize;
    let bubble_width = (w * 2 / 3).max(40);

    // Pre-computed state flags: for full renders these are all default (false/None),
    // for incremental renders they come from pre_scan_events() on the main thread.
    let mut saw_init = pre_scan.saw_init;
    let mut saw_content = pre_scan.saw_content;
    let mut current_model = pre_scan.current_model;
    let mut last_hook = pre_scan.last_hook;
    let mut saw_exit_plan_mode = pre_scan.saw_exit_plan_mode;
    let mut saw_user_after_exit_plan = pre_scan.saw_user_after_exit_plan;
    let mut saw_ask_user_question = pre_scan.saw_ask_user_question;
    let mut saw_user_after_ask = pre_scan.saw_user_after_ask;
    let mut last_ask_input = pre_scan.last_ask_input;

    for event in events.iter() {
        match event {
            DisplayEvent::Init { model, cwd, .. } => {
                current_model = Some(model.clone());
                if saw_init || saw_content {
                    continue;
                }
                saw_init = true;
                render_init(&mut lines, model, cwd);
            }
            DisplayEvent::Hook { name, output } => {
                // Dedup consecutive identical hooks — compare by reference first to avoid clone
                if let Some((ref ln, ref lo)) = last_hook {
                    if ln == name && lo == output {
                        continue;
                    }
                }
                last_hook = Some((name.clone(), output.clone()));
                render_hook(&mut lines, name, output, bubble_width);
            }
            DisplayEvent::Command { name } => {
                render_command(&mut lines, name);
            }
            DisplayEvent::Compacting => {
                lines.push(Line::from(""));
                lines.push(
                    Line::from(vec![Span::styled(
                        " ✓ Context compacted ",
                        Style::default().fg(Color::Black).bg(Color::Green),
                    )])
                    .alignment(Alignment::Center),
                );
            }
            DisplayEvent::Compacted => {
                lines.push(Line::from(""));
                lines.push(
                    Line::from(vec![Span::styled(
                        " ✓ Context compacted ",
                        Style::default().fg(Color::Black).bg(Color::Green),
                    )])
                    .alignment(Alignment::Center),
                );
            }
            DisplayEvent::MayBeCompacting => {
                lines.push(Line::from(""));
                lines.push(
                    Line::from(vec![Span::styled(
                        " ⏳ Compacting context... ",
                        Style::default().fg(Color::Black).bg(Color::Yellow),
                    )])
                    .alignment(Alignment::Center),
                );
            }
            DisplayEvent::Plan { name, content } => {
                saw_content = true;
                last_hook = None;
                render_plan(&mut lines, name, content, w);
            }
            DisplayEvent::UserMessage { content, .. } => {
                // Safety net: if a compaction summary slipped through parsing,
                // render the banner instead of the raw multi-page summary text
                if content
                    .starts_with("This session is being continued from a previous conversation")
                {
                    lines.push(Line::from(""));
                    lines.push(
                        Line::from(vec![Span::styled(
                            " ✓ Context compacted ",
                            Style::default().fg(Color::Black).bg(Color::Green),
                        )])
                        .alignment(Alignment::Center),
                    );
                    continue;
                }
                saw_content = true;
                last_hook = None;
                if saw_exit_plan_mode {
                    saw_user_after_exit_plan = true;
                }
                if saw_ask_user_question {
                    saw_user_after_ask = true;
                }
                // Track bubble position (line index after the empty lines)
                bubble_positions.push((lines.len() + 2, true));
                render_user_message(&mut lines, content, bubble_width, w);
            }
            DisplayEvent::AssistantText { text, .. } => {
                saw_content = true;
                last_hook = None;
                // Track bubble position (line index after the empty lines)
                bubble_positions.push((lines.len() + 2, false));
                lines.push(Line::from(""));
                lines.push(Line::from(""));

                let (_, assistant_color) =
                    assistant_identity(current_model.as_deref().filter(|m| !m.is_empty()));
                lines.push(render_assistant_header_line(
                    current_model.as_deref(),
                    bubble_width,
                ));

                let base_offset = lines.len();
                let (text_lines, table_regions, path_regions) =
                    render_assistant_text_with_paths_colored(
                        text,
                        bubble_width,
                        syntax_highlighter,
                        assistant_color,
                    );
                lines.extend(text_lines);
                // Offset table regions to absolute cache line positions
                for (start, end, raw) in table_regions {
                    clickable_tables.push((start + base_offset, end + base_offset, raw));
                }
                for (line_idx, start_col, end_col, file_path) in path_regions {
                    clickable_paths.push((
                        base_offset + line_idx,
                        start_col,
                        end_col,
                        file_path,
                        String::new(),
                        String::new(),
                        1,
                    ));
                }

                lines.push(Line::from(vec![Span::styled(
                    format!("└{}", "─".repeat(bubble_width - 1)),
                    Style::default().fg(assistant_color),
                )]));
            }
            DisplayEvent::ToolCall {
                tool_name,
                file_path,
                input,
                tool_use_id,
                ..
            } => {
                saw_content = true;
                last_hook = None;
                if tool_name == "ExitPlanMode" {
                    saw_exit_plan_mode = true;
                    saw_user_after_exit_plan = false;
                }
                if tool_name == "AskUserQuestion" {
                    saw_ask_user_question = true;
                    saw_user_after_ask = false;
                    last_ask_input = Some(input.clone());
                }
                // TodoWrite rendered as sticky widget, skip inline display
                if tool_name == "TodoWrite" {
                    continue;
                }
                render_tool_call(
                    &mut lines,
                    &mut animation_indices,
                    &mut clickable_paths,
                    tool_name,
                    file_path,
                    input,
                    tool_use_id,
                    pending_tools,
                    failed_tools,
                    bubble_width,
                    syntax_highlighter,
                );
            }
            DisplayEvent::ToolResult {
                tool_use_id,
                tool_name,
                file_path,
                content,
                is_error,
                ..
            } => {
                saw_content = true;
                last_hook = None;
                // TodoWrite result is noise ("Todos have been modified successfully"), skip it
                if tool_name == "TodoWrite" {
                    continue;
                }
                let is_failed = *is_error || failed_tools.contains(tool_use_id);
                let tool_max = bubble_width + 10;
                lines.extend(render_tool_result(
                    tool_name,
                    file_path.as_deref(),
                    content,
                    is_failed,
                    tool_max,
                ));
                // Show approval prompt immediately after ExitPlanMode result
                if tool_name == "ExitPlanMode" && saw_exit_plan_mode && !saw_user_after_exit_plan {
                    render_plan_approval(&mut lines, w);
                }
                // Show AskUserQuestion options box after the tool result
                if tool_name == "AskUserQuestion" && saw_ask_user_question && !saw_user_after_ask {
                    if let Some(ref input) = last_ask_input {
                        render_ask_user_question(&mut lines, input, w);
                    }
                }
            }
            DisplayEvent::Complete {
                duration_ms,
                cost_usd,
                success,
                ..
            } => {
                render_complete(&mut lines, *duration_ms, *cost_usd, *success);
            }
            DisplayEvent::ModelSwitch { model } => {
                current_model = Some(model.clone());
            }
            DisplayEvent::Filtered => {}
        }
    }

    // Render pending user message (sent but not yet in session file)
    if let Some(msg) = pending_user_message {
        bubble_positions.push((lines.len() + 2, true));
        render_user_message(&mut lines, msg, bubble_width, w);
    }

    (
        lines,
        animation_indices,
        bubble_positions,
        clickable_paths,
        clickable_tables,
    )
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

    // ── render_display_events integration tests ─────────────────────────

    /// Empty events produces empty output.
    #[test]
    fn test_render_events_empty() {
        let mut highlighter = SyntaxHighlighter::new();
        let (lines, anim, bubbles, clicks, _tables) = render_display_events(
            &[],
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        assert!(lines.is_empty());
        assert!(anim.is_empty());
        assert!(bubbles.is_empty());
        assert!(clicks.is_empty());
    }

    /// Single Init event renders session started.
    #[test]
    fn test_render_events_init() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::Init {
            _session_id: "s1".into(),
            cwd: "/project".into(),
            model: "claude-opus-4-20250514".into(),
        }];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Session Started")));
    }

    /// Duplicate Init events are deduplicated.
    #[test]
    fn test_render_events_dedup_init() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![
            DisplayEvent::Init {
                _session_id: "s1".into(),
                cwd: "/a".into(),
                model: "m".into(),
            },
            DisplayEvent::Init {
                _session_id: "s2".into(),
                cwd: "/b".into(),
                model: "m2".into(),
            },
        ];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let text = lines_to_text(&lines);
        // Only one "Session Started" should appear
        let count = text
            .iter()
            .filter(|l| l.contains("Session Started"))
            .count();
        assert_eq!(count, 1);
    }

    /// UserMessage renders with bubble position tracked.
    #[test]
    fn test_render_events_user_message_bubble() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::UserMessage {
            _uuid: "u1".into(),
            content: "Hello".into(),
        }];
        let (lines, _, bubbles, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        assert!(!lines.is_empty());
        assert_eq!(bubbles.len(), 1);
        assert!(bubbles[0].1, "User message bubble should be marked as user");
    }

    /// AssistantText renders with bubble position tracked.
    #[test]
    fn test_render_events_assistant_bubble() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::AssistantText {
            _uuid: "a1".into(),
            _message_id: "m1".into(),
            text: "I'll help you.".into(),
        }];
        let (lines, _, bubbles, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        assert!(!lines.is_empty());
        assert_eq!(bubbles.len(), 1);
        assert!(
            !bubbles[0].1,
            "Assistant bubble should NOT be marked as user"
        );
    }

    #[test]
    fn test_render_events_assistant_file_link_adds_clickable_path() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::AssistantText {
            _uuid: "a1".into(),
            _message_id: "m1".into(),
            text: "See [render_tools.rs](/Users/test/render_tools.rs#L42).".into(),
        }];
        let (_lines, _, _, clickable_paths, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        assert_eq!(clickable_paths.len(), 1);
        assert_eq!(clickable_paths[0].3, "/Users/test/render_tools.rs#L42");
        assert_eq!(clickable_paths[0].6, 1);
    }

    #[test]
    fn test_render_events_codex_assistant_header_uses_codex_label() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![
            DisplayEvent::Init {
                _session_id: "s1".into(),
                cwd: "/project".into(),
                model: "gpt-5.4".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: "a1".into(),
                _message_id: "m1".into(),
                text: "I can help with that.".into(),
            },
        ];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|line| line.contains("Codex")));
    }

    #[test]
    fn test_render_events_codex_assistant_header_right_aligns_model() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![
            DisplayEvent::Init {
                _session_id: "s1".into(),
                cwd: "/project".into(),
                model: "gpt-5.4".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: "a1".into(),
                _message_id: "m1".into(),
                text: "I can help with that.".into(),
            },
        ];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let header = lines_to_text(&lines)
            .into_iter()
            .find(|line| line.contains("Codex"))
            .expect("assistant header line");
        assert!(header.starts_with(" Codex ▶ "));
        assert!(header.ends_with(" gpt-5.4 "));
        assert_eq!(header.chars().count(), (80usize * 2 / 3).max(40));
    }

    #[test]
    fn test_render_events_codex_assistant_header_uses_cyan() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![
            DisplayEvent::Init {
                _session_id: "s1".into(),
                cwd: "/project".into(),
                model: "gpt-5.4".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: "a1".into(),
                _message_id: "m1".into(),
                text: "I can help with that.".into(),
            },
        ];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let codex_header = lines.iter().find_map(|line| {
            line.spans
                .iter()
                .find(|span| span.content.contains("Codex"))
                .map(|span| span.style.bg)
        });
        assert_eq!(codex_header, Some(Some(Color::Cyan)));
    }

    #[test]
    fn test_render_events_codex_assistant_body_gutter_uses_cyan() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![
            DisplayEvent::Init {
                _session_id: "s1".into(),
                cwd: "/project".into(),
                model: "gpt-5.4".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: "a1".into(),
                _message_id: "m1".into(),
                text: "Paragraph line\n```rust\nfn main() {}\n```".into(),
            },
        ];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let gutter_colors: Vec<_> = lines
            .iter()
            .filter_map(|line| line.spans.first())
            .filter(|span| span.content.as_ref() == "│ ")
            .map(|span| span.style.fg)
            .collect();
        assert!(!gutter_colors.is_empty(), "expected assistant gutter lines");
        assert!(gutter_colors
            .iter()
            .all(|color| *color == Some(Color::Cyan)));

        let footer = lines
            .iter()
            .find(|line| {
                line.spans
                    .first()
                    .map(|span| span.content.starts_with('└'))
                    .unwrap_or(false)
            })
            .expect("assistant footer line");
        assert_eq!(footer.spans[0].style.fg, Some(Color::Cyan));
    }

    #[test]
    fn test_render_events_legacy_codex_model_still_renders_codex_header() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![
            DisplayEvent::Init {
                _session_id: "s1".into(),
                cwd: "/project".into(),
                model: "codex".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: "a1".into(),
                _message_id: "m1".into(),
                text: "Stored legacy Codex turn".into(),
            },
        ];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let header = lines_to_text(&lines)
            .into_iter()
            .find(|line| line.contains("Codex"))
            .expect("assistant header line");
        assert!(header.starts_with(" Codex ▶ "));
    }

    #[test]
    fn test_render_events_incremental_preserves_codex_identity() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::AssistantText {
            _uuid: "a1".into(),
            _message_id: "m1".into(),
            text: "Incremental render".into(),
        }];
        let pre_scan = super::super::render_thread::PreScanState {
            saw_init: true,
            saw_content: false,
            current_model: Some("gpt-5.4".into()),
            last_hook: None,
            saw_exit_plan_mode: false,
            saw_user_after_exit_plan: false,
            saw_ask_user_question: false,
            saw_user_after_ask: false,
            last_ask_input: None,
        };
        let (lines, _, _, _, _) = render_display_events_incremental(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
            pre_scan,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|line| line.contains("Codex")));
    }

    /// Hook event renders hook name.
    #[test]
    fn test_render_events_hook() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::Hook {
            name: "pre-tool-use".into(),
            output: "approved".into(),
        }];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("pre-tool-use")));
    }

    /// Duplicate consecutive hooks are deduplicated.
    #[test]
    fn test_render_events_dedup_hooks() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![
            DisplayEvent::Hook {
                name: "hook".into(),
                output: "out".into(),
            },
            DisplayEvent::Hook {
                name: "hook".into(),
                output: "out".into(),
            },
        ];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let text = lines_to_text(&lines);
        let hook_count = text.iter().filter(|l| l.contains("hook")).count();
        assert_eq!(hook_count, 1, "Duplicate hooks should be deduplicated");
    }

    /// Command event renders with slash.
    #[test]
    fn test_render_events_command() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::Command {
            name: "compact".into(),
        }];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("compact")));
    }

    /// Compacting event renders banner.
    #[test]
    fn test_render_events_compacting() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::Compacting];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Context compacted")));
    }

    /// Compacted event renders banner.
    #[test]
    fn test_render_events_compacted() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::Compacted];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Context compacted")));
    }

    /// MayBeCompacting event renders warning banner.
    #[test]
    fn test_render_events_may_be_compacting() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::MayBeCompacting];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Compacting")));
    }

    /// Complete event renders duration and cost.
    #[test]
    fn test_render_events_complete() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::Complete {
            _session_id: "s1".into(),
            success: true,
            duration_ms: 3500,
            cost_usd: 0.02,
        }];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Completed")));
        assert!(text.iter().any(|l| l.contains("3.5s")));
    }

    /// Filtered event produces no output.
    #[test]
    fn test_render_events_filtered() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::Filtered];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        assert!(lines.is_empty());
    }

    /// Pending user message renders at the end.
    #[test]
    fn test_render_events_pending_user_message() {
        let mut highlighter = SyntaxHighlighter::new();
        let (lines, _, bubbles, _, _) = render_display_events(
            &[],
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            Some("Waiting..."),
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Waiting...")));
        assert_eq!(bubbles.len(), 1);
        assert!(bubbles[0].1); // user bubble
    }

    /// TodoWrite tool calls are skipped in rendering.
    #[test]
    fn test_render_events_todowrite_skipped() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::ToolCall {
            _uuid: "u1".into(),
            tool_use_id: "t1".into(),
            tool_name: "TodoWrite".into(),
            file_path: None,
            input: json!({"todos": []}),
        }];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        assert!(lines.is_empty(), "TodoWrite tool calls should be skipped");
    }

    /// TodoWrite results are also skipped.
    #[test]
    fn test_render_events_todowrite_result_skipped() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::ToolResult {
            tool_use_id: "t1".into(),
            tool_name: "TodoWrite".into(),
            file_path: None,
            content: "Todos have been modified successfully".into(),
            is_error: false,
        }];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        assert!(lines.is_empty(), "TodoWrite results should be skipped");
    }

    /// ToolCall with pending status gets animation index.
    #[test]
    fn test_render_events_tool_pending_animation() {
        let mut highlighter = SyntaxHighlighter::new();
        let mut pending = HashSet::new();
        pending.insert("tool1".to_string());
        let events = vec![DisplayEvent::ToolCall {
            _uuid: "u1".into(),
            tool_use_id: "tool1".into(),
            tool_name: "Read".into(),
            file_path: Some("/path/file.rs".into()),
            input: json!({"file_path": "/path/file.rs"}),
        }];
        let (_, anim, _, _, _) = render_display_events(
            &events,
            80,
            &pending,
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        assert!(!anim.is_empty(), "Pending tool should have animation index");
    }

    /// Plan event renders plan content.
    #[test]
    fn test_render_events_plan() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::Plan {
            name: "Implementation".into(),
            content: "Step 1: Do thing".into(),
        }];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("PLAN MODE")));
        assert!(text.iter().any(|l| l.contains("Step 1")));
    }

    /// Compaction summary in user message renders as banner instead.
    #[test]
    fn test_render_events_compaction_summary_replaced() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![DisplayEvent::UserMessage {
            _uuid: "u1".into(),
            content:
                "This session is being continued from a previous conversation. Here is a summary..."
                    .into(),
        }];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let text = lines_to_text(&lines);
        assert!(text.iter().any(|l| l.contains("Context compacted")));
        // The raw compaction text should NOT appear
        assert!(!text.iter().any(|l| l.contains("Here is a summary")));
    }

    /// Multiple event types in sequence.
    #[test]
    fn test_render_events_mixed_sequence() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![
            DisplayEvent::Init {
                _session_id: "s".into(),
                cwd: "/p".into(),
                model: "m".into(),
            },
            DisplayEvent::UserMessage {
                _uuid: "u".into(),
                content: "Do something".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: "a".into(),
                _message_id: "m".into(),
                text: "Sure!".into(),
            },
            DisplayEvent::Complete {
                _session_id: "s".into(),
                success: true,
                duration_ms: 1000,
                cost_usd: 0.01,
            },
        ];
        let (lines, _, bubbles, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        assert!(!lines.is_empty());
        assert_eq!(bubbles.len(), 2); // user + assistant
    }

    /// Init after content is suppressed.
    #[test]
    fn test_render_events_init_after_content_suppressed() {
        let mut highlighter = SyntaxHighlighter::new();
        let events = vec![
            DisplayEvent::UserMessage {
                _uuid: "u".into(),
                content: "Hi".into(),
            },
            DisplayEvent::Init {
                _session_id: "s".into(),
                cwd: "/p".into(),
                model: "m".into(),
            },
        ];
        let (lines, _, _, _, _) = render_display_events(
            &events,
            80,
            &HashSet::new(),
            &HashSet::new(),
            &mut highlighter,
            None,
        );
        let text = lines_to_text(&lines);
        // Init should be suppressed since content was already seen
        assert!(!text.iter().any(|l| l.contains("Session Started")));
    }
}
