//! Tool rendering utilities for TUI
//!
//! Handles extraction of tool parameters and rendering tool results.
//! Delegates to submodules for specific functionality:
//! - `tool_params`: display name mapping, parameter extraction, line truncation
//! - `tool_result`: tool result and write preview rendering
//! - `diff_parse`: diff/patch parsing into structured line types
//! - `diff_render`: diff rendering with syntax highlighting

mod diff_parse;
mod diff_render;
mod tool_params;
mod tool_result;

pub use diff_parse::extract_edit_preview_strings;
pub use diff_render::render_edit_diff;
pub use tool_params::{extract_tool_param, tool_display_name};

#[allow(unused_imports)] // used by tests via `use super::*`
pub use tool_params::truncate_line;
pub use tool_result::{render_tool_result, render_write_preview};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ─── Helpers ──────────────────────────────────────────────────────

    fn spans_text(line: &ratatui::text::Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    // ═══════════════════════════════════════════════════════════════════
    // tool_display_name
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn display_name_grep_uppercase() {
        assert_eq!(tool_display_name("Grep"), "Search");
    }

    #[test]
    fn display_name_grep_lowercase() {
        assert_eq!(tool_display_name("grep"), "Search");
    }

    #[test]
    fn display_name_glob_uppercase() {
        assert_eq!(tool_display_name("Glob"), "Find");
    }

    #[test]
    fn display_name_glob_lowercase() {
        assert_eq!(tool_display_name("glob"), "Find");
    }

    #[test]
    fn display_name_read_passthrough() {
        assert_eq!(tool_display_name("Read"), "Read");
    }

    #[test]
    fn display_name_write_passthrough() {
        assert_eq!(tool_display_name("Write"), "Write");
    }

    #[test]
    fn display_name_bash_passthrough() {
        assert_eq!(tool_display_name("Bash"), "Bash");
    }

    #[test]
    fn display_name_exec_command_maps_to_bash() {
        assert_eq!(tool_display_name("exec_command"), "Bash");
    }

    #[test]
    fn display_name_edit_passthrough() {
        assert_eq!(tool_display_name("Edit"), "Edit");
    }

    #[test]
    fn display_name_task_passthrough() {
        assert_eq!(tool_display_name("Task"), "Task");
    }

    #[test]
    fn display_name_unknown_passthrough() {
        assert_eq!(tool_display_name("CustomTool"), "CustomTool");
    }

    #[test]
    fn display_name_empty_string() {
        assert_eq!(tool_display_name(""), "");
    }

    #[test]
    fn display_name_webfetch_passthrough() {
        assert_eq!(tool_display_name("WebFetch"), "WebFetch");
    }

    #[test]
    fn extract_edit_param_from_patch_fallback() {
        let input = json!({
            "patch": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old\n+new\n*** End Patch"
        });
        assert_eq!(extract_tool_param("Edit", &input), "src/main.rs");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Read
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_read_file_path() {
        let input = json!({"file_path": "/src/main.rs"});
        assert_eq!(extract_tool_param("Read", &input), "/src/main.rs");
    }

    #[test]
    fn extract_read_path_fallback() {
        let input = json!({"path": "/src/lib.rs"});
        assert_eq!(extract_tool_param("Read", &input), "/src/lib.rs");
    }

    #[test]
    fn extract_read_lowercase() {
        let input = json!({"file_path": "/foo.rs"});
        assert_eq!(extract_tool_param("read", &input), "/foo.rs");
    }

    #[test]
    fn extract_read_empty_input() {
        let input = json!({});
        assert_eq!(extract_tool_param("Read", &input), "");
    }

    #[test]
    fn extract_read_null_value() {
        let input = json!({"file_path": null});
        assert_eq!(extract_tool_param("Read", &input), "");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Write
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_write_file_path() {
        let input = json!({"file_path": "/out.txt"});
        assert_eq!(extract_tool_param("Write", &input), "/out.txt");
    }

    #[test]
    fn extract_write_path_fallback() {
        let input = json!({"path": "/out.txt"});
        assert_eq!(extract_tool_param("write", &input), "/out.txt");
    }

    #[test]
    fn extract_write_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("Write", &input), "");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Edit
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_edit_file_path() {
        let input = json!({"file_path": "/src/config.rs"});
        assert_eq!(extract_tool_param("Edit", &input), "/src/config.rs");
    }

    #[test]
    fn extract_edit_path_fallback() {
        let input = json!({"path": "/src/config.rs"});
        assert_eq!(extract_tool_param("edit", &input), "/src/config.rs");
    }

    #[test]
    fn extract_edit_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("Edit", &input), "");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Bash
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_bash_command() {
        let input = json!({"command": "cargo build"});
        assert_eq!(extract_tool_param("Bash", &input), "cargo build");
    }

    #[test]
    fn extract_exec_command_cmd_fallback() {
        let input = json!({"cmd": "pwd"});
        assert_eq!(extract_tool_param("exec_command", &input), "pwd");
    }

    #[test]
    fn extract_write_stdin_poll_command() {
        let input = json!({"session_id": 98333, "chars": ""});
        assert_eq!(
            extract_tool_param("write_stdin", &input),
            "poll session 98333"
        );
    }

    #[test]
    fn extract_bash_lowercase() {
        let input = json!({"command": "ls -la"});
        assert_eq!(extract_tool_param("bash", &input), "ls -la");
    }

    #[test]
    fn extract_bash_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("Bash", &input), "");
    }

    #[test]
    fn extract_bash_long_command() {
        let cmd = "a".repeat(500);
        let input = json!({"command": cmd});
        assert_eq!(extract_tool_param("Bash", &input), cmd);
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Glob
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_glob_pattern() {
        let input = json!({"pattern": "**/*.rs"});
        assert_eq!(extract_tool_param("Glob", &input), "**/*.rs");
    }

    #[test]
    fn extract_glob_lowercase() {
        let input = json!({"pattern": "*.txt"});
        assert_eq!(extract_tool_param("glob", &input), "*.txt");
    }

    #[test]
    fn extract_glob_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("Glob", &input), "");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Grep
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_grep_pattern() {
        let input = json!({"pattern": "TODO"});
        assert_eq!(extract_tool_param("Grep", &input), "TODO");
    }

    #[test]
    fn extract_grep_lowercase() {
        let input = json!({"pattern": "fn main"});
        assert_eq!(extract_tool_param("grep", &input), "fn main");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — WebFetch
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_webfetch_url() {
        let input = json!({"url": "https://example.com"});
        assert_eq!(
            extract_tool_param("WebFetch", &input),
            "https://example.com"
        );
    }

    #[test]
    fn extract_webfetch_lowercase() {
        let input = json!({"url": "https://foo.bar"});
        assert_eq!(extract_tool_param("webfetch", &input), "https://foo.bar");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — WebSearch
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_websearch_query() {
        let input = json!({"query": "rust async"});
        assert_eq!(extract_tool_param("WebSearch", &input), "rust async");
    }

    #[test]
    fn extract_websearch_lowercase() {
        let input = json!({"query": "test query"});
        assert_eq!(extract_tool_param("websearch", &input), "test query");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Task
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_task_with_type_and_desc() {
        let input = json!({"subagent_type": "code", "description": "refactor module"});
        assert_eq!(extract_tool_param("Task", &input), "[code] refactor module");
    }

    #[test]
    fn extract_task_default_agent_type() {
        let input = json!({"description": "do something"});
        assert_eq!(extract_tool_param("Task", &input), "[agent] do something");
    }

    #[test]
    fn extract_task_no_fields() {
        let input = json!({});
        assert_eq!(extract_tool_param("task", &input), "[agent] ");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — LSP
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_lsp_operation_and_file() {
        let input = json!({"operation": "hover", "filePath": "/src/main.rs"});
        assert_eq!(extract_tool_param("LSP", &input), "hover /src/main.rs");
    }

    #[test]
    fn extract_lsp_lowercase() {
        let input = json!({"operation": "goto", "filePath": "/lib.rs"});
        assert_eq!(extract_tool_param("lsp", &input), "goto /lib.rs");
    }

    #[test]
    fn extract_lsp_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("LSP", &input), " ");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Plan modes
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_enter_plan_mode() {
        let input = json!({});
        assert!(extract_tool_param("EnterPlanMode", &input).contains("Planning"));
    }

    #[test]
    fn extract_exit_plan_mode() {
        let input = json!({});
        assert!(extract_tool_param("ExitPlanMode", &input).contains("Plan complete"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Unknown tools (fallback)
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn extract_unknown_tool_file_path() {
        let input = json!({"file_path": "/x.rs"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "/x.rs");
    }

    #[test]
    fn extract_unknown_tool_path() {
        let input = json!({"path": "/y.rs"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "/y.rs");
    }

    #[test]
    fn extract_unknown_tool_command() {
        let input = json!({"command": "echo hi"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "echo hi");
    }

    #[test]
    fn extract_unknown_tool_query() {
        let input = json!({"query": "something"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "something");
    }

    #[test]
    fn extract_unknown_tool_pattern() {
        let input = json!({"pattern": "*.md"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "*.md");
    }

    #[test]
    fn extract_unknown_tool_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("UnknownTool", &input), "");
    }

    #[test]
    fn extract_unknown_tool_priority_file_path_first() {
        let input = json!({"file_path": "/first", "command": "second"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "/first");
    }

    // ═══════════════════════════════════════════════════════════════════
    // truncate_line
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn truncate_fits_exactly() {
        assert_eq!(truncate_line("hello", 5), "hello");
    }

    #[test]
    fn truncate_shorter_than_max() {
        assert_eq!(truncate_line("hi", 10), "hi");
    }

    #[test]
    fn truncate_over_max() {
        assert_eq!(truncate_line("hello world", 5), "hell\u{2026}");
    }

    #[test]
    fn truncate_max_one() {
        assert_eq!(truncate_line("hello", 1), "\u{2026}");
    }

    #[test]
    fn truncate_empty_string() {
        assert_eq!(truncate_line("", 10), "");
    }

    #[test]
    fn truncate_trims_whitespace() {
        assert_eq!(truncate_line("  hello  ", 10), "hello");
    }

    #[test]
    fn truncate_trims_then_truncates() {
        assert_eq!(truncate_line("  hello world  ", 5), "hell\u{2026}");
    }

    #[test]
    fn truncate_unicode_chars() {
        assert_eq!(
            truncate_line("\u{65e5}\u{672c}\u{8a9e}", 3),
            "\u{65e5}\u{672c}\u{8a9e}"
        );
    }

    #[test]
    fn truncate_unicode_over_max() {
        assert_eq!(
            truncate_line("\u{65e5}\u{672c}\u{8a9e}\u{30c6}\u{30b9}\u{30c8}", 4),
            "\u{65e5}\u{672c}\u{8a9e}\u{2026}"
        );
    }

    #[test]
    fn truncate_max_zero() {
        assert_eq!(truncate_line("hello", 0), "\u{2026}");
    }

    #[test]
    fn truncate_single_char_fits() {
        assert_eq!(truncate_line("a", 1), "a");
    }

    #[test]
    fn truncate_two_chars_max_one() {
        assert_eq!(truncate_line("ab", 1), "\u{2026}");
    }

    #[test]
    fn truncate_preserves_special_chars() {
        assert_eq!(truncate_line("@#$%^", 5), "@#$%^");
    }

    #[test]
    fn truncate_emoji() {
        let s = "\u{1f389}\u{1f38a}\u{1f388}\u{1f381}";
        assert_eq!(truncate_line(s, 2), "\u{1f389}\u{2026}");
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — empty content
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_read_empty_content() {
        let lines = render_tool_result("Read", None, "", false, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("empty file"));
    }

    #[test]
    fn render_result_bash_empty_content() {
        let lines = render_tool_result("Bash", None, "", false, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("\u{2713}"));
    }

    #[test]
    fn render_result_unknown_empty_content() {
        let lines = render_tool_result("Unknown", None, "", false, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("\u{2713}"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — Read tool
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_read_single_line() {
        let lines = render_tool_result("Read", None, "one line", false, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("one line"));
    }

    #[test]
    fn render_result_read_two_lines() {
        let lines = render_tool_result("Read", None, "first\nlast", false, 80);
        assert_eq!(lines.len(), 2);
        assert!(spans_text(&lines[0]).contains("first"));
        assert!(spans_text(&lines[1]).contains("last"));
    }

    #[test]
    fn render_result_read_many_lines() {
        let content = (1..=10)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = render_tool_result("Read", None, &content, false, 80);
        assert_eq!(lines.len(), 3);
        assert!(spans_text(&lines[0]).contains("line 1"));
        assert!(spans_text(&lines[1]).contains("10 lines"));
        assert!(spans_text(&lines[2]).contains("line 10"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — Bash tool
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_bash_single_line() {
        let lines = render_tool_result("Bash", None, "output", false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("output"));
    }

    #[test]
    fn render_result_bash_multiple_lines() {
        let lines = render_tool_result("Bash", None, "line1\nline2\nline3", false, 80);
        assert_eq!(lines.len(), 2);
        assert!(spans_text(&lines[0]).contains("line2"));
        assert!(spans_text(&lines[1]).contains("line3"));
    }

    #[test]
    fn render_result_bash_skips_empty_lines() {
        let lines = render_tool_result("Bash", None, "result\n\n\n", false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("result"));
    }

    #[test]
    fn render_result_bash_all_empty_lines() {
        let lines = render_tool_result("Bash", None, "\n\n\n", false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("\u{2713}"));
    }

    #[test]
    fn render_result_exec_command_strips_exec_wrapper() {
        let content = "Chunk ID: 6bf9d8\nWall time: 0.0000 seconds\nProcess exited with code 0\nOriginal token count: 7\nOutput:\n/Users/macbookpro/AZUREAL\n";
        let lines = render_tool_result("exec_command", None, content, false, 80);
        assert_eq!(lines.len(), 1);
        assert_eq!(spans_text(&lines[0]), " \u{2503}  \u{2514}\u{2500} /Users/macbookpro/AZUREAL");
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — Grep tool
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_grep_few_matches() {
        let lines = render_tool_result("Grep", None, "match1\nmatch2", false, 80);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn render_result_grep_exactly_three() {
        let lines = render_tool_result("Grep", None, "a\nb\nc", false, 80);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn render_result_grep_more_than_three() {
        let content = "a\nb\nc\nd\ne";
        let lines = render_tool_result("Grep", None, content, false, 80);
        assert_eq!(lines.len(), 4);
        let last_text = spans_text(lines.last().unwrap());
        assert!(last_text.contains("+2 more"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — Glob tool
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_glob_file_count() {
        let content = "file1.rs\nfile2.rs\nfile3.rs";
        let lines = render_tool_result("Glob", None, content, false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("3 files"));
    }

    #[test]
    fn render_result_glob_single_file() {
        let lines = render_tool_result("Glob", None, "one.rs", false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("1 files"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — Task tool
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_task_few_lines() {
        let lines = render_tool_result("Task", None, "line1\nline2", false, 80);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn render_result_task_many_lines() {
        let content = (1..=10)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = render_tool_result("Task", None, &content, false, 80);
        assert_eq!(lines.len(), 6);
        let last_text = spans_text(lines.last().unwrap());
        assert!(last_text.contains("+5 more lines"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — Unknown/default tool
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_default_few_lines() {
        let lines = render_tool_result("SomeTool", None, "a\nb", false, 80);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn render_result_default_more_than_three() {
        let content = "a\nb\nc\nd\ne";
        let lines = render_tool_result("SomeTool", None, content, false, 80);
        assert_eq!(lines.len(), 4);
        let last_text = spans_text(lines.last().unwrap());
        assert!(last_text.contains("+2 more"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — failed state
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_failed_uses_red() {
        use ratatui::style::Color;
        let lines = render_tool_result("Bash", None, "error occurred", true, 80);
        assert!(!lines.is_empty());
        let has_red = lines[0]
            .spans
            .iter()
            .any(|s| s.style.fg == Some(Color::Red));
        assert!(has_red);
    }

    #[test]
    fn render_result_success_uses_azure() {
        use crate::tui::util::AZURE;
        let lines = render_tool_result("Bash", None, "ok", false, 80);
        assert!(!lines.is_empty());
        let has_azure = lines[0].spans.iter().any(|s| s.style.fg == Some(AZURE));
        assert!(has_azure);
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — system-reminder stripping
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_strips_system_reminder() {
        let content = "real output\n<system-reminder>hidden stuff</system-reminder>";
        let lines = render_tool_result("Bash", None, content, false, 80);
        for line in &lines {
            let text = spans_text(line);
            assert!(!text.contains("hidden"));
        }
    }

    #[test]
    fn render_result_system_reminder_at_start() {
        let content = "<system-reminder>all hidden</system-reminder>";
        let lines = render_tool_result("Bash", None, content, false, 80);
        assert_eq!(lines.len(), 1);
        assert!(spans_text(&lines[0]).contains("\u{2713}"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_tool_result — max_width truncation
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn render_result_narrow_width_truncates() {
        let long_line = "a".repeat(200);
        let lines = render_tool_result("Bash", None, &long_line, false, 30);
        assert!(!lines.is_empty());
        let text = spans_text(&lines[0]);
        assert!(text.len() < 200);
    }

    // ═══════════════════════════════════════════════════════════════════
    // render_write_preview
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn write_preview_with_content() {
        use crate::tui::util::AZURE;
        let input = json!({"content": "// module doc\nfn main() {}\nreturn;\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("3 lines"));
    }

    #[test]
    fn write_preview_shows_purpose_comment() {
        use crate::tui::util::AZURE;
        let input = json!({"content": "// This is the purpose\nfn foo() {}\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("// This is the purpose"));
    }

    #[test]
    fn write_preview_hash_comment() {
        use crate::tui::util::AZURE;
        let input = json!({"content": "# Python module\nimport os\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("# Python module"));
    }

    #[test]
    fn write_preview_no_comment_shows_first_line() {
        use crate::tui::util::AZURE;
        let input = json!({"content": "fn main() {}\nlet x = 1;\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("fn main()"));
    }

    #[test]
    fn write_preview_no_content_field() {
        use crate::tui::util::AZURE;
        let input = json!({"file_path": "/foo.rs"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        assert!(lines.is_empty());
    }

    #[test]
    fn write_preview_empty_content() {
        use crate::tui::util::AZURE;
        let input = json!({"content": ""});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        assert_eq!(lines.len(), 1);
        let text = spans_text(&lines[0]);
        assert!(text.contains("0 lines"));
    }

    #[test]
    fn write_preview_checkmark() {
        use crate::tui::util::AZURE;
        let input = json!({"content": "hello\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("\u{2713}"));
    }

    #[test]
    fn write_preview_triple_slash_comment() {
        use crate::tui::util::AZURE;
        let input = json!({"content": "some code\n/// Doc comment\nmore code\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("/// Doc comment"));
    }

    #[test]
    fn write_preview_inner_doc_comment() {
        use crate::tui::util::AZURE;
        let input = json!({"content": "some code\n//! Inner doc\nmore code\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("//! Inner doc"));
    }

    #[test]
    fn write_preview_block_comment() {
        use crate::tui::util::AZURE;
        let input = json!({"content": "/* Block comment */\ncode\n"});
        let mut lines = Vec::new();
        render_write_preview(&mut lines, &input, AZURE, 80);
        let text = spans_text(&lines[0]);
        assert!(text.contains("/* Block comment */"));
    }

    #[test]
    fn extract_edit_preview_strings_prefers_explicit_fields() {
        let input = json!({
            "old_string": "before",
            "new_string": "after",
            "patch": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-before\n+after\n*** End Patch"
        });
        let (old, new) = extract_edit_preview_strings(&input);
        assert_eq!(old, "before");
        assert_eq!(new, "after");
    }

    #[test]
    fn extract_edit_preview_strings_from_update_patch() {
        let input = json!({
            "patch": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n fn main() {\n-    old_call();\n+    new_call();\n }\n*** End Patch"
        });
        let (old, new) = extract_edit_preview_strings(&input);
        assert_eq!(old, "    old_call();\n}");
        assert_eq!(new, "    new_call();\n}");
    }

    #[test]
    fn extract_edit_preview_strings_from_add_patch() {
        let input = json!({
            "patch": "*** Begin Patch\n*** Add File: src/new.rs\n+fn main() {}\n+println!(\"hi\");\n*** End Patch"
        });
        let (old, new) = extract_edit_preview_strings(&input);
        assert!(old.is_empty());
        assert_eq!(new, "fn main() {}\nprintln!(\"hi\");");
    }

    #[test]
    fn extract_edit_preview_strings_from_unified_diff() {
        let input = json!({
            "unified_diff": "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,3 @@\n fn main() {\n-    old_call();\n+    new_call();\n }\n"
        });
        let (old, new) = extract_edit_preview_strings(&input);
        assert_eq!(old, "    old_call();\n}");
        assert_eq!(new, "    new_call();\n}");
    }

    #[test]
    fn render_edit_diff_from_patch_shows_diff_lines() {
        use crate::syntax::SyntaxHighlighter;
        use crate::tui::util::AZURE;
        let input = json!({
            "patch": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old_value();\n+new_value();\n unchanged();\n*** End Patch"
        });
        let mut lines = Vec::new();
        let mut highlighter = SyntaxHighlighter::new();
        render_edit_diff(
            &mut lines,
            &input,
            &Some("src/main.rs".to_string()),
            AZURE,
            80,
            &mut highlighter,
        );
        let rendered = lines.iter().map(spans_text).collect::<Vec<_>>().join("\n");
        assert!(!rendered.contains("Update File:"));
        assert!(!rendered.contains("@@"));
        assert!(rendered.contains("-old_value();"));
        assert!(rendered.contains("+new_value();"));
        assert!(rendered.contains(" unchanged();"));
    }

    #[test]
    fn render_edit_diff_from_unified_diff_shows_diff_lines() {
        use crate::syntax::SyntaxHighlighter;
        use crate::tui::util::AZURE;
        let input = json!({
            "unified_diff": "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,3 @@\n-old_value();\n+new_value();\n unchanged();\n"
        });
        let mut lines = Vec::new();
        let mut highlighter = SyntaxHighlighter::new();
        render_edit_diff(
            &mut lines,
            &input,
            &Some("src/main.rs".to_string()),
            AZURE,
            80,
            &mut highlighter,
        );
        let rendered = lines.iter().map(spans_text).collect::<Vec<_>>().join("\n");
        assert!(!rendered.contains("diff --git"));
        assert!(!rendered.contains("@@ -1,3 +1,3 @@"));
        assert!(rendered.contains("-old_value();"));
        assert!(rendered.contains("+new_value();"));
        assert!(rendered.contains(" unchanged();"));
    }

    #[test]
    fn render_edit_diff_from_patch_skips_header_and_hunk() {
        use crate::syntax::SyntaxHighlighter;
        use crate::tui::util::AZURE;
        let input = json!({
            "patch": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-old\n+new\n*** End Patch"
        });
        let mut lines = Vec::new();
        let mut highlighter = SyntaxHighlighter::new();
        render_edit_diff(
            &mut lines,
            &input,
            &Some("src/lib.rs".to_string()),
            AZURE,
            80,
            &mut highlighter,
        );
        // Header ("Update File:") and hunk ("@@") lines should be skipped;
        // the first rendered content line is the removed line "-old"
        let rendered: String = lines.iter().flat_map(|l| l.spans.iter()).map(|s| s.content.as_ref()).collect();
        assert!(!rendered.contains("Update File:"));
        assert!(!rendered.contains("@@"));
        assert!(rendered.contains("-old"));
        assert!(rendered.contains("+new"));
    }
}
