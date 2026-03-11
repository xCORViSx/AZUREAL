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
    dunce::canonicalize(&expanded).unwrap_or(expanded)
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
    let canonical = dunce::canonicalize(repo_path).unwrap_or_else(|_| repo_path.to_path_buf());
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
    let canonical = dunce::canonicalize(repo_path).unwrap_or_else(|_| repo_path.to_path_buf());
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── encode_project_path ──

    #[test]
    fn test_encode_simple_ascii_path() {
        let path = Path::new("/Users/dev/project");
        let encoded = encode_project_path(path);
        assert_eq!(encoded, "-Users-dev-project");
    }

    #[test]
    fn test_encode_replaces_non_alphanumeric() {
        let path = Path::new("/home/user/my project (v2)");
        let encoded = encode_project_path(path);
        assert_eq!(encoded, "-home-user-my-project--v2-");
    }

    #[test]
    fn test_encode_preserves_alphanumeric() {
        let path = Path::new("/abc123/XYZ");
        let encoded = encode_project_path(path);
        assert_eq!(encoded, "-abc123-XYZ");
    }

    #[test]
    fn test_encode_long_path_truncates_with_hash() {
        let long_segment = "a".repeat(250);
        let path = PathBuf::from(format!("/{}", long_segment));
        let encoded = encode_project_path(&path);
        assert!(encoded.len() < 250, "encoded length {} should be < 250", encoded.len());
        assert!(encoded.starts_with("-aaa"));
        assert!(encoded.contains('-'), "should have hash separator");
    }

    #[test]
    fn test_encode_short_path_no_truncation() {
        let path = Path::new("/short");
        let encoded = encode_project_path(path);
        assert_eq!(encoded, "-short");
        assert!(encoded.len() <= 200);
    }

    #[test]
    fn test_encode_dots_and_special_chars() {
        let path = Path::new("/home/user/.config/my-app");
        let encoded = encode_project_path(path);
        assert_eq!(encoded, "-home-user--config-my-app");
    }

    // ── radix_36 ──

    #[test]
    fn test_radix_36_zero() {
        assert_eq!(radix_36(0), "0");
    }

    #[test]
    fn test_radix_36_single_digit() {
        assert_eq!(radix_36(1), "1");
        assert_eq!(radix_36(9), "9");
        assert_eq!(radix_36(10), "a");
        assert_eq!(radix_36(35), "z");
    }

    #[test]
    fn test_radix_36_multi_digit() {
        assert_eq!(radix_36(36), "10");
        assert_eq!(radix_36(37), "11");
        assert_eq!(radix_36(1296), "100"); // 36^2
    }

    #[test]
    fn test_radix_36_large_number() {
        let result = radix_36(u64::MAX);
        assert!(!result.is_empty());
        assert!(result.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    // ── display_path ──

    #[test]
    fn test_display_path_under_home() {
        if let Some(home) = dirs::home_dir() {
            let path = home.join("projects/myapp");
            let display = display_path(&path);
            assert_eq!(display, "~/projects/myapp");
        }
    }

    #[test]
    fn test_display_path_outside_home() {
        let path = Path::new("/tmp/something");
        let display = display_path(path);
        assert_eq!(display, "/tmp/something");
    }

    #[test]
    fn test_display_path_root() {
        let path = Path::new("/");
        let display = display_path(path);
        assert_eq!(display, "/");
    }

    // ── PermissionMode defaults ──

    #[test]
    fn test_permission_mode_default_is_ignore() {
        let mode = PermissionMode::default();
        assert!(matches!(mode, PermissionMode::Ignore));
    }

    // ── Config defaults ──

    #[test]
    fn test_config_default() {
        let cfg = Config::default();
        assert!(cfg.anthropic_api_key.is_none());
        assert!(cfg.claude_executable.is_none());
        assert!(matches!(cfg.default_permission_mode, PermissionMode::Ignore));
        assert!(!cfg.verbose);
    }

    #[test]
    fn test_config_claude_executable_default() {
        let cfg = Config::default();
        assert_eq!(cfg.claude_executable(), "claude");
    }

    #[test]
    fn test_config_claude_executable_custom() {
        let cfg = Config {
            claude_executable: Some("/usr/local/bin/claude-code".to_string()),
            ..Config::default()
        };
        assert_eq!(cfg.claude_executable(), "/usr/local/bin/claude-code");
    }

    // ── encode_project_path: unicode ──

    #[test]
    fn test_encode_unicode_path() {
        let path = Path::new("/home/用户/项目");
        let encoded = encode_project_path(path);
        // All non-ASCII chars become '-'
        assert!(encoded.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    #[test]
    fn test_encode_emoji_path() {
        let path = Path::new("/home/user/🚀project");
        let encoded = encode_project_path(path);
        assert!(encoded.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        assert!(encoded.contains("project"));
    }

    #[test]
    fn test_encode_japanese_path() {
        let path = Path::new("/home/ユーザー/テスト");
        let encoded = encode_project_path(path);
        assert!(encoded.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    // ── encode_project_path: special chars ──

    #[test]
    fn test_encode_path_with_plus() {
        let path = Path::new("/home/user/c++/project");
        let encoded = encode_project_path(path);
        assert_eq!(encoded, "-home-user-c---project");
    }

    #[test]
    fn test_encode_path_with_at() {
        let path = Path::new("/home/@user/project");
        let encoded = encode_project_path(path);
        assert_eq!(encoded, "-home--user-project");
    }

    #[test]
    fn test_encode_path_with_spaces() {
        let path = Path::new("/home/my user/my project");
        let encoded = encode_project_path(path);
        assert_eq!(encoded, "-home-my-user-my-project");
    }

    #[test]
    fn test_encode_path_consecutive_special_chars() {
        let path = Path::new("/home/user/...///project");
        let encoded = encode_project_path(path);
        assert_eq!(encoded, "-home-user-------project");
    }

    #[test]
    fn test_encode_path_tilde_and_dollar() {
        let path = Path::new("/home/$USER/~/project");
        let encoded = encode_project_path(path);
        assert_eq!(encoded, "-home--USER---project");
    }

    #[test]
    fn test_encode_path_equals_and_ampersand() {
        let path = Path::new("/a=b&c");
        let encoded = encode_project_path(path);
        assert_eq!(encoded, "-a-b-c");
    }

    #[test]
    fn test_encode_path_brackets_and_braces() {
        let path = Path::new("/test[0]{1}(2)");
        let encoded = encode_project_path(path);
        assert_eq!(encoded, "-test-0--1--2-");
    }

    #[test]
    fn test_encode_path_hash_and_percent() {
        let path = Path::new("/100%/item#1");
        let encoded = encode_project_path(path);
        assert_eq!(encoded, "-100--item-1");
    }

    // ── encode_project_path: boundary lengths ──

    #[test]
    fn test_encode_exact_200_chars() {
        // Build a path that encodes to exactly 200 chars
        // "/" encodes to "-", then 199 alphanumeric chars
        let segment = "a".repeat(199);
        let path = PathBuf::from(format!("/{}", segment));
        let encoded = encode_project_path(&path);
        assert_eq!(encoded.len(), 200);
        assert!(!encoded.contains("--"), "should not have hash suffix at exactly 200");
        // Actually it's "-" + "aaa..." which is 200 chars
        assert_eq!(encoded, format!("-{}", segment));
    }

    #[test]
    fn test_encode_201_chars_triggers_truncation() {
        let segment = "a".repeat(200);
        let path = PathBuf::from(format!("/{}", segment));
        let encoded = encode_project_path(&path);
        // Should be truncated: 200 chars + "-" + hash
        assert!(encoded.len() > 200);
        assert!(encoded.starts_with("-aaa"));
        // The 201st char position starts the hash part
        assert_eq!(&encoded[200..201], "-");
    }

    #[test]
    fn test_encode_199_chars_no_truncation() {
        let segment = "a".repeat(198);
        let path = PathBuf::from(format!("/{}", segment));
        let encoded = encode_project_path(&path);
        assert_eq!(encoded.len(), 199);
    }

    #[test]
    fn test_encode_very_long_path_still_deterministic() {
        let segment = "x".repeat(500);
        let path = PathBuf::from(format!("/{}", segment));
        let encoded1 = encode_project_path(&path);
        let encoded2 = encode_project_path(&path);
        assert_eq!(encoded1, encoded2);
    }

    #[test]
    fn test_encode_empty_path() {
        let path = Path::new("");
        let encoded = encode_project_path(path);
        assert_eq!(encoded, "");
    }

    #[test]
    fn test_encode_root_path() {
        let path = Path::new("/");
        let encoded = encode_project_path(path);
        assert_eq!(encoded, "-");
    }

    // ── radix_36: boundary values ──

    #[test]
    fn test_radix_36_value_35() {
        assert_eq!(radix_36(35), "z");
    }

    #[test]
    fn test_radix_36_value_36() {
        assert_eq!(radix_36(36), "10");
    }

    #[test]
    fn test_radix_36_value_71() {
        // 71 = 1*36 + 35 = "1z"
        assert_eq!(radix_36(71), "1z");
    }

    #[test]
    fn test_radix_36_value_1295() {
        // 1295 = 35*36 + 35 = "zz"
        assert_eq!(radix_36(1295), "zz");
    }

    #[test]
    fn test_radix_36_value_1296() {
        // 36^2 = 1296 = "100"
        assert_eq!(radix_36(1296), "100");
    }

    #[test]
    fn test_radix_36_value_46656() {
        // 36^3 = 46656 = "1000"
        assert_eq!(radix_36(46656), "1000");
    }

    #[test]
    fn test_radix_36_value_10() {
        assert_eq!(radix_36(10), "a");
    }

    #[test]
    fn test_radix_36_value_2() {
        assert_eq!(radix_36(2), "2");
    }

    #[test]
    fn test_radix_36_powers_of_36() {
        assert_eq!(radix_36(1), "1");
        assert_eq!(radix_36(36), "10");
        assert_eq!(radix_36(1296), "100");
        assert_eq!(radix_36(46656), "1000");
        assert_eq!(radix_36(1679616), "10000");
    }

    #[test]
    fn test_radix_36_all_digits_only_valid_chars() {
        for n in [0, 1, 9, 10, 35, 36, 100, 1000, 99999, u64::MAX] {
            let result = radix_36(n);
            assert!(result.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "radix_36({}) = '{}' contains invalid chars", n, result);
        }
    }

    // ── display_path ──

    #[test]
    fn test_display_path_home_itself() {
        if let Some(home) = dirs::home_dir() {
            let display = display_path(&home);
            // home stripped of itself is empty, so we get "~/"
            assert_eq!(display, "~/");
        }
    }

    #[test]
    fn test_display_path_deeply_nested() {
        if let Some(home) = dirs::home_dir() {
            let path = home.join("a/b/c/d/e/f");
            let display = display_path(&path);
            assert_eq!(display, "~/a/b/c/d/e/f");
        }
    }

    #[test]
    fn test_display_path_absolute_not_under_home() {
        let path = Path::new("/var/log/syslog");
        let display = display_path(path);
        assert_eq!(display, "/var/log/syslog");
    }

    #[test]
    fn test_display_path_etc() {
        let path = Path::new("/etc/hosts");
        let display = display_path(path);
        assert_eq!(display, "/etc/hosts");
    }

    #[test]
    fn test_display_path_with_spaces() {
        if let Some(home) = dirs::home_dir() {
            let path = home.join("my projects/cool app");
            let display = display_path(&path);
            assert_eq!(display, "~/my projects/cool app");
        }
    }

    // ── Config serialization ──

    #[test]
    fn test_config_serialize_roundtrip() {
        let cfg = Config {
            anthropic_api_key: Some("sk-test-123".to_string()),
            claude_executable: Some("/usr/bin/claude".to_string()),
            default_permission_mode: PermissionMode::Approve,
            verbose: true,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.anthropic_api_key.as_deref(), Some("sk-test-123"));
        assert_eq!(parsed.claude_executable.as_deref(), Some("/usr/bin/claude"));
        assert!(parsed.verbose);
    }

    #[test]
    fn test_config_deserialize_defaults() {
        let json = "{}";
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert!(cfg.anthropic_api_key.is_none());
        assert!(cfg.claude_executable.is_none());
        assert!(matches!(cfg.default_permission_mode, PermissionMode::Ignore));
        assert!(!cfg.verbose);
    }

    #[test]
    fn test_config_deserialize_partial() {
        let json = r#"{"verbose": true}"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert!(cfg.verbose);
        assert!(cfg.anthropic_api_key.is_none());
    }

    // ── PermissionMode serde ──

    #[test]
    fn test_permission_mode_serialize_approve() {
        let json = serde_json::to_string(&PermissionMode::Approve).unwrap();
        assert_eq!(json, "\"approve\"");
    }

    #[test]
    fn test_permission_mode_serialize_ignore() {
        let json = serde_json::to_string(&PermissionMode::Ignore).unwrap();
        assert_eq!(json, "\"ignore\"");
    }

    #[test]
    fn test_permission_mode_serialize_ask() {
        let json = serde_json::to_string(&PermissionMode::Ask).unwrap();
        assert_eq!(json, "\"ask\"");
    }

    #[test]
    fn test_permission_mode_deserialize_approve() {
        let mode: PermissionMode = serde_json::from_str("\"approve\"").unwrap();
        assert!(matches!(mode, PermissionMode::Approve));
    }

    #[test]
    fn test_permission_mode_deserialize_ignore() {
        let mode: PermissionMode = serde_json::from_str("\"ignore\"").unwrap();
        assert!(matches!(mode, PermissionMode::Ignore));
    }

    #[test]
    fn test_permission_mode_deserialize_ask() {
        let mode: PermissionMode = serde_json::from_str("\"ask\"").unwrap();
        assert!(matches!(mode, PermissionMode::Ask));
    }

    #[test]
    fn test_permission_mode_deserialize_unknown_fails() {
        let result = serde_json::from_str::<PermissionMode>("\"unknown\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_permission_mode_deserialize_empty_fails() {
        let result = serde_json::from_str::<PermissionMode>("\"\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_permission_mode_roundtrip_all_variants() {
        for mode in [PermissionMode::Approve, PermissionMode::Ignore, PermissionMode::Ask] {
            let json = serde_json::to_string(&mode).unwrap();
            let parsed: PermissionMode = serde_json::from_str(&json).unwrap();
            assert_eq!(std::mem::discriminant(&mode), std::mem::discriminant(&parsed));
        }
    }

    // ── PermissionMode traits ──

    #[test]
    fn test_permission_mode_is_copy() {
        let mode = PermissionMode::Approve;
        let mode2 = mode; // Copy
        assert!(matches!(mode, PermissionMode::Approve));
        assert!(matches!(mode2, PermissionMode::Approve));
    }

    #[test]
    fn test_permission_mode_clone() {
        let mode = PermissionMode::Ask;
        let cloned = mode.clone();
        assert!(matches!(cloned, PermissionMode::Ask));
    }

    #[test]
    fn test_permission_mode_debug() {
        let debug = format!("{:?}", PermissionMode::Approve);
        assert_eq!(debug, "Approve");
    }

    // ── ProjectEntry ──

    #[test]
    fn test_project_entry_construction() {
        let entry = ProjectEntry {
            path: PathBuf::from("/home/user/project"),
            display_name: "My Project".to_string(),
        };
        assert_eq!(entry.path, PathBuf::from("/home/user/project"));
        assert_eq!(entry.display_name, "My Project");
    }

    #[test]
    fn test_project_entry_clone() {
        let entry = ProjectEntry {
            path: PathBuf::from("/test"),
            display_name: "test".to_string(),
        };
        let cloned = entry.clone();
        assert_eq!(cloned.path, entry.path);
        assert_eq!(cloned.display_name, entry.display_name);
    }

    #[test]
    fn test_project_entry_debug() {
        let entry = ProjectEntry {
            path: PathBuf::from("/test"),
            display_name: "test".to_string(),
        };
        let debug = format!("{:?}", entry);
        assert!(debug.contains("ProjectEntry"));
        assert!(debug.contains("test"));
    }

    // ── Config with custom executable ──

    #[test]
    fn test_config_claude_executable_empty_string() {
        let cfg = Config {
            claude_executable: Some("".to_string()),
            ..Config::default()
        };
        assert_eq!(cfg.claude_executable(), "");
    }

    #[test]
    fn test_config_verbose_true() {
        let cfg = Config {
            verbose: true,
            ..Config::default()
        };
        assert!(cfg.verbose);
    }

    #[test]
    fn test_config_with_api_key() {
        let cfg = Config {
            anthropic_api_key: Some("sk-ant-api03-test".to_string()),
            ..Config::default()
        };
        assert_eq!(cfg.anthropic_api_key.as_deref(), Some("sk-ant-api03-test"));
    }
}
