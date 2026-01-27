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

/// Parse stream-json output and extract human-readable content
/// Returns None if the line should not be displayed
pub fn parse_stream_json_for_display(line: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let event_type = json.get("type")?.as_str()?;

    match event_type {
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
                            text_parts.push(format!("[Using {}...]", name));
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
