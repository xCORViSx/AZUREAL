//! Codex CLI process spawning
//!
//! Spawns `codex exec --json` processes and returns `AgentEvent`s via mpsc channel.
//! Mirrors `ClaudeProcess` in `src/claude.rs` but builds Codex-specific CLI args.

use anyhow::{Context, Result};
use std::io::BufRead;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use crate::claude::{AgentEvent, AgentOutput};
use crate::config::{Config, PermissionMode};
use crate::models::OutputType;

/// Manages OpenAI Codex CLI processes
pub struct CodexProcess {
    config: Config,
}

impl CodexProcess {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Spawn Codex with the given prompt
    /// resume_session_id: Codex thread_id from previous prompt (for `exec resume`)
    /// model: optional model override (e.g. "gpt-5.4", "gpt-5.3-codex") — passed as --model flag
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

        let executable = self.config.codex_executable();
        let mut args: Vec<String> = Vec::new();

        // Base command: exec --json
        args.push("exec".into());
        args.push("--json".into());

        // Model override
        if let Some(m) = model {
            args.push("--model".into());
            args.push(m.into());
        }

        // Permission mode
        match self.config.default_permission_mode {
            PermissionMode::Ignore => {
                args.push("--dangerously-bypass-approvals-and-sandbox".into());
            }
            PermissionMode::Approve => {
                args.push("--full-auto".into());
            }
            PermissionMode::Ask => {} // default Codex behavior
        }

        // Resume or new session
        if let Some(session_id) = resume_session_id {
            args.push("resume".into());
            args.push(session_id.into());
        }

        // Prompt (always last positional arg)
        args.push(prompt.into());

