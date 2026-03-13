//! Session cache — intermediate translation layer
//!
//! Writes parsed session data to `.azureal/sessions/<backend-N>.json.gz` so the
//! TUI reads from a unified format instead of backend-specific JSONL files.
//! Cache files are gzip-compressed for minimal disk footprint.
//!
//! Files are named sequentially per backend: `claude-1.json.gz`, `claude-2.json.gz`,
//! `codex-1.json.gz`, etc. An `index.json` maps session UUIDs to cache names.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};

use crate::app::session_parser::ParsedSession;
use crate::backend::Backend;
use crate::events::DisplayEvent;

// =========================================================================
// Cache index — maps session UUIDs to sequential cache names
// =========================================================================

/// Maps session UUIDs to their cache filenames (e.g. "claude-1", "codex-3")
#[derive(Serialize, Deserialize, Default)]
struct CacheIndex {
    /// UUID → cache name (without extension)
    #[serde(flatten)]
    map: HashMap<String, String>,
}

/// Path to the index file
fn index_path(project_root: &Path) -> PathBuf {
    project_root.join(".azureal").join("sessions").join("index.json")
}

/// Load the index (returns empty if missing or corrupt)
fn read_index(project_root: &Path) -> CacheIndex {
    let path = index_path(project_root);
    fs::read(&path)
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

/// Save the index atomically
fn write_index(project_root: &Path, index: &CacheIndex) -> io::Result<()> {
    let path = index_path(project_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_vec(index)?;
    fs::write(&tmp, &data)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// Find the next sequential number for a backend prefix by scanning existing values
fn next_number(index: &CacheIndex, prefix: &str) -> u64 {
    let pat = format!("{}-", prefix);
    index.map.values()
        .filter_map(|name| name.strip_prefix(&pat).and_then(|n| n.parse::<u64>().ok()))
        .max()
        .map(|n| n + 1)
        .unwrap_or(1)
}

/// Look up existing cache name for a session UUID
pub fn lookup_cache_name(project_root: &Path, session_id: &str) -> Option<String> {
    let index = read_index(project_root);
    index.map.get(session_id).cloned()
}

/// Get or assign a cache name for a session UUID
pub fn resolve_cache_name(project_root: &Path, session_id: &str, backend: Backend) -> io::Result<String> {
    let mut index = read_index(project_root);
    if let Some(name) = index.map.get(session_id) {
        return Ok(name.clone());
    }
    let prefix = match backend {
        Backend::Claude => "claude",
        Backend::Codex => "codex",
    };
    let n = next_number(&index, prefix);
    let name = format!("{}-{}", prefix, n);
    index.map.insert(session_id.to_string(), name.clone());
    write_index(project_root, &index)?;
    Ok(name)
}

/// Cached session data — mirrors ParsedSession plus source provenance fields
#[derive(Serialize, Deserialize)]
pub struct CachedSession {
    /// Absolute path to the raw JSONL file this cache was built from
    pub source_path: PathBuf,
    /// Byte size of the raw JSONL at time of last parse (invalidation key)
    pub source_size: u64,
    /// Byte offset for incremental resumption against raw JSONL
    pub parse_offset: u64,
    pub events: Vec<DisplayEvent>,
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub pending_tools: HashSet<String>,
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub failed_tools: HashSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_tokens: Option<(u64, u64)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub total_lines: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub parse_errors: usize,
    pub assistant_total: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub assistant_no_message: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub assistant_no_content_arr: usize,
    pub assistant_text_blocks: usize,
    #[serde(default, skip_serializing_if = "is_false")]
    pub awaiting_plan_approval: bool,
}

fn is_zero(v: &usize) -> bool { *v == 0 }
fn is_false(v: &bool) -> bool { !*v }

impl CachedSession {
    /// Build from a freshly parsed session (clones + compacts event data for the cache)
    pub fn from_parsed(parsed: &ParsedSession, source_path: PathBuf, source_size: u64) -> Self {
        let mut events = parsed.events.clone();
        compact_events(&mut events);
        Self {
            source_path,
            source_size,
            parse_offset: parsed.end_offset,
            events,
            pending_tools: parsed.pending_tools.clone(),
            failed_tools: parsed.failed_tools.clone(),
            session_tokens: parsed.session_tokens,
            context_window: parsed.context_window,
            model: parsed.model.clone(),
            total_lines: parsed.total_lines,
            parse_errors: parsed.parse_errors,
            assistant_total: parsed.assistant_total,
            assistant_no_message: parsed.assistant_no_message,
            assistant_no_content_arr: parsed.assistant_no_content_arr,
            assistant_text_blocks: parsed.assistant_text_blocks,
            awaiting_plan_approval: parsed.awaiting_plan_approval,
        }
    }

    /// Apply compaction to strip event data the render pipeline doesn't need
    pub(crate) fn compact(&mut self) {
        compact_events(&mut self.events);
    }

    /// Convert back to ParsedSession for hydration into App state
    pub fn into_parsed(self) -> ParsedSession {
        ParsedSession {
            events: self.events,
            pending_tools: self.pending_tools,
            failed_tools: self.failed_tools,
            total_lines: self.total_lines,
            parse_errors: self.parse_errors,
            assistant_total: self.assistant_total,
            assistant_no_message: self.assistant_no_message,
            assistant_no_content_arr: self.assistant_no_content_arr,
            assistant_text_blocks: self.assistant_text_blocks,
            awaiting_plan_approval: self.awaiting_plan_approval,
            end_offset: self.parse_offset,
            session_tokens: self.session_tokens,
            context_window: self.context_window,
            model: self.model,
        }
    }
}

/// Strip events down to what the render pipeline actually uses
fn compact_events(events: &mut Vec<DisplayEvent>) {
    for event in events.iter_mut() {
        match event {
            DisplayEvent::ToolResult { tool_name, content, .. } => {
                *content = compact_tool_result(tool_name, content);
            }
            DisplayEvent::ToolCall { tool_name, input, .. } => {
                compact_tool_input(tool_name, input);
            }
            _ => {}
        }
    }
}

/// Truncate tool result content to what the render pipeline shows
fn compact_tool_result(tool_name: &str, content: &str) -> String {
    // Strip system reminders (same as render_tool_result)
    let content = content.split("<system-reminder>").next().unwrap_or(content).trim_end();
    let lines: Vec<&str> = content.lines().collect();
    let n = lines.len();

    match tool_name {
        "Read" | "read" => {
            // Render shows: first line + "(N lines)" + last non-empty line
            if n <= 3 { return content.to_string(); }
            let first = lines[0];
            let last = lines.iter().rev().find(|l| !l.trim().is_empty()).unwrap_or(&"");
            format!("{}\n({} lines)\n{}", first, n, last)
        }
        "Bash" | "bash" => {
            // Render shows: last 2 non-empty lines
            let non_empty: Vec<&str> = lines.iter().filter(|l| !l.trim().is_empty()).copied().collect();
            non_empty.iter().rev().take(2).rev().copied().collect::<Vec<_>>().join("\n")
        }
        "Grep" | "grep" => {
            if n <= 3 { return content.to_string(); }
            format!("{}\n(+{} more)", lines[..3].join("\n"), n - 3)
        }
        "Glob" | "glob" => {
            // Render shows: "N files"
            format!("{} files", n)
        }
        "Task" | "task" => {
            if n <= 5 { return content.to_string(); }
            format!("{}\n(+{} more lines)", lines[..5].join("\n"), n - 5)
        }
        _ => {
            if n <= 3 { return content.to_string(); }
            format!("{}\n(+{} more)", lines[..3].join("\n"), n - 3)
        }
    }
}

/// Strip tool input down to display-relevant fields only
fn compact_tool_input(tool_name: &str, input: &mut serde_json::Value) {
    match tool_name {
        // Edit keeps full input (old_string/new_string needed for diff rendering)
        "Edit" | "edit" => {}
        // Write: replace full file content with line count + purpose line
        "Write" | "write" => {
            let summary = input.get("content").and_then(|v| v.as_str()).map(|file_content| {
                let lines: Vec<&str> = file_content.lines().collect();
                let line_count = lines.len();
                let purpose = lines.iter()
                    .find(|l| {
                        let t = l.trim();
                        t.starts_with("//") || t.starts_with('#')
                            || t.starts_with("/*") || t.starts_with("\"\"\"")
                            || t.starts_with("///") || t.starts_with("//!")
                    })
                    .or(lines.first()).copied()
                    .unwrap_or("");
                format!("({} lines) {}", line_count, purpose.trim())
            });
            if let (Some(summary), Some(obj)) = (summary, input.as_object_mut()) {
                obj.insert("content".into(), serde_json::Value::String(summary));
            }
        }
        // All other tools: keep only the one field extract_tool_param reads
        _ => {
            let keys: &[&str] = match tool_name {
                "Bash" | "bash" => &["command"],
                "Glob" | "glob" | "Grep" | "grep" => &["pattern"],
                "Read" | "read" => &["file_path", "path"],
                "WebFetch" | "webfetch" => &["url"],
                "WebSearch" | "websearch" => &["query"],
                "Task" | "task" => &["subagent_type", "description"],
                "LSP" | "lsp" => &["operation", "filePath"],
                _ => &["file_path", "path", "command", "query", "pattern"],
            };
            if let Some(obj) = input.as_object_mut() {
                obj.retain(|k, _| keys.contains(&k.as_str()));
            }
        }
    }
}

/// Returns `.azureal/sessions/<cache_name>.json.gz`
pub fn cache_path(project_root: &Path, cache_name: &str) -> PathBuf {
    project_root.join(".azureal").join("sessions").join(format!("{cache_name}.json.gz"))
}

/// Write parsed session to cache (gzip-compressed, atomic: write tmp then rename)
pub fn write_cache(project_root: &Path, cache_name: &str, cached: &CachedSession) -> io::Result<()> {
    let path = cache_path(project_root, cache_name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_vec(cached)?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&json)?;
    let compressed = encoder.finish()?;
    fs::write(&tmp, &compressed)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// Decompress gzip data and deserialize
fn decompress_cache(compressed: &[u8]) -> Option<CachedSession> {
    let mut decoder = GzDecoder::new(compressed);
    let mut json = Vec::new();
    decoder.read_to_end(&mut json).ok()?;
    serde_json::from_slice(&json).ok()
}

/// Read cache if valid — source_path and source_size must both match.
/// `cache_name` is the indexed name (e.g. "claude-1"), not the UUID.
pub fn read_cache(
    project_root: &Path,
    cache_name: &str,
    source_path: &Path,
    current_source_size: u64,
) -> Option<CachedSession> {
    let path = cache_path(project_root, cache_name);
    let data = fs::read(&path).ok()?;
    let cached = decompress_cache(&data)?;
    if cached.source_path != source_path || cached.source_size != current_source_size {
        return None;
    }
    Some(cached)
}

/// Read cache without source validation (for when raw file is missing).
/// `cache_name` is the indexed name (e.g. "claude-1"), not the UUID.
pub fn read_cache_orphan(project_root: &Path, cache_name: &str) -> Option<CachedSession> {
    let path = cache_path(project_root, cache_name);
    let data = fs::read(&path).ok()?;
    decompress_cache(&data)
}

/// Migrate old UUID-named `.json` cache files to new `.json.gz` format with index entries.
/// Scans `.azureal/sessions/` for `*.json` files (excluding `index.json`), reads each as
/// uncompressed JSON, assigns a sequential cache name, writes compressed `.json.gz`, and
/// removes the old file. No-op if no legacy files exist. Safe to call multiple times.
pub fn migrate_legacy_caches(project_root: &Path, default_backend: Backend) {
    let sessions_dir = project_root.join(".azureal").join("sessions");
    let entries = match fs::read_dir(&sessions_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut migrated = 0u32;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Only process *.json files (not index.json, not *.json.gz, not *.tmp)
        if !name.ends_with(".json") || name == "index.json" {
            continue;
        }

        // Extract UUID from filename (strip .json extension)
        let uuid = name.strip_suffix(".json").unwrap();

        // Read old uncompressed JSON
        let data = match fs::read(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let cached: CachedSession = match serde_json::from_slice(&data) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Detect backend from source_path (codex sessions have "codex" in path)
        let backend = if cached.source_path.to_str().map_or(false, |p| p.contains("codex")) {
            Backend::Codex
        } else {
            default_backend
        };

        // Assign sequential cache name and write compressed
        if let Ok(cache_name) = resolve_cache_name(project_root, uuid, backend) {
            if write_cache(project_root, &cache_name, &cached).is_ok() {
                let _ = fs::remove_file(&path);
                migrated += 1;
            }
        }
    }

    if migrated > 0 {
        eprintln!("[session_cache] migrated {} legacy cache file(s)", migrated);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn sample_events() -> Vec<DisplayEvent> {
        vec![
            DisplayEvent::Init {
                _session_id: "sess-1".into(),
                cwd: "/tmp/project".into(),
                model: "opus".into(),
            },
            DisplayEvent::UserMessage {
                _uuid: "u1".into(),
                content: "Hello".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: "a1".into(),
                _message_id: "m1".into(),
                text: "Hi there!".into(),
            },
            DisplayEvent::ToolCall {
                _uuid: "a1".into(),
                tool_use_id: "tc1".into(),
                tool_name: "Read".into(),
                file_path: Some("/src/main.rs".into()),
                input: serde_json::json!({"file_path": "/src/main.rs"}),
            },
            DisplayEvent::ToolResult {
                tool_use_id: "tc1".into(),
                tool_name: "Read".into(),
                file_path: Some("/src/main.rs".into()),
                content: "fn main() {}".into(),
                is_error: false,
            },
            DisplayEvent::Complete {
                _session_id: "sess-1".into(),
                success: true,
                duration_ms: 5000,
                cost_usd: 0.05,
            },
        ]
    }

    fn sample_parsed() -> ParsedSession {
        ParsedSession {
            events: sample_events(),
            pending_tools: HashSet::new(),
            failed_tools: {
                let mut s = HashSet::new();
                s.insert("fail-1".into());
                s
            },
            total_lines: 20,
            parse_errors: 1,
            assistant_total: 5,
            assistant_no_message: 0,
            assistant_no_content_arr: 0,
            assistant_text_blocks: 3,
            awaiting_plan_approval: false,
            end_offset: 4096,
            session_tokens: Some((1000, 200)),
            context_window: Some(200_000),
            model: Some("opus".into()),
        }
    }

    // =====================================================================
    // CachedSession::from_parsed + into_parsed round-trip
    // =====================================================================

    #[test]
    fn from_parsed_preserves_all_fields() {
        let parsed = sample_parsed();
        let source = PathBuf::from("/home/.claude/projects/abc/sess.jsonl");
        let cached = CachedSession::from_parsed(&parsed, source.clone(), 4096);

        assert_eq!(cached.source_path, source);
        assert_eq!(cached.source_size, 4096);
        assert_eq!(cached.parse_offset, 4096);
        assert_eq!(cached.events.len(), 6);
        assert!(cached.pending_tools.is_empty());
        assert!(cached.failed_tools.contains("fail-1"));
        assert_eq!(cached.total_lines, 20);
        assert_eq!(cached.parse_errors, 1);
        assert_eq!(cached.assistant_total, 5);
        assert_eq!(cached.assistant_no_message, 0);
        assert_eq!(cached.assistant_no_content_arr, 0);
        assert_eq!(cached.assistant_text_blocks, 3);
        assert!(!cached.awaiting_plan_approval);
        assert_eq!(cached.session_tokens, Some((1000, 200)));
        assert_eq!(cached.context_window, Some(200_000));
        assert_eq!(cached.model.as_deref(), Some("opus"));
    }

    #[test]
    fn into_parsed_round_trip() {
        let parsed = sample_parsed();
        let source = PathBuf::from("/source.jsonl");
        let cached = CachedSession::from_parsed(&parsed, source, 9999);
        let restored = cached.into_parsed();

        assert_eq!(restored.events.len(), 6);
        assert_eq!(restored.end_offset, 4096);
        assert_eq!(restored.total_lines, 20);
        assert_eq!(restored.parse_errors, 1);
        assert!(restored.failed_tools.contains("fail-1"));
        assert_eq!(restored.session_tokens, Some((1000, 200)));
        assert_eq!(restored.context_window, Some(200_000));
        assert_eq!(restored.model.as_deref(), Some("opus"));
    }

    // =====================================================================
    // Serialization round-trip
    // =====================================================================

    #[test]
    fn serde_round_trip() {
        let parsed = sample_parsed();
        let source = PathBuf::from("/raw.jsonl");
        let cached = CachedSession::from_parsed(&parsed, source.clone(), 5000);

        let json = serde_json::to_vec(&cached).unwrap();
        let restored: CachedSession = serde_json::from_slice(&json).unwrap();

        assert_eq!(restored.source_path, source);
        assert_eq!(restored.source_size, 5000);
        assert_eq!(restored.parse_offset, 4096);
        assert_eq!(restored.events.len(), 6);
        assert_eq!(restored.model.as_deref(), Some("opus"));
    }

    #[test]
    fn serde_empty_events() {
        let cached = CachedSession {
            source_path: PathBuf::from("/empty.jsonl"),
            source_size: 0,
            parse_offset: 0,
            events: vec![],
            pending_tools: HashSet::new(),
            failed_tools: HashSet::new(),
            session_tokens: None,
            context_window: None,
            model: None,
            total_lines: 0,
            parse_errors: 0,
            assistant_total: 0,
            assistant_no_message: 0,
            assistant_no_content_arr: 0,
            assistant_text_blocks: 0,
            awaiting_plan_approval: false,
        };

        let json = serde_json::to_string(&cached).unwrap();
        let restored: CachedSession = serde_json::from_str(&json).unwrap();
        assert!(restored.events.is_empty());
        assert_eq!(restored.source_size, 0);
    }

    #[test]
    fn serde_all_event_variants() {
        let events = vec![
            DisplayEvent::Init { _session_id: "s".into(), cwd: "/".into(), model: "m".into() },
            DisplayEvent::Hook { name: "pre".into(), output: "ok".into() },
            DisplayEvent::UserMessage { _uuid: "u".into(), content: "hi".into() },
            DisplayEvent::Command { name: "/compact".into() },
            DisplayEvent::Compacting,
            DisplayEvent::Compacted,
            DisplayEvent::MayBeCompacting,
            DisplayEvent::Plan { name: "plan".into(), content: "step 1".into() },
            DisplayEvent::AssistantText { _uuid: "a".into(), _message_id: "m".into(), text: "t".into() },
            DisplayEvent::ToolCall {
                _uuid: "a".into(),
                tool_use_id: "t1".into(),
                tool_name: "Bash".into(),
                file_path: None,
                input: serde_json::json!({"cmd": "ls"}),
            },
            DisplayEvent::ToolResult {
                tool_use_id: "t1".into(),
                tool_name: "Bash".into(),
                file_path: None,
                content: "file.rs".into(),
                is_error: false,
            },
            DisplayEvent::Complete { _session_id: "s".into(), success: true, duration_ms: 100, cost_usd: 0.01 },
            DisplayEvent::Filtered,
        ];

        let cached = CachedSession {
            source_path: PathBuf::from("/all.jsonl"),
            source_size: 999,
            parse_offset: 999,
            events,
            pending_tools: HashSet::new(),
            failed_tools: HashSet::new(),
            session_tokens: None,
            context_window: None,
            model: None,
            total_lines: 13,
            parse_errors: 0,
            assistant_total: 1,
            assistant_no_message: 0,
            assistant_no_content_arr: 0,
            assistant_text_blocks: 1,
            awaiting_plan_approval: false,
        };

        let json = serde_json::to_vec(&cached).unwrap();
        let restored: CachedSession = serde_json::from_slice(&json).unwrap();
        assert_eq!(restored.events.len(), 13);
    }

    // =====================================================================
    // cache_path
    // =====================================================================

    #[test]
    fn cache_path_structure() {
        let path = cache_path(Path::new("/projects/myapp"), "abc-123");
        assert_eq!(path, PathBuf::from("/projects/myapp/.azureal/sessions/abc-123.json.gz"));
    }

    #[test]
    fn cache_path_uuid_with_special_chars() {
        let path = cache_path(Path::new("/proj"), "a1b2c3d4-e5f6-7890-abcd-ef1234567890");
        assert!(path.to_str().unwrap().ends_with("a1b2c3d4-e5f6-7890-abcd-ef1234567890.json.gz"));
    }

    // =====================================================================
    // File I/O (write + read + read_cache_orphan)
    // =====================================================================

    #[test]
    fn write_and_read_cache() {
        let dir = tempfile::tempdir().unwrap();
        let project_root = dir.path();
        let source = project_root.join("raw.jsonl");
        std::fs::write(&source, "fake data").unwrap();

        let parsed = sample_parsed();
        let cached = CachedSession::from_parsed(&parsed, source.clone(), 9);
        write_cache(project_root, "test-sess", &cached).unwrap();

        // Valid read — source path and size match
        let result = read_cache(project_root, "test-sess", &source, 9);
        assert!(result.is_some());
        let restored = result.unwrap();
        assert_eq!(restored.events.len(), 6);
        assert_eq!(restored.source_size, 9);
    }

    #[test]
    fn read_cache_rejects_wrong_size() {
        let dir = tempfile::tempdir().unwrap();
        let project_root = dir.path();
        let source = project_root.join("raw.jsonl");

        let parsed = sample_parsed();
        let cached = CachedSession::from_parsed(&parsed, source.clone(), 100);
        write_cache(project_root, "sess", &cached).unwrap();

        // Size mismatch → None
        let result = read_cache(project_root, "sess", &source, 200);
        assert!(result.is_none());
    }

    #[test]
    fn read_cache_rejects_wrong_source_path() {
        let dir = tempfile::tempdir().unwrap();
        let project_root = dir.path();
        let source = PathBuf::from("/original/path.jsonl");
        let wrong = PathBuf::from("/different/path.jsonl");

        let parsed = sample_parsed();
        let cached = CachedSession::from_parsed(&parsed, source, 100);
        write_cache(project_root, "sess", &cached).unwrap();

        let result = read_cache(project_root, "sess", &wrong, 100);
        assert!(result.is_none());
    }

    #[test]
    fn read_cache_missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let result = read_cache(dir.path(), "nonexistent", Path::new("/x"), 0);
        assert!(result.is_none());
    }

    #[test]
    fn read_cache_orphan_ignores_source_validation() {
        let dir = tempfile::tempdir().unwrap();
        let project_root = dir.path();
        let source = PathBuf::from("/gone/forever.jsonl");

        let parsed = sample_parsed();
        let cached = CachedSession::from_parsed(&parsed, source, 999);
        write_cache(project_root, "orphan", &cached).unwrap();

        // Source file doesn't exist — orphan read still works
        let result = read_cache_orphan(project_root, "orphan");
        assert!(result.is_some());
        assert_eq!(result.unwrap().events.len(), 6);
    }

    #[test]
    fn read_cache_orphan_missing_cache_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let result = read_cache_orphan(dir.path(), "nope");
        assert!(result.is_none());
    }

    #[test]
    fn write_cache_creates_directories() {
        let dir = tempfile::tempdir().unwrap();
        let project_root = dir.path().join("deep").join("nested");
        // Directory doesn't exist yet

        let parsed = sample_parsed();
        let cached = CachedSession::from_parsed(&parsed, PathBuf::from("/s.jsonl"), 10);
        write_cache(&project_root, "sess", &cached).unwrap();

        let result = read_cache_orphan(&project_root, "sess");
        assert!(result.is_some());
    }

    #[test]
    fn write_cache_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let project_root = dir.path();
        let source = PathBuf::from("/raw.jsonl");

        // Write first version
        let mut parsed = sample_parsed();
        parsed.total_lines = 10;
        let cached = CachedSession::from_parsed(&parsed, source.clone(), 100);
        write_cache(project_root, "sess", &cached).unwrap();

        // Write second version
        let mut parsed2 = sample_parsed();
        parsed2.total_lines = 99;
        let cached2 = CachedSession::from_parsed(&parsed2, source.clone(), 200);
        write_cache(project_root, "sess", &cached2).unwrap();

        let result = read_cache(project_root, "sess", &source, 200).unwrap();
        assert_eq!(result.total_lines, 99);
        assert_eq!(result.source_size, 200);
    }

    // =====================================================================
    // Edge cases
    // =====================================================================

    #[test]
    fn serde_tool_result_with_error() {
        let events = vec![DisplayEvent::ToolResult {
            tool_use_id: "t1".into(),
            tool_name: "Bash".into(),
            file_path: Some("/fail.sh".into()),
            content: "exit code 1".into(),
            is_error: true,
        }];

        let cached = CachedSession {
            source_path: PathBuf::from("/x.jsonl"),
            source_size: 1,
            parse_offset: 1,
            events,
            pending_tools: HashSet::new(),
            failed_tools: { let mut s = HashSet::new(); s.insert("t1".into()); s },
            session_tokens: None,
            context_window: None,
            model: None,
            total_lines: 1,
            parse_errors: 0,
            assistant_total: 0,
            assistant_no_message: 0,
            assistant_no_content_arr: 0,
            assistant_text_blocks: 0,
            awaiting_plan_approval: false,
        };

        let json = serde_json::to_vec(&cached).unwrap();
        let restored: CachedSession = serde_json::from_slice(&json).unwrap();
        match &restored.events[0] {
            DisplayEvent::ToolResult { is_error, .. } => assert!(is_error),
            _ => panic!("wrong variant"),
        }
        assert!(restored.failed_tools.contains("t1"));
    }

    #[test]
    fn serde_large_text_content() {
        let big_text = "x".repeat(500_000);
        let events = vec![DisplayEvent::AssistantText {
            _uuid: "u".into(),
            _message_id: "m".into(),
            text: big_text.clone(),
        }];

        let cached = CachedSession {
            source_path: PathBuf::from("/big.jsonl"),
            source_size: 600_000,
            parse_offset: 600_000,
            events,
            pending_tools: HashSet::new(),
            failed_tools: HashSet::new(),
            session_tokens: Some((50_000, 10_000)),
            context_window: Some(200_000),
            model: Some("opus".into()),
            total_lines: 1,
            parse_errors: 0,
            assistant_total: 1,
            assistant_no_message: 0,
            assistant_no_content_arr: 0,
            assistant_text_blocks: 1,
            awaiting_plan_approval: false,
        };

        let json = serde_json::to_vec(&cached).unwrap();
        let restored: CachedSession = serde_json::from_slice(&json).unwrap();
        match &restored.events[0] {
            DisplayEvent::AssistantText { text, .. } => assert_eq!(text.len(), 500_000),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn from_parsed_with_awaiting_plan_approval() {
        let mut parsed = sample_parsed();
        parsed.awaiting_plan_approval = true;
        let cached = CachedSession::from_parsed(&parsed, PathBuf::from("/x"), 1);
        assert!(cached.awaiting_plan_approval);
        let restored = cached.into_parsed();
        assert!(restored.awaiting_plan_approval);
    }

    #[test]
    fn from_parsed_with_none_optionals() {
        let mut parsed = sample_parsed();
        parsed.session_tokens = None;
        parsed.context_window = None;
        parsed.model = None;
        let cached = CachedSession::from_parsed(&parsed, PathBuf::from("/x"), 1);
        assert!(cached.session_tokens.is_none());
        assert!(cached.context_window.is_none());
        assert!(cached.model.is_none());

        let json = serde_json::to_vec(&cached).unwrap();
        let restored: CachedSession = serde_json::from_slice(&json).unwrap();
        assert!(restored.session_tokens.is_none());
    }

    // =====================================================================
    // Serde skip — underscore fields omitted from JSON
    // =====================================================================

    #[test]
    fn serde_skip_underscore_fields() {
        let events = vec![
            DisplayEvent::Init { _session_id: "sess-1".into(), cwd: "/tmp".into(), model: "opus".into() },
            DisplayEvent::UserMessage { _uuid: "uuid-1".into(), content: "Hello".into() },
            DisplayEvent::AssistantText { _uuid: "uuid-2".into(), _message_id: "msg-1".into(), text: "Hi".into() },
            DisplayEvent::ToolCall {
                _uuid: "uuid-3".into(), tool_use_id: "tc1".into(),
                tool_name: "Read".into(), file_path: None, input: serde_json::json!({}),
            },
            DisplayEvent::Complete { _session_id: "sess-1".into(), success: true, duration_ms: 100, cost_usd: 0.01 },
        ];

        let json = serde_json::to_string(&events).unwrap();

        // Underscore fields should NOT appear in JSON
        assert!(!json.contains("_session_id"));
        assert!(!json.contains("_uuid"));
        assert!(!json.contains("_message_id"));

        // Non-underscore fields should be present
        assert!(json.contains("cwd"));
        assert!(json.contains("opus"));
        assert!(json.contains("Hello"));
        assert!(json.contains("tool_use_id"));

        // Round-trip: underscore fields deserialize as empty strings
        let restored: Vec<DisplayEvent> = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.len(), 5);
        match &restored[0] {
            DisplayEvent::Init { _session_id, cwd, .. } => {
                assert_eq!(_session_id, "");
                assert_eq!(cwd, "/tmp");
            }
            _ => panic!("wrong variant"),
        }
        match &restored[2] {
            DisplayEvent::AssistantText { _uuid, _message_id, text } => {
                assert_eq!(_uuid, "");
                assert_eq!(_message_id, "");
                assert_eq!(text, "Hi");
            }
            _ => panic!("wrong variant"),
        }
    }

    // =====================================================================
    // skip_serializing_if — defaults omitted from JSON
    // =====================================================================

    #[test]
    fn serde_skip_defaults() {
        let cached = CachedSession {
            source_path: PathBuf::from("/x.jsonl"),
            source_size: 1,
            parse_offset: 1,
            events: vec![],
            pending_tools: HashSet::new(),
            failed_tools: HashSet::new(),
            session_tokens: None,
            context_window: None,
            model: None,
            total_lines: 0,
            parse_errors: 0,
            assistant_total: 0,
            assistant_no_message: 0,
            assistant_no_content_arr: 0,
            assistant_text_blocks: 0,
            awaiting_plan_approval: false,
        };

        let json = serde_json::to_string(&cached).unwrap();

        // Default values should be omitted
        assert!(!json.contains("pending_tools"));
        assert!(!json.contains("failed_tools"));
        assert!(!json.contains("session_tokens"));
        assert!(!json.contains("context_window"));
        assert!(!json.contains("model"));
        assert!(!json.contains("parse_errors"));
        assert!(!json.contains("assistant_no_message"));
        assert!(!json.contains("assistant_no_content_arr"));
        assert!(!json.contains("awaiting_plan_approval"));

        // Required fields still present
        assert!(json.contains("source_path"));
        assert!(json.contains("source_size"));
        assert!(json.contains("total_lines"));

        // Round-trip restores defaults
        let restored: CachedSession = serde_json::from_str(&json).unwrap();
        assert!(restored.pending_tools.is_empty());
        assert!(restored.failed_tools.is_empty());
        assert!(restored.session_tokens.is_none());
        assert_eq!(restored.parse_errors, 0);
        assert!(!restored.awaiting_plan_approval);
    }

    // =====================================================================
    // Compaction — ToolResult.content
    // =====================================================================

    #[test]
    fn compact_read_result_truncates_large_content() {
        let big_content = (1..=100).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        let result = compact_tool_result("Read", &big_content);
        assert!(result.contains("line 1"));
        assert!(result.contains("(100 lines)"));
        assert!(result.contains("line 100"));
        assert!(!result.contains("line 50"));
    }

    #[test]
    fn compact_read_result_preserves_small_content() {
        let small = "line 1\nline 2\nline 3";
        let result = compact_tool_result("Read", small);
        assert_eq!(result, small);
    }

    #[test]
    fn compact_bash_result_keeps_last_two() {
        let content = "step 1\nstep 2\nstep 3\n\nresult line 1\nresult line 2";
        let result = compact_tool_result("Bash", content);
        assert_eq!(result, "result line 1\nresult line 2");
    }

    #[test]
    fn compact_grep_result_keeps_first_three() {
        let content = "match 1\nmatch 2\nmatch 3\nmatch 4\nmatch 5";
        let result = compact_tool_result("Grep", &content);
        assert!(result.contains("match 1"));
        assert!(result.contains("match 3"));
        assert!(result.contains("(+2 more)"));
        assert!(!result.contains("match 4"));
    }

    #[test]
    fn compact_glob_result_shows_count() {
        let content = "file1.rs\nfile2.rs\nfile3.rs";
        let result = compact_tool_result("Glob", content);
        assert_eq!(result, "3 files");
    }

    #[test]
    fn compact_task_result_keeps_first_five() {
        let content = (1..=10).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        let result = compact_tool_result("Task", &content);
        assert!(result.contains("line 5"));
        assert!(result.contains("(+5 more lines)"));
        assert!(!result.contains("line 6"));
    }

    #[test]
    fn compact_unknown_tool_keeps_first_three() {
        let content = "a\nb\nc\nd\ne";
        let result = compact_tool_result("Unknown", content);
        assert!(result.contains("a\nb\nc"));
        assert!(result.contains("(+2 more)"));
    }

    #[test]
    fn compact_strips_system_reminder() {
        let content = "real output\n<system-reminder>hidden stuff</system-reminder>";
        let result = compact_tool_result("Bash", content);
        assert!(!result.contains("system-reminder"));
        assert!(result.contains("real output"));
    }

    // =====================================================================
    // Compaction — ToolCall.input
    // =====================================================================

    #[test]
    fn compact_write_input_summarizes_content() {
        let mut input = serde_json::json!({
            "file_path": "/src/main.rs",
            "content": "//! Main module\nfn main() {\n    println!(\"hello\");\n}\n"
        });
        compact_tool_input("Write", &mut input);
        let content = input.get("content").unwrap().as_str().unwrap();
        assert!(content.contains("(4 lines)"));
        assert!(content.contains("//! Main module"));
        // file_path preserved
        assert_eq!(input.get("file_path").unwrap().as_str().unwrap(), "/src/main.rs");
    }

    #[test]
    fn compact_edit_input_preserved() {
        let mut input = serde_json::json!({
            "file_path": "/src/lib.rs",
            "old_string": "fn old() {}",
            "new_string": "fn new() {}"
        });
        let original = input.clone();
        compact_tool_input("Edit", &mut input);
        assert_eq!(input, original);
    }

    #[test]
    fn compact_bash_input_strips_extras() {
        let mut input = serde_json::json!({
            "command": "cargo build",
            "timeout": 60000,
            "description": "Build the project"
        });
        compact_tool_input("Bash", &mut input);
        assert_eq!(input.get("command").unwrap().as_str().unwrap(), "cargo build");
        assert!(input.get("timeout").is_none());
        assert!(input.get("description").is_none());
    }

    #[test]
    fn compact_read_input_strips_extras() {
        let mut input = serde_json::json!({
            "file_path": "/src/main.rs",
            "offset": 100,
            "limit": 50
        });
        compact_tool_input("Read", &mut input);
        assert!(input.get("file_path").is_some());
        assert!(input.get("offset").is_none());
        assert!(input.get("limit").is_none());
    }

    #[test]
    fn compact_events_applies_to_from_parsed() {
        let mut parsed = sample_parsed();
        // Add a large Read result
        parsed.events.push(DisplayEvent::ToolResult {
            tool_use_id: "tc-big".into(),
            tool_name: "Read".into(),
            file_path: Some("/big.rs".into()),
            content: (1..=200).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n"),
            is_error: false,
        });

        let cached = CachedSession::from_parsed(&parsed, PathBuf::from("/x"), 1);

        // Original parsed events unchanged
        match &parsed.events.last().unwrap() {
            DisplayEvent::ToolResult { content, .. } => assert!(content.lines().count() == 200),
            _ => panic!("wrong variant"),
        }

        // Cached events compacted
        match &cached.events.last().unwrap() {
            DisplayEvent::ToolResult { content, .. } => {
                assert!(content.contains("(200 lines)"));
                assert!(content.lines().count() <= 3);
            }
            _ => panic!("wrong variant"),
        }
    }

    // =====================================================================
    // Compression — gzip round-trip and size reduction
    // =====================================================================

    #[test]
    fn compressed_cache_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let project_root = dir.path();
        let source = project_root.join("raw.jsonl");
        std::fs::write(&source, "fake data").unwrap();

        let parsed = sample_parsed();
        let cached = CachedSession::from_parsed(&parsed, source.clone(), 9);
        write_cache(project_root, "gz-test", &cached).unwrap();

        // File should be gzip (magic bytes 1f 8b)
        let raw = std::fs::read(cache_path(project_root, "gz-test")).unwrap();
        assert_eq!(raw[0], 0x1f);
        assert_eq!(raw[1], 0x8b);

        // Should read back correctly
        let restored = read_cache(project_root, "gz-test", &source, 9).unwrap();
        assert_eq!(restored.events.len(), cached.events.len());
        assert_eq!(restored.total_lines, 20);
    }

    #[test]
    fn compressed_smaller_than_json() {
        let dir = tempfile::tempdir().unwrap();
        let project_root = dir.path();

        // Build a session with repetitive content (compresses well)
        let mut parsed = sample_parsed();
        for i in 0..50 {
            parsed.events.push(DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text: format!("This is assistant response number {} with some repetitive content that compresses well.", i),
            });
        }
        let cached = CachedSession::from_parsed(&parsed, PathBuf::from("/x.jsonl"), 1);

        // Write compressed
        write_cache(project_root, "size-test", &cached).unwrap();
        let compressed_size = std::fs::metadata(cache_path(project_root, "size-test")).unwrap().len();

        // Compare to uncompressed JSON size
        let json_size = serde_json::to_vec(&cached).unwrap().len() as u64;

        // Gzip should be significantly smaller
        assert!(compressed_size < json_size, "compressed {} should be < json {}", compressed_size, json_size);
        // Typically 3-10x smaller for JSON
        assert!(compressed_size < json_size / 2, "compression ratio should be at least 2x: {} vs {}", compressed_size, json_size);
    }

    #[test]
    fn corrupted_gzip_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let project_root = dir.path();
        let sessions_dir = project_root.join(".azureal").join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        // Write garbage data
        std::fs::write(sessions_dir.join("bad.json.gz"), b"not gzip data").unwrap();
        assert!(read_cache_orphan(project_root, "bad").is_none());
    }

    // =====================================================================
    // CacheIndex — sequential naming
    // =====================================================================

    #[test]
    fn resolve_cache_name_assigns_claude_1_first() {
        let dir = tempfile::tempdir().unwrap();
        let name = resolve_cache_name(dir.path(), "uuid-aaa", Backend::Claude).unwrap();
        assert_eq!(name, "claude-1");
    }

    #[test]
    fn resolve_cache_name_assigns_codex_1_first() {
        let dir = tempfile::tempdir().unwrap();
        let name = resolve_cache_name(dir.path(), "uuid-bbb", Backend::Codex).unwrap();
        assert_eq!(name, "codex-1");
    }

    #[test]
    fn resolve_cache_name_increments_per_backend() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let c1 = resolve_cache_name(root, "uuid-1", Backend::Claude).unwrap();
        let c2 = resolve_cache_name(root, "uuid-2", Backend::Claude).unwrap();
        let x1 = resolve_cache_name(root, "uuid-3", Backend::Codex).unwrap();
        let c3 = resolve_cache_name(root, "uuid-4", Backend::Claude).unwrap();
        let x2 = resolve_cache_name(root, "uuid-5", Backend::Codex).unwrap();

        assert_eq!(c1, "claude-1");
        assert_eq!(c2, "claude-2");
        assert_eq!(x1, "codex-1");
        assert_eq!(c3, "claude-3");
        assert_eq!(x2, "codex-2");
    }

    #[test]
    fn resolve_cache_name_returns_existing_for_same_uuid() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let first = resolve_cache_name(root, "uuid-dup", Backend::Claude).unwrap();
        let second = resolve_cache_name(root, "uuid-dup", Backend::Claude).unwrap();

        assert_eq!(first, "claude-1");
        assert_eq!(second, "claude-1"); // same UUID → same name
    }

    #[test]
    fn lookup_cache_name_returns_none_for_unknown() {
        let dir = tempfile::tempdir().unwrap();
        assert!(lookup_cache_name(dir.path(), "unknown-uuid").is_none());
    }

    #[test]
    fn lookup_cache_name_returns_assigned_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        resolve_cache_name(root, "uuid-look", Backend::Codex).unwrap();
        let found = lookup_cache_name(root, "uuid-look");
        assert_eq!(found, Some("codex-1".to_string()));
    }

    #[test]
    fn next_number_empty_index() {
        let index = CacheIndex::default();
        assert_eq!(next_number(&index, "claude"), 1);
        assert_eq!(next_number(&index, "codex"), 1);
    }

    #[test]
    fn next_number_skips_non_matching_prefix() {
        let mut index = CacheIndex::default();
        index.map.insert("a".into(), "codex-5".into());
        index.map.insert("b".into(), "codex-3".into());
        // "claude" prefix should still start at 1
        assert_eq!(next_number(&index, "claude"), 1);
        assert_eq!(next_number(&index, "codex"), 6);
    }

    #[test]
    fn next_number_handles_gaps() {
        let mut index = CacheIndex::default();
        index.map.insert("a".into(), "claude-1".into());
        index.map.insert("b".into(), "claude-5".into());
        // Should be max+1, not fill gaps
        assert_eq!(next_number(&index, "claude"), 6);
    }

    #[test]
    fn index_persists_across_reads() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        resolve_cache_name(root, "uuid-persist", Backend::Claude).unwrap();

        // Read index from disk in a separate call
        let index = read_index(root);
        assert_eq!(index.map.get("uuid-persist").unwrap(), "claude-1");
    }

    #[test]
    fn index_handles_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let idx_path = index_path(root);
        std::fs::create_dir_all(idx_path.parent().unwrap()).unwrap();
        std::fs::write(&idx_path, b"not json {{{").unwrap();

        // Should return empty index, not panic
        let index = read_index(root);
        assert!(index.map.is_empty());

        // Should overwrite corrupt index
        let name = resolve_cache_name(root, "uuid-fix", Backend::Claude).unwrap();
        assert_eq!(name, "claude-1");
    }

    #[test]
    fn cache_files_use_sequential_names() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let name = resolve_cache_name(root, "uuid-file", Backend::Claude).unwrap();
        let parsed = sample_parsed();
        let cached = CachedSession::from_parsed(&parsed, PathBuf::from("/x.jsonl"), 1);
        write_cache(root, &name, &cached).unwrap();

        // File should be at claude-1.json.gz
        let path = cache_path(root, "claude-1");
        assert!(path.exists());
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "claude-1.json.gz");

        // Should read back via the cache name
        let restored = read_cache_orphan(root, "claude-1");
        assert!(restored.is_some());
    }

    #[test]
    fn mixed_backends_independent_counters() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Interleave Claude and Codex sessions
        assert_eq!(resolve_cache_name(root, "c1", Backend::Claude).unwrap(), "claude-1");
        assert_eq!(resolve_cache_name(root, "x1", Backend::Codex).unwrap(), "codex-1");
        assert_eq!(resolve_cache_name(root, "c2", Backend::Claude).unwrap(), "claude-2");
        assert_eq!(resolve_cache_name(root, "x2", Backend::Codex).unwrap(), "codex-2");
        assert_eq!(resolve_cache_name(root, "x3", Backend::Codex).unwrap(), "codex-3");
        assert_eq!(resolve_cache_name(root, "c3", Backend::Claude).unwrap(), "claude-3");
    }

    // =====================================================================
    // Migration — legacy UUID-named .json → .json.gz
    // =====================================================================

    /// Write an old-style uncompressed UUID.json cache file (for migration tests)
    fn write_legacy_cache(project_root: &Path, uuid: &str, cached: &CachedSession) {
        let dir = project_root.join(".azureal").join("sessions");
        fs::create_dir_all(&dir).unwrap();
        let json = serde_json::to_vec(cached).unwrap();
        fs::write(dir.join(format!("{uuid}.json")), &json).unwrap();
    }

    #[test]
    fn migrate_converts_legacy_json_to_gz() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let parsed = sample_parsed();
        let cached = CachedSession::from_parsed(&parsed, PathBuf::from("/raw.jsonl"), 100);

        // Write old-style UUID.json
        write_legacy_cache(root, "aaaa-bbbb-cccc", &cached);
        let legacy = root.join(".azureal/sessions/aaaa-bbbb-cccc.json");
        assert!(legacy.exists());

        // Migrate
        migrate_legacy_caches(root, Backend::Claude);

        // Old file removed
        assert!(!legacy.exists());

        // New .json.gz created
        let gz = cache_path(root, "claude-1");
        assert!(gz.exists());

        // Index updated
        assert_eq!(lookup_cache_name(root, "aaaa-bbbb-cccc"), Some("claude-1".into()));

        // Data round-trips
        let restored = read_cache_orphan(root, "claude-1").unwrap();
        assert_eq!(restored.events.len(), cached.events.len());
    }

    #[test]
    fn migrate_noop_when_no_legacy_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // No sessions dir at all
        migrate_legacy_caches(root, Backend::Claude);

        // Create dir but only put index.json in it
        let sessions_dir = root.join(".azureal/sessions");
        fs::create_dir_all(&sessions_dir).unwrap();
        fs::write(sessions_dir.join("index.json"), b"{}").unwrap();
        migrate_legacy_caches(root, Backend::Claude);

        // Index unchanged
        let idx = read_index(root);
        assert!(idx.map.is_empty());
    }

    #[test]
    fn migrate_handles_multiple_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let parsed = sample_parsed();

        // Two Claude sessions
        let c1 = CachedSession::from_parsed(&parsed, PathBuf::from("/a.jsonl"), 10);
        let c2 = CachedSession::from_parsed(&parsed, PathBuf::from("/b.jsonl"), 20);
        write_legacy_cache(root, "uuid-1", &c1);
        write_legacy_cache(root, "uuid-2", &c2);

        migrate_legacy_caches(root, Backend::Claude);

        // Both migrated
        assert!(lookup_cache_name(root, "uuid-1").is_some());
        assert!(lookup_cache_name(root, "uuid-2").is_some());

        // Sequential names assigned (order may vary, just check both exist)
        let n1 = lookup_cache_name(root, "uuid-1").unwrap();
        let n2 = lookup_cache_name(root, "uuid-2").unwrap();
        assert!(n1.starts_with("claude-"));
        assert!(n2.starts_with("claude-"));
        assert_ne!(n1, n2);

        // Old files gone
        assert!(!root.join(".azureal/sessions/uuid-1.json").exists());
        assert!(!root.join(".azureal/sessions/uuid-2.json").exists());
    }

    #[test]
    fn migrate_detects_codex_backend_from_source_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let parsed = sample_parsed();
        let cached = CachedSession::from_parsed(
            &parsed,
            PathBuf::from("/home/.codex/sessions/abc.jsonl"),
            50,
        );
        write_legacy_cache(root, "codex-uuid", &cached);

        migrate_legacy_caches(root, Backend::Claude); // default is Claude

        // Should detect codex from source_path
        let name = lookup_cache_name(root, "codex-uuid").unwrap();
        assert!(name.starts_with("codex-"), "expected codex prefix, got {}", name);
    }

    #[test]
    fn migrate_skips_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let sessions_dir = root.join(".azureal/sessions");
        fs::create_dir_all(&sessions_dir).unwrap();

        // Write corrupt JSON
        fs::write(sessions_dir.join("bad-uuid.json"), b"not valid json {{{").unwrap();

        // Also write a valid one
        let parsed = sample_parsed();
        let cached = CachedSession::from_parsed(&parsed, PathBuf::from("/raw.jsonl"), 10);
        write_legacy_cache(root, "good-uuid", &cached);

        migrate_legacy_caches(root, Backend::Claude);

        // Good one migrated
        assert!(lookup_cache_name(root, "good-uuid").is_some());
        // Bad one left in place (not deleted since we couldn't read it)
        assert!(sessions_dir.join("bad-uuid.json").exists());
    }

    #[test]
    fn migrate_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let parsed = sample_parsed();
        let cached = CachedSession::from_parsed(&parsed, PathBuf::from("/raw.jsonl"), 10);
        write_legacy_cache(root, "uuid-idem", &cached);

        migrate_legacy_caches(root, Backend::Claude);
        let name1 = lookup_cache_name(root, "uuid-idem").unwrap();

        // Second call is a no-op (old file already removed)
        migrate_legacy_caches(root, Backend::Claude);
        let name2 = lookup_cache_name(root, "uuid-idem").unwrap();
        assert_eq!(name1, name2);
    }

    #[test]
    fn migrate_ignores_index_json_and_gz_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let sessions_dir = root.join(".azureal/sessions");
        fs::create_dir_all(&sessions_dir).unwrap();

        // Pre-existing index.json
        fs::write(sessions_dir.join("index.json"), b"{\"existing\": \"claude-1\"}").unwrap();
        // Pre-existing .json.gz
        fs::write(sessions_dir.join("claude-1.json.gz"), b"compressed").unwrap();

        migrate_legacy_caches(root, Backend::Claude);

        // Neither touched — index preserved
        let idx = read_index(root);
        assert_eq!(idx.map.get("existing").unwrap(), "claude-1");
        // .gz file still there
        assert!(sessions_dir.join("claude-1.json.gz").exists());
    }

    // =====================================================================
    // Integration — real session file through full pipeline
    // =====================================================================

    #[test]
    fn real_session_full_pipeline() {
        // This session's JSONL file (the one you're reading right now)
        let jsonl = std::path::Path::new(
            "/Users/macbookpro/.claude/projects/-Users-macbookpro-AZUREAL-worktrees-codexsupport/fb57e64f-917c-4cc6-bc11-5ca999597fa4.jsonl"
        );
        if !jsonl.exists() {
            // Skip in CI or when session file is missing
            return;
        }

        let jsonl_size = std::fs::metadata(jsonl).unwrap().len();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Step 1: Resolve sequential name
        let session_id = "fb57e64f-917c-4cc6-bc11-5ca999597fa4";
        let cache_name = resolve_cache_name(root, session_id, Backend::Claude).unwrap();
        assert_eq!(cache_name, "claude-1");

        // Step 2: Parse the real JSONL
        let parsed = crate::app::session_parser::parse_session_file(jsonl);
        assert!(!parsed.events.is_empty(), "parsed 0 events from 19MB JSONL");

        // Step 3: Build cached session with compaction
        let cached = CachedSession::from_parsed(&parsed, jsonl.to_path_buf(), jsonl_size);

        // Step 4: Write compressed cache
        write_cache(root, &cache_name, &cached).unwrap();

        // Step 5: Verify file exists with correct name
        let gz_path = cache_path(root, "claude-1");
        assert!(gz_path.exists());
        assert_eq!(gz_path.file_name().unwrap().to_str().unwrap(), "claude-1.json.gz");
        let gz_size = std::fs::metadata(&gz_path).unwrap().len();

        // Step 6: Verify index.json was created
        let idx = read_index(root);
        assert_eq!(idx.map.get(session_id).unwrap(), "claude-1");

        // Step 7: Read back and verify
        let restored = read_cache(root, "claude-1", jsonl, jsonl_size).unwrap();
        assert_eq!(restored.events.len(), cached.events.len());
        assert_eq!(restored.total_lines, cached.total_lines);
        assert_eq!(restored.source_size, jsonl_size);

        // Step 8: Lookup works
        assert_eq!(lookup_cache_name(root, session_id), Some("claude-1".to_string()));

        // Step 9: Second session gets claude-2
        let name2 = resolve_cache_name(root, "other-uuid", Backend::Claude).unwrap();
        assert_eq!(name2, "claude-2");

        // Print size comparison
        eprintln!("  JSONL:       {:>10} bytes", jsonl_size);
        eprintln!("  Compressed:  {:>10} bytes", gz_size);
        eprintln!("  Ratio:       {:>10.1}x reduction", jsonl_size as f64 / gz_size as f64);
        eprintln!("  Events:      {:>10}", cached.events.len());
        eprintln!("  Cache name:  {}", cache_name);
    }
}
