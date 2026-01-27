use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

use crate::config::database_path;
use crate::migrations::MigrationRunner;
use crate::models::{
    ConversationMessage, MessageType, OutputType, Project, Session, SessionOutput, SessionStatus,
};

/// Database wrapper for Azural
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open or create the database
    pub fn open() -> Result<Self> {
        let path = database_path();
        let conn = Connection::open(&path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;

        // Enable foreign keys
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        Ok(Self { conn })
    }

    /// Run database migrations
    pub fn migrate(&self) -> Result<()> {
        let runner = MigrationRunner::new(&self.conn);
        let applied = runner.run_pending()?;
        if applied > 0 {
            tracing::info!("Applied {} database migration(s)", applied);
        }
        Ok(())
    }

    // ==================== Projects ====================

    /// Get or create a project for the given path
    pub fn get_or_create_project(&self, path: &Path) -> Result<Project> {
        let path_str = path.to_string_lossy();

        // Try to find existing project
        if let Some(project) = self.get_project_by_path(path)? {
            return Ok(project);
        }

        // Create new project
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed".to_string());

        let now = Utc::now();
        self.conn.execute(
            "INSERT INTO projects (name, path, main_branch, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, path_str.as_ref(), "main", now.to_rfc3339(), now.to_rfc3339()],
        )?;

        let id = self.conn.last_insert_rowid();
        Ok(Project {
            id,
            name,
            path: path.to_path_buf(),
            system_prompt: None,
            main_branch: "main".to_string(),
            created_at: now,
            updated_at: now,
        })
    }

    /// Get a project by path
    pub fn get_project_by_path(&self, path: &Path) -> Result<Option<Project>> {
        let path_str = path.to_string_lossy();
        self.conn
            .query_row(
                "SELECT id, name, path, system_prompt, main_branch, created_at, updated_at FROM projects WHERE path = ?1",
                params![path_str.as_ref()],
                |row| {
                    Ok(Project {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        path: PathBuf::from(row.get::<_, String>(2)?),
                        system_prompt: row.get(3)?,
                        main_branch: row.get(4)?,
                        created_at: parse_datetime(&row.get::<_, String>(5)?),
                        updated_at: parse_datetime(&row.get::<_, String>(6)?),
                    })
                },
            )
            .optional()
            .context("Failed to query project")
    }

    /// Get a project by ID
    pub fn get_project(&self, id: i64) -> Result<Option<Project>> {
        self.conn
            .query_row(
                "SELECT id, name, path, system_prompt, main_branch, created_at, updated_at FROM projects WHERE id = ?1",
                params![id],
                |row| {
                    Ok(Project {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        path: PathBuf::from(row.get::<_, String>(2)?),
                        system_prompt: row.get(3)?,
                        main_branch: row.get(4)?,
                        created_at: parse_datetime(&row.get::<_, String>(5)?),
                        updated_at: parse_datetime(&row.get::<_, String>(6)?),
                    })
                },
            )
            .optional()
            .context("Failed to query project")
    }

    /// List all projects
    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, system_prompt, main_branch, created_at, updated_at FROM projects ORDER BY updated_at DESC",
        )?;

        let projects = stmt
            .query_map([], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: PathBuf::from(row.get::<_, String>(2)?),
                    system_prompt: row.get(3)?,
                    main_branch: row.get(4)?,
                    created_at: parse_datetime(&row.get::<_, String>(5)?),
                    updated_at: parse_datetime(&row.get::<_, String>(6)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(projects)
    }

    /// Update project name
    pub fn update_project_name(&self, id: i64, name: &str) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            "UPDATE projects SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![name, now.to_rfc3339(), id],
        )?;
        Ok(())
    }

    /// Update project system prompt
    pub fn update_project_system_prompt(&self, id: i64, system_prompt: Option<&str>) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            "UPDATE projects SET system_prompt = ?1, updated_at = ?2 WHERE id = ?3",
            params![system_prompt, now.to_rfc3339(), id],
        )?;
        Ok(())
    }

    /// Update project main branch
    pub fn update_project_main_branch(&self, id: i64, main_branch: &str) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            "UPDATE projects SET main_branch = ?1, updated_at = ?2 WHERE id = ?3",
            params![main_branch, now.to_rfc3339(), id],
        )?;
        Ok(())
    }

    /// Delete a project and all its sessions (cascading delete via foreign key)
    pub fn delete_project(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Count sessions for a project
    pub fn count_sessions_for_project(&self, project_id: i64) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    // ==================== Sessions ====================

    /// Create a new session
    pub fn create_session(&self, session: &Session) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            "INSERT INTO sessions (id, name, initial_prompt, worktree_name, worktree_path, branch_name, status, project_id, archived, created_at, updated_at, claude_session_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                session.id,
                session.name,
                session.initial_prompt,
                session.worktree_name,
                session.worktree_path.to_string_lossy().as_ref(),
                session.branch_name,
                session.status.as_str(),
                session.project_id,
                session.archived,
                now.to_rfc3339(),
                now.to_rfc3339(),
                session.claude_session_id,
            ],
        )?;
        Ok(())
    }

    /// Ensure a session exists in DB (for foreign key constraints on session_outputs)
    pub fn ensure_session(&self, session: &Session) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            "INSERT OR IGNORE INTO sessions (id, name, initial_prompt, worktree_name, worktree_path, branch_name, status, project_id, archived, created_at, updated_at, claude_session_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                session.id,
                session.name,
                session.initial_prompt,
                session.worktree_name,
                session.worktree_path.to_string_lossy().as_ref(),
                session.branch_name,
                session.status.as_str(),
                session.project_id,
                session.archived,
                now.to_rfc3339(),
                now.to_rfc3339(),
                session.claude_session_id,
            ],
        )?;
        Ok(())
    }

    /// Get a session by ID
    pub fn get_session(&self, id: &str) -> Result<Option<Session>> {
        self.conn
            .query_row(
                "SELECT id, name, initial_prompt, worktree_name, worktree_path, branch_name, status, project_id, pid, exit_code, archived, created_at, updated_at, claude_session_id
                 FROM sessions WHERE id = ?1",
                params![id],
                |row| Ok(row_to_session(row)),
            )
            .optional()
            .context("Failed to query session")
    }

    /// List all sessions (optionally filtered by project)
    pub fn list_sessions(&self) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, initial_prompt, worktree_name, worktree_path, branch_name, status, project_id, pid, exit_code, archived, created_at, updated_at, claude_session_id
             FROM sessions WHERE archived = 0 ORDER BY updated_at DESC",
        )?;

        let sessions = stmt
            .query_map([], |row| Ok(row_to_session(row)))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(sessions)
    }

    /// List sessions for a specific project
    pub fn list_sessions_for_project(&self, project_id: i64) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, initial_prompt, worktree_name, worktree_path, branch_name, status, project_id, pid, exit_code, archived, created_at, updated_at, claude_session_id
             FROM sessions WHERE project_id = ?1 AND archived = 0 ORDER BY updated_at DESC",
        )?;

        let sessions = stmt
            .query_map(params![project_id], |row| Ok(row_to_session(row)))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(sessions)
    }

    /// List archived sessions
    pub fn list_archived_sessions(&self) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, initial_prompt, worktree_name, worktree_path, branch_name, status, project_id, pid, exit_code, archived, created_at, updated_at, claude_session_id
             FROM sessions WHERE archived = 1 ORDER BY updated_at DESC",
        )?;

        let sessions = stmt
            .query_map([], |row| Ok(row_to_session(row)))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(sessions)
    }

    /// List archived sessions for a specific project
    pub fn list_archived_sessions_for_project(&self, project_id: i64) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, initial_prompt, worktree_name, worktree_path, branch_name, status, project_id, pid, exit_code, archived, created_at, updated_at, claude_session_id
             FROM sessions WHERE project_id = ?1 AND archived = 1 ORDER BY updated_at DESC",
        )?;

        let sessions = stmt
            .query_map(params![project_id], |row| Ok(row_to_session(row)))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(sessions)
    }

    /// List sessions eligible for cleanup (completed, failed, or archived)
    pub fn list_cleanable_sessions(&self, project_id: i64) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, initial_prompt, worktree_name, worktree_path, branch_name, status, project_id, pid, exit_code, archived, created_at, updated_at, claude_session_id
             FROM sessions
             WHERE project_id = ?1 AND (status IN ('completed', 'failed', 'stopped') OR archived = 1)
             ORDER BY updated_at DESC",
        )?;

        let sessions = stmt
            .query_map(params![project_id], |row| Ok(row_to_session(row)))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(sessions)
    }

    /// Update session status
    pub fn update_session_status(&self, id: &str, status: SessionStatus) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            "UPDATE sessions SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status.as_str(), now.to_rfc3339(), id],
        )?;
        Ok(())
    }

    /// Update session PID
    pub fn update_session_pid(&self, id: &str, pid: Option<u32>) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            "UPDATE sessions SET pid = ?1, updated_at = ?2 WHERE id = ?3",
            params![pid, now.to_rfc3339(), id],
        )?;
        Ok(())
    }

    /// Update session name
    pub fn update_session_name(&self, id: &str, name: &str) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            "UPDATE sessions SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![name, now.to_rfc3339(), id],
        )?;
        Ok(())
    }

    /// Update session exit code
    pub fn update_session_exit_code(&self, id: &str, exit_code: Option<i32>) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            "UPDATE sessions SET exit_code = ?1, updated_at = ?2 WHERE id = ?3",
            params![exit_code, now.to_rfc3339(), id],
        )?;
        Ok(())
    }

    /// Update session Claude session ID (for --resume)
    pub fn update_session_claude_id(&self, id: &str, claude_session_id: Option<&str>) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            "UPDATE sessions SET claude_session_id = ?1, updated_at = ?2 WHERE id = ?3",
            params![claude_session_id, now.to_rfc3339(), id],
        )?;
        Ok(())
    }

    /// Archive a session
    pub fn archive_session(&self, id: &str) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            "UPDATE sessions SET archived = 1, updated_at = ?1 WHERE id = ?2",
            params![now.to_rfc3339(), id],
        )?;
        Ok(())
    }

    /// Unarchive a session
    pub fn unarchive_session(&self, id: &str) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            "UPDATE sessions SET archived = 0, updated_at = ?1 WHERE id = ?2",
            params![now.to_rfc3339(), id],
        )?;
        Ok(())
    }

    /// Delete a session
    pub fn delete_session(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Search sessions by name (case-insensitive)
    pub fn search_sessions_by_name(&self, query: &str) -> Result<Vec<Session>> {
        let pattern = format!("%{}%", query);
        let mut stmt = self.conn.prepare(
            "SELECT id, name, initial_prompt, worktree_name, worktree_path, branch_name, status, project_id, pid, exit_code, archived, created_at, updated_at, claude_session_id
             FROM sessions WHERE name LIKE ?1 COLLATE NOCASE ORDER BY updated_at DESC",
        )?;

        let sessions = stmt
            .query_map(params![pattern], |row| Ok(row_to_session(row)))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(sessions)
    }

    /// Filter sessions by status
    pub fn filter_sessions_by_status(&self, status: SessionStatus) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, initial_prompt, worktree_name, worktree_path, branch_name, status, project_id, pid, exit_code, archived, created_at, updated_at, claude_session_id
             FROM sessions WHERE status = ?1 ORDER BY updated_at DESC",
        )?;

        let sessions = stmt
            .query_map(params![status.as_str()], |row| Ok(row_to_session(row)))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(sessions)
    }

    /// Filter sessions by status for a specific project
    pub fn filter_sessions_by_status_for_project(
        &self,
        project_id: i64,
        status: SessionStatus,
    ) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, initial_prompt, worktree_name, worktree_path, branch_name, status, project_id, pid, exit_code, archived, created_at, updated_at, claude_session_id
             FROM sessions WHERE project_id = ?1 AND status = ?2 ORDER BY updated_at DESC",
        )?;

        let sessions = stmt
            .query_map(params![project_id, status.as_str()], |row| {
                Ok(row_to_session(row))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(sessions)
    }

    /// Get session by worktree name
    pub fn get_session_by_worktree_name(&self, worktree_name: &str) -> Result<Option<Session>> {
        self.conn
            .query_row(
                "SELECT id, name, initial_prompt, worktree_name, worktree_path, branch_name, status, project_id, pid, exit_code, archived, created_at, updated_at, claude_session_id
                 FROM sessions WHERE worktree_name = ?1",
                params![worktree_name],
                |row| Ok(row_to_session(row)),
            )
            .optional()
            .context("Failed to query session by worktree name")
    }

    /// Get session by branch name
    pub fn get_session_by_branch_name(&self, branch_name: &str) -> Result<Option<Session>> {
        self.conn
            .query_row(
                "SELECT id, name, initial_prompt, worktree_name, worktree_path, branch_name, status, project_id, pid, exit_code, archived, created_at, updated_at, claude_session_id
                 FROM sessions WHERE branch_name = ?1",
                params![branch_name],
                |row| Ok(row_to_session(row)),
            )
            .optional()
            .context("Failed to query session by branch name")
    }

    // ==================== Session Outputs ====================

    /// Add output to a session
    pub fn add_session_output(
        &self,
        session_id: &str,
        output_type: OutputType,
        data: &str,
    ) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            "INSERT INTO session_outputs (session_id, output_type, data, timestamp) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, output_type.as_str(), data, now.to_rfc3339()],
        )?;
        Ok(())
    }

    /// Get outputs for a session
    pub fn get_session_outputs(&self, session_id: &str) -> Result<Vec<SessionOutput>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, output_type, data, timestamp FROM session_outputs WHERE session_id = ?1 ORDER BY timestamp ASC",
        )?;

        let outputs = stmt
            .query_map(params![session_id], |row| {
                Ok(SessionOutput {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    output_type: OutputType::from_str(&row.get::<_, String>(2)?),
                    data: row.get(3)?,
                    timestamp: parse_datetime(&row.get::<_, String>(4)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(outputs)
    }

    /// Get outputs for a session filtered by type
    pub fn get_session_outputs_by_type(
        &self,
        session_id: &str,
        output_type: OutputType,
    ) -> Result<Vec<SessionOutput>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, output_type, data, timestamp FROM session_outputs WHERE session_id = ?1 AND output_type = ?2 ORDER BY timestamp ASC",
        )?;

        let outputs = stmt
            .query_map(params![session_id, output_type.as_str()], |row| {
                Ok(SessionOutput {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    output_type: OutputType::from_str(&row.get::<_, String>(2)?),
                    data: row.get(3)?,
                    timestamp: parse_datetime(&row.get::<_, String>(4)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(outputs)
    }

    /// Clear all outputs for a session
    pub fn clear_session_outputs(&self, session_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM session_outputs WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    /// Count outputs for a session
    pub fn count_session_outputs(&self, session_id: &str) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM session_outputs WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    // ==================== Conversation Messages ====================

    /// Add a conversation message
    pub fn add_conversation_message(
        &self,
        session_id: &str,
        message_type: MessageType,
        content: &str,
    ) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            "INSERT INTO conversation_messages (session_id, message_type, content, timestamp) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, message_type.as_str(), content, now.to_rfc3339()],
        )?;
        Ok(())
    }

    /// Get conversation messages for a session
    pub fn get_conversation_messages(&self, session_id: &str) -> Result<Vec<ConversationMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, message_type, content, timestamp FROM conversation_messages WHERE session_id = ?1 ORDER BY timestamp ASC",
        )?;

        let messages = stmt
            .query_map(params![session_id], |row| {
                Ok(ConversationMessage {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    message_type: MessageType::from_str(&row.get::<_, String>(2)?),
                    content: row.get(3)?,
                    timestamp: parse_datetime(&row.get::<_, String>(4)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(messages)
    }

    /// Get conversation messages for a session filtered by type
    pub fn get_conversation_messages_by_type(
        &self,
        session_id: &str,
        message_type: MessageType,
    ) -> Result<Vec<ConversationMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, message_type, content, timestamp FROM conversation_messages WHERE session_id = ?1 AND message_type = ?2 ORDER BY timestamp ASC",
        )?;

        let messages = stmt
            .query_map(params![session_id, message_type.as_str()], |row| {
                Ok(ConversationMessage {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    message_type: MessageType::from_str(&row.get::<_, String>(2)?),
                    content: row.get(3)?,
                    timestamp: parse_datetime(&row.get::<_, String>(4)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(messages)
    }

    /// Clear all conversation messages for a session
    pub fn clear_conversation_messages(&self, session_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM conversation_messages WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    /// Count conversation messages for a session
    pub fn count_conversation_messages(&self, session_id: &str) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM conversation_messages WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Get the last conversation message for a session
    pub fn get_last_conversation_message(
        &self,
        session_id: &str,
    ) -> Result<Option<ConversationMessage>> {
        self.conn
            .query_row(
                "SELECT id, session_id, message_type, content, timestamp FROM conversation_messages WHERE session_id = ?1 ORDER BY timestamp DESC LIMIT 1",
                params![session_id],
                |row| {
                    Ok(ConversationMessage {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        message_type: MessageType::from_str(&row.get::<_, String>(2)?),
                        content: row.get(3)?,
                        timestamp: parse_datetime(&row.get::<_, String>(4)?),
                    })
                },
            )
            .optional()
            .context("Failed to query last conversation message")
    }
}

fn row_to_session(row: &rusqlite::Row) -> Session {
    Session {
        id: row.get(0).unwrap(),
        name: row.get(1).unwrap(),
        initial_prompt: row.get(2).unwrap(),
        worktree_name: row.get(3).unwrap(),
        worktree_path: PathBuf::from(row.get::<_, String>(4).unwrap()),
        branch_name: row.get(5).unwrap(),
        status: SessionStatus::from_str(&row.get::<_, String>(6).unwrap()),
        project_id: row.get(7).unwrap(),
        pid: row.get(8).unwrap(),
        exit_code: row.get(9).unwrap(),
        archived: row.get(10).unwrap(),
        created_at: parse_datetime(&row.get::<_, String>(11).unwrap()),
        updated_at: parse_datetime(&row.get::<_, String>(12).unwrap()),
        claude_session_id: row.get(13).unwrap(),
    }
}

fn parse_datetime(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
