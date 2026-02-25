use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for Config {
    fn default() -> Self {
        Self {
            anthropic_api_key: None,
            claude_executable: None,
            default_permission_mode: PermissionMode::default(),
            verbose: false,
        }
    }
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
    /// Load config from the `[config]` section of global azufig.
    pub fn load() -> Result<Self> {
        let az = crate::azufig::load_global_azufig();
        Ok(Self {
            anthropic_api_key: az.config.anthropic_api_key,
            claude_executable: az.config.claude_executable,
            default_permission_mode: match az.config.default_permission_mode.as_str() {
                "approve" => PermissionMode::Approve,
                "ask" => PermissionMode::Ask,
                _ => PermissionMode::Ignore,
            },
            verbose: az.config.verbose,
        })
    }

    pub fn claude_executable(&self) -> &str {
        self.claude_executable.as_deref().unwrap_or("claude")
    }
}

/// Get the global Azureal config directory (~/.azureal/)
/// Used for global config like config
pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".azureal")
}

/// Get project-specific Azureal data directory (.azureal/ in git root)
/// Used for debug-output, etc.
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

/// Encode a path the same way Claude CLI does: replace all non-alphanumeric
/// chars with `-`. If the result exceeds 200 chars, truncate and append a hash.
/// Matches Claude CLI v2.1+ `OP()` function exactly.
fn encode_project_path(path: &std::path::Path) -> String {
    let raw = path.to_string_lossy();
    let encoded: String = raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    if encoded.len() <= 200 {
        encoded
    } else {
        // Same truncation + hash scheme as Claude CLI (djb2-style hash, base-36)
        let hash = raw.bytes().fold(0u64, |h, b| {
            h.wrapping_mul(31).wrapping_add(b as u64)
        });
        format!("{}-{}", &encoded[..200], radix_36(hash))
    }
}

fn radix_36(mut n: u64) -> String {
    if n == 0 { return "0".to_string(); }
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut buf = Vec::new();
    while n > 0 {
        buf.push(digits[(n % 36) as usize]);
        n /= 36;
    }
    buf.reverse();
    String::from_utf8(buf).unwrap()
}

/// Get Claude's session file path for a given project path and session ID
/// Claude stores sessions at: ~/.claude/projects/<encoded-path>/<session-id>.jsonl
pub fn claude_session_file(project_path: &std::path::Path, session_id: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let encoded_path = encode_project_path(project_path);
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
    let encoded_path = encode_project_path(worktree_path);
    let dir = home.join(".claude").join("projects").join(&encoded_path);
    if dir.exists() { Some(dir) } else { None }
}

/// Migrate old-encoding project directories to the new encoding.
/// Old Claude CLI versions used `replace('/', '-')` which preserved chars like `+`.
/// Current Claude CLI uses `replace(/[^a-zA-Z0-9]/g, '-')`.
/// Call once at startup to rename any stale directories.
pub fn migrate_project_dirs(worktree_paths: &[std::path::PathBuf]) {
    let Some(home) = dirs::home_dir() else { return };
    let projects_dir = home.join(".claude").join("projects");
    if !projects_dir.exists() { return; }

    for wt_path in worktree_paths {
        let new_name = encode_project_path(wt_path);
        let old_name = wt_path.to_string_lossy().replace('/', "-");
        if new_name == old_name { continue; } // no change needed

        let old_dir = projects_dir.join(&old_name);
        let new_dir = projects_dir.join(&new_name);

        // Old dir exists but new dir doesn't → rename
        if old_dir.exists() && !new_dir.exists() {
            let _ = std::fs::rename(&old_dir, &new_dir);
        }
    }
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

// ── Projects persistence (~/.azureal/projects) ──

/// A registered project entry: absolute path + optional display name
#[derive(Debug, Clone)]
pub struct ProjectEntry {
    pub path: PathBuf,
    pub display_name: String,
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

/// Load all registered projects from `[projects]` in global azufig.
/// Validates each entry: directories that don't exist or aren't git repos
/// are pruned automatically. Format: display_name = "~/path"
pub fn load_projects() -> Vec<ProjectEntry> {
    let az = crate::azufig::load_global_azufig();
    let mut entries = Vec::new();
    let mut pruned = false;
    for (name, raw_path) in &az.projects {
        let resolved = resolve_path(raw_path);
        if !resolved.exists() || !crate::git::Git::is_git_repo(&resolved) {
            pruned = true;
            continue;
        }
        let display_name = if name.is_empty() {
            resolved.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| raw_path.clone())
        } else {
            name.clone()
        };
        entries.push(ProjectEntry { path: resolved, display_name });
    }
    if pruned { save_projects(&entries); }
    entries
}

/// Look up the display name for a repo path from projects.
pub fn project_display_name(repo_path: &std::path::Path) -> Option<String> {
    let canonical = std::fs::canonicalize(repo_path).unwrap_or_else(|_| repo_path.to_path_buf());
    load_projects().into_iter()
        .find(|e| e.path == canonical)
        .map(|e| e.display_name)
}

/// Save the project list to `[projects]` in global azufig (load-modify-save).
/// Format: display_name = "~/path"
pub fn save_projects(entries: &[ProjectEntry]) {
    crate::azufig::update_global_azufig(|az| {
        az.projects = entries.iter()
            .map(|e| (e.display_name.clone(), display_path(&e.path)))
            .collect();
    });
}

/// Auto-register a project path if it isn't already in projects.
/// Called on startup when azureal detects a git repo.
/// Display name: git remote repo name (from origin URL) → folder name fallback.
pub fn register_project(repo_path: &std::path::Path) {
    let canonical = std::fs::canonicalize(repo_path).unwrap_or_else(|_| repo_path.to_path_buf());
    let mut entries = load_projects();
    if entries.iter().any(|e| e.path == canonical) { return; }
    let display_name = repo_name_from_origin(&canonical)
        .unwrap_or_else(|| canonical.file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| canonical.display().to_string()));
    entries.push(ProjectEntry { path: canonical, display_name });
    save_projects(&entries);
}

/// Extract repository name from `git remote get-url origin`.
/// Handles SSH (`git@github.com:user/repo.git`) and HTTPS (`https://github.com/user/repo.git`).
/// Returns just the repo name portion (e.g., "repo"), stripping `.git` suffix.
fn repo_name_from_origin(repo_path: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_path)
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // Take everything after the last '/' or ':', strip .git suffix
    let name = url.rsplit_once('/').map(|(_, n)| n)
        .or_else(|| url.rsplit_once(':').map(|(_, n)| n))?;
    let name = name.strip_suffix(".git").unwrap_or(name);
    if name.is_empty() { None } else { Some(name.to_string()) }
}
