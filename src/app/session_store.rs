//! SQLite-backed session store (.azs file)
//!
//! Replaces the gzip-compressed JSON cache system with a single SQLite database
//! at `.azureal/sessions.azs`. Sessions use S-numbering (S1, S2, S3...) and are
//! backend-agnostic — a single session can span Claude and Codex prompts.
//!
//! The `.azs` extension discourages users from trying to open or tamper with the
//! binary file directly. Internally it is a standard SQLite database with WAL mode.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::events::DisplayEvent;

/// Compaction threshold: when characters since last compaction exceed this,
/// a background agent summarizes the conversation. ~100K tokens at 4 chars/token.
pub const COMPACTION_THRESHOLD: usize = 400_000;

// =========================================================================
// Schema
// =========================================================================

const SCHEMA_V1: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    id       INTEGER PRIMARY KEY,
    name     TEXT NOT NULL DEFAULT '',
    worktree TEXT NOT NULL DEFAULT '',
    created  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    data       TEXT NOT NULL,
    char_len   INTEGER NOT NULL DEFAULT 0,
    UNIQUE(session_id, seq)
);
CREATE INDEX IF NOT EXISTS idx_events_session ON events(session_id, seq);

CREATE TABLE IF NOT EXISTS compactions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    after_seq  INTEGER NOT NULL,
    summary    TEXT NOT NULL,
    created    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_compactions_session ON compactions(session_id);

CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT OR IGNORE INTO meta(key, value) VALUES ('schema_version', '1');
";

// =========================================================================
// Public types
// =========================================================================

/// Summary info for a session (used in session list)
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: i64,
    pub name: String,
    pub worktree: String,
    pub created: String,
    pub event_count: usize,
    pub message_count: usize,
}

/// Compaction summary record
#[derive(Debug, Clone)]
pub struct CompactionInfo {
    pub after_seq: i64,
    pub summary: String,
}

// =========================================================================
// SessionStore
// =========================================================================

/// SQLite-backed session store wrapping a single `.azs` file.
pub struct SessionStore {
    conn: Connection,
}

impl SessionStore {
    /// Open (or create) the session store at `<project_root>/.azureal/sessions.azs`.
    pub fn open(project_root: &Path) -> anyhow::Result<Self> {
        let path = Self::db_path(project_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;\
             PRAGMA synchronous = NORMAL;\
             PRAGMA foreign_keys = ON;"
        )?;
        conn.execute_batch(SCHEMA_V1)?;
        Ok(Self { conn })
    }

