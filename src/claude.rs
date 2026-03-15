use anyhow::{Context, Result};
use portable_pty::CommandBuilder;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use crate::config::{Config, PermissionMode};
use crate::models::OutputType;

/// Output from Claude Code process
#[derive(Debug, Clone)]
pub struct AgentOutput {
    pub output_type: OutputType,
    pub data: String,
}

/// Events from Claude Code process
#[derive(Debug)]
pub enum AgentEvent {
    Output(AgentOutput),
    Started { pid: u32 },
    /// Claude's session ID from init event (for --resume)
    SessionId(String),
    Exited { code: Option<i32> },
}

/// Manages Claude Code CLI processes via PTY
/// PTY spawning (like Crystal) may avoid tool_use ID collision bugs in -p --resume mode
pub struct ClaudeProcess {
    config: Config,
}

impl ClaudeProcess {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Spawn Claude Code with the given prompt via PTY
    /// resume_session_id: Claude session ID from previous prompt's init event (for --resume)
    /// model: optional model override (e.g. "opus", "sonnet") — passed as --model flag
    pub fn spawn(
        &self,
        working_dir: &Path,
        prompt: &str,
        resume_session_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<(mpsc::Receiver<AgentEvent>, u32)> {
        if prompt.is_empty() {
            anyhow::bail!("Prompt cannot be empty");
        }

        let (tx, rx) = mpsc::channel();

        // Build command with PTY (like Crystal does)
        let mut cmd = CommandBuilder::new(self.config.claude_executable());

        // Resume previous conversation if we have a session ID
        if let Some(session_id) = resume_session_id {
            cmd.arg("--resume");
            cmd.arg(session_id);
        }

        // Model override (⌃m cycle selection)
        if let Some(m) = model {
            cmd.arg("--model");
            cmd.arg(m);
        }

        // Prompt and output format
        cmd.arg("-p");
        cmd.arg(prompt);
        cmd.arg("--verbose");
        cmd.arg("--output-format");
        cmd.arg("stream-json");

        // Permission mode
        match self.config.default_permission_mode {
            PermissionMode::Ignore => {
                cmd.arg("--dangerously-skip-permissions");
            }
            PermissionMode::Approve | PermissionMode::Ask => {}
        }

        // Use standard process with separate stdout/stderr to capture hooks
        let mut child = Command::new(self.config.claude_executable())
            .args(cmd.get_argv().iter().skip(1).map(|s| s.to_str().unwrap_or("")))
            .current_dir(working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn Claude")?;

        let pid = child.id();
        let _ = tx.send(AgentEvent::Started { pid });

        // Read stdout
        let stdout = child.stdout.take().context("Failed to get stdout")?;
        let tx_stdout = tx.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line_result in reader.lines() {
                let line = match line_result {
                    Ok(l) => l,
                    Err(_) => break,
                };

                if line.is_empty() {
                    continue;
                }

                // Extract session_id from init event using string search (avoids
                // full JSON parse on EVERY line — init happens once per session).
                if line.contains("\"subtype\":\"init\"") {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                        if let Some(session_id) = json.get("session_id").and_then(|v| v.as_str()) {
                            let _ = tx_stdout.send(AgentEvent::SessionId(session_id.to_string()));
                        }
                    }
                }

                let output = AgentOutput {
                    output_type: OutputType::Stdout,
                    data: format!("{}\n", line),
                };
                if tx_stdout.send(AgentEvent::Output(output)).is_err() {
                    break;
                }
            }
        });

