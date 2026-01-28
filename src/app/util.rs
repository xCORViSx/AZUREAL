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

/// Parse stream-json output and extract human-readable content
/// Returns None if the line should not be displayed
pub fn parse_stream_json_for_display(line: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
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
