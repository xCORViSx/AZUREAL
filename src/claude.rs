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
pub struct ClaudeOutput {
    pub output_type: OutputType,
    pub data: String,
}

/// Events from Claude Code process
#[derive(Debug)]
pub enum ClaudeEvent {
    Output(ClaudeOutput),
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
    /// model: optional model override (e.g. "opus", "sonnet", "haiku") — passed as --model flag
    pub fn spawn(
        &self,
        working_dir: &Path,
        prompt: &str,
        resume_session_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<(mpsc::Receiver<ClaudeEvent>, u32)> {
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
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn Claude")?;

        let pid = child.id();
        let _ = tx.send(ClaudeEvent::Started { pid });

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
                            let _ = tx_stdout.send(ClaudeEvent::SessionId(session_id.to_string()));
                        }
                    }
                }

                let output = ClaudeOutput {
                    output_type: OutputType::Stdout,
                    data: format!("{}\n", line),
                };
                if tx_stdout.send(ClaudeEvent::Output(output)).is_err() {
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
                let output = ClaudeOutput {
                    output_type: OutputType::Stderr,
                    data: format!("{}\n", line),
                };
                if tx_stderr.send(ClaudeEvent::Output(output)).is_err() {
                    break;
                }
            }
        });

        // Wait for process to exit
        thread::spawn(move || {
            let status = child.wait();
            let code = status.ok().and_then(|s| s.code());
            let _ = tx.send(ClaudeEvent::Exited { code });
        });

        Ok((rx, pid))
    }
}
