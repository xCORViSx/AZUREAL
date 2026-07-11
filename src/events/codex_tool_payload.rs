//! Shared decoding for Codex tool-call names and display payloads.

/// Maps a Codex tool name to the stable name and optional path shown by Azureal.
pub(crate) fn map_codex_tool(name: &str, payload: &serde_json::Value) -> (String, Option<String>) {
    match name {
        "shell_command" => {
            let args = parse_tool_args(payload);
            let workdir = args
                .as_object()
                .and_then(|args| args.get("workdir"))
                .and_then(|value| value.as_str())
                .map(str::to_string);
            ("Bash".to_string(), workdir)
        }
        "exec_command" | "write_stdin" => ("Bash".to_string(), None),
        "exec" => ("Exec".to_string(), None),
        "apply_patch" => {
            let patch = raw_tool_input(payload);
            ("Edit".to_string(), extract_patch_file_path(patch))
        }
        _ => (name.to_string(), None),
    }
}

/// Builds the normalized JSON value used to render a Codex tool call.
pub(crate) fn build_tool_input(name: &str, payload: &serde_json::Value) -> serde_json::Value {
    match name {
        "shell_command" => parse_tool_args(payload),
        "exec_command" => normalize_exec_command_input(parse_tool_args(payload)),
        "write_stdin" => normalize_write_stdin_input(parse_tool_args(payload)),
        "exec" => serde_json::json!({ "command": raw_tool_input(payload) }),
        "apply_patch" => serde_json::json!({ "patch": raw_tool_input(payload) }),
        _ => parse_tool_args(payload),
    }
}

/// Returns the string payload used by function and free-form custom tools.
fn raw_tool_input(payload: &serde_json::Value) -> &str {
    payload
        .get("arguments")
        .and_then(|value| value.as_str())
        .or_else(|| payload.get("input").and_then(|value| value.as_str()))
        .unwrap_or("")
}

/// Extracts the first target path named by an apply-patch payload.
fn extract_patch_file_path(patch: &str) -> Option<String> {
    for line in patch.lines() {
        for prefix in ["*** Update File: ", "*** Add File: ", "*** Delete File: "] {
            if let Some(rest) = line.strip_prefix(prefix) {
                return Some(rest.trim().to_string());
            }
        }
    }
    None
}

/// Parses a JSON-encoded function-tool payload, returning an empty object on malformed input.
fn parse_tool_args(payload: &serde_json::Value) -> serde_json::Value {
    serde_json::from_str(raw_tool_input(payload)).unwrap_or_else(|_| serde_json::json!({}))
}

/// Adds the command field expected by Bash-style tool rendering.
fn normalize_exec_command_input(mut args: serde_json::Value) -> serde_json::Value {
    let command = args
        .get("command")
        .or_else(|| args.get("cmd"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    insert_command_field(&mut args, command);
    args
}

/// Describes a write-stdin action in the command field used by the session pane.
fn normalize_write_stdin_input(mut args: serde_json::Value) -> serde_json::Value {
    let command = describe_write_stdin_action(&args);
    insert_command_field(&mut args, command);
    args
}

/// Inserts a normalized command string while repairing non-object inputs.
fn insert_command_field(args: &mut serde_json::Value, command: String) {
    match args {
        serde_json::Value::Object(map) => {
            map.insert("command".into(), serde_json::json!(command));
        }
        _ => {
            *args = serde_json::json!({ "command": command });
        }
    }
}

/// Produces a compact description of polling or writing to a running command session.
fn describe_write_stdin_action(args: &serde_json::Value) -> String {
    let session_suffix = args
        .get("session_id")
        .map(|value| match value {
            serde_json::Value::String(session) => format!(" {session}"),
            serde_json::Value::Number(session) => format!(" {session}"),
            _ => String::new(),
        })
        .unwrap_or_default();
    let chars = args
        .get("chars")
        .and_then(|value| value.as_str())
        .unwrap_or("");
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

/// Regression coverage for Codex tool payload normalization.
#[cfg(test)]
mod tests {
    use super::*;

    /// GPT-5.6 free-form exec calls retain the JavaScript orchestration body.
    #[test]
    fn free_form_exec_preserves_orchestration_source() {
        let payload = serde_json::json!({
            "type": "custom_tool_call",
            "name": "exec",
            "input": "const r = await tools.exec_command({ cmd: \"pwd\" });\ntext(r.output);"
        });

        assert_eq!(map_codex_tool("exec", &payload), ("Exec".into(), None));
        assert_eq!(
            build_tool_input("exec", &payload)["command"],
            payload["input"]
        );
    }

    /// JSON function calls continue to expose their shell command under the normalized key.
    #[test]
    fn exec_command_normalizes_cmd_key() {
        let payload = serde_json::json!({
            "arguments": r#"{"cmd":"pwd","workdir":"/tmp"}"#
        });

        assert_eq!(build_tool_input("exec_command", &payload)["command"], "pwd");
    }

    /// Apply-patch calls retain their patch and expose a target path for link rendering.
    #[test]
    fn apply_patch_preserves_patch_and_target() {
        let patch = "*** Begin Patch\n*** Delete File: src/old.rs\n*** End Patch";
        let payload = serde_json::json!({ "input": patch });

        assert_eq!(
            map_codex_tool("apply_patch", &payload),
            ("Edit".into(), Some("src/old.rs".into()))
        );
        assert_eq!(build_tool_input("apply_patch", &payload)["patch"], patch);
    }
}
