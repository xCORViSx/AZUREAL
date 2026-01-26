//! Database schema migrations
//!
//! Migrations are embedded SQL files that are compiled into the binary.
//! Each migration file is named with a numeric prefix (e.g., 001_initial.sql).
//! Migrations are applied in order and tracked in the schema_migrations table.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

/// A single migration with version and SQL content.
pub struct Migration {
    /// Numeric version (extracted from filename prefix)
    pub version: u32,
    /// Human-readable name (filename without .sql extension)
    pub name: &'static str,
    /// SQL statements to execute
    pub sql: &'static str,
}

/// Returns all registered migrations, sorted by version.
///
/// Add new migrations here by including them with `include_str!`
/// and appending to the vector.
pub fn all_migrations() -> Vec<Migration> {
    vec![
        Migration {
            version: 1,
            name: "001_initial",
            sql: include_str!("001_initial.sql"),
        },
        // Add future migrations here:
        // Migration {
        //     version: 2,
        //     name: "002_add_feature",
        //     sql: include_str!("002_add_feature.sql"),
        // },
    ]
}

/// Runs pending migrations on the database.
pub struct MigrationRunner<'a> {
    conn: &'a Connection,
}

impl<'a> MigrationRunner<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Run all pending migrations.
    ///
    /// Returns the number of migrations applied.
    pub fn run_pending(&self) -> Result<u32> {
        self.ensure_migrations_table()?;

        let applied = self.applied_versions()?;
        let migrations = all_migrations();
        let mut count = 0;

        for migration in migrations {
            if applied.contains(&migration.version) {
                continue;
            }

            // Special handling for migration 001: check if database already exists
            if migration.version == 1 && self.database_already_initialized()? {
                self.mark_as_applied(&migration)?;
                tracing::info!("Marked existing database as migrated: {}", migration.name);
                continue;
            }

            self.apply_migration(&migration)?;
            tracing::info!("Applied migration: {}", migration.name);
            count += 1;
        }

        Ok(count)
    }

    /// Ensure the schema_migrations table exists.
    fn ensure_migrations_table(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at TEXT NOT NULL
            );",
        )?;
        Ok(())
    }

    /// Get list of applied migration versions.
    fn applied_versions(&self) -> Result<Vec<u32>> {
        let mut stmt = self
            .conn
            .prepare("SELECT version FROM schema_migrations ORDER BY version")?;
        let versions = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<u32>, _>>()?;
        Ok(versions)
    }

    /// Check if the database already has tables (pre-migration system).
    fn database_already_initialized(&self) -> Result<bool> {
        let exists: Option<i32> = self
            .conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='projects'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(exists.is_some())
    }

    /// Apply a single migration within a transaction.
    fn apply_migration(&self, migration: &Migration) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        tx.execute_batch(migration.sql)
            .with_context(|| format!("Failed to apply migration {}", migration.name))?;

        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
            params![migration.version, migration.name, Utc::now().to_rfc3339()],
        )?;

        tx.commit()?;
        Ok(())
    }

    /// Mark a migration as applied without running it (for bootstrap).
    fn mark_as_applied(&self, migration: &Migration) -> Result<()> {
        self.conn.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
            params![migration.version, migration.name, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fresh_database_applies_all_migrations() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();

        let runner = MigrationRunner::new(&conn);
        let count = runner.run_pending().unwrap();

        assert_eq!(count, 1);

        // Verify tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"projects".to_string()));
        assert!(tables.contains(&"sessions".to_string()));
        assert!(tables.contains(&"schema_migrations".to_string()));
    }

    #[test]
    fn test_existing_database_marks_as_migrated() {
        let conn = Connection::open_in_memory().unwrap();

        // Simulate existing database by creating projects table
        conn.execute_batch("CREATE TABLE projects (id INTEGER PRIMARY KEY);")
            .unwrap();

        let runner = MigrationRunner::new(&conn);
        let count = runner.run_pending().unwrap();

        // Should mark as applied but not run (count = 0)
        assert_eq!(count, 0);

        // Verify migration is recorded
        let version: u32 = conn
            .query_row("SELECT version FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn test_migrations_are_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();

        let runner = MigrationRunner::new(&conn);

        // Run twice
        let count1 = runner.run_pending().unwrap();
        let count2 = runner.run_pending().unwrap();

        assert_eq!(count1, 1);
        assert_eq!(count2, 0); // No new migrations
    }
}
