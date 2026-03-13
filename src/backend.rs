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
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "codex" | "openai" => Backend::Codex,
            _ => Backend::Claude,
        }
    }
}

/// Wrapper that dispatches spawn() to the correct backend process
pub enum AgentProcess {
    Claude(ClaudeProcess),
    Codex(CodexProcess),
}

impl AgentProcess {
    /// Create from config and backend selection
    pub fn new(config: Config, backend: Backend) -> Self {
        match backend {
            Backend::Claude => AgentProcess::Claude(ClaudeProcess::new(config)),
            Backend::Codex => AgentProcess::Codex(CodexProcess::new(config)),
        }
    }

    /// Spawn a new agent process with the given prompt
    pub fn spawn(
        &self,
        working_dir: &Path,
        prompt: &str,
        resume_session_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<(mpsc::Receiver<AgentEvent>, u32)> {
        match self {
            AgentProcess::Claude(p) => p.spawn(working_dir, prompt, resume_session_id, model),
            AgentProcess::Codex(p) => p.spawn(working_dir, prompt, resume_session_id, model),
        }
    }

    /// Get the active backend kind
    pub fn backend(&self) -> Backend {
        match self {
            AgentProcess::Claude(_) => Backend::Claude,
            AgentProcess::Codex(_) => Backend::Codex,
        }
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
    fn agent_process_new_claude() {
        let config = Config::default();
        let ap = AgentProcess::new(config, Backend::Claude);
        assert_eq!(ap.backend(), Backend::Claude);
    }

    #[test]
    fn agent_process_new_codex() {
        let config = Config::default();
        let ap = AgentProcess::new(config, Backend::Codex);
        assert_eq!(ap.backend(), Backend::Codex);
    }

    #[test]
    fn agent_process_spawn_empty_prompt_fails_claude() {
        let config = Config::default();
        let ap = AgentProcess::new(config, Backend::Claude);
        let result = ap.spawn(Path::new("/tmp"), "", None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn agent_process_spawn_empty_prompt_fails_codex() {
        let config = Config::default();
        let ap = AgentProcess::new(config, Backend::Codex);
        let result = ap.spawn(Path::new("/tmp"), "", None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn agent_process_backend_matches_construction() {
        let claude = AgentProcess::new(Config::default(), Backend::Claude);
        let codex = AgentProcess::new(Config::default(), Backend::Codex);
        assert_eq!(claude.backend(), Backend::Claude);
        assert_eq!(codex.backend(), Backend::Codex);
    }
}
