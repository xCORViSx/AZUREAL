//! Unified configuration file for Azureal (`azufig.toml`).
//!
//! Two TOML files consolidate all persistent state:
//! - **Global**: `~/.azureal/azufig.toml` — app config, projects, global runcmds/presets
//! - **Project-local**: `.azureal/azufig.toml` — filetree options, sessions, healthscope, local runcmds/presets
//!
//! All sections use `#[serde(default)]` so missing sections produce defaults (forward-compat).
//! Save pattern: load-modify-save (read current, update one section, write back) to avoid clobbering.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The filename used for both global and project-local config files.
const AZUFIG_FILENAME: &str = "azufig.toml";

// ── Global azufig (~/.azureal/azufig.toml) ──

/// Top-level structure for the global azufig file.
/// Stores user-wide settings shared across all projects.
/// Each section uses single-bracket `[section]` with flat `key = "value"` pairs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalAzufig {
    /// App config (API key, claude path, permission mode, verbose)
    #[serde(default)]
    pub config: AzufigConfig,
    /// Registered projects: display_name = "~/path/to/project"
    #[serde(default)]
    pub projects: HashMap<String, String>,
    /// Global run commands: name = "shell command"
    #[serde(default)]
    pub runcmds: HashMap<String, String>,
    /// Global preset prompts: name = "prompt text"
    #[serde(default)]
    pub presetprompts: HashMap<String, String>,
}

/// App configuration section — mirrors the old `~/.azureal/config` TOML.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AzufigConfig {
    /// Anthropic API key (optional — Claude Code may have its own)
    pub anthropic_api_key: Option<String>,
    /// Custom path to the Claude Code executable
    pub claude_executable: Option<String>,
    /// Default permission mode for new sessions
    #[serde(default)]
    pub default_permission_mode: String,
    /// Enable verbose logging
    #[serde(default)]
    pub verbose: bool,
}

// ── Project-local azufig (.azureal/azufig.toml) ──

/// Top-level structure for the project-local azufig file.
/// Stores per-project settings like filetree filters, session names, etc.
/// Each section uses single-bracket `[section]` with flat `key = "value"` pairs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectAzufig {
    /// FileTree display options (hidden entry names)
    #[serde(default)]
    pub filetree: AzufigFiletree,
    /// Health scope — directories included in all health scanners (god files, docs, etc.)
    #[serde(default, alias = "godfilescope")]
    pub healthscope: AzufigHealthScope,
    /// Custom session name mappings: session_uuid = "display_name"
    #[serde(default)]
    pub sessions: HashMap<String, String>,
    /// Project-local run commands: name = "shell command"
    #[serde(default)]
    pub runcmds: HashMap<String, String>,
    /// Project-local preset prompts: name = "prompt text"
    #[serde(default)]
    pub presetprompts: HashMap<String, String>,
    /// Git settings: auto-rebase = "yes"/"no", future git-related toggles.
    /// Each worktree can have its own [git] section in its own `.azureal/azufig.toml`.
    #[serde(default)]
    pub git: HashMap<String, String>,
}

/// FileTree display settings — which entries to hide by default.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzufigFiletree {
    /// Entry names (files or directories) hidden in the file tree overlay
    #[serde(default = "default_hidden")]
    pub hidden: Vec<String>,
}

/// Produces the default set of hidden entries for a fresh install.
fn default_hidden() -> Vec<String> {
    vec![
        "worktrees".into(),
        ".git".into(),
        ".claude".into(),
        ".azureal".into(),
        ".DS_Store".into(),
    ]
}

impl Default for AzufigFiletree {
    fn default() -> Self {
        Self { hidden: default_hidden() }
    }
}

/// Directories included in all health scanners (god files, documentation, etc.).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AzufigHealthScope {
    /// Absolute paths to scan directories
    #[serde(default)]
    pub dirs: Vec<String>,
}

// ── Load / Save ──

/// Load the global azufig from `~/.azureal/azufig.toml`.
/// Returns default if the file doesn't exist or can't be parsed.
/// Auto-migrates from old extensionless `azufig` on first load.
pub fn load_global_azufig() -> GlobalAzufig {
    let dir = crate::config::config_dir();
    migrate_old_azufig(&dir);
    load_toml(&dir.join(AZUFIG_FILENAME)).unwrap_or_default()
}

/// Save the global azufig to `~/.azureal/azufig.toml`.
pub fn save_global_azufig(azufig: &GlobalAzufig) {
    let _ = crate::config::ensure_config_dir();
    let path = crate::config::config_dir().join(AZUFIG_FILENAME);
    save_toml(&path, azufig);
}

