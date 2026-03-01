//! Utility functions for app module

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
                        if next_ch.is_ascii_alphabetic() { break; }
                    }
                }
                Some(&']') => {
                    // OSC sequence: \x1b]...(\x07 or \x1b\\)
                    chars.next();
                    while let Some(&next_ch) = chars.peek() {
                        if next_ch == '\x07' { chars.next(); break; }
                        if next_ch == '\x1b' {
                            chars.next();
                            if chars.peek() == Some(&'\\') { chars.next(); }
                            break;
                        }
                        chars.next();
                    }
                }
                _ => { chars.next(); }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Extract the most relevant parameter from a tool's input for display
fn extract_tool_param(tool_name: &str, input: Option<&serde_json::Value>) -> String {
    let input = match input {
        Some(v) => v,
        None => return String::new(),
    };

    let result = match tool_name {
        "Read" | "read" => input.get("file_path").or_else(|| input.get("path")).and_then(|v| v.as_str()),
        "Write" | "write" => input.get("file_path").or_else(|| input.get("path")).and_then(|v| v.as_str()),
        "Edit" | "edit" => input.get("file_path").or_else(|| input.get("path")).and_then(|v| v.as_str()),
        "Bash" | "bash" => input.get("command").and_then(|v| v.as_str()),
        "Glob" | "glob" => input.get("pattern").and_then(|v| v.as_str()),
        "Grep" | "grep" => input.get("pattern").and_then(|v| v.as_str()),
        "Task" | "task" => input.get("description").and_then(|v| v.as_str()),
        "WebFetch" | "webfetch" => input.get("url").and_then(|v| v.as_str()),
        "WebSearch" | "websearch" => input.get("query").and_then(|v| v.as_str()),
        "LSP" | "lsp" => {
            let op = input.get("operation").and_then(|v| v.as_str()).unwrap_or("");
            let file = input.get("filePath").and_then(|v| v.as_str()).unwrap_or("");
            return format!("{} {}", op, file);
        }
        _ => input.get("file_path")
            .or_else(|| input.get("path"))
            .or_else(|| input.get("command"))
            .or_else(|| input.get("query"))
            .or_else(|| input.get("pattern"))
            .and_then(|v| v.as_str()),
    };

    match result {
        Some(s) if s.len() > 60 => format!("{}...", &s[..57]),
        Some(s) => s.to_string(),
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
                    let model = json.get("model").and_then(|m| m.as_str()).unwrap_or("unknown");
                    let cwd = json.get("cwd").and_then(|c| c.as_str()).unwrap_or("");
                    Some(format!("[Session started | {} | {}]\n", model, cwd))
                }
                "hook_response" => {
                    let hook_name = json.get("hook_name").and_then(|n| n.as_str()).unwrap_or("hook");
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
            let content = json.get("message")?.get("content")?.as_str()?;
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

            if text_parts.is_empty() { None } else { Some(format!("Claude: {}\n", text_parts.join("\n"))) }
        }
        "result" => {
            let duration = json.get("duration_ms").and_then(|d| d.as_u64()).unwrap_or(0);
            let cost = json.get("total_cost_usd").and_then(|c| c.as_f64()).unwrap_or(0.0);
            Some(format!("[Done: {:.1}s, ${:.4}]\n", duration as f64 / 1000.0, cost))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── strip_ansi_escapes ──

    #[test]
    fn test_strip_plain_text() {
        assert_eq!(strip_ansi_escapes("hello world"), "hello world");
    }

    #[test]
    fn test_strip_empty_string() {
        assert_eq!(strip_ansi_escapes(""), "");
    }

    #[test]
    fn test_strip_csi_color_codes() {
        assert_eq!(strip_ansi_escapes("\x1b[31mred text\x1b[0m"), "red text");
    }

    #[test]
    fn test_strip_csi_bold() {
        assert_eq!(strip_ansi_escapes("\x1b[1mbold\x1b[0m"), "bold");
    }

    #[test]
    fn test_strip_csi_cursor_movement() {
        assert_eq!(strip_ansi_escapes("\x1b[2Jhello\x1b[H"), "hello");
    }

    #[test]
    fn test_strip_osc_title() {
        assert_eq!(strip_ansi_escapes("\x1b]0;My Title\x07text"), "text");
    }

    #[test]
    fn test_strip_osc_with_st_terminator() {
        assert_eq!(strip_ansi_escapes("\x1b]0;title\x1b\\after"), "after");
    }

    #[test]
    fn test_strip_mixed_ansi() {
        let input = "\x1b[32m✓\x1b[0m Pass \x1b[31m✗\x1b[0m Fail";
        assert_eq!(strip_ansi_escapes(input), "✓ Pass ✗ Fail");
    }

    #[test]
    fn test_strip_multiple_params_csi() {
        assert_eq!(strip_ansi_escapes("\x1b[38;2;255;0;0mred\x1b[0m"), "red");
    }

    #[test]
    fn test_strip_unknown_escape() {
        // Unknown char after ESC: skips ESC + the next char, rest remains
        assert_eq!(strip_ansi_escapes("\x1b(Bhello"), "Bhello");
    }

    // ── extract_tool_param ──

    #[test]
    fn test_extract_read_file_path() {
        let input = json!({"file_path": "/src/main.rs"});
        assert_eq!(extract_tool_param("Read", Some(&input)), "/src/main.rs");
    }

    #[test]
    fn test_extract_bash_command() {
        let input = json!({"command": "cargo build"});
        assert_eq!(extract_tool_param("Bash", Some(&input)), "cargo build");
    }

    #[test]
    fn test_extract_grep_pattern() {
        let input = json!({"pattern": "fn main"});
        assert_eq!(extract_tool_param("Grep", Some(&input)), "fn main");
    }

    #[test]
    fn test_extract_glob_pattern() {
        let input = json!({"pattern": "**/*.rs"});
        assert_eq!(extract_tool_param("Glob", Some(&input)), "**/*.rs");
    }

    #[test]
    fn test_extract_task_description() {
        let input = json!({"description": "Search for tests"});
        assert_eq!(extract_tool_param("Task", Some(&input)), "Search for tests");
    }

    #[test]
    fn test_extract_webfetch_url() {
        let input = json!({"url": "https://example.com"});
        assert_eq!(extract_tool_param("WebFetch", Some(&input)), "https://example.com");
    }

    #[test]
    fn test_extract_websearch_query() {
        let input = json!({"query": "rust async"});
        assert_eq!(extract_tool_param("WebSearch", Some(&input)), "rust async");
    }

    #[test]
    fn test_extract_lsp_operation() {
        let input = json!({"operation": "definition", "filePath": "/src/lib.rs"});
        assert_eq!(extract_tool_param("LSP", Some(&input)), "definition /src/lib.rs");
    }

    #[test]
    fn test_extract_none_input() {
        assert_eq!(extract_tool_param("Read", None), "");
    }

    #[test]
    fn test_extract_missing_field() {
        let input = json!({"other": "value"});
        assert_eq!(extract_tool_param("Read", Some(&input)), "");
    }

    #[test]
    fn test_extract_truncation_at_60() {
        let long_path = format!("/very/long/path/{}", "a".repeat(100));
        let input = json!({"file_path": long_path});
        let result = extract_tool_param("Read", Some(&input));
        assert_eq!(result.len(), 60);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_extract_unknown_tool_fallback() {
        let input = json!({"file_path": "/some/file.txt"});
        assert_eq!(extract_tool_param("UnknownTool", Some(&input)), "/some/file.txt");
    }

    #[test]
    fn test_extract_case_insensitive_tools() {
        let input = json!({"file_path": "/test.rs"});
        assert_eq!(extract_tool_param("read", Some(&input)), "/test.rs");
        assert_eq!(extract_tool_param("write", Some(&input)), "/test.rs");
        assert_eq!(extract_tool_param("edit", Some(&input)), "/test.rs");
    }

    // ── display_text_from_json ──

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

    #[test]
    fn test_display_unknown_type() {
        let json = json!({"type": "unknown_event"});
        assert!(display_text_from_json(&json).is_none());
    }

    #[test]
    fn test_display_missing_type() {
        let json = json!({"data": "something"});
        assert!(display_text_from_json(&json).is_none());
    }

    #[test]
    fn test_display_system_unknown_subtype() {
        let json = json!({
            "type": "system",
            "subtype": "unknown_system_event"
        });
        assert!(display_text_from_json(&json).is_none());
    }

    // ── parse_stream_json_for_display ──

    #[test]
    fn test_parse_stream_json_valid() {
        let line = r#"{"type":"user","message":{"content":"hi"}}"#;
        let result = parse_stream_json_for_display(line).unwrap();
        assert_eq!(result, "You: hi\n");
    }

    #[test]
    fn test_parse_stream_json_invalid() {
        assert!(parse_stream_json_for_display("not json").is_none());
    }

    #[test]
    fn test_parse_stream_json_whitespace() {
        let line = r#"  {"type":"result","duration_ms":1000,"total_cost_usd":0.01}  "#;
        let result = parse_stream_json_for_display(line).unwrap();
        assert!(result.contains("1.0s"));
    }

    // ── strip_ansi_escapes: more edge cases ──

    #[test]
    fn test_strip_nested_escapes() {
        // Bold + color nested: \e[1m\e[31m text \e[0m
        let input = "\x1b[1m\x1b[31mhello\x1b[0m\x1b[0m";
        assert_eq!(strip_ansi_escapes(input), "hello");
    }

    #[test]
    fn test_strip_partial_escape_just_esc() {
        // Just ESC char alone at end of string
        let input = "text\x1b";
        // ESC at end: no next char to peek, loop ends
        assert_eq!(strip_ansi_escapes(input), "text");
    }

    #[test]
    fn test_strip_esc_at_end_of_string() {
        let input = "hello\x1b";
        assert_eq!(strip_ansi_escapes(input), "hello");
    }

    #[test]
    fn test_strip_multiple_consecutive_csi_escapes() {
        let input = "\x1b[1m\x1b[4m\x1b[31m\x1b[42mtext\x1b[0m";
        assert_eq!(strip_ansi_escapes(input), "text");
    }

    #[test]
    fn test_strip_real_terminal_output_git_diff() {
        // Simulated git diff output with colors
        let input = "\x1b[32m+  fn new()\x1b[0m\n\x1b[31m-  fn old()\x1b[0m";
        assert_eq!(strip_ansi_escapes(input), "+  fn new()\n-  fn old()");
    }

    #[test]
    fn test_strip_real_terminal_output_cargo() {
        let input = "\x1b[32m   Compiling\x1b[0m myproject v0.1.0";
        assert_eq!(strip_ansi_escapes(input), "   Compiling myproject v0.1.0");
    }

    #[test]
    fn test_strip_256_color_code() {
        let input = "\x1b[38;5;196mred\x1b[0m";
        assert_eq!(strip_ansi_escapes(input), "red");
    }

    #[test]
    fn test_strip_rgb_color_code() {
        let input = "\x1b[38;2;255;128;0morange\x1b[0m";
        assert_eq!(strip_ansi_escapes(input), "orange");
    }

    #[test]
    fn test_strip_osc_hyperlink() {
        // OSC 8 hyperlink: \e]8;;url\e\\text\e]8;;\e\\
        let input = "\x1b]8;;https://example.com\x1b\\Click here\x1b]8;;\x1b\\";
        assert_eq!(strip_ansi_escapes(input), "Click here");
    }

    #[test]
    fn test_strip_only_escape_sequences() {
        let input = "\x1b[31m\x1b[0m\x1b[1m\x1b[0m";
        assert_eq!(strip_ansi_escapes(input), "");
    }

    #[test]
    fn test_strip_unicode_content_preserved() {
        let input = "\x1b[32m日本語テキスト\x1b[0m";
        assert_eq!(strip_ansi_escapes(input), "日本語テキスト");
    }

    #[test]
    fn test_strip_emoji_content_preserved() {
        let input = "\x1b[1m🚀 Launch!\x1b[0m";
        assert_eq!(strip_ansi_escapes(input), "🚀 Launch!");
    }

    #[test]
    fn test_strip_csi_erase_line() {
        let input = "\x1b[2Koverwritten text";
        assert_eq!(strip_ansi_escapes(input), "overwritten text");
    }

    // ── extract_tool_param: more tools ──

    #[test]
    fn test_extract_notebook_edit() {
        let input = json!({"notebook_path": "/nb.ipynb", "new_source": "print(1)"});
        // NotebookEdit falls through to unknown, tries file_path, path, command, query, pattern
        let result = extract_tool_param("NotebookEdit", Some(&input));
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_todo_write() {
        let input = json!({"todos": [{"content": "Task 1"}]});
        let result = extract_tool_param("TodoWrite", Some(&input));
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_write_with_content() {
        let input = json!({"file_path": "/out.txt", "content": "Hello world"});
        let result = extract_tool_param("Write", Some(&input));
        assert_eq!(result, "/out.txt");
    }

    #[test]
    fn test_extract_numeric_values_ignored() {
        let input = json!({"file_path": 42});
        let result = extract_tool_param("Read", Some(&input));
        // as_str returns None for numeric values
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_nested_object_ignored() {
        let input = json!({"file_path": {"nested": "value"}});
        let result = extract_tool_param("Read", Some(&input));
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_lsp_missing_fields() {
        let input = json!({});
        let result = extract_tool_param("LSP", Some(&input));
        assert_eq!(result, " ");
    }

    #[test]
    fn test_extract_lsp_partial_fields() {
        let input = json!({"operation": "hover"});
        let result = extract_tool_param("LSP", Some(&input));
        assert_eq!(result, "hover ");
    }

    #[test]
    fn test_extract_exact_60_chars_no_truncation() {
        let path = format!("/path/{}", "x".repeat(54)); // "/path/" = 6 + 54 = 60
        let input = json!({"file_path": path});
        let result = extract_tool_param("Read", Some(&input));
        assert_eq!(result.len(), 60);
        assert!(!result.ends_with("..."));
    }

    #[test]
    fn test_extract_61_chars_truncated() {
        let path = format!("/path/{}", "x".repeat(55)); // "/path/" = 6 + 55 = 61
        let input = json!({"file_path": path});
        let result = extract_tool_param("Read", Some(&input));
        assert_eq!(result.len(), 60);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_extract_bash_lowercase() {
        let input = json!({"command": "echo hello"});
        assert_eq!(extract_tool_param("bash", Some(&input)), "echo hello");
    }

    #[test]
    fn test_extract_unknown_tool_query_fallback() {
        let input = json!({"query": "search term"});
        assert_eq!(extract_tool_param("CustomTool", Some(&input)), "search term");
    }

    #[test]
    fn test_extract_unknown_tool_pattern_fallback() {
        let input = json!({"pattern": "*.rs"});
        assert_eq!(extract_tool_param("CustomTool", Some(&input)), "*.rs");
    }

    #[test]
    fn test_extract_unknown_tool_command_fallback() {
        let input = json!({"command": "run"});
        assert_eq!(extract_tool_param("CustomTool", Some(&input)), "run");
    }

    // ── display_text_from_json: more edge cases ──

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

    #[test]
    fn test_display_init_missing_cwd() {
        let json = json!({
            "type": "system",
            "subtype": "init",
            "model": "opus"
        });
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("opus"));
        assert!(result.contains("| ]"));  // cwd defaults to empty string
    }

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

    #[test]
    fn test_display_result_missing_fields_default() {
        let json = json!({"type": "result"});
        let result = display_text_from_json(&json).unwrap();
        assert!(result.contains("0.0s"));
        assert!(result.contains("$0.0000"));
    }

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

    #[test]
    fn test_display_user_message_missing_content() {
        let json = json!({
            "type": "user",
            "message": {}
        });
        assert!(display_text_from_json(&json).is_none());
    }

    #[test]
    fn test_display_user_message_missing_message() {
        let json = json!({"type": "user"});
        assert!(display_text_from_json(&json).is_none());
    }

    #[test]
    fn test_display_assistant_missing_message() {
        let json = json!({"type": "assistant"});
        assert!(display_text_from_json(&json).is_none());
    }

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

    #[test]
    fn test_display_system_missing_subtype() {
        let json = json!({"type": "system"});
        assert!(display_text_from_json(&json).is_none());
    }

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

    #[test]
    fn test_parse_stream_json_empty_string() {
        assert!(parse_stream_json_for_display("").is_none());
    }

    #[test]
    fn test_parse_stream_json_just_whitespace() {
        assert!(parse_stream_json_for_display("   ").is_none());
    }
}
