//! Tool parameter extraction and display name mapping
//!
//! Extracts the most relevant parameter from tool inputs for display in the
//! session pane, maps internal tool names to user-friendly labels, and provides
//! line truncation utilities.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Return the first string value found for any of the requested JSON keys.
fn first_string_field<'a>(input: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| input.get(key)?.as_str())
}

/// Return the best file-like path field from a tool payload.
fn path_field(input: &serde_json::Value) -> Option<&str> {
    first_string_field(
        input,
        &[
            "file_path",
            "path",
            "target_file",
            "relative_path",
            "notebook_path",
            "filePath",
        ],
    )
}

/// Map internal tool names to user-friendly display names
pub fn tool_display_name(tool_name: &str) -> &str {
    match tool_name {
        "Grep" | "grep" => "Search",
        "Glob" | "glob" => "Find",
        "exec_command" | "write_stdin" => "Bash",
        _ => tool_name,
    }
}

/// Extract the most relevant parameter from a tool's input for display
pub fn extract_tool_param(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "Read" | "read" => path_field(input).unwrap_or("").to_string(),
        "Write" | "write" => path_field(input).unwrap_or("").to_string(),
        "Edit" | "edit" | "NotebookEdit" | "notebookedit" => path_field(input)
            .map(str::to_string)
            .or_else(|| {
                input
                    .get("patch")
                    .and_then(|v| v.as_str())
                    .and_then(extract_apply_patch_file_path)
            })
            .unwrap_or_default(),
        "Bash" | "bash" | "exec_command" => {
            // Full command - no truncation
            input
                .get("command")
                .or_else(|| input.get("cmd"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "write_stdin" => input
            .get("command")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| describe_write_stdin_action(input)),
        "Glob" | "glob" => input
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "Grep" | "grep" => input
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "WebFetch" | "webfetch" => input
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "WebSearch" | "websearch" => input
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "Agent" | "agent" | "Task" | "task" => {
            let agent_type = input
                .get("subagent_type")
                .and_then(|v| v.as_str())
                .unwrap_or("agent");
            let desc = input
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            format!("[{}] {}", agent_type, desc)
        }
        "LSP" | "lsp" => {
            let op = input
                .get("operation")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let file = path_field(input).unwrap_or("");
            format!("{} {}", op, file)
        }
        "EnterPlanMode" => "🔍 Planning...".to_string(),
        "ExitPlanMode" => "📋 Plan complete".to_string(),
        _ => {
            // Full parameter - no truncation
            path_field(input)
                .or_else(|| first_string_field(input, &["command", "cmd", "query", "pattern"]))
                .unwrap_or("")
                .to_string()
        }
    }
}

/// Truncate a line to the requested terminal display width with an ellipsis.
pub fn truncate_line(s: &str, max_width: usize) -> String {
    let trimmed = s.trim();
    if UnicodeWidthStr::width(trimmed) <= max_width {
        trimmed.to_string()
    } else if max_width == 0 {
        String::new()
    } else {
        let ellipsis = '…';
        let ellipsis_width = UnicodeWidthChar::width(ellipsis).unwrap_or(1);
        if max_width <= ellipsis_width {
            return ellipsis.to_string();
        }

        let content_width = max_width - ellipsis_width;
        let mut output = String::new();
        let mut used_width = 0usize;
        for ch in trimmed.chars() {
            let width = UnicodeWidthChar::width(ch).unwrap_or(1);
            if used_width + width > content_width {
                break;
            }
            output.push(ch);
            used_width += width;
        }

        output.push(ellipsis);
        output
    }
}

/// Extract the first file path mentioned by an apply-patch payload.
fn extract_apply_patch_file_path(patch: &str) -> Option<String> {
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("*** Update File: ") {
            return Some(rest.trim().to_string());
        }
        if let Some(rest) = line.strip_prefix("*** Add File: ") {
            return Some(rest.trim().to_string());
        }
        if let Some(rest) = line.strip_prefix("*** Delete File: ") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Describe a `write_stdin` call as a short terminal action.
fn describe_write_stdin_action(input: &serde_json::Value) -> String {
    let session_suffix = input
        .get("session_id")
        .map(|v| match v {
            serde_json::Value::String(s) => format!(" {}", s),
            serde_json::Value::Number(n) => format!(" {}", n),
            _ => String::new(),
        })
        .unwrap_or_default();
    let chars = input.get("chars").and_then(|v| v.as_str()).unwrap_or("");
    if chars.is_empty() {
        return format!("poll session{session_suffix}");
    }
    if chars == "\u{3}" {
        return format!("send Ctrl-C to session{session_suffix}");
    }
    let escaped = chars.escape_default().to_string();
    let preview = if escaped.chars().count() > 32 {
        format!("{}...", escaped.chars().take(29).collect::<String>())
    } else {
        escaped
    };
    format!("send \"{preview}\" to session{session_suffix}")
}

#[cfg(test)]
/// Tests tool display names, parameter extraction, and terminal-width truncation.
mod tests {
    use super::*;
    use serde_json::json;

    // ═══════════════════════════════════════════════════════════════════
    // tool_display_name
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies uppercase grep tools are labeled as search actions.
    #[test]
    fn display_name_grep_uppercase() {
        assert_eq!(tool_display_name("Grep"), "Search");
    }

    /// Verifies lowercase grep tools are labeled as search actions.
    #[test]
    fn display_name_grep_lowercase() {
        assert_eq!(tool_display_name("grep"), "Search");
    }

    /// Verifies uppercase glob tools are labeled as find actions.
    #[test]
    fn display_name_glob_uppercase() {
        assert_eq!(tool_display_name("Glob"), "Find");
    }

    /// Verifies lowercase glob tools are labeled as find actions.
    #[test]
    fn display_name_glob_lowercase() {
        assert_eq!(tool_display_name("glob"), "Find");
    }

    /// Verifies read tools keep their native display label.
    #[test]
    fn display_name_read_passthrough() {
        assert_eq!(tool_display_name("Read"), "Read");
    }

    /// Verifies write tools keep their native display label.
    #[test]
    fn display_name_write_passthrough() {
        assert_eq!(tool_display_name("Write"), "Write");
    }

    /// Verifies bash tools keep their native display label.
    #[test]
    fn display_name_bash_passthrough() {
        assert_eq!(tool_display_name("Bash"), "Bash");
    }

    /// Verifies API exec commands are displayed as bash actions.
    #[test]
    fn display_name_exec_command_maps_to_bash() {
        assert_eq!(tool_display_name("exec_command"), "Bash");
    }

    /// Verifies edit tools keep their native display label.
    #[test]
    fn display_name_edit_passthrough() {
        assert_eq!(tool_display_name("Edit"), "Edit");
    }

    /// Verifies task tools keep their native display label.
    #[test]
    fn display_name_task_passthrough() {
        assert_eq!(tool_display_name("Task"), "Task");
    }

    /// Verifies unknown tool names pass through unchanged.
    #[test]
    fn display_name_unknown_passthrough() {
        assert_eq!(tool_display_name("CustomTool"), "CustomTool");
    }

    /// Verifies empty tool names remain empty.
    #[test]
    fn display_name_empty_string() {
        assert_eq!(tool_display_name(""), "");
    }

    /// Verifies web fetch tools keep their native display label.
    #[test]
    fn display_name_webfetch_passthrough() {
        assert_eq!(tool_display_name("WebFetch"), "WebFetch");
    }

    /// Verifies edit parameters recover a file path from an apply-patch payload.
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

    /// Verifies read tools prefer the `file_path` field.
    #[test]
    fn extract_read_file_path() {
        let input = json!({"file_path": "/src/main.rs"});
        assert_eq!(extract_tool_param("Read", &input), "/src/main.rs");
    }

    /// Verifies read tools fall back to the `path` field.
    #[test]
    fn extract_read_path_fallback() {
        let input = json!({"path": "/src/lib.rs"});
        assert_eq!(extract_tool_param("Read", &input), "/src/lib.rs");
    }

    /// Verifies lowercase read tool names use the same path extraction.
    #[test]
    fn extract_read_lowercase() {
        let input = json!({"file_path": "/foo.rs"});
        assert_eq!(extract_tool_param("read", &input), "/foo.rs");
    }

    /// Verifies read tools return an empty parameter when no path exists.
    #[test]
    fn extract_read_empty_input() {
        let input = json!({});
        assert_eq!(extract_tool_param("Read", &input), "");
    }

    /// Verifies read tools ignore null path values.
    #[test]
    fn extract_read_null_value() {
        let input = json!({"file_path": null});
        assert_eq!(extract_tool_param("Read", &input), "");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Write
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies write tools prefer the `file_path` field.
    #[test]
    fn extract_write_file_path() {
        let input = json!({"file_path": "/out.txt"});
        assert_eq!(extract_tool_param("Write", &input), "/out.txt");
    }

    /// Verifies lowercase write tools fall back to the `path` field.
    #[test]
    fn extract_write_path_fallback() {
        let input = json!({"path": "/out.txt"});
        assert_eq!(extract_tool_param("write", &input), "/out.txt");
    }

    /// Verifies write tools return an empty parameter without path data.
    #[test]
    fn extract_write_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("Write", &input), "");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Edit
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies edit tools prefer the `file_path` field.
    #[test]
    fn extract_edit_file_path() {
        let input = json!({"file_path": "/src/config.rs"});
        assert_eq!(extract_tool_param("Edit", &input), "/src/config.rs");
    }

    /// Verifies lowercase edit tools fall back to the `path` field.
    #[test]
    fn extract_edit_path_fallback() {
        let input = json!({"path": "/src/config.rs"});
        assert_eq!(extract_tool_param("edit", &input), "/src/config.rs");
    }

    /// Verifies edit tools return an empty parameter without path or patch data.
    #[test]
    fn extract_edit_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("Edit", &input), "");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Bash
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies bash tools display the full command string.
    #[test]
    fn extract_bash_command() {
        let input = json!({"command": "cargo build"});
        assert_eq!(extract_tool_param("Bash", &input), "cargo build");
    }

    /// Verifies API exec commands fall back to the `cmd` field.
    #[test]
    fn extract_exec_command_cmd_fallback() {
        let input = json!({"cmd": "pwd"});
        assert_eq!(extract_tool_param("exec_command", &input), "pwd");
    }

    /// Verifies empty `write_stdin` calls are described as session polling.
    #[test]
    fn extract_write_stdin_poll_command() {
        let input = json!({"session_id": 98333, "chars": ""});
        assert_eq!(
            extract_tool_param("write_stdin", &input),
            "poll session 98333"
        );
    }

    /// Verifies lowercase bash tools display the full command string.
    #[test]
    fn extract_bash_lowercase() {
        let input = json!({"command": "ls -la"});
        assert_eq!(extract_tool_param("bash", &input), "ls -la");
    }

    /// Verifies bash tools return an empty parameter without command data.
    #[test]
    fn extract_bash_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("Bash", &input), "");
    }

    /// Verifies long bash commands are not truncated during extraction.
    #[test]
    fn extract_bash_long_command() {
        let cmd = "a".repeat(500);
        let input = json!({"command": cmd});
        assert_eq!(extract_tool_param("Bash", &input), cmd);
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Glob
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies glob tools extract the search pattern.
    #[test]
    fn extract_glob_pattern() {
        let input = json!({"pattern": "**/*.rs"});
        assert_eq!(extract_tool_param("Glob", &input), "**/*.rs");
    }

    /// Verifies lowercase glob tools extract the search pattern.
    #[test]
    fn extract_glob_lowercase() {
        let input = json!({"pattern": "*.txt"});
        assert_eq!(extract_tool_param("glob", &input), "*.txt");
    }

    /// Verifies glob tools return an empty parameter without a pattern.
    #[test]
    fn extract_glob_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("Glob", &input), "");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Grep
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies grep tools extract the search pattern.
    #[test]
    fn extract_grep_pattern() {
        let input = json!({"pattern": "TODO"});
        assert_eq!(extract_tool_param("Grep", &input), "TODO");
    }

    /// Verifies lowercase grep tools extract the search pattern.
    #[test]
    fn extract_grep_lowercase() {
        let input = json!({"pattern": "fn main"});
        assert_eq!(extract_tool_param("grep", &input), "fn main");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — WebFetch
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies web fetch tools extract the URL.
    #[test]
    fn extract_webfetch_url() {
        let input = json!({"url": "https://example.com"});
        assert_eq!(
            extract_tool_param("WebFetch", &input),
            "https://example.com"
        );
    }

    /// Verifies lowercase web fetch tools extract the URL.
    #[test]
    fn extract_webfetch_lowercase() {
        let input = json!({"url": "https://foo.bar"});
        assert_eq!(extract_tool_param("webfetch", &input), "https://foo.bar");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — WebSearch
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies web search tools extract the query.
    #[test]
    fn extract_websearch_query() {
        let input = json!({"query": "rust async"});
        assert_eq!(extract_tool_param("WebSearch", &input), "rust async");
    }

    /// Verifies lowercase web search tools extract the query.
    #[test]
    fn extract_websearch_lowercase() {
        let input = json!({"query": "test query"});
        assert_eq!(extract_tool_param("websearch", &input), "test query");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Task
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies task tools combine subagent type and description.
    #[test]
    fn extract_task_with_type_and_desc() {
        let input = json!({"subagent_type": "code", "description": "refactor module"});
        assert_eq!(extract_tool_param("Task", &input), "[code] refactor module");
    }

    /// Verifies task tools default the subagent label when omitted.
    #[test]
    fn extract_task_default_agent_type() {
        let input = json!({"description": "do something"});
        assert_eq!(extract_tool_param("Task", &input), "[agent] do something");
    }

    /// Verifies task tools preserve the default shape without fields.
    #[test]
    fn extract_task_no_fields() {
        let input = json!({});
        assert_eq!(extract_tool_param("task", &input), "[agent] ");
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — LSP
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies LSP tools combine operation and file path.
    #[test]
    fn extract_lsp_operation_and_file() {
        let input = json!({"operation": "hover", "filePath": "/src/main.rs"});
        assert_eq!(extract_tool_param("LSP", &input), "hover /src/main.rs");
    }

    /// Verifies lowercase LSP tools combine operation and file path.
    #[test]
    fn extract_lsp_lowercase() {
        let input = json!({"operation": "goto", "filePath": "/lib.rs"});
        assert_eq!(extract_tool_param("lsp", &input), "goto /lib.rs");
    }

    /// Verifies LSP tools preserve the empty operation/file separator.
    #[test]
    fn extract_lsp_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("LSP", &input), " ");
    }

    /// Verifies notebook edit tools display their notebook path.
    #[test]
    fn extract_notebook_edit_path() {
        let input = json!({"notebook_path": "/analysis/notebook.ipynb"});
        assert_eq!(
            extract_tool_param("NotebookEdit", &input),
            "/analysis/notebook.ipynb"
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Plan modes
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies enter-plan-mode tools produce a planning label.
    #[test]
    fn extract_enter_plan_mode() {
        let input = json!({});
        assert!(extract_tool_param("EnterPlanMode", &input).contains("Planning"));
    }

    /// Verifies exit-plan-mode tools produce a plan-complete label.
    #[test]
    fn extract_exit_plan_mode() {
        let input = json!({});
        assert!(extract_tool_param("ExitPlanMode", &input).contains("Plan complete"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // extract_tool_param — Unknown tools (fallback)
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies unknown tools prefer the `file_path` fallback.
    #[test]
    fn extract_unknown_tool_file_path() {
        let input = json!({"file_path": "/x.rs"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "/x.rs");
    }

    /// Verifies unknown tools fall back to the `path` field.
    #[test]
    fn extract_unknown_tool_path() {
        let input = json!({"path": "/y.rs"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "/y.rs");
    }

    /// Verifies unknown tools fall back to the `target_file` field.
    #[test]
    fn extract_unknown_tool_target_file() {
        let input = json!({"target_file": "/target.rs"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "/target.rs");
    }

    /// Verifies unknown tools fall back to the `relative_path` field.
    #[test]
    fn extract_unknown_tool_relative_path() {
        let input = json!({"relative_path": "src/lib.rs"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "src/lib.rs");
    }

    /// Verifies unknown tools fall back to the camel-case `filePath` field.
    #[test]
    fn extract_unknown_tool_file_path_camel_case() {
        let input = json!({"filePath": "/camel.rs"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "/camel.rs");
    }

    /// Verifies unknown tools fall back to the `command` field.
    #[test]
    fn extract_unknown_tool_command() {
        let input = json!({"command": "echo hi"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "echo hi");
    }

    /// Verifies unknown tools fall back to the `cmd` field.
    #[test]
    fn extract_unknown_tool_cmd() {
        let input = json!({"cmd": "pwd"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "pwd");
    }

    /// Verifies unknown tools fall back to the `query` field.
    #[test]
    fn extract_unknown_tool_query() {
        let input = json!({"query": "something"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "something");
    }

    /// Verifies unknown tools fall back to the `pattern` field.
    #[test]
    fn extract_unknown_tool_pattern() {
        let input = json!({"pattern": "*.md"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "*.md");
    }

    /// Verifies unknown tools return an empty parameter without known fields.
    #[test]
    fn extract_unknown_tool_empty() {
        let input = json!({});
        assert_eq!(extract_tool_param("UnknownTool", &input), "");
    }

    /// Verifies unknown tools prefer file paths over lower-priority fallbacks.
    #[test]
    fn extract_unknown_tool_priority_file_path_first() {
        let input = json!({"file_path": "/first", "command": "second"});
        assert_eq!(extract_tool_param("UnknownTool", &input), "/first");
    }

    // ═══════════════════════════════════════════════════════════════════
    // truncate_line
    // ═══════════════════════════════════════════════════════════════════

    /// Verifies text that exactly fits the width is unchanged.
    #[test]
    fn truncate_fits_exactly() {
        assert_eq!(truncate_line("hello", 5), "hello");
    }

    /// Verifies text shorter than the width is unchanged.
    #[test]
    fn truncate_shorter_than_max() {
        assert_eq!(truncate_line("hi", 10), "hi");
    }

    /// Verifies overflowing ASCII text is ellipsized to the width.
    #[test]
    fn truncate_over_max() {
        assert_eq!(truncate_line("hello world", 5), "hell\u{2026}");
    }

    /// Verifies a one-column budget renders only the ellipsis.
    #[test]
    fn truncate_max_one() {
        assert_eq!(truncate_line("hello", 1), "\u{2026}");
    }

    /// Verifies empty text remains empty.
    #[test]
    fn truncate_empty_string() {
        assert_eq!(truncate_line("", 10), "");
    }

    /// Verifies surrounding whitespace is trimmed before truncation.
    #[test]
    fn truncate_trims_whitespace() {
        assert_eq!(truncate_line("  hello  ", 10), "hello");
    }

    /// Verifies trimmed text is still ellipsized when it exceeds the width.
    #[test]
    fn truncate_trims_then_truncates() {
        assert_eq!(truncate_line("  hello world  ", 5), "hell\u{2026}");
    }

    /// Verifies wide Unicode text is truncated by display width.
    #[test]
    fn truncate_unicode_chars() {
        assert_eq!(
            truncate_line("\u{65e5}\u{672c}\u{8a9e}", 3),
            "\u{65e5}\u{2026}"
        );
    }

    /// Verifies longer wide Unicode text stays inside the display budget.
    #[test]
    fn truncate_unicode_over_max() {
        let truncated = truncate_line("\u{65e5}\u{672c}\u{8a9e}\u{30c6}\u{30b9}\u{30c8}", 4);
        assert_eq!(truncated, "\u{65e5}\u{2026}");
        assert!(UnicodeWidthStr::width(truncated.as_str()) <= 4);
    }

    /// Verifies a zero-column budget returns no visible text.
    #[test]
    fn truncate_max_zero() {
        assert_eq!(truncate_line("hello", 0), "");
    }

    /// Verifies one-character text that fits is unchanged.
    #[test]
    fn truncate_single_char_fits() {
        assert_eq!(truncate_line("a", 1), "a");
    }

    /// Verifies multi-character text with a one-column budget renders an ellipsis.
    #[test]
    fn truncate_two_chars_max_one() {
        assert_eq!(truncate_line("ab", 1), "\u{2026}");
    }

    /// Verifies ASCII punctuation is preserved when it fits.
    #[test]
    fn truncate_preserves_special_chars() {
        assert_eq!(truncate_line("@#$%^", 5), "@#$%^");
    }

    /// Verifies emoji text is ellipsized without exceeding the column budget.
    #[test]
    fn truncate_emoji() {
        let s = "\u{1f389}\u{1f38a}\u{1f388}\u{1f381}";
        assert_eq!(truncate_line(s, 2), "\u{2026}");
    }
}
