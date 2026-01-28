use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{BufRead, BufReader, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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

/// Handle for writing to an interactive Claude session
pub struct InteractiveSession {
    writer: Box<dyn Write + Send>,
    session_file: PathBuf,
    file_pos: u64,
    /// Track tool calls by ID so we can match results to calls
    tool_calls: std::collections::HashMap<String, (String, Option<String>)>,
}

impl InteractiveSession {
    /// Send a prompt to the interactive Claude session
    pub fn send_prompt(&mut self, prompt: &str) -> Result<()> {
        writeln!(self.writer, "{}", prompt)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Poll session file for new events, returns parsed DisplayEvents
    pub fn poll_events(&mut self) -> Vec<crate::events::DisplayEvent> {
        let mut events = Vec::new();

        let file = match std::fs::File::open(&self.session_file) {
            Ok(f) => f,
            Err(_) => return events,
        };

        let metadata = match file.metadata() {
            Ok(m) => m,
            Err(_) => return events,
        };

        if metadata.len() <= self.file_pos {
            return events;
        }

        let mut reader = BufReader::new(file);
        if reader.seek(std::io::SeekFrom::Start(self.file_pos)).is_err() {
            return events;
        }

        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                let parsed = parse_session_events(&json, &mut self.tool_calls);
                events.extend(parsed);
            }
            line.clear();
        }

        self.file_pos = metadata.len();
        events
    }
}

