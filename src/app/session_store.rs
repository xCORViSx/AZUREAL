//! SQLite-backed session store (.azs file)
//!
//! Replaces the gzip-compressed JSON cache system with a single SQLite database
//! at `.azureal/sessions.azs`. Sessions use S-numbering (S1, S2, S3...) and are
//! backend-agnostic — a single session can span Claude and Codex prompts.
//!
//! The `.azs` extension discourages users from trying to open or tamper with the
//! binary file directly. Internally it is a standard SQLite database (DELETE journal mode).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::events::DisplayEvent;

/// Compress JSON string with zstd (level 3 — fast, good ratio).
fn compress_data(json: &str) -> Vec<u8> {
    zstd::encode_all(json.as_bytes(), 3).unwrap_or_else(|_| json.as_bytes().to_vec())
}

/// Decompress zstd-compressed event data back to JSON string.
fn decompress_data(blob: &[u8]) -> String {
    zstd::decode_all(blob)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default()
}

/// Compaction threshold: when characters since last compaction exceed this,
/// a background agent summarizes the conversation. ~100K tokens at 4 chars/token.
pub const COMPACTION_THRESHOLD: usize = 400_000;
const SCHEMA_VERSION: &str = "3";
const LEGACY_SESSION_COLUMNS: [&str; 3] = ["context_tokens", "output_tokens", "context_window"];

// =========================================================================
// Schema
// =========================================================================

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    id               INTEGER PRIMARY KEY,
    name             TEXT NOT NULL DEFAULT '',
    worktree         TEXT NOT NULL DEFAULT '',
    created          TEXT NOT NULL DEFAULT (datetime('now')),
    completed        INTEGER,
    duration_ms      INTEGER,
    cost_usd         REAL,
    last_claude_uuid TEXT NOT NULL DEFAULT ''
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
INSERT OR IGNORE INTO meta(key, value) VALUES ('schema_version', '3');
";

// =========================================================================
// Public types
// =========================================================================

