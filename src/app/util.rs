//! Utility functions for app module

/// Maximum number of visible characters shown for a tool parameter.
const TOOL_PARAM_MAX_CHARS: usize = 60;

/// Number of parameter characters retained before appending an ellipsis.
const TOOL_PARAM_TRUNCATED_CHARS: usize = TOOL_PARAM_MAX_CHARS - 3;

/// Strip ANSI escape sequences from text
pub fn strip_ansi_escapes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek() {
                Some(&'[') => {
                    // CSI sequence: \x1b[...letter
                    chars.next();
                    while let Some(&next_ch) = chars.peek() {
                        chars.next();
                        if next_ch.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(&']') => {
                    // OSC sequence: \x1b]...(\x07 or \x1b\\)
                    chars.next();
                    while let Some(&next_ch) = chars.peek() {
                        if next_ch == '\x07' {
                            chars.next();
                            break;
                        }
                        if next_ch == '\x1b' {
                            chars.next();
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                        chars.next();
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Return a display-safe tool parameter capped by character count.
fn truncate_tool_param(s: &str) -> String {
    if s.chars().count() > TOOL_PARAM_MAX_CHARS {
        let prefix = s
            .chars()
            .take(TOOL_PARAM_TRUNCATED_CHARS)
            .collect::<String>();
        format!("{prefix}...")
    } else {
        s.to_string()
    }
}

/// Extract a non-negative result cost from event JSON for display.
fn display_result_cost_usd(json: &serde_json::Value) -> f64 {
    match json.get("total_cost_usd").and_then(|c| c.as_f64()) {
        Some(cost) if cost.is_finite() && cost > 0.0 => cost,
        _ => 0.0,
    }
}

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

/// Extract the most relevant parameter from a tool's input for display
fn extract_tool_param(tool_name: &str, input: Option<&serde_json::Value>) -> String {
    let input = match input {
        Some(v) => v,
        None => return String::new(),
    };

    let result = match tool_name {
        "Read" | "read" | "Write" | "write" | "Edit" | "edit" | "NotebookEdit" | "notebookedit" => {
            path_field(input)
        }
        "Bash" | "bash" | "exec_command" => first_string_field(input, &["command", "cmd"]),
        "Glob" | "glob" => input.get("pattern").and_then(|v| v.as_str()),
        "Grep" | "grep" => input.get("pattern").and_then(|v| v.as_str()),
        "Task" | "task" => input.get("description").and_then(|v| v.as_str()),
        "WebFetch" | "webfetch" => input.get("url").and_then(|v| v.as_str()),
        "WebSearch" | "websearch" => input.get("query").and_then(|v| v.as_str()),
        "LSP" | "lsp" => {
            let op = input
                .get("operation")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let file = path_field(input).unwrap_or("");
            return format!("{} {}", op, file);
        }
        _ => path_field(input)
            .or_else(|| first_string_field(input, &["command", "cmd", "query", "pattern"])),
    };

    match result {
        Some(s) => truncate_tool_param(s),
        None => String::new(),
    }
}

/// Parse stream-json output and extract human-readable content.
/// Returns None if the line should not be displayed.
/// Convenience wrapper over `display_text_from_json` for callers with raw strings.
#[allow(dead_code)]
pub fn parse_stream_json_for_display(line: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    display_text_from_json(&json)
}

/// Extract human-readable display text from a pre-parsed JSON value.
/// Avoids re-parsing the same line when the caller already has a Value.
pub fn display_text_from_json(json: &serde_json::Value) -> Option<String> {
    let event_type = json.get("type")?.as_str()?;

    match event_type {
        "system" => {
            let subtype = json.get("subtype").and_then(|s| s.as_str())?;
            match subtype {
                "init" => {
                    let model = json
                        .get("model")
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown");
                    let cwd = json.get("cwd").and_then(|c| c.as_str()).unwrap_or("");
                    Some(format!("[Session started | {} | {}]\n", model, cwd))
                }
                "hook_response" => {
                    let hook_name = json
                        .get("hook_name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("hook");
                    let output = json.get("output").and_then(|o| o.as_str()).unwrap_or("");
                    if output.is_empty() {
                        Some(format!("[Hook: {}]\n", hook_name))
                    } else {
                        Some(format!("[Hook: {} | {}]\n", hook_name, output.trim()))
                    }
                }
                _ => None,
            }
        }
        "user" => {
            let raw_content = json.get("message")?.get("content")?.as_str()?;
            let content =
                crate::app::context_injection::sanitize_user_message_content(raw_content)?;
            if content.starts_with("This session is being continued from a previous conversation") {
                return Some("[Context compacted]\n".to_string());
            }
            if content.contains("<local-command-stdout>")
                || content.contains("<local-command-caveat>")
            {
                return None;
            }
            Some(format!("You: {}\n", content))
        }
        "assistant" => {
            let content = json.get("message")?.get("content")?.as_array()?;
            let mut text_parts = Vec::new();

            for block in content {
                if let Some(block_type) = block.get("type").and_then(|t| t.as_str()) {
                    match block_type {
                        "text" => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                text_parts.push(text.to_string());
                            }
                        }
                        "tool_use" => {
                            let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                            let input = block.get("input");
                            let param = extract_tool_param(name, input);
                            if param.is_empty() {
                                text_parts.push(format!("[Using {}...]", name));
                            } else {
                                text_parts.push(format!("[Using {} | {}]", name, param));
                            }
                        }
                        _ => {}
                    }
                }
            }

            if text_parts.is_empty() {
                None
            } else {
                Some(format!("Claude: {}\n", text_parts.join("\n")))
            }
        }
        "result" => {
            let duration = json
                .get("duration_ms")
                .and_then(|d| d.as_u64())
                .unwrap_or(0);
            let cost = display_result_cost_usd(json);
            Some(format!(
                "[Done: {:.1}s, ${:.4}]\n",
                duration as f64 / 1000.0,
                cost
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
/// Tests for display utility parsing, ANSI stripping, and tool parameter formatting.
mod tests {
    use super::*;
    use serde_json::json;

    // ── strip_ansi_escapes ──

    /// Verifies strip plain text.
    #[test]
    fn test_strip_plain_text() {
        assert_eq!(strip_ansi_escapes("hello world"), "hello world");
    }

    /// Verifies strip empty string.
    #[test]
    fn test_strip_empty_string() {
        assert_eq!(strip_ansi_escapes(""), "");
    }

    /// Verifies strip csi color codes.
    #[test]
    fn test_strip_csi_color_codes() {
        assert_eq!(strip_ansi_escapes("\x1b[31mred text\x1b[0m"), "red text");
    }

    /// Verifies strip csi bold.
    #[test]
    fn test_strip_csi_bold() {
        assert_eq!(strip_ansi_escapes("\x1b[1mbold\x1b[0m"), "bold");
    }

    /// Verifies strip csi cursor movement.
    #[test]
    fn test_strip_csi_cursor_movement() {
        assert_eq!(strip_ansi_escapes("\x1b[2Jhello\x1b[H"), "hello");
    }

    /// Verifies strip osc title.
    #[test]
    fn test_strip_osc_title() {
        assert_eq!(strip_ansi_escapes("\x1b]0;My Title\x07text"), "text");
    }

    /// Verifies strip osc with st terminator.
    #[test]
    fn test_strip_osc_with_st_terminator() {
        assert_eq!(strip_ansi_escapes("\x1b]0;title\x1b\\after"), "after");
    }

    /// Verifies strip mixed ansi.
    #[test]
    fn test_strip_mixed_ansi() {
        let input = "\x1b[32m✓\x1b[0m Pass \x1b[31m✗\x1b[0m Fail";
        assert_eq!(strip_ansi_escapes(input), "✓ Pass ✗ Fail");
    }

    /// Verifies strip multiple params csi.
    #[test]
    fn test_strip_multiple_params_csi() {
        assert_eq!(strip_ansi_escapes("\x1b[38;2;255;0;0mred\x1b[0m"), "red");
    }

    /// Verifies strip unknown escape.
    #[test]
    fn test_strip_unknown_escape() {
        // Unknown char after ESC: skips ESC + the next char, rest remains
        assert_eq!(strip_ansi_escapes("\x1b(Bhello"), "Bhello");
    }

    // ── extract_tool_param ──

    /// Verifies extract read file path.
    #[test]
    fn test_extract_read_file_path() {
        let input = json!({"file_path": "/src/main.rs"});
        assert_eq!(extract_tool_param("Read", Some(&input)), "/src/main.rs");
    }

    /// Verifies extract bash command.
    #[test]
    fn test_extract_bash_command() {
        let input = json!({"command": "cargo build"});
        assert_eq!(extract_tool_param("Bash", Some(&input)), "cargo build");
    }

    /// Verifies extract grep pattern.
    #[test]
    fn test_extract_grep_pattern() {
        let input = json!({"pattern": "fn main"});
        assert_eq!(extract_tool_param("Grep", Some(&input)), "fn main");
    }

    /// Verifies extract glob pattern.
    #[test]
    fn test_extract_glob_pattern() {
        let input = json!({"pattern": "**/*.rs"});
        assert_eq!(extract_tool_param("Glob", Some(&input)), "**/*.rs");
    }

    /// Verifies extract task description.
    #[test]
    fn test_extract_task_description() {
        let input = json!({"description": "Search for tests"});
        assert_eq!(extract_tool_param("Task", Some(&input)), "Search for tests");
    }

    /// Verifies extract webfetch url.
    #[test]
    fn test_extract_webfetch_url() {
        let input = json!({"url": "https://example.com"});
        assert_eq!(
            extract_tool_param("WebFetch", Some(&input)),
            "https://example.com"
        );
    }

    /// Verifies extract websearch query.
    #[test]
    fn test_extract_websearch_query() {
        let input = json!({"query": "rust async"});
        assert_eq!(extract_tool_param("WebSearch", Some(&input)), "rust async");
    }

    /// Verifies extract lsp operation.
    #[test]
    fn test_extract_lsp_operation() {
        let input = json!({"operation": "definition", "filePath": "/src/lib.rs"});
        assert_eq!(
            extract_tool_param("LSP", Some(&input)),
            "definition /src/lib.rs"
        );
    }

    /// Verifies extract none input.
    #[test]
    fn test_extract_none_input() {
        assert_eq!(extract_tool_param("Read", None), "");
    }

    /// Verifies extract missing field.
    #[test]
    fn test_extract_missing_field() {
        let input = json!({"other": "value"});
        assert_eq!(extract_tool_param("Read", Some(&input)), "");
    }

    /// Verifies extract truncation at 60.
    #[test]
    fn test_extract_truncation_at_60() {
        let long_path = format!("/very/long/path/{}", "a".repeat(100));
        let input = json!({"file_path": long_path});
        let result = extract_tool_param("Read", Some(&input));
        assert_eq!(result.len(), 60);
        assert!(result.ends_with("..."));
    }

    /// Verifies truncating a multibyte parameter does not slice inside a character.
    #[test]
    fn test_extract_truncation_handles_multibyte_characters() {
        let long_path = format!("/tmp/{}", "日本語".repeat(30));
        let input = json!({"file_path": long_path});
        let result = extract_tool_param("Read", Some(&input));
        assert_eq!(result.chars().count(), 60);
        assert!(result.ends_with("..."));
    }

    /// Verifies extract unknown tool fallback.
    #[test]
    fn test_extract_unknown_tool_fallback() {
        let input = json!({"file_path": "/some/file.txt"});
        assert_eq!(
            extract_tool_param("UnknownTool", Some(&input)),
            "/some/file.txt"
        );
    }

    /// Verifies extract case insensitive tools.
    #[test]
    fn test_extract_case_insensitive_tools() {
        let input = json!({"file_path": "/test.rs"});
        assert_eq!(extract_tool_param("read", Some(&input)), "/test.rs");
        assert_eq!(extract_tool_param("write", Some(&input)), "/test.rs");
        assert_eq!(extract_tool_param("edit", Some(&input)), "/test.rs");
    }

    // ── display_text_from_json ──

    /// Verifies display init event.
    #[test]
    fn test_display_init_event() {
        let json = json!({
            "type": "system",
            "subtype": "init",
            "model": "claude-opus-4-6",
            "cwd": "/home/user/project"
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("claude-opus-4-6"));
        assert!(result.contains("/home/user/project"));
        assert!(result.starts_with("[Session started"));
    }

    /// Verifies display hook response.
    #[test]
    fn test_display_hook_response() {
        let json = json!({
            "type": "system",
            "subtype": "hook_response",
            "hook_name": "pre-commit",
            "output": "All checks passed"
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("pre-commit"));
        assert!(result.contains("All checks passed"));
    }

    /// Verifies display hook empty output.
    #[test]
    fn test_display_hook_empty_output() {
        let json = json!({
            "type": "system",
            "subtype": "hook_response",
            "hook_name": "post-build",
            "output": ""
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("post-build"));
        assert!(!result.contains("|"));
    }

    /// Verifies display user message.
    #[test]
    fn test_display_user_message() {
        let json = json!({
            "type": "user",
            "message": {
                "content": "Hello Claude"
            }
        });
        let result = display_text_from_json(&json).unwrap();
        assert_eq!(result, "You: Hello Claude\n");
    }

    /// Verifies display user message strips injected context.
    #[test]
    fn test_display_user_message_strips_injected_context() {
        let json = json!({
            "type": "user",
            "message": {
                "content": format!(
                    "{}\nprior hidden context\n{}\n\nactual prompt",
                    crate::app::context_injection::CONTEXT_OPEN,
                    crate::app::context_injection::CONTEXT_CLOSE,
                )
            }
        });
        let result = display_text_from_json(&json).unwrap();
        assert_eq!(result, "You: actual prompt\n");
    }

    /// Verifies display user message hides hidden codex context.
    #[test]
    fn test_display_user_message_hides_hidden_codex_context() {
        let json = json!({
            "type": "user",
            "message": {
                "content": concat!(
                    "# AGENTS.md instructions for /tmp/project\n",
                    "<INSTRUCTIONS>\n",
                    "hidden\n",
                    "</INSTRUCTIONS>"
                )
            }
        });
        assert!(display_text_from_json(&json).is_none());
    }

    /// Verifies display assistant text.
    #[test]
    fn test_display_assistant_text() {
        let json = json!({
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "text", "text": "Here is the answer."}
                ]
            }
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.starts_with("Claude: "));
        assert!(result.contains("Here is the answer."));
    }

    /// Verifies display assistant tool use.
    #[test]
    fn test_display_assistant_tool_use() {
        let json = json!({
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "tool_use", "name": "Read", "input": {"file_path": "/src/main.rs"}}
                ]
            }
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("[Using Read | /src/main.rs]"));
    }

    /// Verifies display assistant tool no param.
    #[test]
    fn test_display_assistant_tool_no_param() {
        let json = json!({
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "tool_use", "name": "SomeTool", "input": {}}
                ]
            }
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("[Using SomeTool...]"));
    }

    /// Verifies display assistant mixed content.
    #[test]
    fn test_display_assistant_mixed_content() {
        let json = json!({
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "text", "text": "Let me check."},
                    {"type": "tool_use", "name": "Bash", "input": {"command": "ls -la"}}
                ]
            }
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("Let me check."));
        assert!(result.contains("[Using Bash | ls -la]"));
    }

    /// Verifies display assistant empty content.
    #[test]
    fn test_display_assistant_empty_content() {
        let json = json!({
            "type": "assistant",
            "message": {
                "content": []
            }
        });
        assert!(display_text_from_json(&json).is_none());
    }

    /// Verifies display result event.
    #[test]
    fn test_display_result_event() {
        let json = json!({
            "type": "result",
            "duration_ms": 5000,
            "total_cost_usd": 0.0123
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("5.0s"));
        assert!(result.contains("$0.0123"));
    }

    /// Verifies display unknown type.
    #[test]
    fn test_display_unknown_type() {
        let json = json!({"type": "unknown_event"});
        assert!(display_text_from_json(&json).is_none());
    }

    /// Verifies display missing type.
    #[test]
    fn test_display_missing_type() {
        let json = json!({"data": "something"});
        assert!(display_text_from_json(&json).is_none());
    }

    /// Verifies display system unknown subtype.
    #[test]
    fn test_display_system_unknown_subtype() {
        let json = json!({
            "type": "system",
            "subtype": "unknown_system_event"
        });
        assert!(display_text_from_json(&json).is_none());
    }

    // ── parse_stream_json_for_display ──

    /// Verifies parse stream json valid.
    #[test]
    fn test_parse_stream_json_valid() {
        let line = r#"{"type":"user","message":{"content":"hi"}}"#;
        let result = parse_stream_json_for_display(line).unwrap();
        assert_eq!(result, "You: hi\n");
    }

    /// Verifies parse stream json invalid.
    #[test]
    fn test_parse_stream_json_invalid() {
        assert!(parse_stream_json_for_display("not json").is_none());
    }

    /// Verifies parse stream json whitespace.
    #[test]
    fn test_parse_stream_json_whitespace() {
        let line = r#"  {"type":"result","duration_ms":1000,"total_cost_usd":0.01}  "#;
        let result = parse_stream_json_for_display(line).unwrap();
        assert!(result.contains("1.0s"));
    }

    // ── strip_ansi_escapes: more edge cases ──

    /// Verifies strip nested escapes.
    #[test]
    fn test_strip_nested_escapes() {
        // Bold + color nested: \e[1m\e[31m text \e[0m
        let input = "\x1b[1m\x1b[31mhello\x1b[0m\x1b[0m";
        assert_eq!(strip_ansi_escapes(input), "hello");
    }

    /// Verifies strip partial escape just esc.
    #[test]
    fn test_strip_partial_escape_just_esc() {
        // Just ESC char alone at end of string
        let input = "text\x1b";
        // ESC at end: no next char to peek, loop ends
        assert_eq!(strip_ansi_escapes(input), "text");
    }

    /// Verifies strip esc at end of string.
    #[test]
    fn test_strip_esc_at_end_of_string() {
        let input = "hello\x1b";
        assert_eq!(strip_ansi_escapes(input), "hello");
    }

    /// Verifies strip multiple consecutive csi escapes.
    #[test]
    fn test_strip_multiple_consecutive_csi_escapes() {
        let input = "\x1b[1m\x1b[4m\x1b[31m\x1b[42mtext\x1b[0m";
        assert_eq!(strip_ansi_escapes(input), "text");
    }

    /// Verifies strip real terminal output git diff.
    #[test]
    fn test_strip_real_terminal_output_git_diff() {
        // Simulated git diff output with colors
        let input = "\x1b[32m+  fn new()\x1b[0m\n\x1b[31m-  fn old()\x1b[0m";
        assert_eq!(strip_ansi_escapes(input), "+  fn new()\n-  fn old()");
    }

    /// Verifies strip real terminal output cargo.
    #[test]
    fn test_strip_real_terminal_output_cargo() {
        let input = "\x1b[32m   Compiling\x1b[0m myproject v0.1.0";
        assert_eq!(strip_ansi_escapes(input), "   Compiling myproject v0.1.0");
    }

    /// Verifies strip 256 color code.
    #[test]
    fn test_strip_256_color_code() {
        let input = "\x1b[38;5;196mred\x1b[0m";
        assert_eq!(strip_ansi_escapes(input), "red");
    }

    /// Verifies strip rgb color code.
    #[test]
    fn test_strip_rgb_color_code() {
        let input = "\x1b[38;2;255;128;0morange\x1b[0m";
        assert_eq!(strip_ansi_escapes(input), "orange");
    }

    /// Verifies strip osc hyperlink.
    #[test]
    fn test_strip_osc_hyperlink() {
        // OSC 8 hyperlink: \e]8;;url\e\\text\e]8;;\e\\
        let input = "\x1b]8;;https://example.com\x1b\\Click here\x1b]8;;\x1b\\";
        assert_eq!(strip_ansi_escapes(input), "Click here");
    }

    /// Verifies strip only escape sequences.
    #[test]
    fn test_strip_only_escape_sequences() {
        let input = "\x1b[31m\x1b[0m\x1b[1m\x1b[0m";
        assert_eq!(strip_ansi_escapes(input), "");
    }

    /// Verifies strip unicode content preserved.
    #[test]
    fn test_strip_unicode_content_preserved() {
        let input = "\x1b[32m日本語テキスト\x1b[0m";
        assert_eq!(strip_ansi_escapes(input), "日本語テキスト");
    }

    /// Verifies strip emoji content preserved.
    #[test]
    fn test_strip_emoji_content_preserved() {
        let input = "\x1b[1m🚀 Launch!\x1b[0m";
        assert_eq!(strip_ansi_escapes(input), "🚀 Launch!");
    }

    /// Verifies strip csi erase line.
    #[test]
    fn test_strip_csi_erase_line() {
        let input = "\x1b[2Koverwritten text";
        assert_eq!(strip_ansi_escapes(input), "overwritten text");
    }

    // ── extract_tool_param: more tools ──

    /// Verifies extract notebook edit.
    #[test]
    fn test_extract_notebook_edit() {
        let input = json!({"notebook_path": "/nb.ipynb", "new_source": "print(1)"});
        let result = extract_tool_param("NotebookEdit", Some(&input));
        assert_eq!(result, "/nb.ipynb");
    }

    /// Verifies extract todo write.
    #[test]
    fn test_extract_todo_write() {
        let input = json!({"todos": [{"content": "Task 1"}]});
        let result = extract_tool_param("TodoWrite", Some(&input));
        assert_eq!(result, "");
    }

    /// Verifies extract write with content.
    #[test]
    fn test_extract_write_with_content() {
        let input = json!({"file_path": "/out.txt", "content": "Hello world"});
        let result = extract_tool_param("Write", Some(&input));
        assert_eq!(result, "/out.txt");
    }

    /// Verifies extract numeric values ignored.
    #[test]
    fn test_extract_numeric_values_ignored() {
        let input = json!({"file_path": 42});
        let result = extract_tool_param("Read", Some(&input));
        // as_str returns None for numeric values
        assert_eq!(result, "");
    }

    /// Verifies extract nested object ignored.
    #[test]
    fn test_extract_nested_object_ignored() {
        let input = json!({"file_path": {"nested": "value"}});
        let result = extract_tool_param("Read", Some(&input));
        assert_eq!(result, "");
    }

    /// Verifies extract lsp missing fields.
    #[test]
    fn test_extract_lsp_missing_fields() {
        let input = json!({});
        let result = extract_tool_param("LSP", Some(&input));
        assert_eq!(result, " ");
    }

    /// Verifies extract lsp partial fields.
    #[test]
    fn test_extract_lsp_partial_fields() {
        let input = json!({"operation": "hover"});
        let result = extract_tool_param("LSP", Some(&input));
        assert_eq!(result, "hover ");
    }

    /// Verifies extract exact 60 chars no truncation.
    #[test]
    fn test_extract_exact_60_chars_no_truncation() {
        let path = format!("/path/{}", "x".repeat(54)); // "/path/" = 6 + 54 = 60
        let input = json!({"file_path": path});
        let result = extract_tool_param("Read", Some(&input));
        assert_eq!(result.len(), 60);
        assert!(!result.ends_with("..."));
    }

    /// Verifies extract 61 chars truncated.
    #[test]
    fn test_extract_61_chars_truncated() {
        let path = format!("/path/{}", "x".repeat(55)); // "/path/" = 6 + 55 = 61
        let input = json!({"file_path": path});
        let result = extract_tool_param("Read", Some(&input));
        assert_eq!(result.len(), 60);
        assert!(result.ends_with("..."));
    }

    /// Verifies extract bash lowercase.
    #[test]
    fn test_extract_bash_lowercase() {
        let input = json!({"command": "echo hello"});
        assert_eq!(extract_tool_param("bash", Some(&input)), "echo hello");
    }

    /// Verifies extract unknown tool query fallback.
    #[test]
    fn test_extract_unknown_tool_query_fallback() {
        let input = json!({"query": "search term"});
        assert_eq!(
            extract_tool_param("CustomTool", Some(&input)),
            "search term"
        );
    }

    /// Verifies extract unknown tool pattern fallback.
    #[test]
    fn test_extract_unknown_tool_pattern_fallback() {
        let input = json!({"pattern": "*.rs"});
        assert_eq!(extract_tool_param("CustomTool", Some(&input)), "*.rs");
    }

    /// Verifies extract unknown tool target file fallback.
    #[test]
    fn test_extract_unknown_tool_target_file_fallback() {
        let input = json!({"target_file": "/tmp/target.rs"});
        assert_eq!(
            extract_tool_param("CustomTool", Some(&input)),
            "/tmp/target.rs"
        );
    }

    /// Verifies extract unknown tool relative path fallback.
    #[test]
    fn test_extract_unknown_tool_relative_path_fallback() {
        let input = json!({"relative_path": "src/main.rs"});
        assert_eq!(
            extract_tool_param("CustomTool", Some(&input)),
            "src/main.rs"
        );
    }

    /// Verifies extract unknown tool command fallback.
    #[test]
    fn test_extract_unknown_tool_command_fallback() {
        let input = json!({"command": "run"});
        assert_eq!(extract_tool_param("CustomTool", Some(&input)), "run");
    }

    /// Verifies extract unknown tool cmd fallback.
    #[test]
    fn test_extract_unknown_tool_cmd_fallback() {
        let input = json!({"cmd": "pwd"});
        assert_eq!(extract_tool_param("CustomTool", Some(&input)), "pwd");
    }

    // ── display_text_from_json: more edge cases ──

    /// Verifies display init missing model.
    #[test]
    fn test_display_init_missing_model() {
        let json = json!({
            "type": "system",
            "subtype": "init",
            "cwd": "/proj"
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("unknown"));
        assert!(result.contains("/proj"));
    }

    /// Verifies display init missing cwd.
    #[test]
    fn test_display_init_missing_cwd() {
        let json = json!({
            "type": "system",
            "subtype": "init",
            "model": "opus"
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("opus"));
        assert!(result.contains("| ]")); // cwd defaults to empty string
    }

    /// Verifies display result zero duration.
    #[test]
    fn test_display_result_zero_duration() {
        let json = json!({
            "type": "result",
            "duration_ms": 0,
            "total_cost_usd": 0.0
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("0.0s"));
        assert!(result.contains("$0.0000"));
    }

    /// Verifies display result missing fields default.
    #[test]
    fn test_display_result_missing_fields_default() {
        let json = json!({"type": "result"});
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("0.0s"));
        assert!(result.contains("$0.0000"));
    }

    /// Verifies corrupt negative result costs render as zero dollars.
    #[test]
    fn test_display_result_negative_cost_defaults_to_zero() {
        let json = json!({
            "type": "result",
            "duration_ms": 1000,
            "total_cost_usd": -1.25
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("1.0s"));
        assert!(result.contains("$0.0000"));
    }

    /// Verifies display assistant unknown block type.
    #[test]
    fn test_display_assistant_unknown_block_type() {
        let json = json!({
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "thinking", "text": "internal thought"}
                ]
            }
        });
        // Unknown block types are skipped, resulting in no text_parts → None
        assert!(display_text_from_json(&json).is_none());
    }

    /// Verifies display assistant multiple text blocks.
    #[test]
    fn test_display_assistant_multiple_text_blocks() {
        let json = json!({
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "text", "text": "First paragraph."},
                    {"type": "text", "text": "Second paragraph."}
                ]
            }
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("First paragraph."));
        assert!(result.contains("Second paragraph."));
    }

    /// Verifies display assistant text and unknown.
    #[test]
    fn test_display_assistant_text_and_unknown() {
        let json = json!({
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "text", "text": "Visible"},
                    {"type": "image", "source": "data:..."}
                ]
            }
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("Visible"));
    }

    /// Verifies display user message missing content.
    #[test]
    fn test_display_user_message_missing_content() {
        let json = json!({
            "type": "user",
            "message": {}
        });
        assert!(display_text_from_json(&json).is_none());
    }

    /// Verifies display user message missing message.
    #[test]
    fn test_display_user_message_missing_message() {
        let json = json!({"type": "user"});
        assert!(display_text_from_json(&json).is_none());
    }

    /// Verifies display assistant missing message.
    #[test]
    fn test_display_assistant_missing_message() {
        let json = json!({"type": "assistant"});
        assert!(display_text_from_json(&json).is_none());
    }

    /// Verifies display assistant content not array.
    #[test]
    fn test_display_assistant_content_not_array() {
        let json = json!({
            "type": "assistant",
            "message": {
                "content": "just a string"
            }
        });
        assert!(display_text_from_json(&json).is_none());
    }

    /// Verifies display system missing subtype.
    #[test]
    fn test_display_system_missing_subtype() {
        let json = json!({"type": "system"});
        assert!(display_text_from_json(&json).is_none());
    }

    /// Verifies display hook no output field.
    #[test]
    fn test_display_hook_no_output_field() {
        let json = json!({
            "type": "system",
            "subtype": "hook_response",
            "hook_name": "lint"
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("lint"));
    }

    /// Verifies display result large values.
    #[test]
    fn test_display_result_large_values() {
        let json = json!({
            "type": "result",
            "duration_ms": 3_600_000,
            "total_cost_usd": 99.9999
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("3600.0s"));
        assert!(result.contains("$99.9999"));
    }

    /// Verifies parse stream json empty string.
    #[test]
    fn test_parse_stream_json_empty_string() {
        assert!(parse_stream_json_for_display("").is_none());
    }

    /// Verifies parse stream json just whitespace.
    #[test]
    fn test_parse_stream_json_just_whitespace() {
        assert!(parse_stream_json_for_display("   ").is_none());
    }
}
