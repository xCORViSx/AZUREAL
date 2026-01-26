use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::io::Read;
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
    Exited { code: Option<i32> },
    Error(String),
}

/// Manages Claude Code CLI process
pub struct ClaudeProcess {
    config: Config,
}

impl ClaudeProcess {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Spawn Claude Code with the given prompt in the specified directory
    pub fn spawn(
        &self,
        working_dir: &Path,
        prompt: &str,
        resume_session_id: Option<&str>,
    ) -> Result<mpsc::Receiver<ClaudeEvent>> {
        let (tx, rx) = mpsc::channel();

        // Build the command
        let mut cmd = CommandBuilder::new(self.config.claude_executable());

        // Add arguments
        cmd.arg("-p");
        cmd.arg(prompt);
        cmd.arg("--verbose");
        cmd.arg("--output-format");
        cmd.arg("stream-json");

        // Add permission mode
        match self.config.default_permission_mode {
            PermissionMode::Ignore => {
                cmd.arg("--dangerously-skip-permissions");
            }
            PermissionMode::Approve => {
                // Default behavior - approve via stdin
            }
            PermissionMode::Ask => {
                // Default behavior
            }
        }

        // Resume session if specified
        if let Some(session_id) = resume_session_id {
            cmd.arg("--resume");
            cmd.arg(session_id);
        }

        cmd.cwd(working_dir);

        // Set up PTY
        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("Failed to create PTY")?;

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .context("Failed to spawn Claude process")?;

        let pid = child.process_id().unwrap_or(0);
        let _ = tx.send(ClaudeEvent::Started { pid });

        // Read output in a separate thread - stream chunks instead of lines
        let mut reader = pair.master.try_clone_reader()
            .context("Failed to clone PTY reader")?;

        let tx_clone = tx.clone();
        thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        // Convert bytes to string, handling potentially invalid UTF-8
                        let chunk = String::from_utf8_lossy(&buffer[..n]).to_string();

                        let output = ClaudeOutput {
                            output_type: OutputType::Stdout,
                            data: chunk,
                        };

                        if tx_clone.send(ClaudeEvent::Output(output)).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx_clone.send(ClaudeEvent::Error(e.to_string()));
                        break;
                    }
                }
            }
        });

        // Wait for process to exit in another thread
        thread::spawn(move || {
            let status = child.wait();
            let code = status.ok().and_then(|s| {
                if s.success() {
                    Some(0)
                } else {
                    // Try to get exit code
                    None
                }
            });
            let _ = tx.send(ClaudeEvent::Exited { code });
        });

        Ok(rx)
    }

    /// Send input to a running Claude process (for follow-up prompts)
    pub fn send_input(&self, _session_id: &str, _input: &str) -> Result<()> {
        // TODO: Implement input sending via PTY writer
        // This requires keeping track of the PTY writer for each session
        Ok(())
    }
}


/// Parse Claude's JSON output into a more usable format
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type")]
pub enum ClaudeMessage {
    #[serde(rename = "assistant")]
    Assistant { message: AssistantMessage },
    #[serde(rename = "user")]
    User { message: UserMessage },
    #[serde(rename = "result")]
    Result { result: String, subtype: Option<String> },
    #[serde(rename = "system")]
    System { message: String },
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AssistantMessage {
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct UserMessage {
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

impl ClaudeMessage {
    pub fn parse(json_str: &str) -> Option<Self> {
        serde_json::from_str(json_str).ok()
    }
}
