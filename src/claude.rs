use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
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
    /// Map of session IDs to their PTY masters for bidirectional communication
    active_ptys: Arc<Mutex<HashMap<String, Box<dyn MasterPty + Send>>>>,
}

impl ClaudeProcess {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            active_ptys: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Spawn Claude Code with the given prompt in the specified directory
    pub fn spawn(
        &self,
        session_id: String,
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
        if let Some(resume_id) = resume_session_id {
            cmd.arg("--resume");
            cmd.arg(resume_id);
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

        // Store the master PTY for later input (bidirectional communication)
        let master_pty = pair.master;
        {
            let mut active_ptys = self.active_ptys.lock().unwrap();
            active_ptys.insert(session_id.clone(), master_pty);
        }

        // Clone PTY reader for output thread - stream chunks instead of lines
        let active_ptys_clone = self.active_ptys.clone();
        let session_id_for_reader = session_id.clone();
        let mut reader = {
            let active_ptys = active_ptys_clone.lock().unwrap();
            if let Some(pty) = active_ptys.get(&session_id_for_reader) {
                pty.try_clone_reader()
                    .context("Failed to clone PTY reader")?
            } else {
                anyhow::bail!("PTY not found for session");
            }
        };

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
        let active_ptys_for_exit = self.active_ptys.clone();
        let session_id_for_exit = session_id.clone();
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

            // Clean up PTY when process exits
            let mut active_ptys = active_ptys_for_exit.lock().unwrap();
            active_ptys.remove(&session_id_for_exit);

            let _ = tx.send(ClaudeEvent::Exited { code });
        });

        Ok(rx)
    }

    /// Send input to a running Claude process (for follow-up prompts and approvals)
    pub fn send_input(&self, session_id: &str, input: &str) -> Result<()> {
        let mut active_ptys = self.active_ptys.lock().unwrap();

        if let Some(pty) = active_ptys.get_mut(session_id) {
            let mut writer = pty.take_writer()
                .context("Failed to get PTY writer")?;

            // Write input followed by newline
            writeln!(writer, "{}", input)
                .context("Failed to write to PTY")?;

            writer.flush()
                .context("Failed to flush PTY writer")?;

            Ok(())
        } else {
            anyhow::bail!("No active PTY found for session: {}", session_id)
        }
    }

    /// Check if a session has an active PTY (is running)
    pub fn is_session_running(&self, session_id: &str) -> bool {
        let active_ptys = self.active_ptys.lock().unwrap();
        active_ptys.contains_key(session_id)
    }

    /// Stop a running session by closing its PTY
    pub fn stop_session(&self, session_id: &str) -> Result<()> {
        let mut active_ptys = self.active_ptys.lock().unwrap();

        if active_ptys.remove(session_id).is_some() {
            Ok(())
        } else {
            anyhow::bail!("No active PTY found for session: {}", session_id)
        }
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
