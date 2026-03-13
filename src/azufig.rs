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
    /// Custom path to the Codex CLI executable
    pub codex_executable: Option<String>,
    /// Default permission mode for new sessions
    #[serde(default)]
    pub default_permission_mode: String,
    /// Enable verbose logging
    #[serde(default)]
    pub verbose: bool,
    /// Backend to use: "claude" (default) or "codex"
    pub backend: Option<String>,
}

// ── Project-local azufig (.azureal/azufig.toml) ──

/// Top-level structure for the project-local azufig file.
/// Stores per-project settings like filetree filters, run commands, etc.
/// Each section uses single-bracket `[section]` with flat `key = "value"` pairs.
/// Session names are stored in `.azureal/sessions/index.json`, not here.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectAzufig {
    /// FileTree display options (hidden entry names)
    #[serde(default)]
    pub filetree: AzufigFiletree,
    /// Health scope — directories included in all health scanners (god files, docs, etc.)
    #[serde(default, alias = "godfilescope")]
    pub healthscope: AzufigHealthScope,
    /// Project-local run commands: name = "shell command"
    #[serde(default)]
    pub runcmds: HashMap<String, String>,
    /// Project-local preset prompts: name = "prompt text"
    #[serde(default)]
    pub presetprompts: HashMap<String, String>,
    /// Git settings: auto-rebase per branch, auto-resolve file list, future toggles.
    /// Project-scoped — always at main worktree root, shared by all worktrees.
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

// ── Auto-rebase helpers ──

/// Enable or disable auto-rebase for a branch in the project-local azufig.
pub fn set_auto_rebase(project_root: &Path, branch: &str, enabled: bool) {
    let key = format!("auto-rebase/{}", branch);
    update_project_azufig(project_root, |azufig| {
        if enabled {
            azufig.git.insert(key, "true".to_string());
        } else {
            azufig.git.remove(&key);
        }
    });
}

/// Load all branches with auto-rebase enabled from the project-local azufig.
pub fn load_auto_rebase_branches(project_root: &Path) -> std::collections::HashSet<String> {
    let azufig = load_project_azufig(project_root);
    azufig.git.iter()
        .filter(|(k, v)| k.starts_with("auto-rebase/") && v == &"true")
        .map(|(k, _)| k.strip_prefix("auto-rebase/").unwrap().to_string())
        .collect()
}

// ── Auto-resolve file helpers ──

/// Default files that are auto-resolved during rebase via union merge.
const DEFAULT_AUTO_RESOLVE: &[&str] = &["AGENTS.md", "CHANGELOG.md", "README.md", "CLAUDE.md"];

/// Load the auto-resolve file list from the project-local azufig.
/// Returns the default list when no config exists yet.
pub fn load_auto_resolve_files(project_root: &Path) -> Vec<String> {
    let azufig = load_project_azufig(project_root);
    let files: Vec<String> = azufig.git.iter()
        .filter(|(k, v)| k.starts_with("auto-resolve/") && *v == "true")
        .map(|(k, _)| k.strip_prefix("auto-resolve/").unwrap().to_string())
        .collect();
    if files.is_empty() {
        DEFAULT_AUTO_RESOLVE.iter().map(|s| s.to_string()).collect()
    } else {
        files
    }
}

