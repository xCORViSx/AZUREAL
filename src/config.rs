use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct Config {
    /// Anthropic API key (optional, Claude Code may have its own)
    pub anthropic_api_key: Option<String>,
    /// Custom path to Claude Code executable
    pub claude_executable: Option<String>,
    /// Default permission mode for new sessions
    #[serde(default)]
    pub default_permission_mode: PermissionMode,
    /// Enable verbose logging
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
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


impl Config {
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

    pub fn save(&self) -> Result<()> {
        let config_path = config_file_path();
        let content = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;
        std::fs::write(&config_path, content)
            .context("Failed to write config file")?;
        Ok(())
    }

    pub fn claude_executable(&self) -> &str {
        self.claude_executable.as_deref().unwrap_or("claude")
    }
}

/// Get the global Azureal config directory (~/.azureal/)
/// Used for global config like config.toml
pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".azureal")
}

/// Get project-specific Azureal data directory (.azureal/ in git root)
/// Used for run_commands.json, debug-output.txt, etc.
/// Returns None if not in a git repository
pub fn project_data_dir() -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if output.status.success() {
        let path = String::from_utf8(output.stdout).ok()?;
        Some(PathBuf::from(path.trim()).join(".azureal"))
    } else {
        None
    }
}

/// Get the config file path (~/.azureal/config.toml)
pub fn config_file_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// Ensure the global config directory exists
pub fn ensure_config_dir() -> Result<()> {
    let dir = config_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .context("Failed to create config directory")?;
    }
    Ok(())
}

/// Ensure the project data directory exists (creates .azureal/ in git root)
/// Only call this when actually writing project-specific data
pub fn ensure_project_data_dir() -> Result<Option<PathBuf>> {
    if let Some(dir) = project_data_dir() {
        if !dir.exists() {
            std::fs::create_dir_all(&dir)
                .context("Failed to create project data directory")?;
        }
        Ok(Some(dir))
    } else {
        Ok(None)
    }
}

/// Get Claude's session file path for a given project path and session ID
/// Claude stores sessions at: ~/.claude/projects/<encoded-path>/<session-id>.jsonl
pub fn claude_session_file(project_path: &std::path::Path, session_id: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    // Claude encodes paths by replacing / with -
    let encoded_path = project_path.to_string_lossy().replace('/', "-");
    let session_file = home
        .join(".claude")
        .join("projects")
        .join(&encoded_path)
        .join(format!("{}.jsonl", session_id));
    if session_file.exists() { Some(session_file) } else { None }
}

/// Get Claude's project directory for a given worktree path
pub fn claude_project_dir(worktree_path: &std::path::Path) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let encoded_path = worktree_path.to_string_lossy().replace('/', "-");
    let dir = home.join(".claude").join("projects").join(&encoded_path);
    if dir.exists() { Some(dir) } else { None }
}

/// Format a SystemTime as a relative or absolute time string (called once at load, not per-frame)
fn format_time(mtime: std::time::SystemTime) -> String {
    let Ok(dur) = std::time::SystemTime::now().duration_since(mtime) else {
        return "future".to_string();
    };
    let secs = dur.as_secs();
    if secs < 60 { return format!("{}s ago", secs); }
    if secs < 3600 { return format!("{}m ago", secs / 60); }
    if secs < 86400 { return format!("{}h ago", secs / 3600); }
    if secs < 604800 { return format!("{}d ago", secs / 86400); }
    // Older than a week: show date
    let datetime = chrono::DateTime::<chrono::Local>::from(mtime);
    datetime.format("%b %d").to_string()
}

/// List all Claude session files for a worktree, sorted by modification time (newest first)
/// Returns (session_id, path, pre-formatted_time_string) to avoid per-frame formatting
pub fn list_claude_sessions(worktree_path: &std::path::Path) -> Vec<(String, PathBuf, String)> {
    let Some(project_dir) = claude_project_dir(worktree_path) else { return Vec::new() };

    let mut sessions: Vec<(String, PathBuf, std::time::SystemTime)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&project_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let mtime = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                    sessions.push((stem.to_string(), path, mtime));
                }
            }
        }
    }
    // Sort by modification time, newest first
    sessions.sort_by(|a, b| b.2.cmp(&a.2));
    // Pre-format time strings (expensive chrono call done once, not per-frame)
    sessions.into_iter().map(|(id, path, mtime)| (id, path, format_time(mtime))).collect()
}

/// Find the most recent Claude session for a worktree
pub fn find_latest_claude_session(worktree_path: &std::path::Path) -> Option<String> {
    list_claude_sessions(worktree_path).first().map(|(id, _, _)| id.clone())
}

// ── Projects persistence (~/.azureal/projects.txt) ──

/// A registered project entry: absolute path + optional display name
#[derive(Debug, Clone)]
pub struct ProjectEntry {
    pub path: PathBuf,
    pub display_name: String,
}

/// Path to the projects registry file
fn projects_file_path() -> PathBuf {
    config_dir().join("projects.txt")
}

/// Expand ~ to home dir and canonicalize if the path exists on disk
fn resolve_path(raw: &str) -> PathBuf {
    let expanded = if let Some(rest) = raw.strip_prefix("~/") {
        dirs::home_dir().unwrap_or_default().join(rest)
    } else if raw == "~" {
        dirs::home_dir().unwrap_or_default()
    } else {
        PathBuf::from(raw)
    };
    std::fs::canonicalize(&expanded).unwrap_or(expanded)
}

/// Shorten a path by replacing home dir with ~
pub fn display_path(path: &std::path::Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rel) = path.strip_prefix(&home) {
            return format!("~/{}", rel.display());
        }
    }
    path.display().to_string()
}

/// Load all registered projects from ~/.azureal/projects.txt
/// Format per line: `/path/to/repo` or `/path/to/repo|Display Name`
/// Lines starting with # are comments. Empty lines skipped.
pub fn load_projects() -> Vec<ProjectEntry> {
    let path = projects_file_path();
    let Ok(content) = std::fs::read_to_string(&path) else { return Vec::new() };
    content.lines().filter_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { return None; }
        let (raw_path, name) = if let Some((p, n)) = line.split_once('|') {
            (p.trim(), Some(n.trim().to_string()))
        } else {
            (line, None)
        };
        let resolved = resolve_path(raw_path);
        // Derive display name from last path component if not specified
        let display_name = name.unwrap_or_else(|| {
            resolved.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| raw_path.to_string())
        });
        Some(ProjectEntry { path: resolved, display_name })
    }).collect()
}

/// Save the project list back to ~/.azureal/projects.txt
pub fn save_projects(entries: &[ProjectEntry]) {
    let path = projects_file_path();
    let content: String = entries.iter().map(|e| {
        let short = display_path(&e.path);
        let derived = e.path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        // Only write |name if it differs from the auto-derived name
        if e.display_name != derived {
            format!("{}|{}", short, e.display_name)
        } else {
            short
        }
    }).collect::<Vec<_>>().join("\n");
    let _ = std::fs::write(&path, if content.is_empty() { content } else { content + "\n" });
}

/// Auto-register a project path if it isn't already in projects.txt.
/// Called on startup when azureal detects a git repo.
pub fn register_project(repo_path: &std::path::Path) {
    let canonical = std::fs::canonicalize(repo_path).unwrap_or_else(|_| repo_path.to_path_buf());
    let mut entries = load_projects();
    // Check if already registered (compare canonical paths)
    if entries.iter().any(|e| e.path == canonical) { return; }
    let display_name = canonical.file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| canonical.display().to_string());
    entries.push(ProjectEntry { path: canonical, display_name });
    save_projects(&entries);
}