    /// Open an in-memory store (for tests).
    #[cfg(test)]
    pub fn open_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA_V1)?;
        Ok(Self { conn })
    }

    /// Path to the `.azs` database file.
    pub fn db_path(project_root: &Path) -> PathBuf {
        project_root.join(".azureal").join("sessions.azs")
    }

    // =====================================================================
    // Session CRUD
    // =====================================================================

    /// Create a new session for the given worktree branch. Returns the S-number (id).
    pub fn create_session(&self, worktree: &str) -> anyhow::Result<i64> {
        self.conn.execute(
            "INSERT INTO sessions(worktree) VALUES (?1)",
            params![worktree],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Rename a session (set user-assigned display name).
    pub fn rename_session(&self, id: i64, name: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE sessions SET name = ?1 WHERE id = ?2",
            params![name, id],
        )?;
        Ok(())
    }

    /// Delete a session and all its events/compactions (CASCADE).
    pub fn delete_session(&self, id: i64) -> anyhow::Result<()> {
        self.conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// List all sessions, optionally filtered by worktree.
    pub fn list_sessions(&self, worktree: Option<&str>) -> anyhow::Result<Vec<SessionInfo>> {
        let (sql, filter): (&str, Box<dyn rusqlite::ToSql>) = match worktree {
            Some(wt) => (
                "SELECT s.id, s.name, s.worktree, s.created, \
                    COALESCE(e.cnt, 0), COALESCE(m.cnt, 0) \
                 FROM sessions s \
                 LEFT JOIN (SELECT session_id, COUNT(*) as cnt FROM events GROUP BY session_id) e \
                    ON e.session_id = s.id \
                 LEFT JOIN (SELECT session_id, COUNT(*) as cnt FROM events \
                    WHERE kind IN ('UserMessage','AssistantText') GROUP BY session_id) m \
                    ON m.session_id = s.id \
                 WHERE s.worktree = ?1 \
                 ORDER BY s.id",
                Box::new(wt.to_string()),
            ),
            None => (
                "SELECT s.id, s.name, s.worktree, s.created, \
                    COALESCE(e.cnt, 0), COALESCE(m.cnt, 0) \
                 FROM sessions s \
                 LEFT JOIN (SELECT session_id, COUNT(*) as cnt FROM events GROUP BY session_id) e \
                    ON e.session_id = s.id \
                 LEFT JOIN (SELECT session_id, COUNT(*) as cnt FROM events \
                    WHERE kind IN ('UserMessage','AssistantText') GROUP BY session_id) m \
                    ON m.session_id = s.id \
                 ORDER BY s.id",
                Box::new(""),
            ),
        };

        let mut stmt = if worktree.is_some() {
            let mut s = self.conn.prepare(sql)?;
            let rows = s.query_map(params![&*filter], |row| {
                Ok(SessionInfo {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    worktree: row.get(2)?,
                    created: row.get(3)?,
                    event_count: row.get::<_, i64>(4)? as usize,
                    message_count: row.get::<_, i64>(5)? as usize,
                })
            })?.collect::<Result<Vec<_>, _>>()?;
            return Ok(rows);
        } else {
            self.conn.prepare(sql)?
        };

        let rows = stmt.query_map([], |row| {
            Ok(SessionInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                worktree: row.get(2)?,
                created: row.get(3)?,
                event_count: row.get::<_, i64>(4)? as usize,
                message_count: row.get::<_, i64>(5)? as usize,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Load all session display names: id → name (or "S{id}" if unnamed).
    pub fn load_all_session_names(&self) -> HashMap<i64, String> {
        let mut stmt = match self.conn.prepare("SELECT id, name FROM sessions") {
            Ok(s) => s,
            Err(_) => return HashMap::new(),
        };
        let iter = match stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            Ok((id, name))
        }) {
            Ok(it) => it,
            Err(_) => return HashMap::new(),
        };
        iter.filter_map(|r| r.ok())
            .map(|(id, name)| {
                let display = if name.is_empty() { format!("S{}", id) } else { name };
                (id, display)
            })
            .collect()
    }

    // =====================================================================
    // Events
    // =====================================================================

    /// Next sequence number for a session's events.
    fn next_seq(&self, session_id: i64) -> anyhow::Result<i64> {
        let max: Option<i64> = self.conn.query_row(
            "SELECT MAX(seq) FROM events WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(max.unwrap_or(0) + 1)
    }

    /// Append display events to a session. Returns the number of events inserted.
    /// Skips `Filtered` events.
    pub fn append_events(&self, session_id: i64, events: &[DisplayEvent]) -> anyhow::Result<usize> {
        let mut seq = self.next_seq(session_id)?;
        let tx = self.conn.unchecked_transaction()?;
        let mut stmt = tx.prepare(
            "INSERT INTO events(session_id, seq, kind, data, char_len) VALUES (?1, ?2, ?3, ?4, ?5)"
        )?;
        let mut count = 0usize;
        for event in events {
            if matches!(event, DisplayEvent::Filtered) { continue; }
            let compacted = compact_event(event);
            let kind = event_kind(&compacted);
            let data = serde_json::to_string(&compacted).unwrap_or_default();
            let char_len = event_char_len(event) as i64;
            stmt.execute(params![session_id, seq, kind, data, char_len])?;
            seq += 1;
            count += 1;
        }
        drop(stmt);
        tx.commit()?;
        Ok(count)
    }

    /// Load all events for a session in order.
    pub fn load_events(&self, session_id: i64) -> anyhow::Result<Vec<DisplayEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT data FROM events WHERE session_id = ?1 ORDER BY seq"
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            let json: String = row.get(0)?;
            Ok(json)
        })?.collect::<Result<Vec<_>, _>>()?;

        let mut events = Vec::with_capacity(rows.len());
        for json in rows {
            if let Ok(ev) = serde_json::from_str::<DisplayEvent>(&json) {
                events.push(ev);
            }
        }
        Ok(events)
    }

    /// Load events from a specific sequence position onward (for context building).
    pub fn load_events_from(&self, session_id: i64, from_seq: i64) -> anyhow::Result<Vec<DisplayEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT data FROM events WHERE session_id = ?1 AND seq >= ?2 ORDER BY seq"
        )?;
        let rows = stmt.query_map(params![session_id, from_seq], |row| {
            let json: String = row.get(0)?;
            Ok(json)
        })?.collect::<Result<Vec<_>, _>>()?;

        let mut events = Vec::with_capacity(rows.len());
        for json in rows {
            if let Ok(ev) = serde_json::from_str::<DisplayEvent>(&json) {
                events.push(ev);
            }
        }
        Ok(events)
    }

    /// Count events, optionally filtered by kind(s).
    pub fn count_events(&self, session_id: i64, kinds: Option<&[&str]>) -> anyhow::Result<usize> {
        let count: i64 = match kinds {
            Some(ks) if !ks.is_empty() => {
                let placeholders: Vec<String> = ks.iter().enumerate()
                    .map(|(i, _)| format!("?{}", i + 2))
                    .collect();
                let sql = format!(
                    "SELECT COUNT(*) FROM events WHERE session_id = ?1 AND kind IN ({})",
                    placeholders.join(",")
                );
                let mut stmt = self.conn.prepare(&sql)?;
                let mut idx = 1;
                stmt.raw_bind_parameter(idx, session_id)?;
                for k in ks {
                    idx += 1;
                    stmt.raw_bind_parameter(idx, *k)?;
                }
                let mut rows = stmt.raw_query();
                rows.next()?.map(|r| r.get(0)).transpose()?.unwrap_or(0)
            }
            _ => {
                self.conn.query_row(
                    "SELECT COUNT(*) FROM events WHERE session_id = ?1",
                    params![session_id],
                    |row| row.get(0),
                )?
            }
        };
        Ok(count as usize)
    }

    /// Message count (UserMessage + AssistantText only).
    pub fn message_count(&self, session_id: i64) -> anyhow::Result<usize> {
        self.count_events(session_id, Some(&["UserMessage", "AssistantText"]))
    }

    // =====================================================================
    // Compaction
    // =====================================================================

    /// Total character count of events since the last compaction (or all events if none).
    pub fn total_chars_since_compaction(&self, session_id: i64) -> anyhow::Result<usize> {
        let after_seq = self.latest_compaction(session_id)?
            .map(|c| c.after_seq)
            .unwrap_or(0);
        let sum: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(char_len), 0) FROM events WHERE session_id = ?1 AND seq > ?2",
            params![session_id, after_seq],
            |row| row.get(0),
        )?;
        Ok(sum as usize)
    }

    /// Store a compaction summary.
    pub fn store_compaction(&self, session_id: i64, after_seq: i64, summary: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO compactions(session_id, after_seq, summary) VALUES (?1, ?2, ?3)",
            params![session_id, after_seq, summary],
        )?;
        Ok(())
    }

    /// Get the latest compaction for a session.
    pub fn latest_compaction(&self, session_id: i64) -> anyhow::Result<Option<CompactionInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT after_seq, summary FROM compactions WHERE session_id = ?1 ORDER BY after_seq DESC LIMIT 1"
        )?;
        let mut rows = stmt.query_map(params![session_id], |row| {
            Ok(CompactionInfo {
                after_seq: row.get(0)?,
                summary: row.get(1)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    /// Maximum event sequence number for a session.
    pub fn max_seq(&self, session_id: i64) -> anyhow::Result<i64> {
        let max: Option<i64> = self.conn.query_row(
            "SELECT MAX(seq) FROM events WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(max.unwrap_or(0))
    }

    /// Build the context payload for context injection.
    /// Returns events from the last compaction onward, plus the compaction summary if any.
    pub fn build_context(&self, session_id: i64) -> anyhow::Result<Option<ContextPayload>> {
        let compaction = self.latest_compaction(session_id)?;
        let from_seq = compaction.as_ref().map(|c| c.after_seq + 1).unwrap_or(1);
        let events = self.load_events_from(session_id, from_seq)?;
        if events.is_empty() && compaction.is_none() {
            return Ok(None);
        }
        Ok(Some(ContextPayload {
            compaction_summary: compaction.map(|c| c.summary),
            events,
        }))
    }
}

/// Context payload for injection into prompts.
#[derive(Debug, Clone)]
pub struct ContextPayload {
    pub compaction_summary: Option<String>,
    pub events: Vec<DisplayEvent>,
}

// =========================================================================
// Event helpers
// =========================================================================

/// Extract the variant name as a string for the `kind` column.
fn event_kind(event: &DisplayEvent) -> &'static str {
    match event {
        DisplayEvent::Init { .. } => "Init",
        DisplayEvent::Hook { .. } => "Hook",
        DisplayEvent::UserMessage { .. } => "UserMessage",
        DisplayEvent::Command { .. } => "Command",
        DisplayEvent::Compacting => "Compacting",
        DisplayEvent::Compacted => "Compacted",
        DisplayEvent::MayBeCompacting => "MayBeCompacting",
        DisplayEvent::Plan { .. } => "Plan",
        DisplayEvent::AssistantText { .. } => "AssistantText",
        DisplayEvent::ToolCall { .. } => "ToolCall",
        DisplayEvent::ToolResult { .. } => "ToolResult",
        DisplayEvent::Complete { .. } => "Complete",
        DisplayEvent::Filtered => "Filtered",
    }
}

/// Compact a DisplayEvent for storage, minimizing JSON size while preserving
/// everything the render pipeline needs. Mirrors the display rules in
/// `render_tools.rs`: ToolResult content is truncated to what's actually shown,
/// ToolCall input is stripped to only the key field `extract_tool_param` reads.
fn compact_event(event: &DisplayEvent) -> DisplayEvent {
    match event {
        DisplayEvent::ToolResult { tool_use_id, tool_name, file_path, content, is_error } => {
            // Strip system-reminder blocks first
            let content = if let Some(start) = content.find("<system-reminder>") {
                &content[..start]
            } else {
                content.as_str()
            }.trim_end();

            let lines: Vec<&str> = content.lines().collect();
            let compacted = match tool_name.as_str() {
                "Read" | "read" => {
                    // First + last non-empty line
                    if lines.len() <= 2 {
                        content.to_string()
                    } else {
                        let first = lines[0];
                        let last = lines.iter().rev().find(|l| !l.trim().is_empty()).unwrap_or(&"");
                        format!("{}\n  ({} lines)\n{}", first, lines.len(), last)
                    }
                }
                "Bash" | "bash" => {
                    // Last 2 non-empty lines
                    let non_empty: Vec<&str> = lines.iter().filter(|l| !l.trim().is_empty()).copied().collect();
                    non_empty.iter().rev().take(2).rev().copied().collect::<Vec<_>>().join("\n")
                }
                "Grep" | "grep" => {
                    // First 3 lines
                    if lines.len() <= 3 {
                        content.to_string()
                    } else {
                        let mut s: String = lines[..3].join("\n");
                        s.push_str(&format!("\n  (+{} more)", lines.len() - 3));
                        s
                    }
                }
                "Glob" | "glob" => {
                    // Just file count
                    format!("{} files", lines.len())
                }
                "Task" | "task" => {
                    // First 5 lines
                    if lines.len() <= 5 {
                        content.to_string()
                    } else {
                        let mut s: String = lines[..5].join("\n");
                        s.push_str(&format!("\n  (+{} more lines)", lines.len() - 5));
                        s
                    }
                }
                _ => {
                    // Default: first 3 lines
                    if lines.len() <= 3 {
                        content.to_string()
                    } else {
                        let mut s: String = lines[..3].join("\n");
                        s.push_str(&format!("\n  (+{} more)", lines.len() - 3));
                        s
                    }
                }
            };

            DisplayEvent::ToolResult {
                tool_use_id: tool_use_id.clone(),
                tool_name: tool_name.clone(),
                file_path: file_path.clone(),
                content: compacted,
                is_error: *is_error,
            }
        }
        DisplayEvent::ToolCall { _uuid, tool_use_id, tool_name, file_path, input } => {
            // Strip input to only the key field the render pipeline reads.
            // Edit is kept fully (needed for inline diff rendering).
            let compacted_input = match tool_name.as_str() {
                "Edit" | "edit" => input.clone(),
                "Write" | "write" => {
                    // Replace content with line count + purpose line summary
                    let mut obj = serde_json::Map::new();
                    if let Some(fp) = input.get("file_path").or_else(|| input.get("path")) {
                        obj.insert("file_path".into(), fp.clone());
                    }
                    if let Some(content) = input.get("content").and_then(|v| v.as_str()) {
                        let content_lines: Vec<&str> = content.lines().collect();
                        let purpose = content_lines.iter()
                            .find(|l| {
                                let t = l.trim();
                                t.starts_with("//") || t.starts_with('#') ||
                                t.starts_with("/*") || t.starts_with("\"\"\"") ||
                                t.starts_with("///") || t.starts_with("//!")
                            })
                            .or(content_lines.first()).copied()
                            .unwrap_or("");
                        obj.insert("_lines".into(), serde_json::json!(content_lines.len()));
                        if !purpose.is_empty() {
                            obj.insert("_purpose".into(), serde_json::json!(purpose.trim()));
                        }
                    }
                    serde_json::Value::Object(obj)
                }
                _ => {
                    // Keep only the key field extract_tool_param reads
                    let key_fields: &[&str] = match tool_name.as_str() {
                        "Read" | "read" => &["file_path", "path"],
                        "Bash" | "bash" => &["command"],
                        "Glob" | "glob" | "Grep" | "grep" => &["pattern"],
                        "WebFetch" | "webfetch" => &["url"],
                        "WebSearch" | "websearch" => &["query"],
                        "Task" | "task" => &["subagent_type", "description"],
                        "LSP" | "lsp" => &["operation", "filePath"],
                        _ => &["file_path", "path", "command", "query", "pattern"],
                    };
                    let mut obj = serde_json::Map::new();
                    for &k in key_fields {
                        if let Some(v) = input.get(k) {
                            obj.insert(k.into(), v.clone());
                        }
                    }
                    serde_json::Value::Object(obj)
                }
            };

            DisplayEvent::ToolCall {
                _uuid: String::new(),
                tool_use_id: tool_use_id.clone(),
                tool_name: tool_name.clone(),
                file_path: file_path.clone(),
                input: compacted_input,
            }
        }
        // All other variants pass through unchanged
        _ => event.clone(),
    }
}

/// Estimate the displayable character count of an event (for compaction threshold).
fn event_char_len(event: &DisplayEvent) -> usize {
    match event {
        DisplayEvent::UserMessage { content, .. } => content.len(),
        DisplayEvent::AssistantText { text, .. } => text.len(),
        DisplayEvent::ToolCall { tool_name, input, .. } => {
            tool_name.len() + input.to_string().len()
        }
        DisplayEvent::ToolResult { content, .. } => content.len(),
        DisplayEvent::Plan { content, .. } => content.len(),
        DisplayEvent::Hook { output, .. } => output.len(),
        DisplayEvent::Command { name } => name.len(),
        _ => 0,
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_events() -> Vec<DisplayEvent> {
        vec![
            DisplayEvent::UserMessage {
                _uuid: String::new(),
                content: "Hello".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text: "Hi there!".into(),
            },
            DisplayEvent::ToolCall {
                _uuid: String::new(),
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
        ]
    }

    // ── open / schema ──

    #[test]
    fn open_memory_creates_tables() {
        let store = SessionStore::open_memory().unwrap();
        let version: String = store.conn.query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(version, "1");
    }

    #[test]
    fn open_memory_idempotent() {
        let store = SessionStore::open_memory().unwrap();
        store.conn.execute_batch(SCHEMA_V1).unwrap();
        let version: String = store.conn.query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(version, "1");
    }

    // ── create_session ──

    #[test]
    fn create_session_returns_sequential_ids() {
        let store = SessionStore::open_memory().unwrap();
        let s1 = store.create_session("main").unwrap();
        let s2 = store.create_session("main").unwrap();
        let s3 = store.create_session("feat-a").unwrap();
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert_eq!(s3, 3);
    }

    #[test]
    fn create_session_default_name_empty() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        let name: String = store.conn.query_row(
            "SELECT name FROM sessions WHERE id = ?1", params![id], |r| r.get(0),
        ).unwrap();
        assert!(name.is_empty());
    }

    // ── rename_session ──

    #[test]
    fn rename_session_sets_name() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.rename_session(id, "Feature Work").unwrap();
        let name: String = store.conn.query_row(
            "SELECT name FROM sessions WHERE id = ?1", params![id], |r| r.get(0),
        ).unwrap();
        assert_eq!(name, "Feature Work");
    }

    #[test]
    fn rename_nonexistent_session_ok() {
        let store = SessionStore::open_memory().unwrap();
        store.rename_session(999, "nope").unwrap();
    }

    // ── delete_session ──

    #[test]
    fn delete_session_removes_row() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.delete_session(id).unwrap();
        let count: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE id = ?1", params![id], |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn delete_session_cascades_events() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &sample_events()).unwrap();
        store.delete_session(id).unwrap();
        let count: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM events WHERE session_id = ?1", params![id], |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn delete_session_cascades_compactions() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.store_compaction(id, 5, "summary").unwrap();
        store.delete_session(id).unwrap();
        let count: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM compactions WHERE session_id = ?1", params![id], |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 0);
    }

    // ── list_sessions ──

    #[test]
    fn list_sessions_all() {
        let store = SessionStore::open_memory().unwrap();
        store.create_session("main").unwrap();
        store.create_session("feat-a").unwrap();
        let list = store.list_sessions(None).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, 1);
        assert_eq!(list[1].id, 2);
    }

    #[test]
    fn list_sessions_filtered_by_worktree() {
        let store = SessionStore::open_memory().unwrap();
        store.create_session("main").unwrap();
        store.create_session("feat-a").unwrap();
        store.create_session("main").unwrap();
        let list = store.list_sessions(Some("main")).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].worktree, "main");
    }

    #[test]
    fn list_sessions_includes_counts() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &sample_events()).unwrap();
        let list = store.list_sessions(None).unwrap();
        assert_eq!(list[0].event_count, 4);
        assert_eq!(list[0].message_count, 2); // UserMessage + AssistantText
    }

    #[test]
    fn list_sessions_empty_session_zero_counts() {
        let store = SessionStore::open_memory().unwrap();
        store.create_session("main").unwrap();
        let list = store.list_sessions(None).unwrap();
        assert_eq!(list[0].event_count, 0);
        assert_eq!(list[0].message_count, 0);
    }

    // ── append_events / load_events round-trip ──

    #[test]
    fn append_and_load_round_trip() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        let events = sample_events();
        let count = store.append_events(id, &events).unwrap();
        assert_eq!(count, 4);
        let loaded = store.load_events(id).unwrap();
        assert_eq!(loaded.len(), 4);
    }

    #[test]
    fn append_skips_filtered() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        let events = vec![
            DisplayEvent::UserMessage { _uuid: String::new(), content: "hi".into() },
            DisplayEvent::Filtered,
            DisplayEvent::AssistantText { _uuid: String::new(), _message_id: String::new(), text: "hello".into() },
        ];
        let count = store.append_events(id, &events).unwrap();
        assert_eq!(count, 2);
        let loaded = store.load_events(id).unwrap();
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn append_preserves_order() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &sample_events()).unwrap();
        let loaded = store.load_events(id).unwrap();
        assert!(matches!(loaded[0], DisplayEvent::UserMessage { .. }));
        assert!(matches!(loaded[1], DisplayEvent::AssistantText { .. }));
        assert!(matches!(loaded[2], DisplayEvent::ToolCall { .. }));
        assert!(matches!(loaded[3], DisplayEvent::ToolResult { .. }));
    }

    #[test]
    fn append_incremental_continues_sequence() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &[
            DisplayEvent::UserMessage { _uuid: String::new(), content: "first".into() },
        ]).unwrap();
        store.append_events(id, &[
            DisplayEvent::AssistantText { _uuid: String::new(), _message_id: String::new(), text: "second".into() },
        ]).unwrap();
        let loaded = store.load_events(id).unwrap();
        assert_eq!(loaded.len(), 2);
        match &loaded[0] {
            DisplayEvent::UserMessage { content, .. } => assert_eq!(content, "first"),
            _ => panic!("wrong variant"),
        }
        match &loaded[1] {
            DisplayEvent::AssistantText { text, .. } => assert_eq!(text, "second"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn append_empty_events_ok() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        let count = store.append_events(id, &[]).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn load_events_empty_session() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        let loaded = store.load_events(id).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn load_events_nonexistent_session() {
        let store = SessionStore::open_memory().unwrap();
        let loaded = store.load_events(999).unwrap();
        assert!(loaded.is_empty());
    }

    // ── load_events_from ──

    #[test]
    fn load_events_from_seq() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &sample_events()).unwrap();
        let loaded = store.load_events_from(id, 3).unwrap();
        assert_eq!(loaded.len(), 2); // seq 3 and 4
        assert!(matches!(loaded[0], DisplayEvent::ToolCall { .. }));
    }

    #[test]
    fn load_events_from_beyond_max_returns_empty() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &sample_events()).unwrap();
        let loaded = store.load_events_from(id, 100).unwrap();
        assert!(loaded.is_empty());
    }

    // ── count_events ──

    #[test]
    fn count_events_all() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &sample_events()).unwrap();
        assert_eq!(store.count_events(id, None).unwrap(), 4);
    }

    #[test]
    fn count_events_by_kind() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &sample_events()).unwrap();
        assert_eq!(store.count_events(id, Some(&["UserMessage"])).unwrap(), 1);
        assert_eq!(store.count_events(id, Some(&["UserMessage", "AssistantText"])).unwrap(), 2);
    }

    #[test]
    fn count_events_empty_session() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        assert_eq!(store.count_events(id, None).unwrap(), 0);
    }

    // ── message_count ──

    #[test]
    fn message_count_matches_user_and_assistant() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &sample_events()).unwrap();
        assert_eq!(store.message_count(id).unwrap(), 2);
    }

    // ── compaction ──

    #[test]
    fn store_and_retrieve_compaction() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.store_compaction(id, 10, "Summary of first 10 events").unwrap();
        let info = store.latest_compaction(id).unwrap().unwrap();
        assert_eq!(info.after_seq, 10);
        assert_eq!(info.summary, "Summary of first 10 events");
    }

    #[test]
    fn latest_compaction_returns_most_recent() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.store_compaction(id, 5, "first").unwrap();
        store.store_compaction(id, 15, "second").unwrap();
        let info = store.latest_compaction(id).unwrap().unwrap();
        assert_eq!(info.after_seq, 15);
        assert_eq!(info.summary, "second");
    }

    #[test]
    fn latest_compaction_none_if_no_compactions() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        assert!(store.latest_compaction(id).unwrap().is_none());
    }

    #[test]
    fn total_chars_since_compaction_all_events() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &sample_events()).unwrap();
        let chars = store.total_chars_since_compaction(id).unwrap();
        assert!(chars > 0);
    }

    #[test]
    fn total_chars_since_compaction_after_compact() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &sample_events()).unwrap();
        let before = store.total_chars_since_compaction(id).unwrap();
        store.store_compaction(id, store.max_seq(id).unwrap(), "summary").unwrap();
        let after = store.total_chars_since_compaction(id).unwrap();
        assert!(before > 0);
        assert_eq!(after, 0);
    }

    #[test]
    fn total_chars_since_compaction_only_counts_new() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &[
            DisplayEvent::UserMessage { _uuid: String::new(), content: "12345".into() },
        ]).unwrap();
        store.store_compaction(id, store.max_seq(id).unwrap(), "s").unwrap();
        store.append_events(id, &[
            DisplayEvent::UserMessage { _uuid: String::new(), content: "abc".into() },
        ]).unwrap();
        let chars = store.total_chars_since_compaction(id).unwrap();
        assert_eq!(chars, 3);
    }

    // ── max_seq ──

    #[test]
    fn max_seq_empty() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        assert_eq!(store.max_seq(id).unwrap(), 0);
    }

    #[test]
    fn max_seq_after_append() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &sample_events()).unwrap();
        assert_eq!(store.max_seq(id).unwrap(), 4);
    }

    // ── build_context ──

    #[test]
    fn build_context_empty_session_returns_none() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        assert!(store.build_context(id).unwrap().is_none());
    }

    #[test]
    fn build_context_returns_all_events_no_compaction() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &sample_events()).unwrap();
        let payload = store.build_context(id).unwrap().unwrap();
        assert!(payload.compaction_summary.is_none());
        assert_eq!(payload.events.len(), 4);
    }

    #[test]
    fn build_context_returns_events_after_compaction() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &[
            DisplayEvent::UserMessage { _uuid: String::new(), content: "old".into() },
        ]).unwrap();
        store.store_compaction(id, store.max_seq(id).unwrap(), "Summary of old stuff").unwrap();
        store.append_events(id, &[
            DisplayEvent::UserMessage { _uuid: String::new(), content: "new".into() },
        ]).unwrap();
        let payload = store.build_context(id).unwrap().unwrap();
        assert_eq!(payload.compaction_summary.as_deref(), Some("Summary of old stuff"));
        assert_eq!(payload.events.len(), 1);
        match &payload.events[0] {
            DisplayEvent::UserMessage { content, .. } => assert_eq!(content, "new"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn build_context_compaction_no_new_events() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &sample_events()).unwrap();
        store.store_compaction(id, store.max_seq(id).unwrap(), "All summarized").unwrap();
        let payload = store.build_context(id).unwrap().unwrap();
        assert_eq!(payload.compaction_summary.as_deref(), Some("All summarized"));
        assert!(payload.events.is_empty());
    }

    // ── load_all_session_names ──

    #[test]
    fn load_all_session_names_defaults_to_s_number() {
        let store = SessionStore::open_memory().unwrap();
        store.create_session("main").unwrap();
        store.create_session("main").unwrap();
        let names = store.load_all_session_names();
        assert_eq!(names.get(&1), Some(&"S1".to_string()));
        assert_eq!(names.get(&2), Some(&"S2".to_string()));
    }

    #[test]
    fn load_all_session_names_uses_custom_name() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.rename_session(id, "Feature Work").unwrap();
        let names = store.load_all_session_names();
        assert_eq!(names.get(&id), Some(&"Feature Work".to_string()));
    }

    // ── event_kind ──

    #[test]
    fn event_kind_all_variants() {
        assert_eq!(event_kind(&DisplayEvent::UserMessage { _uuid: String::new(), content: String::new() }), "UserMessage");
        assert_eq!(event_kind(&DisplayEvent::AssistantText { _uuid: String::new(), _message_id: String::new(), text: String::new() }), "AssistantText");
        assert_eq!(event_kind(&DisplayEvent::ToolCall { _uuid: String::new(), tool_use_id: String::new(), tool_name: String::new(), file_path: None, input: serde_json::Value::Null }), "ToolCall");
        assert_eq!(event_kind(&DisplayEvent::ToolResult { tool_use_id: String::new(), tool_name: String::new(), file_path: None, content: String::new(), is_error: false }), "ToolResult");
        assert_eq!(event_kind(&DisplayEvent::Init { _session_id: String::new(), cwd: String::new(), model: String::new() }), "Init");
        assert_eq!(event_kind(&DisplayEvent::Hook { name: String::new(), output: String::new() }), "Hook");
        assert_eq!(event_kind(&DisplayEvent::Command { name: String::new() }), "Command");
        assert_eq!(event_kind(&DisplayEvent::Compacting), "Compacting");
        assert_eq!(event_kind(&DisplayEvent::Compacted), "Compacted");
        assert_eq!(event_kind(&DisplayEvent::MayBeCompacting), "MayBeCompacting");
        assert_eq!(event_kind(&DisplayEvent::Plan { name: String::new(), content: String::new() }), "Plan");
        assert_eq!(event_kind(&DisplayEvent::Complete { _session_id: String::new(), success: true, duration_ms: 0, cost_usd: 0.0 }), "Complete");
        assert_eq!(event_kind(&DisplayEvent::Filtered), "Filtered");
    }

    // ── event_char_len ──

    #[test]
    fn event_char_len_user_message() {
        let ev = DisplayEvent::UserMessage { _uuid: String::new(), content: "hello".into() };
        assert_eq!(event_char_len(&ev), 5);
    }

    #[test]
    fn event_char_len_assistant_text() {
        let ev = DisplayEvent::AssistantText { _uuid: String::new(), _message_id: String::new(), text: "hi there!".into() };
        assert_eq!(event_char_len(&ev), 9);
    }

    #[test]
    fn event_char_len_unit_variants() {
        assert_eq!(event_char_len(&DisplayEvent::Compacting), 0);
        assert_eq!(event_char_len(&DisplayEvent::Filtered), 0);
    }

    // ── filesystem open ──

    #[test]
    fn open_creates_azs_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::open(dir.path()).unwrap();
        let id = store.create_session("main").unwrap();
        assert_eq!(id, 1);
        assert!(SessionStore::db_path(dir.path()).exists());
    }

    #[test]
    fn open_existing_db_preserves_data() {
        let dir = tempfile::tempdir().unwrap();
        {
            let store = SessionStore::open(dir.path()).unwrap();
            store.create_session("main").unwrap();
            store.append_events(1, &sample_events()).unwrap();
        }
        {
            let store = SessionStore::open(dir.path()).unwrap();
            let loaded = store.load_events(1).unwrap();
            assert_eq!(loaded.len(), 4);
        }
    }

    // ── serde round-trip fidelity ──

    #[test]
    fn round_trip_preserves_tool_call_key_field() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &[
            DisplayEvent::ToolCall {
                _uuid: String::new(),
                tool_use_id: "tc1".into(),
                tool_name: "Read".into(),
                file_path: Some("/src/main.rs".into()),
                input: serde_json::json!({"file_path": "/src/main.rs", "offset": 10}),
            },
        ]).unwrap();
        let loaded = store.load_events(id).unwrap();
        match &loaded[0] {
            DisplayEvent::ToolCall { input, tool_name, .. } => {
                assert_eq!(tool_name, "Read");
                // Compaction keeps file_path but strips offset
                assert_eq!(input.get("file_path").unwrap().as_str().unwrap(), "/src/main.rs");
                assert!(input.get("offset").is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_preserves_tool_result_is_error() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &[
            DisplayEvent::ToolResult {
                tool_use_id: "tc1".into(),
                tool_name: "Bash".into(),
                file_path: None,
                content: "error: not found".into(),
                is_error: true,
            },
        ]).unwrap();
        let loaded = store.load_events(id).unwrap();
        match &loaded[0] {
            DisplayEvent::ToolResult { is_error, content, .. } => {
                assert!(*is_error);
                assert_eq!(content, "error: not found");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_preserves_complete() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &[
            DisplayEvent::Complete {
                _session_id: String::new(),
                success: true,
                duration_ms: 5000,
                cost_usd: 0.05,
            },
        ]).unwrap();
        let loaded = store.load_events(id).unwrap();
        match &loaded[0] {
            DisplayEvent::Complete { success, duration_ms, cost_usd, .. } => {
                assert!(*success);
                assert_eq!(*duration_ms, 5000);
                assert!((*cost_usd - 0.05).abs() < f64::EPSILON);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_preserves_unit_variants() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &[
            DisplayEvent::Compacting,
            DisplayEvent::Compacted,
            DisplayEvent::MayBeCompacting,
        ]).unwrap();
        let loaded = store.load_events(id).unwrap();
        assert_eq!(loaded.len(), 3);
        assert!(matches!(loaded[0], DisplayEvent::Compacting));
        assert!(matches!(loaded[1], DisplayEvent::Compacted));
        assert!(matches!(loaded[2], DisplayEvent::MayBeCompacting));
    }

    // ── isolation between sessions ──

    #[test]
    fn events_isolated_between_sessions() {
        let store = SessionStore::open_memory().unwrap();
        let s1 = store.create_session("main").unwrap();
        let s2 = store.create_session("feat").unwrap();
        store.append_events(s1, &[
            DisplayEvent::UserMessage { _uuid: String::new(), content: "s1 msg".into() },
        ]).unwrap();
        store.append_events(s2, &[
            DisplayEvent::UserMessage { _uuid: String::new(), content: "s2 msg".into() },
            DisplayEvent::AssistantText { _uuid: String::new(), _message_id: String::new(), text: "s2 reply".into() },
        ]).unwrap();
        assert_eq!(store.load_events(s1).unwrap().len(), 1);
        assert_eq!(store.load_events(s2).unwrap().len(), 2);
    }

    #[test]
    fn compaction_isolated_between_sessions() {
        let store = SessionStore::open_memory().unwrap();
        let s1 = store.create_session("main").unwrap();
        let s2 = store.create_session("main").unwrap();
        store.store_compaction(s1, 5, "s1 summary").unwrap();
        assert!(store.latest_compaction(s1).unwrap().is_some());
        assert!(store.latest_compaction(s2).unwrap().is_none());
    }

    // ── compact_event — ToolResult.content ──

    #[test]
    fn compact_read_truncates_large() {
        let content = (0..100).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        let ev = DisplayEvent::ToolResult {
            tool_use_id: "t".into(), tool_name: "Read".into(),
            file_path: None, content, is_error: false,
        };
        let c = compact_event(&ev);
        match &c {
            DisplayEvent::ToolResult { content, .. } => {
                assert!(content.contains("line 0"));
                assert!(content.contains("(100 lines)"));
                assert!(content.contains("line 99"));
                assert!(!content.contains("line 50"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn compact_read_preserves_small() {
        let ev = DisplayEvent::ToolResult {
            tool_use_id: "t".into(), tool_name: "Read".into(),
            file_path: None, content: "only line".into(), is_error: false,
        };
        match &compact_event(&ev) {
            DisplayEvent::ToolResult { content, .. } => assert_eq!(content, "only line"),
            _ => panic!(),
        }
    }

    #[test]
    fn compact_bash_keeps_last_two() {
        let content = "line1\n\nline2\nline3\nline4";
        let ev = DisplayEvent::ToolResult {
            tool_use_id: "t".into(), tool_name: "Bash".into(),
            file_path: None, content: content.into(), is_error: false,
        };
        match &compact_event(&ev) {
            DisplayEvent::ToolResult { content, .. } => {
                assert!(content.contains("line3"));
                assert!(content.contains("line4"));
                assert!(!content.contains("line1"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn compact_grep_keeps_first_three() {
        let content = (0..10).map(|i| format!("match {}", i)).collect::<Vec<_>>().join("\n");
        let ev = DisplayEvent::ToolResult {
            tool_use_id: "t".into(), tool_name: "Grep".into(),
            file_path: None, content, is_error: false,
        };
        match &compact_event(&ev) {
            DisplayEvent::ToolResult { content, .. } => {
                assert!(content.contains("match 0"));
                assert!(content.contains("match 2"));
                assert!(content.contains("+7 more"));
                assert!(!content.contains("match 5"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn compact_glob_shows_count() {
        let content = "a.rs\nb.rs\nc.rs\nd.rs\ne.rs";
        let ev = DisplayEvent::ToolResult {
            tool_use_id: "t".into(), tool_name: "Glob".into(),
            file_path: None, content: content.into(), is_error: false,
        };
        match &compact_event(&ev) {
            DisplayEvent::ToolResult { content, .. } => assert_eq!(content, "5 files"),
            _ => panic!(),
        }
    }

    #[test]
    fn compact_task_keeps_first_five() {
        let content = (0..20).map(|i| format!("output {}", i)).collect::<Vec<_>>().join("\n");
        let ev = DisplayEvent::ToolResult {
            tool_use_id: "t".into(), tool_name: "Task".into(),
            file_path: None, content, is_error: false,
        };
        match &compact_event(&ev) {
            DisplayEvent::ToolResult { content, .. } => {
                assert!(content.contains("output 0"));
                assert!(content.contains("output 4"));
                assert!(content.contains("+15 more lines"));
                assert!(!content.contains("output 10"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn compact_default_keeps_first_three() {
        let content = "a\nb\nc\nd\ne";
        let ev = DisplayEvent::ToolResult {
            tool_use_id: "t".into(), tool_name: "WebFetch".into(),
            file_path: None, content: content.into(), is_error: false,
        };
        match &compact_event(&ev) {
            DisplayEvent::ToolResult { content, .. } => {
                assert!(content.contains("a\nb\nc"));
                assert!(content.contains("+2 more"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn compact_strips_system_reminder() {
        let content = "real output\n<system-reminder>secret stuff</system-reminder>";
        let ev = DisplayEvent::ToolResult {
            tool_use_id: "t".into(), tool_name: "Read".into(),
            file_path: None, content: content.into(), is_error: false,
        };
        match &compact_event(&ev) {
            DisplayEvent::ToolResult { content, .. } => {
                assert!(!content.contains("system-reminder"));
                assert!(content.contains("real output"));
            }
            _ => panic!(),
        }
    }

    // ── compact_event — ToolCall.input ──

    #[test]
    fn compact_write_summarizes_content() {
        let code = "// Main entry point\nfn main() {\n    println!(\"hello\");\n}\n";
        let ev = DisplayEvent::ToolCall {
            _uuid: "u".into(), tool_use_id: "t".into(), tool_name: "Write".into(),
            file_path: Some("/src/main.rs".into()),
            input: serde_json::json!({"file_path": "/src/main.rs", "content": code}),
        };
        match &compact_event(&ev) {
            DisplayEvent::ToolCall { input, .. } => {
                assert_eq!(input.get("file_path").unwrap().as_str().unwrap(), "/src/main.rs");
                assert_eq!(input.get("_lines").unwrap().as_u64().unwrap(), 4);
                assert!(input.get("_purpose").unwrap().as_str().unwrap().contains("Main entry point"));
                assert!(input.get("content").is_none());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn compact_edit_preserved() {
        let ev = DisplayEvent::ToolCall {
            _uuid: "u".into(), tool_use_id: "t".into(), tool_name: "Edit".into(),
            file_path: Some("/f.rs".into()),
            input: serde_json::json!({"file_path": "/f.rs", "old_string": "a", "new_string": "b"}),
        };
        match &compact_event(&ev) {
            DisplayEvent::ToolCall { input, .. } => {
                assert!(input.get("old_string").is_some());
                assert!(input.get("new_string").is_some());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn compact_bash_strips_extras() {
        let ev = DisplayEvent::ToolCall {
            _uuid: "u".into(), tool_use_id: "t".into(), tool_name: "Bash".into(),
            file_path: None,
            input: serde_json::json!({"command": "cargo build", "timeout": 120000, "description": "Build"}),
        };
        match &compact_event(&ev) {
            DisplayEvent::ToolCall { input, .. } => {
                assert_eq!(input.get("command").unwrap().as_str().unwrap(), "cargo build");
                assert!(input.get("timeout").is_none());
                assert!(input.get("description").is_none());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn compact_read_strips_extras() {
        let ev = DisplayEvent::ToolCall {
            _uuid: "u".into(), tool_use_id: "t".into(), tool_name: "Read".into(),
            file_path: Some("/f.rs".into()),
            input: serde_json::json!({"file_path": "/f.rs", "offset": 100, "limit": 50}),
        };
        match &compact_event(&ev) {
            DisplayEvent::ToolCall { input, .. } => {
                assert!(input.get("file_path").is_some());
                assert!(input.get("offset").is_none());
                assert!(input.get("limit").is_none());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn compact_passthrough_user_message() {
        let ev = DisplayEvent::UserMessage { _uuid: "u".into(), content: "hello".into() };
        match &compact_event(&ev) {
            DisplayEvent::UserMessage { content, .. } => assert_eq!(content, "hello"),
            _ => panic!(),
        }
    }

    #[test]
    fn compact_passthrough_assistant_text() {
        let ev = DisplayEvent::AssistantText { _uuid: "u".into(), _message_id: "m".into(), text: "reply".into() };
        match &compact_event(&ev) {
            DisplayEvent::AssistantText { text, .. } => assert_eq!(text, "reply"),
            _ => panic!(),
        }
    }

    // ── compact integration with append_events ──

    #[test]
    fn append_events_compacts_tool_result() {
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("test").unwrap();
        let big_content = (0..100).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        let events = vec![DisplayEvent::ToolResult {
            tool_use_id: "t".into(), tool_name: "Read".into(),
            file_path: None, content: big_content, is_error: false,
        }];
        store.append_events(sid, &events).unwrap();

        let loaded = store.load_events(sid).unwrap();
        match &loaded[0] {
            DisplayEvent::ToolResult { content, .. } => {
                // Should be compacted, not the full 100 lines
                assert!(content.contains("(100 lines)"));
                assert!(!content.contains("line 50"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn append_events_compacts_tool_call_input() {
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("test").unwrap();
        let events = vec![DisplayEvent::ToolCall {
            _uuid: "u".into(), tool_use_id: "t".into(), tool_name: "Bash".into(),
            file_path: None,
            input: serde_json::json!({"command": "ls", "timeout": 120000}),
        }];
        store.append_events(sid, &events).unwrap();

        let loaded = store.load_events(sid).unwrap();
        match &loaded[0] {
            DisplayEvent::ToolCall { input, .. } => {
                assert!(input.get("command").is_some());
                assert!(input.get("timeout").is_none());
            }
            _ => panic!(),
        }
    }
}
