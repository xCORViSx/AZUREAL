use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{BufReader, Read};
use std::path::Path;
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
    Error(String),
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
    pub fn spawn(
        &self,
        working_dir: &Path,
        prompt: &str,
        resume_session_id: Option<&str>,
    ) -> Result<mpsc::Receiver<ClaudeEvent>> {
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

        cmd.cwd(working_dir);

        // Create PTY (80x30 like Crystal's xterm-color)
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 30,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("Failed to create PTY")?;

        // Spawn child in PTY
        let mut child = pair.slave.spawn_command(cmd).context("Failed to spawn Claude in PTY")?;

        // Get process ID (portable-pty returns Option<u32>)
        let pid = child.process_id().unwrap_or(0);
        let _ = tx.send(ClaudeEvent::Started { pid });

        // Read from PTY master
        let reader = pair.master.try_clone_reader().context("Failed to clone PTY reader")?;
        let tx_output = tx.clone();

        thread::spawn(move || {
            let buf_reader = BufReader::new(reader);
            let mut line_buffer = String::new();

            // PTY output may not be line-buffered, so we need to handle partial lines
            for byte_result in buf_reader.bytes() {
                match byte_result {
                    Ok(byte) => {
                        let ch = byte as char;
                        if ch == '\n' {
                            // Process complete line
                            let line = line_buffer.trim_end().to_string();
                            line_buffer.clear();

                            if line.is_empty() {
                                continue;
                            }

                            // Parse JSON to extract session_id from init event
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                                if json.get("type").and_then(|v| v.as_str()) == Some("system")
                                    && json.get("subtype").and_then(|v| v.as_str()) == Some("init")
                                {
                                    if let Some(session_id) =
                                        json.get("session_id").and_then(|v| v.as_str())
                                    {
                                        let _ = tx_output
                                            .send(ClaudeEvent::SessionId(session_id.to_string()));
                                    }
                                }
                            }

                            // Send output for display
                            let output = ClaudeOutput {
                                output_type: OutputType::Stdout,
                                data: format!("{}\n", line),
                            };
                            if tx_output.send(ClaudeEvent::Output(output)).is_err() {
                                break;
                            }
                        } else if ch != '\r' {
                            // Skip carriage returns, accumulate other chars
                            line_buffer.push(ch);
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // Wait for process to exit
        thread::spawn(move || {
            let status = child.wait();
            let code = status.ok().map(|s| s.exit_code() as i32);
            let _ = tx.send(ClaudeEvent::Exited { code });
        });

        Ok(rx)
    }
}
