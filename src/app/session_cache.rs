//! Session cache — intermediate translation layer
//!
//! Writes parsed session data to `.azureal/sessions/<session-id>.json` so the
//! TUI reads from a unified format instead of backend-specific JSONL files.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::session_parser::ParsedSession;
use crate::events::DisplayEvent;

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
    pub pending_tools: HashSet<String>,
    pub failed_tools: HashSet<String>,
    pub session_tokens: Option<(u64, u64)>,
    pub context_window: Option<u64>,
    pub model: Option<String>,
    pub total_lines: usize,
    pub parse_errors: usize,
    pub assistant_total: usize,
    pub assistant_no_message: usize,
    pub assistant_no_content_arr: usize,
    pub assistant_text_blocks: usize,
    pub awaiting_plan_approval: bool,
}

impl CachedSession {
    /// Build from a freshly parsed session (clones event data for the cache)
    pub fn from_parsed(parsed: &ParsedSession, source_path: PathBuf, source_size: u64) -> Self {
        Self {
            source_path,
            source_size,
            parse_offset: parsed.end_offset,
            events: parsed.events.clone(),
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

/// Returns `.azureal/sessions/<session-id>.json`
pub fn cache_path(project_root: &Path, session_id: &str) -> PathBuf {
    project_root.join(".azureal").join("sessions").join(format!("{session_id}.json"))
}

/// Write parsed session to cache (atomic: write tmp then rename)
pub fn write_cache(project_root: &Path, session_id: &str, cached: &CachedSession) -> io::Result<()> {
    let path = cache_path(project_root, session_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_vec(cached)?;
    fs::write(&tmp, &data)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// Read cache if valid — source_path and source_size must both match
pub fn read_cache(
    project_root: &Path,
    session_id: &str,
    source_path: &Path,
    current_source_size: u64,
) -> Option<CachedSession> {
    let path = cache_path(project_root, session_id);
    let data = fs::read(&path).ok()?;
    let cached: CachedSession = serde_json::from_slice(&data).ok()?;
    if cached.source_path != source_path || cached.source_size != current_source_size {
        return None;
    }
    Some(cached)
}

/// Read cache without source validation (for when raw file is missing)
pub fn read_cache_orphan(project_root: &Path, session_id: &str) -> Option<CachedSession> {
    let path = cache_path(project_root, session_id);
    let data = fs::read(&path).ok()?;
    serde_json::from_slice(&data).ok()
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
        assert_eq!(path, PathBuf::from("/projects/myapp/.azureal/sessions/abc-123.json"));
    }

    #[test]
    fn cache_path_uuid_with_special_chars() {
        let path = cache_path(Path::new("/proj"), "a1b2c3d4-e5f6-7890-abcd-ef1234567890");
        assert!(path.to_str().unwrap().ends_with("a1b2c3d4-e5f6-7890-abcd-ef1234567890.json"));
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
}