/// Save the auto-resolve file list to the project-local azufig.
pub fn save_auto_resolve_files(project_root: &Path, files: &[String]) {
    update_project_azufig(project_root, |azufig| {
        azufig.git.retain(|k, _| !k.starts_with("auto-resolve/"));
        for file in files {
            azufig.git.insert(format!("auto-resolve/{}", file), "true".into());
        }
    });
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_bare_key ──

    #[test]
    fn test_bare_key_simple() {
        assert!(is_bare_key("hello"));
        assert!(is_bare_key("HELLO"));
        assert!(is_bare_key("hello123"));
    }

    #[test]
    fn test_bare_key_with_underscores_dashes() {
        assert!(is_bare_key("my_key"));
        assert!(is_bare_key("my-key"));
        assert!(is_bare_key("my_key-123"));
    }

    #[test]
    fn test_bare_key_empty() {
        assert!(!is_bare_key(""));
    }

    #[test]
    fn test_bare_key_with_spaces() {
        assert!(!is_bare_key("hello world"));
        assert!(!is_bare_key(" hello"));
    }

    #[test]
    fn test_bare_key_with_special_chars() {
        assert!(!is_bare_key("key.name"));
        assert!(!is_bare_key("key=value"));
        assert!(!is_bare_key("key/path"));
        assert!(!is_bare_key("key@host"));
        assert!(!is_bare_key("key(1)"));
    }

    #[test]
    fn test_bare_key_single_chars() {
        assert!(is_bare_key("a"));
        assert!(is_bare_key("Z"));
        assert!(is_bare_key("0"));
        assert!(is_bare_key("_"));
        assert!(is_bare_key("-"));
    }

    // ── strip_unnecessary_key_quotes ──

    #[test]
    fn test_strip_quotes_bare_key() {
        let input = "\"SomeKey\" = \"value\"\n";
        let result = strip_unnecessary_key_quotes(input);
        assert_eq!(result, "SomeKey = \"value\"\n");
    }

    #[test]
    fn test_strip_quotes_preserves_needed_quotes() {
        let input = "\"Key With Spaces\" = \"value\"\n";
        let result = strip_unnecessary_key_quotes(input);
        assert_eq!(result, "\"Key With Spaces\" = \"value\"\n");
    }

    #[test]
    fn test_strip_quotes_section_headers() {
        let input = "[config]\n\"api_key\" = \"sk-123\"\n";
        let result = strip_unnecessary_key_quotes(input);
        assert_eq!(result, "[config]\napi_key = \"sk-123\"\n");
    }

    #[test]
    fn test_strip_quotes_mixed() {
        let input = "\"bare_ok\" = \"val1\"\n\"has spaces\" = \"val2\"\n\"also-ok\" = \"val3\"\n";
        let result = strip_unnecessary_key_quotes(input);
        assert_eq!(result, "bare_ok = \"val1\"\n\"has spaces\" = \"val2\"\nalso-ok = \"val3\"\n");
    }

    #[test]
    fn test_strip_quotes_no_quotes() {
        let input = "already_unquoted = \"value\"\n";
        let result = strip_unnecessary_key_quotes(input);
        assert_eq!(result, "already_unquoted = \"value\"\n");
    }

    #[test]
    fn test_strip_quotes_with_indent() {
        let input = "  \"indented\" = \"value\"\n";
        let result = strip_unnecessary_key_quotes(input);
        assert_eq!(result, "  indented = \"value\"\n");
    }

    #[test]
    fn test_strip_quotes_special_key_preserved() {
        let input = "\"key/with/slashes\" = \"value\"\n";
        let result = strip_unnecessary_key_quotes(input);
        assert_eq!(result, "\"key/with/slashes\" = \"value\"\n");
    }

    // ── default_hidden ──

    #[test]
    fn test_default_hidden_contents() {
        let hidden = default_hidden();
        assert!(hidden.contains(&"worktrees".to_string()));
        assert!(hidden.contains(&".git".to_string()));
        assert!(hidden.contains(&".claude".to_string()));
        assert!(hidden.contains(&".azureal".to_string()));
        assert!(hidden.contains(&".DS_Store".to_string()));
        assert_eq!(hidden.len(), 5);
    }

    // ── AzufigFiletree default ──

    #[test]
    fn test_filetree_default() {
        let ft = AzufigFiletree::default();
        assert_eq!(ft.hidden.len(), 5);
    }

    // ── GlobalAzufig default ──

    #[test]
    fn test_global_azufig_default() {
        let az = GlobalAzufig::default();
        assert!(az.projects.is_empty());
        assert!(az.runcmds.is_empty());
        assert!(az.presetprompts.is_empty());
    }

    // ── ProjectAzufig default ──

    #[test]
    fn test_project_azufig_default() {
        let az = ProjectAzufig::default();
        assert!(az.runcmds.is_empty());
        assert!(az.presetprompts.is_empty());
        assert!(az.git.is_empty());
    }

    // ── AzufigConfig default ──

    #[test]
    fn test_azufig_config_default() {
        let cfg = AzufigConfig::default();
        assert!(cfg.anthropic_api_key.is_none());
        assert!(cfg.claude_executable.is_none());
        assert_eq!(cfg.default_permission_mode, "");
        assert!(!cfg.verbose);
    }

    // ── TOML round-trip ──

    #[test]
    fn test_global_azufig_toml_roundtrip() {
        let mut az = GlobalAzufig::default();
        az.projects.insert("MyProject".to_string(), "~/dev/myproject".to_string());
        az.runcmds.insert("1_Build".to_string(), "cargo build".to_string());

        let toml_str = toml::to_string_pretty(&az).unwrap();
        let parsed: GlobalAzufig = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.projects.get("MyProject").unwrap(), "~/dev/myproject");
        assert_eq!(parsed.runcmds.get("1_Build").unwrap(), "cargo build");
    }

    #[test]
    fn test_project_azufig_toml_roundtrip() {
        let mut az = ProjectAzufig::default();
        az.git.insert("auto-rebase/feature".to_string(), "true".to_string());

        let toml_str = toml::to_string_pretty(&az).unwrap();
        let parsed: ProjectAzufig = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.git.get("auto-rebase/feature").unwrap(), "true");
    }

    // ── DEFAULT_AUTO_RESOLVE ──

    #[test]
    fn test_default_auto_resolve_files() {
        assert!(DEFAULT_AUTO_RESOLVE.contains(&"AGENTS.md"));
        assert!(DEFAULT_AUTO_RESOLVE.contains(&"CHANGELOG.md"));
        assert!(DEFAULT_AUTO_RESOLVE.contains(&"README.md"));
        assert!(DEFAULT_AUTO_RESOLVE.contains(&"CLAUDE.md"));
        assert_eq!(DEFAULT_AUTO_RESOLVE.len(), 4);
    }

    // ── AZUFIG_FILENAME ──

    #[test]
    fn test_azufig_filename() {
        assert_eq!(AZUFIG_FILENAME, "azufig.toml");
    }

    // ── is_bare_key: exhaustive ASCII checks ──

    #[test]
    fn test_bare_key_all_lowercase_letters() {
        for c in 'a'..='z' {
            assert!(is_bare_key(&c.to_string()), "'{}' should be a valid bare key char", c);
        }
    }

    #[test]
    fn test_bare_key_all_uppercase_letters() {
        for c in 'A'..='Z' {
            assert!(is_bare_key(&c.to_string()), "'{}' should be a valid bare key char", c);
        }
    }

    #[test]
    fn test_bare_key_all_digits() {
        for c in '0'..='9' {
            assert!(is_bare_key(&c.to_string()), "'{}' should be a valid bare key char", c);
        }
    }

    #[test]
    fn test_bare_key_numbers_only() {
        assert!(is_bare_key("12345"));
        assert!(is_bare_key("007"));
    }

    #[test]
    fn test_bare_key_underscores_only() {
        assert!(is_bare_key("_"));
        assert!(is_bare_key("___"));
    }

    #[test]
    fn test_bare_key_dashes_only() {
        assert!(is_bare_key("-"));
        assert!(is_bare_key("---"));
    }

    #[test]
    fn test_bare_key_invalid_ascii_printable() {
        let invalid = "!\"#$%&'()*+,./:;<>?@[\\]^`{|}~";
        for c in invalid.chars() {
            assert!(!is_bare_key(&c.to_string()), "'{}' should NOT be a valid bare key char", c);
        }
    }

    #[test]
    fn test_bare_key_multi_byte_unicode() {
        assert!(!is_bare_key("ñ"));
        assert!(!is_bare_key("日本語"));
        assert!(!is_bare_key("über"));
        assert!(!is_bare_key("café"));
    }

    #[test]
    fn test_bare_key_mixed_valid_invalid() {
        assert!(!is_bare_key("hello world"));
        assert!(!is_bare_key("key.name"));
        assert!(!is_bare_key("path/to/thing"));
        assert!(is_bare_key("valid-key_123"));
    }

    #[test]
    fn test_bare_key_tab_and_newline() {
        assert!(!is_bare_key("key\ttab"));
        assert!(!is_bare_key("key\nnewline"));
    }

    // ── strip_unnecessary_key_quotes: more edge cases ──

    #[test]
    fn test_strip_quotes_empty_string() {
        assert_eq!(strip_unnecessary_key_quotes(""), "");
    }

    #[test]
    fn test_strip_quotes_no_newline_at_end() {
        // Input without trailing newline: function adds newline per line
        let input = "\"key\" = \"value\"";
        let result = strip_unnecessary_key_quotes(input);
        assert_eq!(result, "key = \"value\"\n");
    }

    #[test]
    fn test_strip_quotes_multiple_equals_in_value() {
        let input = "\"key\" = \"a=b=c\"\n";
        let result = strip_unnecessary_key_quotes(input);
        assert_eq!(result, "key = \"a=b=c\"\n");
    }

    #[test]
    fn test_strip_quotes_value_containing_quotes() {
        let input = "\"key\" = \"value with \\\"quotes\\\"\"\n";
        let result = strip_unnecessary_key_quotes(input);
        assert_eq!(result, "key = \"value with \\\"quotes\\\"\"\n");
    }

    #[test]
    fn test_strip_quotes_nested_sections() {
        let input = "[section]\n\"key1\" = \"val1\"\n\n[other]\n\"key2\" = \"val2\"\n";
        let result = strip_unnecessary_key_quotes(input);
        assert_eq!(result, "[section]\nkey1 = \"val1\"\n\n[other]\nkey2 = \"val2\"\n");
    }

    #[test]
    fn test_strip_quotes_array_values() {
        // Array value: should not strip since after the closing quote we don't have " = "
        let input = "\"array_key\" = [\"a\", \"b\"]\n";
        let result = strip_unnecessary_key_quotes(input);
        assert_eq!(result, "array_key = [\"a\", \"b\"]\n");
    }

    #[test]
    fn test_strip_quotes_key_only_digits() {
        let input = "\"123\" = \"numeric key\"\n";
        let result = strip_unnecessary_key_quotes(input);
        assert_eq!(result, "123 = \"numeric key\"\n");
    }

    #[test]
    fn test_strip_quotes_key_with_dot_stays_quoted() {
        let input = "\"auto-resolve/AGENTS.md\" = \"true\"\n";
        let result = strip_unnecessary_key_quotes(input);
        // '/' is not allowed in bare keys, so stays quoted
        assert_eq!(result, "\"auto-resolve/AGENTS.md\" = \"true\"\n");
    }

    #[test]
    fn test_strip_quotes_empty_key_stays_quoted() {
        let input = "\"\" = \"empty key\"\n";
        let result = strip_unnecessary_key_quotes(input);
        // Empty string is not a valid bare key
        assert_eq!(result, "\"\" = \"empty key\"\n");
    }

    // ── TOML round-trips with all fields ──

    #[test]
    fn test_global_azufig_all_fields_roundtrip() {
        let az = GlobalAzufig {
            config: AzufigConfig {
                anthropic_api_key: Some("sk-test-123".to_string()),
                claude_executable: Some("/usr/bin/claude".to_string()),
                codex_executable: None,
                backend: None,
                default_permission_mode: "approve".to_string(),
                verbose: true,
            },
            projects: {
                let mut m = HashMap::new();
                m.insert("proj1".to_string(), "~/dev/proj1".to_string());
                m.insert("proj2".to_string(), "~/work/proj2".to_string());
                m
            },
            runcmds: {
                let mut m = HashMap::new();
                m.insert("1_Build".to_string(), "cargo build".to_string());
                m.insert("2_Test".to_string(), "cargo test".to_string());
                m
            },
            presetprompts: {
                let mut m = HashMap::new();
                m.insert("review".to_string(), "Review this PR".to_string());
                m
            },
        };

        let toml_str = toml::to_string_pretty(&az).unwrap();
        let parsed: GlobalAzufig = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.config.anthropic_api_key, az.config.anthropic_api_key);
        assert_eq!(parsed.config.claude_executable, az.config.claude_executable);
        assert_eq!(parsed.config.default_permission_mode, "approve");
        assert!(parsed.config.verbose);
        assert_eq!(parsed.projects.len(), 2);
        assert_eq!(parsed.runcmds.len(), 2);
        assert_eq!(parsed.presetprompts.len(), 1);
    }

    #[test]
    fn test_project_azufig_all_fields_roundtrip() {
        let az = ProjectAzufig {
            filetree: AzufigFiletree {
                hidden: vec!["target".into(), ".git".into(), "node_modules".into()],
            },
            healthscope: AzufigHealthScope {
                dirs: vec!["/src".into(), "/lib".into()],
            },
            runcmds: {
                let mut m = HashMap::new();
                m.insert("lint".to_string(), "cargo clippy".to_string());
                m
            },
            presetprompts: {
                let mut m = HashMap::new();
                m.insert("fix".to_string(), "Fix this bug".to_string());
                m
            },
            git: {
                let mut m = HashMap::new();
                m.insert("auto-rebase/feature".to_string(), "true".to_string());
                m.insert("auto-resolve/AGENTS.md".to_string(), "true".to_string());
                m
            },
        };

        let toml_str = toml::to_string_pretty(&az).unwrap();
        let parsed: ProjectAzufig = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.filetree.hidden.len(), 3);
        assert_eq!(parsed.healthscope.dirs.len(), 2);
        assert_eq!(parsed.runcmds.len(), 1);
        assert_eq!(parsed.presetprompts.len(), 1);
        assert_eq!(parsed.git.len(), 2);
    }

    #[test]
    fn test_azufig_config_all_fields_roundtrip() {
        let cfg = AzufigConfig {
            anthropic_api_key: Some("key".to_string()),
            claude_executable: Some("/bin/claude".to_string()),
            codex_executable: None,
            backend: None,
            default_permission_mode: "ask".to_string(),
            verbose: true,
        };
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: AzufigConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.anthropic_api_key.as_deref(), Some("key"));
        assert_eq!(parsed.claude_executable.as_deref(), Some("/bin/claude"));
        assert_eq!(parsed.default_permission_mode, "ask");
        assert!(parsed.verbose);
    }

    // ── AzufigHealthScope default ──

    #[test]
    fn test_health_scope_default() {
        let scope = AzufigHealthScope::default();
        assert!(scope.dirs.is_empty());
    }

    #[test]
    fn test_health_scope_with_dirs() {
        let scope = AzufigHealthScope {
            dirs: vec!["/src".into(), "/tests".into()],
        };
        assert_eq!(scope.dirs.len(), 2);
        assert!(scope.dirs.contains(&"/src".to_string()));
    }

    // ── Deserialization from partial TOML ──

    #[test]
    fn test_global_azufig_from_empty_toml() {
        let parsed: GlobalAzufig = toml::from_str("").unwrap();
        assert!(parsed.projects.is_empty());
        assert!(parsed.runcmds.is_empty());
        assert!(parsed.presetprompts.is_empty());
        assert!(parsed.config.anthropic_api_key.is_none());
    }

    #[test]
    fn test_global_azufig_from_partial_toml() {
        let toml_str = r#"
[projects]
MyProj = "~/dev/myproj"
"#;
        let parsed: GlobalAzufig = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.projects.get("MyProj").unwrap(), "~/dev/myproj");
        assert!(parsed.runcmds.is_empty());
        assert!(parsed.config.anthropic_api_key.is_none());
    }

    #[test]
    fn test_project_azufig_from_partial_toml() {
        let toml_str = r#"
[git]
"auto-rebase/main" = "true"
"#;
        let parsed: ProjectAzufig = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.git.get("auto-rebase/main").unwrap(), "true");
        assert_eq!(parsed.filetree.hidden.len(), 5); // defaults
        assert!(parsed.runcmds.is_empty());
    }

    #[test]
    fn test_project_azufig_filetree_only() {
        let toml_str = r#"
[filetree]
hidden = ["target", ".git"]
"#;
        let parsed: ProjectAzufig = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.filetree.hidden.len(), 2);
        assert!(parsed.filetree.hidden.contains(&"target".to_string()));
    }

    #[test]
    fn test_config_from_partial_toml() {
        let toml_str = r#"
verbose = true
"#;
        let parsed: AzufigConfig = toml::from_str(toml_str).unwrap();
        assert!(parsed.verbose);
        assert!(parsed.anthropic_api_key.is_none());
        assert!(parsed.claude_executable.is_none());
        assert_eq!(parsed.default_permission_mode, "");
    }

    // ── DEFAULT_AUTO_RESOLVE specific entries ──

    #[test]
    fn test_default_auto_resolve_specific_entries() {
        assert_eq!(DEFAULT_AUTO_RESOLVE[0], "AGENTS.md");
        assert_eq!(DEFAULT_AUTO_RESOLVE[1], "CHANGELOG.md");
        assert_eq!(DEFAULT_AUTO_RESOLVE[2], "README.md");
        assert_eq!(DEFAULT_AUTO_RESOLVE[3], "CLAUDE.md");
    }

    #[test]
    fn test_default_auto_resolve_order() {
        // Verify alphabetical-ish order (AGENTS, CHANGELOG, README, CLAUDE)
        // Actually the order is as defined, not alphabetical
        let files: Vec<&str> = DEFAULT_AUTO_RESOLVE.to_vec();
        assert_eq!(files, vec!["AGENTS.md", "CHANGELOG.md", "README.md", "CLAUDE.md"]);
    }

    #[test]
    fn test_default_auto_resolve_all_markdown() {
        for file in DEFAULT_AUTO_RESOLVE {
            assert!(file.ends_with(".md"), "auto-resolve file '{}' should end with .md", file);
        }
    }

    // ── AzufigFiletree ──

    #[test]
    fn test_filetree_custom_hidden() {
        let ft = AzufigFiletree {
            hidden: vec!["target".into(), "node_modules".into()],
        };
        assert_eq!(ft.hidden.len(), 2);
    }

    #[test]
    fn test_filetree_empty_hidden() {
        let ft = AzufigFiletree { hidden: vec![] };
        assert!(ft.hidden.is_empty());
    }

    #[test]
    fn test_filetree_serialize_roundtrip() {
        let ft = AzufigFiletree {
            hidden: vec!["a".into(), "b".into(), "c".into()],
        };
        let toml_str = toml::to_string_pretty(&ft).unwrap();
        let parsed: AzufigFiletree = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.hidden, ft.hidden);
    }

    #[test]
    fn test_filetree_clone() {
        let ft = AzufigFiletree::default();
        let cloned = ft.clone();
        assert_eq!(ft.hidden, cloned.hidden);
    }

    // ── GlobalAzufig ──

    #[test]
    fn test_global_azufig_clone() {
        let mut az = GlobalAzufig::default();
        az.projects.insert("A".into(), "~/a".into());
        let cloned = az.clone();
        assert_eq!(cloned.projects.get("A").unwrap(), "~/a");
    }

    #[test]
    fn test_global_azufig_debug() {
        let az = GlobalAzufig::default();
        let dbg = format!("{:?}", az);
        assert!(dbg.contains("GlobalAzufig"));
    }

    // ── ProjectAzufig ──

    #[test]
    fn test_project_azufig_clone() {
        let mut az = ProjectAzufig::default();
        az.runcmds.insert("build".into(), "cargo build".into());
        let cloned = az.clone();
        assert_eq!(cloned.runcmds.get("build").unwrap(), "cargo build");
    }

    #[test]
    fn test_project_azufig_debug() {
        let az = ProjectAzufig::default();
        let dbg = format!("{:?}", az);
        assert!(dbg.contains("ProjectAzufig"));
    }

    // ── AzufigConfig ──

    #[test]
    fn test_azufig_config_clone() {
        let cfg = AzufigConfig {
            anthropic_api_key: Some("key".into()),
            claude_executable: None,
            codex_executable: None,
            backend: None,
            default_permission_mode: "ignore".into(),
            verbose: false,
        };
        let cloned = cfg.clone();
        assert_eq!(cloned.anthropic_api_key, cfg.anthropic_api_key);
        assert_eq!(cloned.default_permission_mode, "ignore");
    }

    #[test]
    fn test_azufig_config_debug() {
        let cfg = AzufigConfig::default();
        let dbg = format!("{:?}", cfg);
        assert!(dbg.contains("AzufigConfig"));
    }

    // ── AzufigHealthScope ──

    #[test]
    fn test_health_scope_clone() {
        let scope = AzufigHealthScope {
            dirs: vec!["/src".into()],
        };
        let cloned = scope.clone();
        assert_eq!(cloned.dirs, scope.dirs);
    }

    #[test]
    fn test_health_scope_serialize_roundtrip() {
        let scope = AzufigHealthScope {
            dirs: vec!["/a".into(), "/b".into()],
        };
        let toml_str = toml::to_string_pretty(&scope).unwrap();
        let parsed: AzufigHealthScope = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.dirs, scope.dirs);
    }

    // ── godfilescope alias ──

    #[test]
    fn test_project_azufig_godfilescope_alias() {
        let toml_str = r#"
[godfilescope]
dirs = ["/legacy"]
"#;
        let parsed: ProjectAzufig = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.healthscope.dirs, vec!["/legacy"]);
    }
}