/// Parse a session JSONL event into DisplayEvents (can return multiple for messages with multiple blocks)
fn parse_session_events(json: &serde_json::Value, tool_calls: &mut std::collections::HashMap<String, (String, Option<String>)>) -> Vec<crate::events::DisplayEvent> {
    let mut events = Vec::new();
    let event_type = match json.get("type").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => return events,
    };

    match event_type {
        "user" => {
            let message = match json.get("message") {
                Some(m) => m,
                None => return events,
            };
            let content_val = match message.get("content") {
                Some(c) => c,
                None => return events,
            };

            // String content = user prompt
            if let Some(content) = content_val.as_str() {
                events.push(crate::events::DisplayEvent::UserMessage {
                    uuid: json.get("uuid").and_then(|u| u.as_str()).unwrap_or("").to_string(),
                    content: content.to_string(),
                });
            }
            // Array content = tool results
            else if let Some(arr) = content_val.as_array() {
                for block in arr {
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                        let tool_use_id = block.get("tool_use_id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                        let (tool_name, file_path) = tool_calls.get(&tool_use_id).cloned().unwrap_or(("Unknown".to_string(), None));

                        let content = if let Some(s) = block.get("content").and_then(|c| c.as_str()) {
                            s.to_string()
                        } else if let Some(arr) = block.get("content").and_then(|c| c.as_array()) {
                            arr.iter()
                                .filter_map(|b| if b.get("type").and_then(|t| t.as_str()) == Some("text") { b.get("text").and_then(|t| t.as_str()) } else { None })
                                .collect::<Vec<_>>().join("\n")
                        } else { String::new() };

                        if !content.is_empty() {
                            events.push(crate::events::DisplayEvent::ToolResult { tool_use_id, tool_name, file_path, content });
                        }
                    }
                }
            }
        }
        "assistant" => {
            let message = match json.get("message") {
                Some(m) => m,
                None => return events,
            };
            let content_arr = match message.get("content").and_then(|c| c.as_array()) {
                Some(arr) => arr,
                None => return events,
            };
            let uuid = json.get("uuid").and_then(|u| u.as_str()).unwrap_or("").to_string();
            let message_id = message.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();

            for block in content_arr {
                let block_type = match block.get("type").and_then(|t| t.as_str()) {
                    Some(t) => t,
                    None => continue,
                };
                match block_type {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            events.push(crate::events::DisplayEvent::AssistantText {
                                uuid: uuid.clone(), message_id: message_id.clone(), text: text.to_string(),
                            });
                        }
                    }
                    "tool_use" => {
                        let tool_name = block.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                        let tool_id = block.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                        let input = block.get("input").cloned().unwrap_or(serde_json::Value::Null);
                        let file_path = input.get("file_path").or(input.get("path")).and_then(|p| p.as_str()).map(|s| s.to_string());
                        tool_calls.insert(tool_id.clone(), (tool_name.clone(), file_path.clone()));
                        events.push(crate::events::DisplayEvent::ToolCall {
                            uuid: uuid.clone(), tool_use_id: tool_id, tool_name, file_path, input,
                        });
                    }
                    _ => {}
                }
            }
        }
        // Skip system/init events for realtime polling - they're handled at session load
        _ => {}
    }
    events
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

    /// Spawn Claude in interactive PTY mode (keeps process alive)
    /// Returns an InteractiveSession handle for sending prompts and a receiver for events
    pub fn spawn_interactive(
        &self,
        working_dir: &Path,
        resume_session_id: Option<&str>,
    ) -> Result<(InteractiveSession, mpsc::Receiver<ClaudeEvent>)> {
        let (tx, rx) = mpsc::channel();

        // Create PTY
        let pty_system = native_pty_system();
        let pty_pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }).context("Failed to open PTY")?;

        // Build command for interactive mode (no -p)
        let mut cmd = CommandBuilder::new(self.config.claude_executable());

        // Resume if we have a session ID
        if let Some(session_id) = resume_session_id {
            cmd.arg("--resume");
            cmd.arg(session_id);
        }

        // Permission mode
        match self.config.default_permission_mode {
            PermissionMode::Ignore => {
                cmd.arg("--dangerously-skip-permissions");
            }
            PermissionMode::Approve | PermissionMode::Ask => {}
        }

        cmd.cwd(working_dir);

        // Spawn child process
        let mut child = pty_pair.slave.spawn_command(cmd)
            .context("Failed to spawn Claude in interactive mode")?;

        let pid = child.process_id().unwrap_or(0);
        let _ = tx.send(ClaudeEvent::Started { pid });

        // Get writer for sending prompts
        let writer = pty_pair.master.take_writer()
            .context("Failed to get PTY writer")?;

        // Determine session file path
        let encoded_path = working_dir.to_string_lossy().replace('/', "-");
        let claude_dir = dirs::home_dir()
            .context("No home dir")?
            .join(".claude")
            .join("projects")
            .join(&encoded_path);

        // Find session file (use resume_session_id or wait for new one)
        let session_file = if let Some(sid) = resume_session_id {
            claude_dir.join(format!("{}.jsonl", sid))
        } else {
            // For new sessions, we'll need to discover the session ID
            // Poll directory for newest .jsonl file after a brief delay
            thread::sleep(Duration::from_millis(500));
            crate::config::find_latest_claude_session(working_dir)
                .map(|id| claude_dir.join(format!("{}.jsonl", id)))
                .unwrap_or_else(|| claude_dir.join("unknown.jsonl"))
        };

        let file_pos = session_file.metadata().map(|m| m.len()).unwrap_or(0);

        // Read PTY output in background (for raw terminal display)
        let reader = pty_pair.master.try_clone_reader()
            .context("Failed to clone PTY reader")?;
        let tx_output = tx.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buf[..n]).to_string();
                        let output = ClaudeOutput {
                            output_type: OutputType::Stdout,
                            data,
                        };
                        if tx_output.send(ClaudeEvent::Output(output)).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // Wait for process exit in background
        let tx_exit = tx;
        thread::spawn(move || {
            let status = child.wait();
            let code = status.ok().map(|s| s.exit_code() as i32);
            let _ = tx_exit.send(ClaudeEvent::Exited { code });
        });

        let session = InteractiveSession {
            writer,
            session_file,
            file_pos,
            tool_calls: std::collections::HashMap::new(),
        };

        Ok((session, rx))
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

                // Parse JSON to extract session_id from init event
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                    if json.get("type").and_then(|v| v.as_str()) == Some("system")
                        && json.get("subtype").and_then(|v| v.as_str()) == Some("init")
                    {
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

        Ok(rx)
    }
}
