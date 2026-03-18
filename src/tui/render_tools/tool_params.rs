//! Tool parameter extraction and display name mapping
//!
//! Extracts the most relevant parameter from tool inputs for display in the
//! session pane, maps internal tool names to user-friendly labels, and provides
//! line truncation utilities.

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
        "Read" | "read" => input
            .get("file_path")
            .or_else(|| input.get("path"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "Write" | "write" => input
            .get("file_path")
            .or_else(|| input.get("path"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "Edit" | "edit" => input
            .get("file_path")
            .or_else(|| input.get("path"))
            .and_then(|v| v.as_str())
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
            let file = input.get("filePath").and_then(|v| v.as_str()).unwrap_or("");
            format!("{} {}", op, file)
        }
        "EnterPlanMode" => "🔍 Planning...".to_string(),
        "ExitPlanMode" => "📋 Plan complete".to_string(),
        _ => {
            // Full parameter - no truncation
            input
                .get("file_path")
                .or_else(|| input.get("path"))
                .or_else(|| input.get("command"))
                .or_else(|| input.get("cmd"))
                .or_else(|| input.get("query"))
                .or_else(|| input.get("pattern"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
    }
}

/// Truncate a line to max length with ellipsis indicator
pub fn truncate_line(s: &str, max_len: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max_len {
        trimmed.to_string()
    } else if max_len > 1 {
        format!("{}…", trimmed.chars().take(max_len - 1).collect::<String>())
    } else {
        "…".to_string()
    }
}

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
mod tests {
    use super::*;
    use serde_json::json;

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
}
