//! Multi-backend agent abstraction
//!
//! Dispatches between Claude Code CLI and OpenAI Codex CLI.
//! Both backends produce `AgentEvent` for the event loop and
//! `DisplayEvent` for the TUI — the rest of the app is backend-agnostic.

use std::path::Path;
use std::sync::mpsc;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::claude::{AgentEvent, ClaudeProcess};
use crate::codex::CodexProcess;
use crate::config::Config;

/// Which AI backend to use
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Claude,
    Codex,
}

impl Backend {
    /// Return the other backend (Claude↔Codex).
    pub fn alternate(self) -> Self {
        match self {
            Backend::Claude => Backend::Codex,
            Backend::Codex => Backend::Claude,
        }
    }
}

impl Default for Backend {
    fn default() -> Self {
        Backend::Claude
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Backend::Claude => write!(f, "claude"),
            Backend::Codex => write!(f, "codex"),
        }
    }
}

impl Backend {
    /// Parse from string (for config loading)
    #[allow(dead_code)]
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "codex" | "openai" => Backend::Codex,
            _ => Backend::Claude,
        }
    }
}

/// Holds both backend processes and dispatches spawn() based on model selection.
/// The backend is determined at spawn time from the model name, not at construction.
pub struct AgentProcess {
    claude: ClaudeProcess,
    codex: CodexProcess,
}

impl AgentProcess {
    /// Create with both backends available
    pub fn new(config: Config) -> Self {
        AgentProcess {
            claude: ClaudeProcess::new(config.clone()),
            codex: CodexProcess::new(config),
        }
    }

    /// Spawn a new agent process on an explicitly chosen backend.
    pub fn spawn_on_backend(
        &self,
        backend: Backend,
        working_dir: &Path,
        prompt: &str,
        resume_session_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<(mpsc::Receiver<AgentEvent>, u32)> {
        match backend {
            Backend::Claude => self
                .claude
                .spawn(working_dir, prompt, resume_session_id, model),
            Backend::Codex => self
                .codex
                .spawn(working_dir, prompt, resume_session_id, model),
        }
    }

    /// Spawn a new agent process. The backend is selected automatically
    /// based on the model name (gpt-* → Codex, else → Claude).
    pub fn spawn(
        &self,
        working_dir: &Path,
        prompt: &str,
        resume_session_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<(mpsc::Receiver<AgentEvent>, u32)> {
        let backend = model
            .map(crate::app::state::backend_for_model)
            .unwrap_or(Backend::Claude);
        self.spawn_on_backend(backend, working_dir, prompt, resume_session_id, model)
    }
}

/// Kill a process and all its children by PID.
/// On Unix, uses process group kill (SIGTERM to -pgid) so all descendants die.
/// On Windows, uses `taskkill /T` (tree kill) to terminate the process tree.
pub fn kill_process_tree(pid: u32) {
    #[cfg(unix)]
    {
        // Send SIGTERM to the entire process group. The spawned process is a
        // process group leader (via `process_group(0)` in claude.rs / codex.rs),
        // so killing -pgid reaches all children (cargo test, subagents, etc.).
        unsafe {
            libc::killpg(pid as libc::pid_t, libc::SIGTERM);
        }
    }
    #[cfg(windows)]
    {
        // /T = terminate child processes, /F = force
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
    }
}

/// Forcefully kill a process group with SIGKILL (cannot be ignored).
/// Used during app shutdown after SIGTERM has been given time to take effect.
#[cfg(unix)]
pub fn kill_process_tree_force(pid: u32) {
    unsafe {
        libc::killpg(pid as libc::pid_t, libc::SIGKILL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Backend enum ──

    #[test]
    fn backend_default_is_claude() {
        assert_eq!(Backend::default(), Backend::Claude);
    }

    #[test]
    fn backend_display_claude() {
        assert_eq!(Backend::Claude.to_string(), "claude");
    }

    #[test]
    fn backend_display_codex() {
        assert_eq!(Backend::Codex.to_string(), "codex");
    }

    #[test]
    fn backend_from_str_claude() {
        assert_eq!(Backend::from_str_loose("claude"), Backend::Claude);
    }

    #[test]
    fn backend_from_str_codex() {
        assert_eq!(Backend::from_str_loose("codex"), Backend::Codex);
    }

    #[test]
    fn backend_from_str_openai() {
        assert_eq!(Backend::from_str_loose("openai"), Backend::Codex);
    }

    #[test]
    fn backend_from_str_unknown_defaults_claude() {
        assert_eq!(Backend::from_str_loose("gemini"), Backend::Claude);
    }

    #[test]
    fn backend_from_str_empty_defaults_claude() {
        assert_eq!(Backend::from_str_loose(""), Backend::Claude);
    }

    #[test]
    fn backend_from_str_case_insensitive() {
        assert_eq!(Backend::from_str_loose("CODEX"), Backend::Codex);
        assert_eq!(Backend::from_str_loose("Claude"), Backend::Claude);
        assert_eq!(Backend::from_str_loose("OpenAI"), Backend::Codex);
    }

    #[test]
    fn backend_equality() {
        assert_eq!(Backend::Claude, Backend::Claude);
        assert_eq!(Backend::Codex, Backend::Codex);
        assert_ne!(Backend::Claude, Backend::Codex);
    }

    #[test]
    fn backend_clone() {
        let b = Backend::Codex;
        let c = b;
        assert_eq!(b, c);
    }

    #[test]
    fn backend_debug() {
        let dbg = format!("{:?}", Backend::Claude);
        assert!(dbg.contains("Claude"));
        let dbg = format!("{:?}", Backend::Codex);
        assert!(dbg.contains("Codex"));
    }

    #[test]
    fn backend_serde_roundtrip() {
        let json = serde_json::to_string(&Backend::Codex).unwrap();
        assert_eq!(json, "\"codex\"");
        let parsed: Backend = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Backend::Codex);

        let json = serde_json::to_string(&Backend::Claude).unwrap();
        assert_eq!(json, "\"claude\"");
        let parsed: Backend = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Backend::Claude);
    }

    #[test]
    fn backend_serde_deserialize_unknown_fails() {
        let result: Result<Backend, _> = serde_json::from_str("\"gemini\"");
        assert!(result.is_err());
    }

    // ── AgentProcess ──

    #[test]
    fn agent_process_new() {
        let _ap = AgentProcess::new(Config::default());
        // Both backends available — no panic
    }

    #[test]
    fn agent_process_spawn_empty_prompt_fails_claude() {
        let ap = AgentProcess::new(Config::default());
        let result = ap.spawn(Path::new("/tmp"), "", None, Some("opus"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn agent_process_spawn_empty_prompt_fails_codex() {
        let ap = AgentProcess::new(Config::default());
        let result = ap.spawn(Path::new("/tmp"), "", None, Some("gpt-5.4"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn agent_process_spawn_no_model_defaults_claude() {
        let ap = AgentProcess::new(Config::default());
        let result = ap.spawn(Path::new("/tmp"), "", None, None);
        assert!(result.is_err());
        // Default (None) → Claude backend, which rejects empty prompts
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn agent_process_spawn_on_backend_empty_prompt_fails_claude() {
        let ap = AgentProcess::new(Config::default());
        let result = ap.spawn_on_backend(Backend::Claude, Path::new("/tmp"), "", None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn agent_process_spawn_on_backend_empty_prompt_fails_codex() {
        let ap = AgentProcess::new(Config::default());
        let result = ap.spawn_on_backend(Backend::Codex, Path::new("/tmp"), "", None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }
}