        let mut child = Command::new(executable)
            .args(&args)
            .current_dir(working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn Codex")?;

        let pid = child.id();
        let _ = tx.send(AgentEvent::Started { pid });

        // Read stdout (JSONL events)
        let stdout = child.stdout.take().context("Failed to get stdout")?;
        let tx_stdout = tx.clone();
        thread::spawn(move || {
            let reader = std::io::BufReader::new(stdout);
            for line_result in reader.lines() {
                let line = match line_result {
                    Ok(l) => l,
                    Err(_) => break,
                };

                if line.is_empty() {
                    continue;
                }

                // Extract session ID from thread.started event
                if line.contains("\"thread.started\"") {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                        if let Some(thread_id) = json.get("thread_id").and_then(|v| v.as_str()) {
                            let _ = tx_stdout.send(AgentEvent::SessionId(thread_id.to_string()));
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

        // Read stderr
        let stderr = child.stderr.take().context("Failed to get stderr")?;
        let tx_stderr = tx.clone();
        thread::spawn(move || {
            let reader = std::io::BufReader::new(stderr);
            for line_result in reader.lines() {
                let line = match line_result {
                    Ok(l) => l,
                    Err(_) => break,
                };

                if line.is_empty() {
                    continue;
                }

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

    // ── CodexProcess construction ──

    #[test]
    fn codex_process_new_default_config() {
        let config = Config::default();
        let process = CodexProcess::new(config);
        assert_eq!(process.config.codex_executable(), "codex");
    }

    #[test]
    fn codex_process_new_custom_executable() {
        let config = Config {
            codex_executable: Some("/usr/local/bin/codex-cli".to_string()),
            ..Config::default()
        };
        let process = CodexProcess::new(config);
        assert_eq!(process.config.codex_executable(), "/usr/local/bin/codex-cli");
    }

    #[test]
    fn codex_process_new_with_api_key() {
        let config = Config {
            anthropic_api_key: Some("sk-test-key".to_string()),
            ..Config::default()
        };
        let process = CodexProcess::new(config);
        assert_eq!(process.config.anthropic_api_key.as_deref(), Some("sk-test-key"));
    }

    #[test]
    fn codex_process_new_verbose() {
        let config = Config {
            verbose: true,
            ..Config::default()
        };
        let process = CodexProcess::new(config);
        assert!(process.config.verbose);
    }

    #[test]
    fn codex_process_new_ignore_permissions() {
        let config = Config {
            default_permission_mode: PermissionMode::Ignore,
            ..Config::default()
        };
        let process = CodexProcess::new(config);
        assert!(matches!(process.config.default_permission_mode, PermissionMode::Ignore));
    }

    #[test]
    fn codex_process_new_approve_permissions() {
        let config = Config {
            default_permission_mode: PermissionMode::Approve,
            ..Config::default()
        };
        let process = CodexProcess::new(config);
        assert!(matches!(process.config.default_permission_mode, PermissionMode::Approve));
    }

    #[test]
    fn codex_process_new_ask_permissions() {
        let config = Config {
            default_permission_mode: PermissionMode::Ask,
            ..Config::default()
        };
        let process = CodexProcess::new(config);
        assert!(matches!(process.config.default_permission_mode, PermissionMode::Ask));
    }

    // ── CodexProcess::spawn validation ──

    #[test]
    fn codex_process_spawn_empty_prompt_fails() {
        let config = Config::default();
        let process = CodexProcess::new(config);
        let result = process.spawn(Path::new("/tmp"), "", None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    // ── AgentEvent channel communication (codex-flavored) ──

    #[test]
    fn codex_event_channel_send_receive() {
        let (tx, rx) = mpsc::channel();
        tx.send(AgentEvent::Started { pid: 200 }).unwrap();
        tx.send(AgentEvent::Output(AgentOutput {
            output_type: OutputType::Stdout,
            data: r#"{"type":"thread.started","thread_id":"abc"}"#.into(),
        })).unwrap();
        tx.send(AgentEvent::SessionId("abc".into())).unwrap();
        tx.send(AgentEvent::Exited { code: Some(0) }).unwrap();
        assert!(matches!(rx.recv().unwrap(), AgentEvent::Started { pid: 200 }));
        assert!(matches!(rx.recv().unwrap(), AgentEvent::Output(_)));
        assert!(matches!(rx.recv().unwrap(), AgentEvent::SessionId(_)));
        assert!(matches!(rx.recv().unwrap(), AgentEvent::Exited { code: Some(0) }));
    }

    #[test]
    fn codex_thread_id_extraction_logic() {
        // Verify the substring detection used in spawn()
        let line = r#"{"type":"thread.started","thread_id":"019ce52c-cfe9-7d13-869a-cf0ca4ce00e4"}"#;
        assert!(line.contains("\"thread.started\""));
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        let thread_id = parsed.get("thread_id").and_then(|v| v.as_str()).unwrap();
        assert_eq!(thread_id, "019ce52c-cfe9-7d13-869a-cf0ca4ce00e4");
    }

    #[test]
    fn codex_non_thread_started_line_not_detected() {
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"hello"}}"#;
        assert!(!line.contains("\"thread.started\""));
    }

    #[test]
    fn codex_thread_id_missing_from_json() {
        let line = r#"{"type":"thread.started"}"#;
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        let thread_id = parsed.get("thread_id").and_then(|v| v.as_str());
        assert!(thread_id.is_none());
    }

    // ── Config field access ──

    #[test]
    fn codex_process_config_all_fields() {
        let config = Config {
            anthropic_api_key: Some("key".into()),
            claude_executable: Some("/bin/claude".into()),
            codex_executable: Some("/bin/codex".into()),
            default_permission_mode: PermissionMode::Approve,
            verbose: true,
            backend: crate::backend::Backend::Codex,
        };
        let process = CodexProcess::new(config);
        assert_eq!(process.config.codex_executable(), "/bin/codex");
        assert!(process.config.verbose);
    }

    #[test]
    fn codex_process_config_none_executable() {
        let config = Config::default();
        let process = CodexProcess::new(config);
        assert_eq!(process.config.codex_executable(), "codex");
    }
}
