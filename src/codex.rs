//! Codex CLI process spawning
//!
//! Spawns `codex exec --json` processes and returns `AgentEvent`s via mpsc channel.
//! Mirrors `ClaudeProcess` in `src/claude.rs` but builds Codex-specific CLI args.

use anyhow::{Context, Result};
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use crate::claude::{AgentEvent, AgentOutput};
use crate::config::{Config, PermissionMode};
use crate::models::OutputType;

/// Maximum Codex prompt payload Azureal sends after context injection.
///
/// Codex reads the prompt from stdin, so this is not an argv limit. The cap is a
/// conservative guardrail for model-side context overhead when a session has not
/// compacted yet or compaction is delayed.
const MAX_CODEX_STDIN_PROMPT_CHARS: usize = 180_000;

/// Marker inserted when Azureal has to omit older raw context for Codex.
const CODEX_CONTEXT_TRUNCATION_NOTICE: &str = "\n[Azureal omitted older raw session context here to keep the Codex input under its prompt budget. Use the previous summary above and the recent context below.]\n\n";

/// Transcript marker that separates a compaction summary from raw recent events.
const CONVERSATION_CONTINUES_MARKER: &str = "[Conversation continues]\n\n";

/// Extract a Codex session/thread id from a JSONL event line.
fn extract_codex_session_id(line: &str) -> Option<String> {
    if !(line.contains("\"thread.started\"") || line.contains("\"type\":\"session_meta\"")) {
        return None;
    }

    let json = serde_json::from_str::<serde_json::Value>(line).ok()?;
    match json.get("type").and_then(|v| v.as_str()) {
        Some("thread.started") => json
            .get("thread_id")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        Some("session_meta") => json
            .get("payload")
            .and_then(|p| p.get("id"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        _ => None,
    }
}

/// Build Codex CLI arguments that read the prompt from stdin.
fn build_codex_exec_args(
    config: &Config,
    resume_session_id: Option<&str>,
    model: Option<&str>,
) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();

    args.push("exec".into());
    args.push("--json".into());

    if let Some(m) = model {
        args.push("--model".into());
        args.push(m.into());
    }

    match config.default_permission_mode {
        PermissionMode::Ignore => {
            args.push("--dangerously-bypass-approvals-and-sandbox".into());
        }
        PermissionMode::Approve => {
            args.push("--full-auto".into());
        }
        PermissionMode::Ask => {}
    }

    if let Some(session_id) = resume_session_id {
        args.push("resume".into());
        args.push(session_id.into());
    }

    args.push("-".into());
    args
}

/// Prepare a prompt for Codex stdin while bounding oversized injected context.
fn prepare_codex_stdin_prompt(prompt: &str) -> String {
    if prompt.len() <= MAX_CODEX_STDIN_PROMPT_CHARS {
        return prompt.to_string();
    }
    trim_injected_context_prompt(prompt, MAX_CODEX_STDIN_PROMPT_CHARS)
        .unwrap_or_else(|| prompt.to_string())
}

/// Trim Azureal's hidden context wrapper while preserving the visible user prompt.
fn trim_injected_context_prompt(prompt: &str, max_len: usize) -> Option<String> {
    let open_pos = prompt.find(crate::app::context_injection::CONTEXT_OPEN)?;
    let open_end = open_pos + crate::app::context_injection::CONTEXT_OPEN.len();
    let close_pos = prompt.find(crate::app::context_injection::CONTEXT_CLOSE)?;
    if close_pos <= open_end {
        return None;
    }

    let prefix = &prompt[..open_end];
    let body = &prompt[open_end..close_pos];
    let suffix = &prompt[close_pos..];
    let summary_prefix = body.find(CONVERSATION_CONTINUES_MARKER).map(|idx| {
        let end = idx + CONVERSATION_CONTINUES_MARKER.len();
        &body[..end]
    });
    let summary_len = summary_prefix.map(str::len).unwrap_or(0);
    let fixed_len =
        prefix.len() + summary_len + CODEX_CONTEXT_TRUNCATION_NOTICE.len() + suffix.len();
    if fixed_len >= max_len {
        return None;
    }

    let tail_budget = max_len - fixed_len;
    let tail_source = summary_prefix
        .map(|summary| &body[summary.len()..])
        .unwrap_or(body);
    let tail = tail_after_line_boundary(tail_source, tail_budget);
    let mut trimmed = String::with_capacity(fixed_len + tail.len());
    trimmed.push_str(prefix);
    if let Some(summary) = summary_prefix {
        trimmed.push_str(summary);
    }
    trimmed.push_str(CODEX_CONTEXT_TRUNCATION_NOTICE);
    trimmed.push_str(tail);
    trimmed.push_str(suffix);
    Some(trimmed)
}

/// Return the newest suffix of `text`, starting at a UTF-8 and line boundary.
fn tail_after_line_boundary(text: &str, max_len: usize) -> &str {
    if text.len() <= max_len {
        return text;
    }
    let mut start = text.len().saturating_sub(max_len);
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    if start == 0 || text[..start].ends_with('\n') {
        return &text[start..];
    }
    if let Some(newline_idx) = text[start..].find('\n') {
        &text[start + newline_idx + 1..]
    } else {
        &text[start..]
    }
}

/// Manages OpenAI Codex CLI processes
pub struct CodexProcess {
    config: Config,
}

/// Process-management methods for launching Codex CLI commands.
impl CodexProcess {
    /// Create a Codex process manager from runtime configuration.
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Spawn Codex with the given prompt
    /// resume_session_id: Codex thread_id from previous prompt (for `exec resume`)
    /// model: optional model override (e.g. "gpt-5.5", "gpt-5.4-mini") — passed as --model flag
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
        let args = build_codex_exec_args(&self.config, resume_session_id, model);
        let stdin_prompt = prepare_codex_stdin_prompt(prompt);

        let mut cmd_builder = Command::new(executable);
        cmd_builder
            .args(&args)
            .current_dir(working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Make child its own process group leader so kill_process_tree() can
        // kill it and all its descendants
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd_builder.process_group(0);
        }

        let mut child = cmd_builder.spawn().context("Failed to spawn Codex")?;

        let pid = child.id();
        let _ = tx.send(AgentEvent::Started { pid });

        // Write the full prompt through stdin instead of argv. Large injected
        // transcripts can exceed platform command-line limits before Codex even
        // starts; stdin keeps process spawning independent from prompt size.
        let mut stdin = child.stdin.take().context("Failed to get stdin")?;
        let stdin_thread = thread::spawn(move || {
            let _ = stdin.write_all(stdin_prompt.as_bytes());
        });

        // Read stdout (JSONL events)
        let stdout = child.stdout.take().context("Failed to get stdout")?;
        let tx_stdout = tx.clone();
        let stdout_thread = thread::spawn(move || {
            let reader = std::io::BufReader::new(stdout);
            for line_result in reader.lines() {
                let line = match line_result {
                    Ok(l) => l,
                    Err(_) => break,
                };

                if line.is_empty() {
                    continue;
                }

                if let Some(session_id) = extract_codex_session_id(&line) {
                    let _ = tx_stdout.send(AgentEvent::SessionId(session_id));
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
        let stderr_thread = thread::spawn(move || {
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
            let _ = stdin_thread.join();
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            let code = status.ok().and_then(|s| s.code());
            let _ = tx.send(AgentEvent::Exited { code });
        });

        Ok((rx, pid))
    }
}

/// Regression tests for Codex CLI process setup and event parsing.
#[cfg(test)]
mod tests {
    use super::*;

    // ── CodexProcess construction ──

    /// Default configuration uses the `codex` executable name.
    #[test]
    fn codex_process_new_default_config() {
        let config = Config::default();
        let process = CodexProcess::new(config);
        assert_eq!(process.config.codex_executable(), "codex");
    }

    /// Custom configuration keeps the configured Codex executable path.
    #[test]
    fn codex_process_new_custom_executable() {
        let config = Config {
            codex_executable: Some("/usr/local/bin/codex-cli".to_string()),
            ..Config::default()
        };
        let process = CodexProcess::new(config);
        assert_eq!(
            process.config.codex_executable(),
            "/usr/local/bin/codex-cli"
        );
    }

    /// Existing API-key configuration is preserved when constructing the process manager.
    #[test]
    fn codex_process_new_with_api_key() {
        let config = Config {
            anthropic_api_key: Some("sk-test-key".to_string()),
            ..Config::default()
        };
        let process = CodexProcess::new(config);
        assert_eq!(
            process.config.anthropic_api_key.as_deref(),
            Some("sk-test-key")
        );
    }

    /// Verbose configuration is preserved on the process manager.
    #[test]
    fn codex_process_new_verbose() {
        let config = Config {
            verbose: true,
            ..Config::default()
        };
        let process = CodexProcess::new(config);
        assert!(process.config.verbose);
    }

    /// Ignore permission mode maps through configuration unchanged.
    #[test]
    fn codex_process_new_ignore_permissions() {
        let config = Config {
            default_permission_mode: PermissionMode::Ignore,
            ..Config::default()
        };
        let process = CodexProcess::new(config);
        assert!(matches!(
            process.config.default_permission_mode,
            PermissionMode::Ignore
        ));
    }

    /// Approve permission mode maps through configuration unchanged.
    #[test]
    fn codex_process_new_approve_permissions() {
        let config = Config {
            default_permission_mode: PermissionMode::Approve,
            ..Config::default()
        };
        let process = CodexProcess::new(config);
        assert!(matches!(
            process.config.default_permission_mode,
            PermissionMode::Approve
        ));
    }

    /// Ask permission mode maps through configuration unchanged.
    #[test]
    fn codex_process_new_ask_permissions() {
        let config = Config {
            default_permission_mode: PermissionMode::Ask,
            ..Config::default()
        };
        let process = CodexProcess::new(config);
        assert!(matches!(
            process.config.default_permission_mode,
            PermissionMode::Ask
        ));
    }

    // ── CodexProcess::spawn validation ──

    /// Spawning rejects empty prompts before invoking the Codex executable.
    #[test]
    fn codex_process_spawn_empty_prompt_fails() {
        let config = Config::default();
        let process = CodexProcess::new(config);
        let result = process.spawn(Path::new("/tmp"), "", None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    /// Codex exec args use stdin marker instead of placing the prompt in argv.
    #[test]
    fn codex_exec_args_read_prompt_from_stdin() {
        let args = build_codex_exec_args(&Config::default(), None, Some("gpt-5.5"));
        assert_eq!(args.last().map(String::as_str), Some("-"));
        assert!(args.windows(2).any(|pair| pair == ["--model", "gpt-5.5"]));
        assert!(!args.iter().any(|arg| arg.contains("visible prompt")));
    }

    /// Resume args keep the session id while still reading the prompt from stdin.
    #[test]
    fn codex_exec_args_resume_read_prompt_from_stdin() {
        let args = build_codex_exec_args(&Config::default(), Some("session-123"), None);
        assert!(args
            .windows(2)
            .any(|pair| pair == ["resume", "session-123"]));
        assert_eq!(args.last().map(String::as_str), Some("-"));
    }

    /// Ignore permissions map to Codex's bypass flag.
    #[test]
    fn codex_exec_args_ignore_permissions() {
        let config = Config {
            default_permission_mode: PermissionMode::Ignore,
            ..Config::default()
        };
        let args = build_codex_exec_args(&config, None, None);
        assert!(args
            .iter()
            .any(|arg| arg == "--dangerously-bypass-approvals-and-sandbox"));
    }

    /// Approve permissions map to Codex full-auto mode.
    #[test]
    fn codex_exec_args_approve_permissions() {
        let config = Config {
            default_permission_mode: PermissionMode::Approve,
            ..Config::default()
        };
        let args = build_codex_exec_args(&config, None, None);
        assert!(args.iter().any(|arg| arg == "--full-auto"));
    }

    /// Short prompts pass through stdin preparation unchanged.
    #[test]
    fn prepare_codex_stdin_prompt_keeps_short_prompt() {
        assert_eq!(prepare_codex_stdin_prompt("short"), "short");
    }

    /// Oversized non-context prompts are preserved because they are user content.
    #[test]
    fn prepare_codex_stdin_prompt_keeps_plain_large_prompt() {
        let prompt = "x".repeat(MAX_CODEX_STDIN_PROMPT_CHARS + 1);
        assert_eq!(prepare_codex_stdin_prompt(&prompt), prompt);
    }

    /// Oversized injected context is trimmed while preserving the user prompt.
    #[test]
    fn prepare_codex_stdin_prompt_trims_injected_context() {
        let context = "old\n".repeat(MAX_CODEX_STDIN_PROMPT_CHARS / 2);
        let prompt = format!(
            "{}\n{}{}\n\nvisible prompt",
            crate::app::context_injection::CONTEXT_OPEN,
            context,
            crate::app::context_injection::CONTEXT_CLOSE
        );

        let prepared = prepare_codex_stdin_prompt(&prompt);

        assert!(prepared.len() < prompt.len());
        assert!(prepared.len() <= MAX_CODEX_STDIN_PROMPT_CHARS);
        assert!(prepared.contains(CODEX_CONTEXT_TRUNCATION_NOTICE.trim()));
        assert!(prepared.ends_with("visible prompt"));
        assert!(prepared.contains(crate::app::context_injection::CONTEXT_CLOSE));
    }

    /// Context trimming preserves a compaction summary before dropping older raw events.
    #[test]
    fn trim_injected_context_prompt_preserves_compaction_summary() {
        let summary =
            "\n[Previous conversation summary]\nsummary text\n\n[Conversation continues]\n\n";
        let context = format!(
            "{}{}",
            summary,
            "old\n".repeat(MAX_CODEX_STDIN_PROMPT_CHARS / 2)
        );
        let prompt = format!(
            "{}{}{}\n\nvisible prompt",
            crate::app::context_injection::CONTEXT_OPEN,
            context,
            crate::app::context_injection::CONTEXT_CLOSE
        );

        let prepared = prepare_codex_stdin_prompt(&prompt);

        assert!(prepared.contains("summary text"));
        assert!(prepared.contains(CONVERSATION_CONTINUES_MARKER));
        assert!(prepared.ends_with("visible prompt"));
    }

    /// Tail limiting starts on a line boundary after trimming older content.
    #[test]
    fn tail_after_line_boundary_skips_partial_first_line() {
        assert_eq!(tail_after_line_boundary("abc\ndef\nghi", 7), "def\nghi");
    }

    // ── AgentEvent channel communication (codex-flavored) ──

    /// Agent events sent over a channel are received in order.
    #[test]
    fn codex_event_channel_send_receive() {
        let (tx, rx) = mpsc::channel();
        tx.send(AgentEvent::Started { pid: 200 }).unwrap();
        tx.send(AgentEvent::Output(AgentOutput {
            output_type: OutputType::Stdout,
            data: r#"{"type":"thread.started","thread_id":"abc"}"#.into(),
        }))
        .unwrap();
        tx.send(AgentEvent::SessionId("abc".into())).unwrap();
        tx.send(AgentEvent::Exited { code: Some(0) }).unwrap();
        assert!(matches!(
            rx.recv().unwrap(),
            AgentEvent::Started { pid: 200 }
        ));
        assert!(matches!(rx.recv().unwrap(), AgentEvent::Output(_)));
        assert!(matches!(rx.recv().unwrap(), AgentEvent::SessionId(_)));
        assert!(matches!(
            rx.recv().unwrap(),
            AgentEvent::Exited { code: Some(0) }
        ));
    }

    /// `thread.started` JSONL events expose the Codex thread id.
    #[test]
    fn codex_thread_id_extraction_logic() {
        let line =
            r#"{"type":"thread.started","thread_id":"019ce52c-cfe9-7d13-869a-cf0ca4ce00e4"}"#;
        assert_eq!(
            extract_codex_session_id(line).as_deref(),
            Some("019ce52c-cfe9-7d13-869a-cf0ca4ce00e4")
        );
    }

    /// `session_meta` JSONL events expose the Codex session id.
    #[test]
    fn codex_session_meta_id_extraction_logic() {
        let line = r#"{"type":"session_meta","payload":{"id":"019cf628-b245-7a21-ae00-bbaf2cd408dc","cwd":"/tmp"}}"#;
        assert_eq!(
            extract_codex_session_id(line).as_deref(),
            Some("019cf628-b245-7a21-ae00-bbaf2cd408dc")
        );
    }

    /// Non-session JSONL events do not produce a session id.
    #[test]
    fn codex_non_thread_started_line_not_detected() {
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"hello"}}"#;
        assert!(extract_codex_session_id(line).is_none());
    }

    /// Malformed session-start events with no id are ignored.
    #[test]
    fn codex_thread_id_missing_from_json() {
        let line = r#"{"type":"thread.started"}"#;
        assert!(extract_codex_session_id(line).is_none());
    }

    // ── Config field access ──

    /// All relevant configuration fields remain accessible from the process manager.
    #[test]
    fn codex_process_config_all_fields() {
        let config = Config {
            anthropic_api_key: Some("key".into()),
            claude_executable: Some("/bin/claude".into()),
            codex_executable: Some("/bin/codex".into()),
            default_permission_mode: PermissionMode::Approve,
            verbose: true,
        };
        let process = CodexProcess::new(config);
        assert_eq!(process.config.codex_executable(), "/bin/codex");
        assert!(process.config.verbose);
    }

    /// Missing executable configuration falls back to the default command name.
    #[test]
    fn codex_process_config_none_executable() {
        let config = Config::default();
        let process = CodexProcess::new(config);
        assert_eq!(process.config.codex_executable(), "codex");
    }
}