/// Summary info for a session (used in session list).
/// Completion fields are display-only metadata — never injected into prompts.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SessionInfo {
    pub id: i64,
    pub name: String,
    pub worktree: String,
    pub created: String,
    pub event_count: usize,
    pub message_count: usize,
    /// Whether the session completed (true = success, false = failed, None = still running/unknown)
    pub completed: Option<bool>,
    /// Session duration in milliseconds (from Complete event)
    pub duration_ms: Option<u64>,
    /// API cost in USD (from Complete event)
    pub cost_usd: Option<f64>,
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
            "PRAGMA journal_mode = DELETE;\
             PRAGMA synchronous = NORMAL;\
             PRAGMA foreign_keys = ON;",
        )?;
        conn.execute_batch(SCHEMA)?;
        Self::migrate_schema(&conn)?;
        Ok(Self { conn })
    }

    /// Open an in-memory store (for tests).
    #[cfg(test)]
    pub fn open_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        Self::migrate_schema(&conn)?;
        Ok(Self { conn })
    }

    fn migrate_schema(conn: &Connection) -> anyhow::Result<()> {
        if !Self::column_exists(conn, "sessions", "last_claude_uuid")? {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN last_claude_uuid TEXT NOT NULL DEFAULT '';",
            )?;
        }
        for column in LEGACY_SESSION_COLUMNS {
            if Self::column_exists(conn, "sessions", column)? {
                conn.execute_batch(&format!("ALTER TABLE sessions DROP COLUMN {column};"))?;
            }
        }
        conn.execute(
            "UPDATE meta SET value = ?1 WHERE key = 'schema_version'",
            params![SCHEMA_VERSION],
        )?;
        Ok(())
    }

    fn column_exists(conn: &Connection, table: &str, column: &str) -> anyhow::Result<bool> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let name: String = row.get(1)?;
            if name == column {
                return Ok(true);
            }
        }
        Ok(false)
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

    /// Next S-number: `max(id) + 1` across all sessions, defaulting to 1.
    pub fn next_s_number(&self) -> i64 {
        self.conn
            .query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM sessions", [], |row| {
                row.get(0)
            })
            .unwrap_or(1)
    }

    /// Update the last Claude UUID for a session (for JSONL recovery on restart).
    pub fn set_session_uuid(&self, id: i64, uuid: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE sessions SET last_claude_uuid = ?1 WHERE id = ?2",
            params![uuid, id],
        )?;
        Ok(())
    }

    /// Get sessions with non-empty last_claude_uuid (for orphan recovery).
    pub fn sessions_with_uuid(&self) -> anyhow::Result<Vec<(i64, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, worktree, last_claude_uuid FROM sessions WHERE last_claude_uuid != ''",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Clear the Claude UUID for a session (after successful JSONL ingestion).
    pub fn clear_session_uuid(&self, id: i64) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE sessions SET last_claude_uuid = '' WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Rename a session (set user-assigned display name).
    pub fn rename_session(&self, id: i64, name: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE sessions SET name = ?1 WHERE id = ?2",
            params![name, id],
        )?;
        Ok(())
    }

    /// Mark a session as completed with duration and cost (display-only metadata).
    #[allow(dead_code)]
    pub fn mark_completed(
        &self,
        id: i64,
        success: bool,
        duration_ms: u64,
        cost_usd: f64,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE sessions SET completed = ?1, duration_ms = ?2, cost_usd = ?3 WHERE id = ?4",
            params![success as i64, duration_ms as i64, cost_usd, id],
        )?;
        Ok(())
    }

    /// Delete a session and all its events/compactions (CASCADE).
    /// Runs VACUUM afterward to reclaim disk space from deleted rows.
    #[allow(dead_code)]
    pub fn delete_session(&self, id: i64) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        let _ = self.conn.execute_batch("VACUUM;");
        Ok(())
    }

    /// List all sessions, optionally filtered by worktree.
    pub fn list_sessions(&self, worktree: Option<&str>) -> anyhow::Result<Vec<SessionInfo>> {
        let (sql, filter): (&str, Box<dyn rusqlite::ToSql>) = match worktree {
            Some(wt) => (
                "SELECT s.id, s.name, s.worktree, s.created, \
                    COALESCE(e.cnt, 0), COALESCE(m.cnt, 0), \
                    s.completed, s.duration_ms, s.cost_usd \
                 FROM sessions s \
                 LEFT JOIN (SELECT session_id, COUNT(*) as cnt FROM events GROUP BY session_id) e \
                    ON e.session_id = s.id \
                 LEFT JOIN (SELECT session_id, COUNT(*) as cnt FROM events \
                    WHERE kind IN ('UserMessage','AssistantText') GROUP BY session_id) m \
                    ON m.session_id = s.id \
                 LEFT JOIN (SELECT session_id, MAX(id) as max_eid FROM events GROUP BY session_id) latest \
                    ON latest.session_id = s.id \
                 WHERE s.worktree = ?1 \
                 ORDER BY COALESCE(latest.max_eid, 0) DESC, s.id DESC",
                Box::new(wt.to_string()),
            ),
            None => (
                "SELECT s.id, s.name, s.worktree, s.created, \
                    COALESCE(e.cnt, 0), COALESCE(m.cnt, 0), \
                    s.completed, s.duration_ms, s.cost_usd \
                 FROM sessions s \
                 LEFT JOIN (SELECT session_id, COUNT(*) as cnt FROM events GROUP BY session_id) e \
                    ON e.session_id = s.id \
                 LEFT JOIN (SELECT session_id, COUNT(*) as cnt FROM events \
                    WHERE kind IN ('UserMessage','AssistantText') GROUP BY session_id) m \
                    ON m.session_id = s.id \
                 LEFT JOIN (SELECT session_id, MAX(id) as max_eid FROM events GROUP BY session_id) latest \
                    ON latest.session_id = s.id \
                 ORDER BY COALESCE(latest.max_eid, 0) DESC, s.id DESC",
                Box::new(""),
            ),
        };

        let map_row = |row: &rusqlite::Row| -> rusqlite::Result<SessionInfo> {
            let completed_raw: Option<i64> = row.get(6)?;
            let duration_raw: Option<i64> = row.get(7)?;
            Ok(SessionInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                worktree: row.get(2)?,
                created: row.get(3)?,
                event_count: row.get::<_, i64>(4)? as usize,
                message_count: row.get::<_, i64>(5)? as usize,
                completed: completed_raw.map(|v| v != 0),
                duration_ms: duration_raw.map(|v| v as u64),
                cost_usd: row.get(8)?,
            })
        };

        if worktree.is_some() {
            let mut s = self.conn.prepare(sql)?;
            let rows = s
                .query_map(params![&*filter], map_row)?
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(rows);
        }

        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt
            .query_map([], map_row)?
            .collect::<Result<Vec<_>, _>>()?;
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
                let display = if name.is_empty() {
                    format!("S{}", id)
                } else {
                    name
                };
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
    /// Skips `Filtered` events. Data is zstd-compressed before storage.
    /// Automatically updates session completion metadata when a `Complete` event is stored.
    pub fn append_events(&self, session_id: i64, events: &[DisplayEvent]) -> anyhow::Result<usize> {
        let mut seq = self.next_seq(session_id)?;
        let tx = self.conn.unchecked_transaction()?;
        let mut stmt = tx.prepare(
            "INSERT INTO events(session_id, seq, kind, data, char_len) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        let mut count = 0usize;
        let mut completion: Option<(bool, u64, f64)> = None;
        for event in events {
            if matches!(
                event,
                DisplayEvent::Filtered | DisplayEvent::MayBeCompacting
            ) {
                continue;
            }
            if let DisplayEvent::Complete {
                success,
                duration_ms,
                cost_usd,
                ..
            } = event
            {
                completion = Some((*success, *duration_ms, *cost_usd));
            }
            let compacted = compact_event(event);
            let kind = event_kind(&compacted);
            let json = serde_json::to_string(&compacted).unwrap_or_default();
            let data = compress_data(&json);
            let char_len = event_char_len(event) as i64;
            stmt.execute(params![session_id, seq, kind, data, char_len])?;
            seq += 1;
            count += 1;
        }
        drop(stmt);
        if let Some((success, duration_ms, cost_usd)) = completion {
            tx.execute(
                "UPDATE sessions SET completed = ?1, duration_ms = ?2, cost_usd = ?3 WHERE id = ?4",
                params![success as i64, duration_ms as i64, cost_usd, session_id],
            )?;
        }
        tx.commit()?;
        Ok(count)
    }

    /// Load all events for a session in order.
    pub fn load_events(&self, session_id: i64) -> anyhow::Result<Vec<DisplayEvent>> {
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM events WHERE session_id = ?1 ORDER BY seq")?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                let blob: Vec<u8> = row.get(0)?;
                Ok(blob)
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut events = Vec::with_capacity(rows.len());
        for blob in rows {
            let json = decompress_data(&blob);
            if let Ok(ev) = serde_json::from_str::<DisplayEvent>(&json) {
                events.push(ev);
            }
        }
        Ok(events)
    }

    /// Load events from a specific sequence position onward (for context building).
    pub fn load_events_from(
        &self,
        session_id: i64,
        from_seq: i64,
    ) -> anyhow::Result<Vec<DisplayEvent>> {
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM events WHERE session_id = ?1 AND seq >= ?2 ORDER BY seq")?;
        let rows = stmt
            .query_map(params![session_id, from_seq], |row| {
                let blob: Vec<u8> = row.get(0)?;
                Ok(blob)
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut events = Vec::with_capacity(rows.len());
        for blob in rows {
            let json = decompress_data(&blob);
            if let Ok(ev) = serde_json::from_str::<DisplayEvent>(&json) {
                events.push(ev);
            }
        }
        Ok(events)
    }

    /// Count events, optionally filtered by kind(s).
    #[allow(dead_code)]
    pub fn count_events(&self, session_id: i64, kinds: Option<&[&str]>) -> anyhow::Result<usize> {
        let count: i64 = match kinds {
            Some(ks) if !ks.is_empty() => {
                let placeholders: Vec<String> = ks
                    .iter()
                    .enumerate()
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
            _ => self.conn.query_row(
                "SELECT COUNT(*) FROM events WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )?,
        };
        Ok(count as usize)
    }

    /// Message count (UserMessage + AssistantText only).
    #[allow(dead_code)]
    pub fn message_count(&self, session_id: i64) -> anyhow::Result<usize> {
        self.count_events(session_id, Some(&["UserMessage", "AssistantText"]))
    }

    // =====================================================================
    // Compaction
    // =====================================================================

    /// Total character count of events since the last compaction (or all events if none).
    pub fn total_chars_since_compaction(&self, session_id: i64) -> anyhow::Result<usize> {
        let after_seq = self
            .latest_compaction(session_id)?
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
    pub fn store_compaction(
        &self,
        session_id: i64,
        after_seq: i64,
        summary: &str,
    ) -> anyhow::Result<()> {
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

    /// Find the compaction boundary: the seq just before the Nth-to-last UserMessage.
    /// Returns `None` if there are fewer than `keep` UserMessages since `from_seq`,
    /// meaning there's not enough old content to compact.
    pub fn compaction_boundary(
        &self,
        session_id: i64,
        from_seq: i64,
        keep: usize,
    ) -> anyhow::Result<Option<i64>> {
        // Get seqs of all UserMessage events since from_seq, ordered newest first
        let mut stmt = self.conn.prepare(
            "SELECT seq FROM events WHERE session_id = ?1 AND seq >= ?2 AND kind = 'UserMessage' ORDER BY seq DESC"
        )?;
        let seqs: Vec<i64> = stmt
            .query_map(params![session_id, from_seq], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        if seqs.len() <= keep {
            return Ok(None); // Not enough user messages to justify compaction
        }

        // The boundary is one less than the seq of the (keep+1)th-from-last UserMessage
        // i.e. we keep the last `keep` UserMessages and everything after them as raw events
        let boundary_user_seq = seqs[keep - 1]; // seq of the Nth-to-last UserMessage (keep=3 → 3rd from end)
        Ok(Some(boundary_user_seq - 1))
    }

    /// Maximum event sequence number for a session.
    #[allow(dead_code)]
    pub fn max_seq(&self, session_id: i64) -> anyhow::Result<i64> {
        let max: Option<i64> = self.conn.query_row(
            "SELECT MAX(seq) FROM events WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(max.unwrap_or(0))
    }

    /// Load events in a range [from_seq, through_seq] (inclusive).
    pub fn load_events_range(
        &self,
        session_id: i64,
        from_seq: i64,
        through_seq: i64,
    ) -> anyhow::Result<Vec<DisplayEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT data FROM events WHERE session_id = ?1 AND seq >= ?2 AND seq <= ?3 ORDER BY seq"
        )?;
        let rows = stmt
            .query_map(params![session_id, from_seq, through_seq], |row| {
                let blob: Vec<u8> = row.get(0)?;
                Ok(blob)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut events = Vec::with_capacity(rows.len());
        for blob in rows {
            let json = decompress_data(&blob);
            if let Ok(ev) = serde_json::from_str::<DisplayEvent>(&json) {
                events.push(ev);
            }
        }
        Ok(events)
    }

    /// Search event data across sessions for a query string. Returns up to `limit`
    /// results as (session_id, preview_text). Searches only text-bearing event kinds.
    /// Decompresses each event and filters in Rust (data column is zstd-compressed).
    pub fn search_events(
        &self,
        worktree: Option<&str>,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<(i64, String)>> {
        let query_lower = query.to_lowercase();
        let sql = match worktree {
            Some(_) => {
                "SELECT e.session_id, e.data FROM events e \
                        JOIN sessions s ON s.id = e.session_id \
                        WHERE s.worktree = ?1 AND e.kind IN ('UserMessage','AssistantText') \
                        ORDER BY e.session_id, e.seq"
            }
            None => {
                "SELECT e.session_id, e.data FROM events e \
                        WHERE e.kind IN ('UserMessage','AssistantText') \
                        ORDER BY e.session_id, e.seq"
            }
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows: Vec<(i64, Vec<u8>)> = if let Some(wt) = worktree {
            stmt.query_map(params![wt], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect()
        } else {
            stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect()
        };
        let mut results = Vec::new();
        for (session_id, blob) in rows {
            if results.len() >= limit {
                break;
            }
            let json = decompress_data(&blob);
            if json.to_lowercase().contains(&query_lower) {
                results.push((session_id, json));
            }
        }
        Ok(results)
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
        DisplayEvent::ModelSwitch { .. } => "ModelSwitch",
        DisplayEvent::Filtered => "Filtered",
    }
}

/// Compact a DisplayEvent for storage, minimizing JSON size while preserving
/// everything the render pipeline needs. Mirrors the display rules in
/// `render_tools.rs`: ToolResult content is truncated to what's actually shown,
/// ToolCall input is stripped to only the key field `extract_tool_param` reads.
fn compact_event(event: &DisplayEvent) -> DisplayEvent {
    match event {
        DisplayEvent::ToolResult {
            tool_use_id,
            tool_name,
            file_path,
            content,
            is_error,
        } => {
            // Strip system-reminder blocks first
            let content = if let Some(start) = content.find("<system-reminder>") {
                &content[..start]
            } else {
                content.as_str()
            }
            .trim_end();

            let lines: Vec<&str> = content.lines().collect();
            let compacted = match tool_name.as_str() {
                "Read" | "read" => {
                    // First + last non-empty line
                    if lines.len() <= 2 {
                        content.to_string()
                    } else {
                        let first = lines[0];
                        let last = lines
                            .iter()
                            .rev()
                            .find(|l| !l.trim().is_empty())
                            .unwrap_or(&"");
                        format!("{}\n  ({} lines)\n{}", first, lines.len(), last)
                    }
                }
                "Bash" | "bash" => {
                    // Last 2 non-empty lines
                    let non_empty: Vec<&str> = lines
                        .iter()
                        .filter(|l| !l.trim().is_empty())
                        .copied()
                        .collect();
                    non_empty
                        .iter()
                        .rev()
                        .take(2)
                        .rev()
                        .copied()
                        .collect::<Vec<_>>()
                        .join("\n")
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
        DisplayEvent::ToolCall {
            _uuid,
            tool_use_id,
            tool_name,
            file_path,
            input,
        } => {
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
                        let purpose = content_lines
                            .iter()
                            .find(|l| {
                                let t = l.trim();
                                t.starts_with("//")
                                    || t.starts_with('#')
                                    || t.starts_with("/*")
                                    || t.starts_with("\"\"\"")
                                    || t.starts_with("///")
                                    || t.starts_with("//!")
                            })
                            .or(content_lines.first())
                            .copied()
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
                        "Agent" | "agent" | "Task" | "task" => &["subagent_type", "description"],
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

pub(crate) fn event_dedup_key(event: &DisplayEvent) -> String {
    let compacted = compact_event(event);
    serde_json::to_string(&compacted).unwrap_or_default()
}

pub(crate) fn overlap_prefix_len(existing: &[DisplayEvent], appended: &[DisplayEvent]) -> usize {
    let mut prefix_skip = 0usize;
    if !existing.is_empty() {
        while prefix_skip < appended.len() {
            if matches!(appended[prefix_skip], DisplayEvent::Init { .. }) {
                prefix_skip += 1;
            } else {
                break;
            }
        }
    }

    let appended = &appended[prefix_skip..];
    let max_overlap = existing.len().min(appended.len());
    if max_overlap == 0 {
        return prefix_skip;
    }

    let existing_keys: Vec<String> = existing.iter().map(event_dedup_key).collect();
    let appended_keys: Vec<String> = appended.iter().map(event_dedup_key).collect();

    for overlap in (1..=max_overlap).rev() {
        if existing_keys[existing_keys.len() - overlap..] == appended_keys[..overlap] {
            return prefix_skip + overlap;
        }
    }

    prefix_skip
}

/// Estimate the displayable character count of an event (for compaction threshold).
/// Character count for a display event (used for context tracking).
pub fn event_char_len(event: &DisplayEvent) -> usize {
    match event {
        DisplayEvent::UserMessage { content, .. } => content.len(),
        DisplayEvent::AssistantText { text, .. } => text.len(),
        DisplayEvent::ToolCall {
            tool_name, input, ..
        } => tool_name.len() + input.to_string().len(),
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
        let version: String = store
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        for column in LEGACY_SESSION_COLUMNS {
            assert!(!SessionStore::column_exists(&store.conn, "sessions", column).unwrap());
        }
    }

    #[test]
    fn open_memory_idempotent() {
        let store = SessionStore::open_memory().unwrap();
        store.conn.execute_batch(SCHEMA).unwrap();
        SessionStore::migrate_schema(&store.conn).unwrap();
        let version: String = store
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        for column in LEGACY_SESSION_COLUMNS {
            assert!(!SessionStore::column_exists(&store.conn, "sessions", column).unwrap());
        }
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
        let name: String = store
            .conn
            .query_row(
                "SELECT name FROM sessions WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(name.is_empty());
    }

    // ── rename_session ──

    #[test]
    fn rename_session_sets_name() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.rename_session(id, "Feature Work").unwrap();
        let name: String = store
            .conn
            .query_row(
                "SELECT name FROM sessions WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
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
        let count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn delete_session_cascades_events() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.append_events(id, &sample_events()).unwrap();
        store.delete_session(id).unwrap();
        let count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE session_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn delete_session_cascades_compactions() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store.store_compaction(id, 5, "summary").unwrap();
        store.delete_session(id).unwrap();
        let count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM compactions WHERE session_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
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
        // Ordered by most recent activity DESC, then id DESC (newest first)
        assert_eq!(list[0].id, 2);
        assert_eq!(list[1].id, 1);
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

    #[test]
    fn list_sessions_ordered_by_most_recent_activity() {
        let store = SessionStore::open_memory().unwrap();
        let id1 = store.create_session("main").unwrap();
        let id2 = store.create_session("main").unwrap();
        let id3 = store.create_session("main").unwrap();
        // Add events to session 1 (oldest) AFTER creating all sessions
        // — its events have the highest autoincrement IDs
        store.append_events(id1, &sample_events()).unwrap();
        let list = store.list_sessions(Some("main")).unwrap();
        assert_eq!(list.len(), 3);
        // Session 1 has the most recent events → first
        assert_eq!(list[0].id, id1);
        // Sessions 2 and 3 have no events → ordered by id DESC
        assert_eq!(list[1].id, id3);
        assert_eq!(list[2].id, id2);
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
            DisplayEvent::UserMessage {
                _uuid: String::new(),
                content: "hi".into(),
            },
            DisplayEvent::Filtered,
            DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text: "hello".into(),
            },
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
        store
            .append_events(
                id,
                &[DisplayEvent::UserMessage {
                    _uuid: String::new(),
                    content: "first".into(),
                }],
            )
            .unwrap();
        store
            .append_events(
                id,
                &[DisplayEvent::AssistantText {
                    _uuid: String::new(),
                    _message_id: String::new(),
                    text: "second".into(),
                }],
            )
            .unwrap();
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
        assert_eq!(
            store
                .count_events(id, Some(&["UserMessage", "AssistantText"]))
                .unwrap(),
            2
        );
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
        store
            .store_compaction(id, 10, "Summary of first 10 events")
            .unwrap();
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
        store
            .store_compaction(id, store.max_seq(id).unwrap(), "summary")
            .unwrap();
        let after = store.total_chars_since_compaction(id).unwrap();
        assert!(before > 0);
        assert_eq!(after, 0);
    }

    #[test]
    fn total_chars_since_compaction_only_counts_new() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store
            .append_events(
                id,
                &[DisplayEvent::UserMessage {
                    _uuid: String::new(),
                    content: "12345".into(),
                }],
            )
            .unwrap();
        store
            .store_compaction(id, store.max_seq(id).unwrap(), "s")
            .unwrap();
        store
            .append_events(
                id,
                &[DisplayEvent::UserMessage {
                    _uuid: String::new(),
                    content: "abc".into(),
                }],
            )
            .unwrap();
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
        store
            .append_events(
                id,
                &[DisplayEvent::UserMessage {
                    _uuid: String::new(),
                    content: "old".into(),
                }],
            )
            .unwrap();
        store
            .store_compaction(id, store.max_seq(id).unwrap(), "Summary of old stuff")
            .unwrap();
        store
            .append_events(
                id,
                &[DisplayEvent::UserMessage {
                    _uuid: String::new(),
                    content: "new".into(),
                }],
            )
            .unwrap();
        let payload = store.build_context(id).unwrap().unwrap();
        assert_eq!(
            payload.compaction_summary.as_deref(),
            Some("Summary of old stuff")
        );
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
        store
            .store_compaction(id, store.max_seq(id).unwrap(), "All summarized")
            .unwrap();
        let payload = store.build_context(id).unwrap().unwrap();
        assert_eq!(
            payload.compaction_summary.as_deref(),
            Some("All summarized")
        );
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
        assert_eq!(
            event_kind(&DisplayEvent::UserMessage {
                _uuid: String::new(),
                content: String::new()
            }),
            "UserMessage"
        );
        assert_eq!(
            event_kind(&DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text: String::new()
            }),
            "AssistantText"
        );
        assert_eq!(
            event_kind(&DisplayEvent::ToolCall {
                _uuid: String::new(),
                tool_use_id: String::new(),
                tool_name: String::new(),
                file_path: None,
                input: serde_json::Value::Null
            }),
            "ToolCall"
        );
        assert_eq!(
            event_kind(&DisplayEvent::ToolResult {
                tool_use_id: String::new(),
                tool_name: String::new(),
                file_path: None,
                content: String::new(),
                is_error: false
            }),
            "ToolResult"
        );
        assert_eq!(
            event_kind(&DisplayEvent::Init {
                _session_id: String::new(),
                cwd: String::new(),
                model: String::new()
            }),
            "Init"
        );
        assert_eq!(
            event_kind(&DisplayEvent::Hook {
                name: String::new(),
                output: String::new()
            }),
            "Hook"
        );
        assert_eq!(
            event_kind(&DisplayEvent::Command {
                name: String::new()
            }),
            "Command"
        );
        assert_eq!(event_kind(&DisplayEvent::Compacting), "Compacting");
        assert_eq!(event_kind(&DisplayEvent::Compacted), "Compacted");
        assert_eq!(
            event_kind(&DisplayEvent::MayBeCompacting),
            "MayBeCompacting"
        );
        assert_eq!(
            event_kind(&DisplayEvent::Plan {
                name: String::new(),
                content: String::new()
            }),
            "Plan"
        );
        assert_eq!(
            event_kind(&DisplayEvent::Complete {
                _session_id: String::new(),
                success: true,
                duration_ms: 0,
                cost_usd: 0.0
            }),
            "Complete"
        );
        assert_eq!(
            event_kind(&DisplayEvent::ModelSwitch {
                model: String::new()
            }),
            "ModelSwitch"
        );
        assert_eq!(event_kind(&DisplayEvent::Filtered), "Filtered");
    }

    // ── event_char_len ──

    #[test]
    fn event_char_len_user_message() {
        let ev = DisplayEvent::UserMessage {
            _uuid: String::new(),
            content: "hello".into(),
        };
        assert_eq!(event_char_len(&ev), 5);
    }

    #[test]
    fn event_char_len_assistant_text() {
        let ev = DisplayEvent::AssistantText {
            _uuid: String::new(),
            _message_id: String::new(),
            text: "hi there!".into(),
        };
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

    #[test]
    fn open_migrates_legacy_usage_columns_out_of_existing_db() {
        let dir = tempfile::tempdir().unwrap();
        let path = SessionStore::db_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "
            CREATE TABLE sessions (
                id               INTEGER PRIMARY KEY,
                name             TEXT NOT NULL DEFAULT '',
                worktree         TEXT NOT NULL DEFAULT '',
                created          TEXT NOT NULL DEFAULT (datetime('now')),
                completed        INTEGER,
                duration_ms      INTEGER,
                cost_usd         REAL,
                last_claude_uuid TEXT NOT NULL DEFAULT '',
                context_tokens   INTEGER,
                output_tokens    INTEGER,
                context_window   INTEGER
            );
            CREATE TABLE meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            INSERT INTO meta(key, value) VALUES ('schema_version', '2');
            ",
        )
        .unwrap();
        drop(conn);

        let store = SessionStore::open(dir.path()).unwrap();
        let version: String = store
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        for column in LEGACY_SESSION_COLUMNS {
            assert!(!SessionStore::column_exists(&store.conn, "sessions", column).unwrap());
        }
        assert!(SessionStore::column_exists(&store.conn, "sessions", "last_claude_uuid").unwrap());
    }

    // ── serde round-trip fidelity ──

    #[test]
    fn round_trip_preserves_tool_call_key_field() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store
            .append_events(
                id,
                &[DisplayEvent::ToolCall {
                    _uuid: String::new(),
                    tool_use_id: "tc1".into(),
                    tool_name: "Read".into(),
                    file_path: Some("/src/main.rs".into()),
                    input: serde_json::json!({"file_path": "/src/main.rs", "offset": 10}),
                }],
            )
            .unwrap();
        let loaded = store.load_events(id).unwrap();
        match &loaded[0] {
            DisplayEvent::ToolCall {
                input, tool_name, ..
            } => {
                assert_eq!(tool_name, "Read");
                // Compaction keeps file_path but strips offset
                assert_eq!(
                    input.get("file_path").unwrap().as_str().unwrap(),
                    "/src/main.rs"
                );
                assert!(input.get("offset").is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_preserves_tool_result_is_error() {
        let store = SessionStore::open_memory().unwrap();
        let id = store.create_session("main").unwrap();
        store
            .append_events(
                id,
                &[DisplayEvent::ToolResult {
                    tool_use_id: "tc1".into(),
                    tool_name: "Bash".into(),
                    file_path: None,
                    content: "error: not found".into(),
                    is_error: true,
                }],
            )
            .unwrap();
        let loaded = store.load_events(id).unwrap();
        match &loaded[0] {
            DisplayEvent::ToolResult {
                is_error, content, ..
            } => {
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
        store
            .append_events(
                id,
                &[DisplayEvent::Complete {
                    _session_id: String::new(),
                    success: true,
                    duration_ms: 5000,
                    cost_usd: 0.05,
                }],
            )
            .unwrap();
        let loaded = store.load_events(id).unwrap();
        match &loaded[0] {
            DisplayEvent::Complete {
                success,
                duration_ms,
                cost_usd,
                ..
            } => {
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
        // MayBeCompacting and Filtered are skipped by append_events
        store
            .append_events(
                id,
                &[
                    DisplayEvent::Compacting,
                    DisplayEvent::Compacted,
                    DisplayEvent::MayBeCompacting,
                ],
            )
            .unwrap();
        let loaded = store.load_events(id).unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(matches!(loaded[0], DisplayEvent::Compacting));
        assert!(matches!(loaded[1], DisplayEvent::Compacted));
    }

    // ── isolation between sessions ──

    #[test]
    fn events_isolated_between_sessions() {
        let store = SessionStore::open_memory().unwrap();
        let s1 = store.create_session("main").unwrap();
        let s2 = store.create_session("feat").unwrap();
        store
            .append_events(
                s1,
                &[DisplayEvent::UserMessage {
                    _uuid: String::new(),
                    content: "s1 msg".into(),
                }],
            )
            .unwrap();
        store
            .append_events(
                s2,
                &[
                    DisplayEvent::UserMessage {
                        _uuid: String::new(),
                        content: "s2 msg".into(),
                    },
                    DisplayEvent::AssistantText {
                        _uuid: String::new(),
                        _message_id: String::new(),
                        text: "s2 reply".into(),
                    },
                ],
            )
            .unwrap();
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
        let content = (0..100)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let ev = DisplayEvent::ToolResult {
            tool_use_id: "t".into(),
            tool_name: "Read".into(),
            file_path: None,
            content,
            is_error: false,
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
            tool_use_id: "t".into(),
            tool_name: "Read".into(),
            file_path: None,
            content: "only line".into(),
            is_error: false,
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
            tool_use_id: "t".into(),
            tool_name: "Bash".into(),
            file_path: None,
            content: content.into(),
            is_error: false,
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
        let content = (0..10)
            .map(|i| format!("match {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let ev = DisplayEvent::ToolResult {
            tool_use_id: "t".into(),
            tool_name: "Grep".into(),
            file_path: None,
            content,
            is_error: false,
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
            tool_use_id: "t".into(),
            tool_name: "Glob".into(),
            file_path: None,
            content: content.into(),
            is_error: false,
        };
        match &compact_event(&ev) {
            DisplayEvent::ToolResult { content, .. } => assert_eq!(content, "5 files"),
            _ => panic!(),
        }
    }

    #[test]
    fn compact_task_keeps_first_five() {
        let content = (0..20)
            .map(|i| format!("output {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let ev = DisplayEvent::ToolResult {
            tool_use_id: "t".into(),
            tool_name: "Task".into(),
            file_path: None,
            content,
            is_error: false,
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
            tool_use_id: "t".into(),
            tool_name: "WebFetch".into(),
            file_path: None,
            content: content.into(),
            is_error: false,
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
            tool_use_id: "t".into(),
            tool_name: "Read".into(),
            file_path: None,
            content: content.into(),
            is_error: false,
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
            _uuid: "u".into(),
            tool_use_id: "t".into(),
            tool_name: "Write".into(),
            file_path: Some("/src/main.rs".into()),
            input: serde_json::json!({"file_path": "/src/main.rs", "content": code}),
        };
        match &compact_event(&ev) {
            DisplayEvent::ToolCall { input, .. } => {
                assert_eq!(
                    input.get("file_path").unwrap().as_str().unwrap(),
                    "/src/main.rs"
                );
                assert_eq!(input.get("_lines").unwrap().as_u64().unwrap(), 4);
                assert!(input
                    .get("_purpose")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .contains("Main entry point"));
                assert!(input.get("content").is_none());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn compact_edit_preserved() {
        let ev = DisplayEvent::ToolCall {
            _uuid: "u".into(),
            tool_use_id: "t".into(),
            tool_name: "Edit".into(),
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
            _uuid: "u".into(),
            tool_use_id: "t".into(),
            tool_name: "Bash".into(),
            file_path: None,
            input: serde_json::json!({"command": "cargo build", "timeout": 120000, "description": "Build"}),
        };
        match &compact_event(&ev) {
            DisplayEvent::ToolCall { input, .. } => {
                assert_eq!(
                    input.get("command").unwrap().as_str().unwrap(),
                    "cargo build"
                );
                assert!(input.get("timeout").is_none());
                assert!(input.get("description").is_none());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn compact_read_strips_extras() {
        let ev = DisplayEvent::ToolCall {
            _uuid: "u".into(),
            tool_use_id: "t".into(),
            tool_name: "Read".into(),
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
        let ev = DisplayEvent::UserMessage {
            _uuid: "u".into(),
            content: "hello".into(),
        };
        match &compact_event(&ev) {
            DisplayEvent::UserMessage { content, .. } => assert_eq!(content, "hello"),
            _ => panic!(),
        }
    }

    #[test]
    fn compact_passthrough_assistant_text() {
        let ev = DisplayEvent::AssistantText {
            _uuid: "u".into(),
            _message_id: "m".into(),
            text: "reply".into(),
        };
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
        let big_content = (0..100)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let events = vec![DisplayEvent::ToolResult {
            tool_use_id: "t".into(),
            tool_name: "Read".into(),
            file_path: None,
            content: big_content,
            is_error: false,
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
            _uuid: "u".into(),
            tool_use_id: "t".into(),
            tool_name: "Bash".into(),
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

    // ── compaction_boundary ──

    fn insert_conversation(store: &SessionStore, sid: i64, user_count: usize) {
        // Insert alternating User/Assistant messages
        for i in 0..user_count {
            store
                .append_events(
                    sid,
                    &[
                        DisplayEvent::UserMessage {
                            _uuid: String::new(),
                            content: format!("prompt {}", i + 1),
                        },
                        DisplayEvent::AssistantText {
                            _uuid: String::new(),
                            _message_id: String::new(),
                            text: format!("reply {}", i + 1),
                        },
                    ],
                )
                .unwrap();
        }
    }

    #[test]
    fn boundary_none_when_fewer_than_keep() {
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();
        insert_conversation(&store, sid, 2); // Only 2 user messages
        let boundary = store.compaction_boundary(sid, 1, 3).unwrap();
        assert!(boundary.is_none());
    }

    #[test]
    fn boundary_none_when_exactly_keep() {
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();
        insert_conversation(&store, sid, 3); // Exactly 3
        let boundary = store.compaction_boundary(sid, 1, 3).unwrap();
        assert!(boundary.is_none());
    }

    #[test]
    fn boundary_returns_seq_before_third_to_last_user() {
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();
        insert_conversation(&store, sid, 5);
        // 10 events: seqs 1..=10
        // UserMessages at seqs 1, 3, 5, 7, 9
        // keep=3 → keep seqs 5, 7, 9 (3rd, 4th, 5th user msgs)
        // boundary = seq 5 - 1 = 4
        let boundary = store.compaction_boundary(sid, 1, 3).unwrap();
        assert_eq!(boundary, Some(4));
    }

    #[test]
    fn boundary_with_four_user_msgs() {
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();
        insert_conversation(&store, sid, 4);
        // 8 events: seqs 1..=8
        // UserMessages at seqs 1, 3, 5, 7
        // keep=3 → keep seqs 3, 5, 7
        // boundary = 3 - 1 = 2
        let boundary = store.compaction_boundary(sid, 1, 3).unwrap();
        assert_eq!(boundary, Some(2));
    }

    #[test]
    fn boundary_respects_from_seq() {
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();
        insert_conversation(&store, sid, 6);
        // UserMessages at seqs 1, 3, 5, 7, 9, 11
        // from_seq=5 → only considers seqs 5, 7, 9, 11 (4 user msgs)
        // keep=3 → keep 7, 9, 11; boundary = 7 - 1 = 6
        let boundary = store.compaction_boundary(sid, 5, 3).unwrap();
        assert_eq!(boundary, Some(6));
    }

    #[test]
    fn boundary_from_seq_too_late_returns_none() {
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();
        insert_conversation(&store, sid, 5);
        // UserMessages at seqs 1, 3, 5, 7, 9
        // from_seq=7 → only seqs 7, 9 (2 user msgs) < keep=3
        let boundary = store.compaction_boundary(sid, 7, 3).unwrap();
        assert!(boundary.is_none());
    }

    // ── load_events_range ──

    #[test]
    fn load_range_returns_subset() {
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();
        insert_conversation(&store, sid, 5); // 10 events, seqs 1..=10

        let events = store.load_events_range(sid, 3, 6).unwrap();
        assert_eq!(events.len(), 4); // seqs 3, 4, 5, 6
    }

    #[test]
    fn load_range_single_event() {
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();
        insert_conversation(&store, sid, 3);

        let events = store.load_events_range(sid, 1, 1).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            DisplayEvent::UserMessage { content, .. } => assert_eq!(content, "prompt 1"),
            _ => panic!("expected UserMessage"),
        }
    }

    #[test]
    fn load_range_empty_when_no_match() {
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();
        insert_conversation(&store, sid, 2);

        let events = store.load_events_range(sid, 100, 200).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn load_range_preserves_order() {
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();
        insert_conversation(&store, sid, 3);
        // seqs 1..=6: User1, Asst1, User2, Asst2, User3, Asst3

        let events = store.load_events_range(sid, 1, 6).unwrap();
        assert_eq!(events.len(), 6);
        // First should be UserMessage, second AssistantText, alternating
        assert!(matches!(&events[0], DisplayEvent::UserMessage { .. }));
        assert!(matches!(&events[1], DisplayEvent::AssistantText { .. }));
        assert!(matches!(&events[4], DisplayEvent::UserMessage { .. }));
        assert!(matches!(&events[5], DisplayEvent::AssistantText { .. }));
    }

    #[test]
    fn boundary_and_range_integration() {
        // End-to-end: insert 5 exchanges, find boundary, load pre-boundary events
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();
        insert_conversation(&store, sid, 5);
        // UserMessages at seqs 1, 3, 5, 7, 9
        // boundary(keep=3) = seq 4 (everything before 3rd-to-last UserMessage)

        let boundary = store.compaction_boundary(sid, 1, 3).unwrap().unwrap();
        assert_eq!(boundary, 4);

        let pre = store.load_events_range(sid, 1, boundary).unwrap();
        assert_eq!(pre.len(), 4); // seqs 1, 2, 3, 4

        // Post-boundary = seqs 5..=10 (the 3 kept user+assistant pairs)
        let post = store.load_events_range(sid, boundary + 1, 10).unwrap();
        assert_eq!(post.len(), 6); // 3 user + 3 assistant
    }

    // =====================================================================
    // Integration: store → build_context → inject → strip → re-store
    // =====================================================================

    #[test]
    fn integration_full_round_trip_no_compaction() {
        // Simulate: first exchange stored, second prompt uses context injection,
        // agent response stored with injected context stripped.
        use crate::app::context_injection::{build_context_prompt, strip_injected_context};

        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();

        // First exchange: user prompt + assistant reply
        store
            .append_events(
                sid,
                &[
                    DisplayEvent::UserMessage {
                        _uuid: String::new(),
                        content: "fix the auth bug".into(),
                    },
                    DisplayEvent::AssistantText {
                        _uuid: String::new(),
                        _message_id: String::new(),
                        text: "I'll check the auth module.".into(),
                    },
                    DisplayEvent::ToolCall {
                        _uuid: String::new(),
                        tool_use_id: "tc1".into(),
                        tool_name: "Read".into(),
                        file_path: Some("/src/auth.rs".into()),
                        input: serde_json::json!({"file_path": "/src/auth.rs"}),
                    },
                    DisplayEvent::ToolResult {
                        tool_use_id: "tc1".into(),
                        tool_name: "Read".into(),
                        file_path: Some("/src/auth.rs".into()),
                        content: "fn authenticate() { todo!() }".into(),
                        is_error: false,
                    },
                    DisplayEvent::Complete {
                        _session_id: String::new(),
                        success: true,
                        duration_ms: 3000,
                        cost_usd: 0.02,
                    },
                ],
            )
            .unwrap();

        // Build context for second prompt
        let payload = store.build_context(sid).unwrap().unwrap();
        assert!(payload.compaction_summary.is_none());
        assert_eq!(payload.events.len(), 5);

        // Inject context into second prompt
        let user_prompt = "now add error handling";
        let injected = build_context_prompt(&payload, user_prompt);
        assert!(injected.contains("<azureal-session-context>"));
        assert!(injected.contains("fix the auth bug"));
        assert!(injected.contains("I'll check the auth module."));
        assert!(injected.ends_with(user_prompt));

        // Simulate: agent sees injected prompt, produces response.
        // On exit, we strip the context and store the clean exchange.
        let stripped = strip_injected_context(&injected);
        assert_eq!(stripped, user_prompt);

        // Store the second exchange with clean user message
        store
            .append_events(
                sid,
                &[
                    DisplayEvent::UserMessage {
                        _uuid: String::new(),
                        content: stripped.to_string(),
                    },
                    DisplayEvent::AssistantText {
                        _uuid: String::new(),
                        _message_id: String::new(),
                        text: "Added error handling.".into(),
                    },
                ],
            )
            .unwrap();

        // Verify all 7 events stored, no context tags leaked
        let all = store.load_events(sid).unwrap();
        assert_eq!(all.len(), 7);
        for ev in &all {
            if let DisplayEvent::UserMessage { content, .. } = ev {
                assert!(!content.contains("<azureal-session-context>"));
                assert!(!content.contains("</azureal-session-context>"));
            }
        }
    }

    #[test]
    fn integration_context_injection_preserves_tool_calls() {
        // Verify tool calls in context build are present in the injected prompt
        use crate::app::context_injection::build_context_prompt;

        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("feat").unwrap();
        store
            .append_events(
                sid,
                &[
                    DisplayEvent::UserMessage {
                        _uuid: String::new(),
                        content: "search for main".into(),
                    },
                    DisplayEvent::ToolCall {
                        _uuid: String::new(),
                        tool_use_id: "tc1".into(),
                        tool_name: "Grep".into(),
                        file_path: None,
                        input: serde_json::json!({"pattern": "fn main"}),
                    },
                    DisplayEvent::ToolResult {
                        tool_use_id: "tc1".into(),
                        tool_name: "Grep".into(),
                        file_path: None,
                        content: "src/main.rs:1:fn main()".into(),
                        is_error: false,
                    },
                    DisplayEvent::AssistantText {
                        _uuid: String::new(),
                        _message_id: String::new(),
                        text: "Found it in main.rs".into(),
                    },
                ],
            )
            .unwrap();

        let payload = store.build_context(sid).unwrap().unwrap();
        let injected = build_context_prompt(&payload, "next step");

        // Tool call and result should appear in context
        assert!(injected.contains("## Tool: Grep (fn main)"));
        assert!(injected.contains("[Result: Grep]"));
        assert!(injected.contains("src/main.rs:1:fn main()"));
    }

    #[test]
    fn integration_compaction_full_cycle() {
        // Simulate the complete compaction lifecycle:
        // 1. Multiple exchanges build up content
        // 2. Compaction boundary computed
        // 3. Pre-boundary events used for compaction prompt
        // 4. Summary stored
        // 5. build_context returns summary + recent events
        // 6. Context injection includes summary prefix
        use crate::app::context_injection::{build_compaction_prompt, build_context_prompt};

        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();

        // Insert 6 exchanges (12 events)
        for i in 1..=6 {
            store
                .append_events(
                    sid,
                    &[
                        DisplayEvent::UserMessage {
                            _uuid: String::new(),
                            content: format!("question {}", i),
                        },
                        DisplayEvent::AssistantText {
                            _uuid: String::new(),
                            _message_id: String::new(),
                            text: format!("answer {}", i),
                        },
                    ],
                )
                .unwrap();
        }

        // Find boundary (keep last 3 user messages)
        let boundary = store.compaction_boundary(sid, 1, 3).unwrap().unwrap();
        // UserMessages at seqs 1, 3, 5, 7, 9, 11
        // keep=3 → keep 7, 9, 11 → boundary = 7 - 1 = 6
        assert_eq!(boundary, 6);

        // Load pre-boundary events for compaction
        let pre_events = store.load_events_range(sid, 1, boundary).unwrap();
        assert_eq!(pre_events.len(), 6); // q1, a1, q2, a2, q3, a3

        // Build compaction prompt (what the summarization agent would see)
        let compact_payload = ContextPayload {
            compaction_summary: None,
            events: pre_events,
        };
        let compact_prompt = build_compaction_prompt(&compact_payload);
        assert!(compact_prompt.contains("question 1"));
        assert!(compact_prompt.contains("answer 3"));
        assert!(!compact_prompt.contains("question 4")); // not in pre-boundary

        // Simulate: compaction agent returns summary
        let summary = "User asked 3 questions about the system. All were answered.";
        store.store_compaction(sid, boundary, summary).unwrap();

        // Now build_context should return summary + post-boundary events
        let payload = store.build_context(sid).unwrap().unwrap();
        assert_eq!(payload.compaction_summary.as_deref(), Some(summary));
        assert_eq!(payload.events.len(), 6); // q4, a4, q5, a5, q6, a6

        // Context injection should include summary prefix + recent events
        let injected = build_context_prompt(&payload, "question 7");
        assert!(injected.contains("[Previous conversation summary]"));
        assert!(injected.contains(summary));
        assert!(injected.contains("[Conversation continues]"));
        assert!(injected.contains("question 4"));
        assert!(injected.contains("answer 6"));
        assert!(injected.ends_with("question 7"));
        // Old questions should NOT appear (they're summarized)
        assert!(!injected.contains("question 1"));
    }

    #[test]
    fn integration_multiple_compactions() {
        // Verify that a second compaction replaces the first in build_context
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();

        // First batch: 5 exchanges
        insert_conversation(&store, sid, 5);
        store.store_compaction(sid, 4, "First summary").unwrap();

        // Second batch: 5 more exchanges (seqs 11..=20)
        for i in 6..=10 {
            store
                .append_events(
                    sid,
                    &[
                        DisplayEvent::UserMessage {
                            _uuid: String::new(),
                            content: format!("prompt {}", i),
                        },
                        DisplayEvent::AssistantText {
                            _uuid: String::new(),
                            _message_id: String::new(),
                            text: format!("reply {}", i),
                        },
                    ],
                )
                .unwrap();
        }

        // Second compaction at seq 14
        store
            .store_compaction(sid, 14, "Second summary (includes first)")
            .unwrap();

        // build_context should use the latest compaction
        let payload = store.build_context(sid).unwrap().unwrap();
        assert_eq!(
            payload.compaction_summary.as_deref(),
            Some("Second summary (includes first)")
        );

        // Events should be from seq 15 onward only
        let events = &payload.events;
        // Verify first event is from a later prompt (not prompt 1-7)
        if let DisplayEvent::UserMessage { content, .. } = &events[0] {
            assert!(content.starts_with("prompt "));
            let num: usize = content.strip_prefix("prompt ").unwrap().parse().unwrap();
            assert!(num >= 8, "expected prompt 8+, got prompt {}", num);
        } else {
            panic!("expected UserMessage first");
        }
    }

    #[test]
    fn integration_strip_preserves_multiline_user_prompt() {
        // Verify stripping works with complex multi-line prompts
        use crate::app::context_injection::{build_context_prompt, strip_injected_context};

        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();
        store
            .append_events(
                sid,
                &[
                    DisplayEvent::UserMessage {
                        _uuid: String::new(),
                        content: "hello".into(),
                    },
                    DisplayEvent::AssistantText {
                        _uuid: String::new(),
                        _message_id: String::new(),
                        text: "hi".into(),
                    },
                ],
            )
            .unwrap();

        let multiline_prompt =
            "fix this:\n\n```rust\nfn main() {\n    panic!()\n}\n```\n\nalso update tests";
        let payload = store.build_context(sid).unwrap().unwrap();
        let injected = build_context_prompt(&payload, multiline_prompt);
        let stripped = strip_injected_context(&injected);
        assert_eq!(stripped, multiline_prompt);
    }

    #[test]
    fn integration_compaction_threshold_detection() {
        // Verify total_chars_since_compaction correctly tracks accumulation
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();

        // Insert a large message (simulate 100K chars worth of content)
        let big_msg = "x".repeat(200_000);
        store
            .append_events(
                sid,
                &[
                    DisplayEvent::UserMessage {
                        _uuid: String::new(),
                        content: big_msg.clone(),
                    },
                    DisplayEvent::AssistantText {
                        _uuid: String::new(),
                        _message_id: String::new(),
                        text: big_msg,
                    },
                ],
            )
            .unwrap();

        let chars = store.total_chars_since_compaction(sid).unwrap();
        assert_eq!(chars, 400_000);
        assert!(chars >= COMPACTION_THRESHOLD);

        // After compaction, counter resets
        store
            .store_compaction(sid, store.max_seq(sid).unwrap(), "summary")
            .unwrap();
        let chars_after = store.total_chars_since_compaction(sid).unwrap();
        assert_eq!(chars_after, 0);

        // New events accumulate fresh
        store
            .append_events(
                sid,
                &[DisplayEvent::UserMessage {
                    _uuid: String::new(),
                    content: "short".into(),
                }],
            )
            .unwrap();
        let chars_new = store.total_chars_since_compaction(sid).unwrap();
        assert_eq!(chars_new, 5);
    }

    #[test]
    fn integration_empty_session_context_injection_passthrough() {
        // First prompt on a new session: no context, prompt passes through unchanged
        use crate::app::context_injection::build_context_prompt;

        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();

        let payload = store.build_context(sid).unwrap();
        assert!(payload.is_none());

        // When payload is None, the caller doesn't inject — prompt goes through raw.
        // Simulate that logic:
        let prompt = "hello world";
        let result = match payload {
            Some(p) => build_context_prompt(&p, prompt),
            None => prompt.to_string(),
        };
        assert_eq!(result, "hello world");
    }

    #[test]
    fn integration_compaction_only_summary_no_events() {
        // Edge case: all events are compacted, only summary remains
        use crate::app::context_injection::build_context_prompt;

        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();

        store
            .append_events(
                sid,
                &[
                    DisplayEvent::UserMessage {
                        _uuid: String::new(),
                        content: "old work".into(),
                    },
                    DisplayEvent::AssistantText {
                        _uuid: String::new(),
                        _message_id: String::new(),
                        text: "done".into(),
                    },
                ],
            )
            .unwrap();
        store
            .store_compaction(
                sid,
                store.max_seq(sid).unwrap(),
                "All prior work summarized.",
            )
            .unwrap();

        let payload = store.build_context(sid).unwrap().unwrap();
        assert_eq!(
            payload.compaction_summary.as_deref(),
            Some("All prior work summarized.")
        );
        assert!(payload.events.is_empty());

        let injected = build_context_prompt(&payload, "continue");
        assert!(injected.contains("[Previous conversation summary]"));
        assert!(injected.contains("All prior work summarized."));
        assert!(injected.ends_with("continue"));
    }

    #[test]
    fn integration_store_compact_event_reduces_size() {
        // Verify that storing compacted events uses less space than raw
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();

        // Big Read result (100 lines)
        let big_content = (0..100)
            .map(|i| format!("line {}: {}", i, "x".repeat(80)))
            .collect::<Vec<_>>()
            .join("\n");
        store
            .append_events(
                sid,
                &[DisplayEvent::ToolResult {
                    tool_use_id: "t".into(),
                    tool_name: "Read".into(),
                    file_path: None,
                    content: big_content.clone(),
                    is_error: false,
                }],
            )
            .unwrap();

        // Load back — should be compacted (first + last line + count)
        let loaded = store.load_events(sid).unwrap();
        match &loaded[0] {
            DisplayEvent::ToolResult { content, .. } => {
                assert!(
                    content.len() < big_content.len() / 2,
                    "compacted content should be much smaller"
                );
                assert!(content.contains("(100 lines)"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn integration_three_prompt_session_full_flow() {
        // Simulate a realistic 3-prompt session lifecycle
        use crate::app::context_injection::{build_context_prompt, strip_injected_context};

        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("feat-x").unwrap();

        // === Prompt 1: no context ===
        let p1 = "create a new module";
        // No payload for first prompt
        assert!(store.build_context(sid).unwrap().is_none());

        // Store prompt 1 exchange
        store.append_events(sid, &[
            DisplayEvent::UserMessage { _uuid: String::new(), content: p1.into() },
            DisplayEvent::AssistantText { _uuid: String::new(), _message_id: String::new(), text: "Created src/module.rs".into() },
            DisplayEvent::ToolCall {
                _uuid: String::new(), tool_use_id: "tc1".into(), tool_name: "Write".into(),
                file_path: Some("/src/module.rs".into()),
                input: serde_json::json!({"file_path": "/src/module.rs", "content": "pub fn hello() {}"}),
            },
            DisplayEvent::ToolResult {
                tool_use_id: "tc1".into(), tool_name: "Write".into(),
                file_path: Some("/src/module.rs".into()), content: "OK".into(), is_error: false,
            },
            DisplayEvent::Complete { _session_id: String::new(), success: true, duration_ms: 2000, cost_usd: 0.01 },
        ]).unwrap();

        // === Prompt 2: with context from prompt 1 ===
        let p2 = "add tests for the module";
        let payload2 = store.build_context(sid).unwrap().unwrap();
        assert_eq!(payload2.events.len(), 5);
        let injected2 = build_context_prompt(&payload2, p2);
        assert!(injected2.contains("create a new module"));
        let stripped2 = strip_injected_context(&injected2);
        assert_eq!(stripped2, p2);

        // Store prompt 2 exchange (with clean prompt)
        store
            .append_events(
                sid,
                &[
                    DisplayEvent::UserMessage {
                        _uuid: String::new(),
                        content: stripped2.to_string(),
                    },
                    DisplayEvent::AssistantText {
                        _uuid: String::new(),
                        _message_id: String::new(),
                        text: "Added tests.".into(),
                    },
                    DisplayEvent::Complete {
                        _session_id: String::new(),
                        success: true,
                        duration_ms: 1500,
                        cost_usd: 0.008,
                    },
                ],
            )
            .unwrap();

        // === Prompt 3: with context from prompts 1+2 ===
        let p3 = "run cargo test";
        let payload3 = store.build_context(sid).unwrap().unwrap();
        assert_eq!(payload3.events.len(), 8); // 5 from p1 + 3 from p2
        let injected3 = build_context_prompt(&payload3, p3);
        assert!(injected3.contains("create a new module"));
        assert!(injected3.contains("add tests for the module"));
        assert!(injected3.contains("Added tests."));
        let stripped3 = strip_injected_context(&injected3);
        assert_eq!(stripped3, p3);

        // Verify total event count in store
        assert_eq!(store.load_events(sid).unwrap().len(), 8);
        assert_eq!(store.message_count(sid).unwrap(), 4); // 3 user + 1 assistant (UserMessage + AssistantText kinds)
    }
}
