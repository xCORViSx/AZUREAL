use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Claude Code settings
    pub claude: ClaudeConfig,
    /// TUI settings
    pub tui: TuiConfig,
    /// Session settings
    pub session: SessionConfig,
    /// Git settings
    pub git: GitConfig,
}

/// Claude Code CLI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeConfig {
    /// Anthropic API key (optional, Claude Code may have its own)
    pub api_key: Option<String>,
    /// Custom path to Claude Code executable
    pub executable: Option<String>,
    /// Default permission mode for new sessions
    pub permission_mode: PermissionMode,
    /// Model to use (e.g., "claude-sonnet-4-20250514")
    pub model: Option<String>,
    /// Default system prompt to prepend to all sessions
    pub system_prompt: Option<String>,
}

/// TUI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TuiConfig {
    /// Maximum number of output lines to keep in memory
    pub max_output_lines: usize,
    /// Show timestamps in output
    pub show_timestamps: bool,
    /// Auto-scroll output when new content arrives
    pub auto_scroll: bool,
    /// Sidebar width in characters
    pub sidebar_width: u16,
}

/// Session configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    /// Default worktree directory name (relative to project root)
    pub worktree_dir: String,
    /// Branch prefix for session branches
    pub branch_prefix: String,
    /// Auto-start Claude when creating a new session
    pub auto_start: bool,
}

/// Git configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GitConfig {
    /// Default main branch name if not detected
    pub default_main_branch: String,
    /// Auto-rebase before starting a session
    pub auto_rebase: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    /// Approve all permissions automatically
    Approve,
    /// Ignore/skip permissions
    #[default]
    Ignore,
    /// Ask for each permission (default Claude behavior)
    Ask,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            claude: ClaudeConfig::default(),
            tui: TuiConfig::default(),
            session: SessionConfig::default(),
            git: GitConfig::default(),
        }
    }
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            executable: None,
            permission_mode: PermissionMode::default(),
            model: None,
            system_prompt: None,
        }
    }
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            max_output_lines: 10000,
            show_timestamps: false,
            auto_scroll: true,
            sidebar_width: 30,
        }
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            worktree_dir: ".worktrees".to_string(),
            branch_prefix: "crystal".to_string(),
            auto_start: true,
        }
    }
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            default_main_branch: "main".to_string(),
            auto_rebase: false,
        }
    }
}

impl Config {
    /// Load configuration from file, returning defaults if file doesn't exist
    pub fn load() -> Result<Self> {
        let config_path = config_file_path();
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .context("Failed to read config file")?;
            toml::from_str(&content).context("Failed to parse config file")
        } else {
            Ok(Config::default())
        }
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let config_path = config_file_path();
        let content = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;
        std::fs::write(&config_path, content)
            .context("Failed to write config file")?;
        Ok(())
    }

    /// Initialize a new config file with documented defaults
    pub fn init() -> Result<()> {
        let config_path = config_file_path();
        if config_path.exists() {
            anyhow::bail!(
                "Config file already exists at {}. Use --force to overwrite.",
                config_path.display()
            );
        }
        Self::write_default_config(&config_path)
    }

    /// Force initialize a new config file, overwriting if exists
    pub fn init_force() -> Result<()> {
        let config_path = config_file_path();
        Self::write_default_config(&config_path)
    }

    /// Write the default config with documentation comments
    fn write_default_config(path: &PathBuf) -> Result<()> {
        let content = Self::default_config_with_comments();
        std::fs::write(path, content)
            .context("Failed to write config file")?;
        Ok(())
    }

    /// Generate default config content with documentation comments
    fn default_config_with_comments() -> String {
        r#"# Crystal Configuration File
# Location: ~/.crystal-rs/config.toml

# Claude Code CLI settings
[claude]
# Anthropic API key (optional - Claude Code may use its own)
# api_key = "sk-ant-..."

# Path to Claude Code executable (defaults to "claude" in PATH)
# executable = "/usr/local/bin/claude"

# Permission mode for tool execution:
#   "approve" - Auto-approve all permissions
#   "ignore"  - Skip permission prompts (default)
#   "ask"     - Ask for each permission (default Claude behavior)
permission_mode = "ignore"

# Model to use (optional - uses Claude's default if not set)
# model = "claude-sonnet-4-20250514"

# Default system prompt prepended to all sessions (optional)
# system_prompt = "You are a helpful coding assistant."

# TUI (Terminal User Interface) settings
[tui]
# Maximum output lines to keep in memory
max_output_lines = 10000

# Show timestamps in output
show_timestamps = false

# Auto-scroll output when new content arrives
auto_scroll = true

# Sidebar width in characters
sidebar_width = 30

# Session settings
[session]
# Directory for worktrees (relative to project root)
worktree_dir = ".worktrees"

# Branch prefix for session branches (creates branches like "crystal/session-name")
branch_prefix = "crystal"

# Auto-start Claude when creating a new session
auto_start = true

# Git settings
[git]
# Default main branch name if not detected from remote
default_main_branch = "main"

# Auto-rebase onto main branch before starting a session
auto_rebase = false
"#.to_string()
    }

    /// Get the Claude executable path
    pub fn claude_executable(&self) -> &str {
        self.claude.executable.as_deref().unwrap_or("claude")
    }

    /// Get permission mode for backward compatibility
    pub fn permission_mode(&self) -> PermissionMode {
        self.claude.permission_mode
    }

    /// Display the current configuration
    pub fn display(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_else(|_| "Failed to serialize config".to_string())
    }
}

/// Get the Crystal config directory (~/.crystal)
pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".crystal-rs")
}

/// Get the config file path
pub fn config_file_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// Get the database file path
pub fn database_path() -> PathBuf {
    config_dir().join("crystal.db")
}

/// Ensure the config directory exists
pub fn ensure_config_dir() -> Result<()> {
    let dir = config_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .context("Failed to create config directory")?;
    }
    Ok(())
}