        // Read stderr - hooks might be here
        let stderr = child.stderr.take().context("Failed to get stderr")?;
        let tx_stderr = tx.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line_result in reader.lines() {
                let line = match line_result {
                    Ok(l) => l,
                    Err(_) => break,
                };

                if line.is_empty() {
                    continue;
                }

                // Send stderr output (might contain hooks)
                let output = AgentOutput {
                    output_type: OutputType::Stderr,
                    data: format!("{}\n", line),
                };
                if tx_stderr.send(AgentEvent::Output(output)).is_err() {
                    break;
                }
            }
        });

        // Wait for process to exit
        thread::spawn(move || {
            let status = child.wait();
            let code = status.ok().and_then(|s| s.code());
            let _ = tx.send(AgentEvent::Exited { code });
        });

        Ok((rx, pid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PermissionMode;

    // ── AgentOutput construction ──

    #[test]
    fn test_claude_output_stdout() {
        let output = AgentOutput {
            output_type: OutputType::Stdout,
            data: "hello\n".to_string(),
        };
        assert_eq!(output.output_type, OutputType::Stdout);
        assert_eq!(output.data, "hello\n");
    }

    #[test]
    fn test_claude_output_stderr() {
        let output = AgentOutput {
            output_type: OutputType::Stderr,
            data: "error message\n".to_string(),
        };
        assert_eq!(output.output_type, OutputType::Stderr);
    }

    #[test]
    fn test_claude_output_system() {
        let output = AgentOutput {
            output_type: OutputType::System,
            data: "system event".to_string(),
        };
        assert_eq!(output.output_type, OutputType::System);
    }

    #[test]
    fn test_claude_output_json() {
        let output = AgentOutput {
            output_type: OutputType::Json,
            data: r#"{"type":"text"}"#.to_string(),
        };
        assert_eq!(output.output_type, OutputType::Json);
    }

    #[test]
    fn test_claude_output_error() {
        let output = AgentOutput {
            output_type: OutputType::Error,
            data: "fatal error".to_string(),
        };
        assert_eq!(output.output_type, OutputType::Error);
    }

    #[test]
    fn test_claude_output_hook() {
        let output = AgentOutput {
            output_type: OutputType::Hook,
            data: "hook output".to_string(),
        };
        assert_eq!(output.output_type, OutputType::Hook);
    }

    #[test]
    fn test_claude_output_empty_data() {
        let output = AgentOutput {
            output_type: OutputType::Stdout,
            data: String::new(),
        };
        assert!(output.data.is_empty());
    }

    #[test]
    fn test_claude_output_clone() {
        let output = AgentOutput {
            output_type: OutputType::Stdout,
            data: "test".to_string(),
        };
        let cloned = output.clone();
        assert_eq!(cloned.output_type, output.output_type);
        assert_eq!(cloned.data, output.data);
    }

    #[test]
    fn test_claude_output_debug() {
        let output = AgentOutput {
            output_type: OutputType::Stdout,
            data: "debug test".to_string(),
        };
        let debug = format!("{:?}", output);
        assert!(debug.contains("AgentOutput"));
        assert!(debug.contains("Stdout"));
        assert!(debug.contains("debug test"));
    }

    #[test]
    fn test_claude_output_large_data() {
        let large = "x".repeat(100_000);
        let output = AgentOutput {
            output_type: OutputType::Stdout,
            data: large.clone(),
        };
        assert_eq!(output.data.len(), 100_000);
    }

    #[test]
    fn test_claude_output_unicode_data() {
        let output = AgentOutput {
            output_type: OutputType::Stdout,
            data: "日本語テスト 🚀".to_string(),
        };
        assert!(output.data.contains("日本語"));
        assert!(output.data.contains("🚀"));
    }

    #[test]
    fn test_claude_output_multiline_data() {
        let output = AgentOutput {
            output_type: OutputType::Stdout,
            data: "line1\nline2\nline3\n".to_string(),
        };
        assert_eq!(output.data.lines().count(), 3);
    }

    // ── AgentEvent variants ──

    #[test]
    fn test_claude_event_output() {
        let event = AgentEvent::Output(AgentOutput {
            output_type: OutputType::Stdout,
            data: "test".to_string(),
        });
        assert!(matches!(event, AgentEvent::Output(_)));
    }

    #[test]
    fn test_claude_event_started() {
        let event = AgentEvent::Started { pid: 12345 };
        if let AgentEvent::Started { pid } = event {
            assert_eq!(pid, 12345);
        } else {
            panic!("expected Started");
        }
    }

    #[test]
    fn test_claude_event_started_zero_pid() {
        let event = AgentEvent::Started { pid: 0 };
        if let AgentEvent::Started { pid } = event {
            assert_eq!(pid, 0);
        }
    }

    #[test]
    fn test_claude_event_started_large_pid() {
        let event = AgentEvent::Started { pid: u32::MAX };
        if let AgentEvent::Started { pid } = event {
            assert_eq!(pid, u32::MAX);
        }
    }

    #[test]
    fn test_claude_event_session_id() {
        let event = AgentEvent::SessionId("sess-abc-123-def".to_string());
        if let AgentEvent::SessionId(id) = event {
            assert_eq!(id, "sess-abc-123-def");
        } else {
            panic!("expected SessionId");
        }
    }

    #[test]
    fn test_claude_event_session_id_empty() {
        let event = AgentEvent::SessionId(String::new());
        if let AgentEvent::SessionId(id) = event {
            assert!(id.is_empty());
        }
    }

    #[test]
    fn test_claude_event_exited_success() {
        let event = AgentEvent::Exited { code: Some(0) };
        if let AgentEvent::Exited { code } = event {
            assert_eq!(code, Some(0));
        } else {
            panic!("expected Exited");
        }
    }

    #[test]
    fn test_claude_event_exited_failure() {
        let event = AgentEvent::Exited { code: Some(1) };
        if let AgentEvent::Exited { code } = event {
            assert_eq!(code, Some(1));
        }
    }

    #[test]
    fn test_claude_event_exited_signal() {
        let event = AgentEvent::Exited { code: None };
        if let AgentEvent::Exited { code } = event {
            assert!(code.is_none());
        }
    }

    #[test]
    fn test_claude_event_exited_error_code() {
        let event = AgentEvent::Exited { code: Some(127) };
        if let AgentEvent::Exited { code } = event {
            assert_eq!(code, Some(127));
        }
    }

    #[test]
    fn test_claude_event_debug() {
        let event = AgentEvent::Started { pid: 42 };
        let debug = format!("{:?}", event);
        assert!(debug.contains("Started"));
        assert!(debug.contains("42"));
    }

    #[test]
    fn test_claude_event_output_debug() {
        let event = AgentEvent::Output(AgentOutput {
            output_type: OutputType::Stderr,
            data: "err".to_string(),
        });
        let debug = format!("{:?}", event);
        assert!(debug.contains("Output"));
    }

    // ── ClaudeProcess construction ──

    #[test]
    fn test_claude_process_new_default_config() {
        let config = Config::default();
        let process = ClaudeProcess::new(config);
        assert_eq!(process.config.claude_executable(), "claude");
    }

    #[test]
    fn test_claude_process_new_custom_executable() {
        let config = Config {
            claude_executable: Some("/usr/local/bin/claude-code".to_string()),
            ..Config::default()
        };
        let process = ClaudeProcess::new(config);
        assert_eq!(process.config.claude_executable(), "/usr/local/bin/claude-code");
    }

    #[test]
    fn test_claude_process_new_with_api_key() {
        let config = Config {
            anthropic_api_key: Some("sk-test-key".to_string()),
            ..Config::default()
        };
        let process = ClaudeProcess::new(config);
        assert_eq!(process.config.anthropic_api_key.as_deref(), Some("sk-test-key"));
    }

    #[test]
    fn test_claude_process_new_verbose() {
        let config = Config {
            verbose: true,
            ..Config::default()
        };
        let process = ClaudeProcess::new(config);
        assert!(process.config.verbose);
    }

    #[test]
    fn test_claude_process_new_ignore_permissions() {
        let config = Config {
            default_permission_mode: PermissionMode::Ignore,
            ..Config::default()
        };
        let process = ClaudeProcess::new(config);
        assert!(matches!(process.config.default_permission_mode, PermissionMode::Ignore));
    }

    #[test]
    fn test_claude_process_new_approve_permissions() {
        let config = Config {
            default_permission_mode: PermissionMode::Approve,
            ..Config::default()
        };
        let process = ClaudeProcess::new(config);
        assert!(matches!(process.config.default_permission_mode, PermissionMode::Approve));
    }

    #[test]
    fn test_claude_process_new_ask_permissions() {
        let config = Config {
            default_permission_mode: PermissionMode::Ask,
            ..Config::default()
        };
        let process = ClaudeProcess::new(config);
        assert!(matches!(process.config.default_permission_mode, PermissionMode::Ask));
    }

    // ── ClaudeProcess::spawn: validation ──

    #[test]
    fn test_claude_process_spawn_empty_prompt_fails() {
        let config = Config::default();
        let process = ClaudeProcess::new(config);
        let result = process.spawn(
            std::path::Path::new("/tmp"),
            "",
            None,
            None,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    // ── AgentEvent channel communication ──

    #[test]
    fn test_claude_event_channel_send_receive() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(AgentEvent::Started { pid: 100 }).unwrap();
        tx.send(AgentEvent::Output(AgentOutput {
            output_type: OutputType::Stdout,
            data: "test\n".to_string(),
        })).unwrap();
        tx.send(AgentEvent::Exited { code: Some(0) }).unwrap();
        assert!(matches!(rx.recv().unwrap(), AgentEvent::Started { pid: 100 }));
        assert!(matches!(rx.recv().unwrap(), AgentEvent::Output(_)));
        assert!(matches!(rx.recv().unwrap(), AgentEvent::Exited { code: Some(0) }));
    }

    #[test]
    fn test_claude_event_channel_session_id() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(AgentEvent::SessionId("uuid-test".to_string())).unwrap();
        if let AgentEvent::SessionId(id) = rx.recv().unwrap() {
            assert_eq!(id, "uuid-test");
        }
    }

    #[test]
    fn test_claude_event_channel_try_recv_empty() {
        let (_tx, rx) = std::sync::mpsc::channel::<AgentEvent>();
        assert!(rx.try_recv().is_err());
    }

    // ── OutputType combinations ──

    #[test]
    fn test_all_output_types_in_claude_output() {
        let types = [
            OutputType::Stdout,
            OutputType::Stderr,
            OutputType::System,
            OutputType::Json,
            OutputType::Error,
            OutputType::Hook,
        ];
        for ot in types {
            let output = AgentOutput {
                output_type: ot,
                data: "test".to_string(),
            };
            assert_eq!(output.output_type, ot);
        }
    }

    // ── Config in ClaudeProcess ──

    #[test]
    fn test_claude_process_config_all_fields() {
        let config = Config {
            anthropic_api_key: Some("key".to_string()),
            claude_executable: Some("/bin/claude".to_string()),
            codex_executable: None,
            backend: crate::backend::Backend::Claude,
            default_permission_mode: PermissionMode::Approve,
            verbose: true,
        };
        let process = ClaudeProcess::new(config);
        assert_eq!(process.config.claude_executable(), "/bin/claude");
        assert!(process.config.verbose);
        assert_eq!(process.config.anthropic_api_key.as_deref(), Some("key"));
    }

    #[test]
    fn test_claude_process_config_none_executable() {
        let config = Config::default();
        let process = ClaudeProcess::new(config);
        assert_eq!(process.config.claude_executable(), "claude");
    }

    // ── AgentEvent: all variants constructable ──

    #[test]
    fn test_all_claude_event_variants_exist() {
        let events: Vec<AgentEvent> = vec![
            AgentEvent::Output(AgentOutput { output_type: OutputType::Stdout, data: String::new() }),
            AgentEvent::Started { pid: 1 },
            AgentEvent::SessionId("id".to_string()),
            AgentEvent::Exited { code: Some(0) },
        ];
        assert_eq!(events.len(), 4);
    }

    #[test]
    fn test_claude_event_exited_negative_code() {
        let event = AgentEvent::Exited { code: Some(-1) };
        if let AgentEvent::Exited { code } = event {
            assert_eq!(code, Some(-1));
        }
    }

    // ── AgentOutput data formatting ──

    #[test]
    fn test_claude_output_data_with_newline_suffix() {
        // The spawn method appends \n to each line
        let output = AgentOutput {
            output_type: OutputType::Stdout,
            data: format!("{}\n", r#"{"type":"text","text":"hello"}"#),
        };
        assert!(output.data.ends_with('\n'));
    }

    #[test]
    fn test_claude_output_json_parsing() {
        let json_line = r#"{"type":"assistant","subtype":"init","session_id":"abc"}"#;
        let output = AgentOutput {
            output_type: OutputType::Stdout,
            data: format!("{}\n", json_line),
        };
        // Verify the data contains valid JSON
        let trimmed = output.data.trim();
        let parsed: serde_json::Value = serde_json::from_str(trimmed).unwrap();
        assert_eq!(parsed["session_id"], "abc");
    }

    // ── Session ID extraction logic (mirrors spawn stdout thread) ──

    #[test]
    fn test_init_line_detected_by_substring() {
        // The spawn thread uses contains("\"subtype\":\"init\"") to detect init events
        let line = r#"{"type":"system","subtype":"init","session_id":"sess-xyz"}"#;
        assert!(line.contains("\"subtype\":\"init\""));
    }

    #[test]
    fn test_non_init_line_not_detected() {
        let line = r#"{"type":"assistant","text":"hello world"}"#;
        assert!(!line.contains("\"subtype\":\"init\""));
    }

    #[test]
    fn test_session_id_extracted_from_init_json() {
        let line = r#"{"type":"system","subtype":"init","session_id":"my-session-id-123"}"#;
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        let session_id = parsed.get("session_id").and_then(|v| v.as_str()).unwrap();
        assert_eq!(session_id, "my-session-id-123");
    }

    #[test]
    fn test_session_id_missing_from_json() {
        let line = r#"{"type":"system","subtype":"init"}"#;
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        let session_id = parsed.get("session_id").and_then(|v| v.as_str());
        assert!(session_id.is_none());
    }

    // ── AgentOutput data with special characters ──

    #[test]
    fn test_claude_output_data_with_tabs() {
        let output = AgentOutput {
            output_type: OutputType::Stdout,
            data: "col1\tcol2\tcol3\n".to_string(),
        };
        assert!(output.data.contains('\t'));
    }

    #[test]
    fn test_claude_output_data_with_json_special_chars() {
        let output = AgentOutput {
            output_type: OutputType::Stderr,
            data: r#"{"key":"value with \"quotes\""}"#.to_string(),
        };
        assert!(output.data.contains("key"));
    }

    // ── ClaudeProcess verbose field ──

    #[test]
    fn test_claude_process_verbose_false_by_default() {
        let config = Config::default();
        let process = ClaudeProcess::new(config);
        assert!(!process.config.verbose);
    }

    #[test]
    fn test_claude_process_no_api_key_by_default() {
        let config = Config::default();
        let process = ClaudeProcess::new(config);
        assert!(process.config.anthropic_api_key.is_none());
    }

    // ── AgentEvent: SessionId with special chars ──

    #[test]
    fn test_session_id_with_uuid_format() {
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        let event = AgentEvent::SessionId(uuid.to_string());
        if let AgentEvent::SessionId(id) = event {
            assert_eq!(id.len(), 36);
            assert!(id.contains('-'));
        }
    }

    #[test]
    fn test_session_id_unicode() {
        let event = AgentEvent::SessionId("日本語".to_string());
        if let AgentEvent::SessionId(id) = event {
            assert_eq!(id, "日本語");
        }
    }
}