/// Load the project-local azufig from `.azureal/azufig.toml` within the given project root.
/// Returns default if the file doesn't exist or can't be parsed.
/// Auto-migrates from old extensionless `azufig` on first load.
pub fn load_project_azufig(project_root: &Path) -> ProjectAzufig {
    let dir = project_root.join(".azureal");
    migrate_old_azufig(&dir);
    load_toml(&dir.join(AZUFIG_FILENAME)).unwrap_or_default()
}

/// Save the project-local azufig to `.azureal/azufig.toml` within the given project root.
/// Creates `.azureal/` directory if it doesn't exist.
pub fn save_project_azufig(project_root: &Path, azufig: &ProjectAzufig) {
    let dir = project_root.join(".azureal");
    let _ = std::fs::create_dir_all(&dir);
    save_toml(&dir.join(AZUFIG_FILENAME), azufig);
}

// ── Helpers: load-modify-save pattern ──

/// Read + update + write the global azufig. The closure receives a mutable
/// reference to the current state; after it returns, the file is overwritten.
pub fn update_global_azufig(f: impl FnOnce(&mut GlobalAzufig)) {
    let mut azufig = load_global_azufig();
    f(&mut azufig);
    save_global_azufig(&azufig);
}

/// Read + update + write the project-local azufig. Same load-modify-save pattern.
pub fn update_project_azufig(project_root: &Path, f: impl FnOnce(&mut ProjectAzufig)) {
    let mut azufig = load_project_azufig(project_root);
    f(&mut azufig);
    save_project_azufig(project_root, &azufig);
}

// ── Mock test helper (delete after testing) ──

/// Second mock conflict — tests that the auto-rebase dialog is visible before RCR.
/// Deliberately conflicts with the auto-rebase helpers on the health branch.
pub fn mock_conflict_test_v2() -> &'static str {
    "This is a second mock conflict to test the auto-rebase dialog"
}

// ── Internal TOML I/O ──

/// Parse a TOML file into the given type. Returns None on any failure.
fn load_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}

/// Serialize a value to pretty TOML and write it to disk.
/// Post-processes to strip unnecessary quotes from keys — TOML bare keys
/// allow `[A-Za-z0-9_-]`, so keys matching that pattern are unquoted for
/// a cleaner, more uniform look (e.g., `AZUREAL = "~/path"` not `"AZUREAL" = "~/path"`).
fn save_toml<T: Serialize>(path: &Path, value: &T) {
    if let Ok(content) = toml::to_string_pretty(value) {
        let cleaned = strip_unnecessary_key_quotes(&content);
        let _ = std::fs::write(path, cleaned);
    }
}

/// Remove quotes from TOML keys that only contain bare-key-safe chars.
/// Bare keys in TOML may contain: A-Z a-z 0-9 _ -
/// Transforms `"SomeKey" = "value"` → `SomeKey = "value"` but leaves
/// `"Key With Spaces" = "value"` quoted.
fn strip_unnecessary_key_quotes(toml_str: &str) -> String {
    let mut result = String::with_capacity(toml_str.len());
    for line in toml_str.lines() {
        let trimmed = line.trim_start();
        // Only process lines that start with a quoted key followed by `=` or `.`
        if trimmed.starts_with('"') {
            if let Some(end_quote) = trimmed[1..].find('"') {
                let key = &trimmed[1..1 + end_quote];
                let after = &trimmed[1 + end_quote + 1..];
                // Check if what follows the closing quote is ` = ` (key-value pair)
                // and the key only has bare-key-safe characters
                if after.starts_with(" = ") && is_bare_key(key) {
                    let indent = &line[..line.len() - trimmed.len()];
                    result.push_str(indent);
                    result.push_str(key);
                    result.push_str(after);
                    result.push('\n');
                    continue;
                }
            }
        }
        result.push_str(line);
        result.push('\n');
    }
    result
}

/// Check if a string qualifies as a TOML bare key (only A-Z, a-z, 0-9, _, -).
fn is_bare_key(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

// ── Migration ──

/// Rename old extensionless `azufig` → `azufig.toml` if the new file doesn't
/// exist yet. This is a one-time migration — once `azufig.toml` exists, the
/// old file is ignored (user can clean it up manually).
fn migrate_old_azufig(dir: &Path) {
    let new_path = dir.join(AZUFIG_FILENAME);
    if new_path.exists() { return; }
    let old_path = dir.join("azufig");
    if old_path.is_file() {
        let _ = std::fs::rename(&old_path, &new_path);
    }
}
