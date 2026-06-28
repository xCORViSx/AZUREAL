//! Session-independent prompt input history.
//!
//! The prompt box uses this store for Up/Down navigation instead of reading
//! `UserMessage` events from the currently viewed transcript. That lets a fresh
//! session recall prompts that were sent before the session was created.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::app::App;

/// Number of most-recent prompts retained in the default history file.
pub(crate) const DEFAULT_PROMPT_HISTORY_LIMIT: usize = 200;

/// Serialized shape written to disk for prompt input history.
#[derive(Debug, Default, Deserialize, Serialize)]
struct PromptHistoryFile {
    /// Prompts in oldest-to-newest order.
    entries: Vec<String>,
}

/// Accepted on-disk formats for prompt history.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PromptHistoryDisk {
    /// Current object-based format.
    Current(PromptHistoryFile),
    /// Legacy bare-array format accepted for forgiving upgrades.
    Legacy(Vec<String>),
}

/// Persistent, bounded history for prompts submitted through the prompt box.
#[derive(Debug, Clone)]
pub(crate) struct PromptHistoryStore {
    path: Option<PathBuf>,
    entries: Vec<String>,
    limit: usize,
}

/// Loading, mutation, and persistence methods for prompt history.
impl PromptHistoryStore {
    /// Build the default prompt history store used by normal application runs.
    #[cfg(not(test))]
    pub(crate) fn load_default() -> Self {
        let path = Self::default_path();
        Self::load_at(path.clone(), DEFAULT_PROMPT_HISTORY_LIMIT)
            .unwrap_or_else(|_| Self::empty(Some(path), DEFAULT_PROMPT_HISTORY_LIMIT))
    }

    /// Build an in-memory prompt history store for tests.
    #[cfg(test)]
    pub(crate) fn load_default() -> Self {
        Self::in_memory(DEFAULT_PROMPT_HISTORY_LIMIT)
    }

    /// Return the default path for persisted prompt history.
    #[cfg(not(test))]
    pub(crate) fn default_path() -> PathBuf {
        crate::config::config_dir().join("prompt_history.json")
    }

    /// Load a prompt history store from a path, treating missing or invalid data as empty history.
    pub(crate) fn load_at(path: impl Into<PathBuf>, limit: usize) -> Result<Self> {
        let path = path.into();
        let limit = normalize_limit(limit);
        let entries = match std::fs::read_to_string(&path) {
            Ok(raw) => parse_entries(&raw, limit),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(err) => return Err(err.into()),
        };
        Ok(Self {
            path: Some(path),
            entries,
            limit,
        })
    }

    /// Create an in-memory history store that never writes to disk.
    #[cfg(test)]
    pub(crate) fn in_memory(limit: usize) -> Self {
        Self::empty(None, limit)
    }

    /// Return the stored prompts in oldest-to-newest order.
    pub(crate) fn entries(&self) -> &[String] {
        &self.entries
    }

    /// Record a submitted prompt, returning whether it was accepted into history.
    pub(crate) fn record(&mut self, prompt: &str) -> Result<bool> {
        let Some(prompt) = normalize_prompt(prompt) else {
            return Ok(false);
        };
        let mut entries = self.entries.clone();
        remove_prompt_duplicates(&mut entries, &prompt);
        entries.push(prompt);
        enforce_limit_for_entries(&mut entries, self.limit);
        self.save_entries(&entries)?;
        self.entries = entries;
        Ok(true)
    }

    /// Construct an empty history store with an optional backing path.
    fn empty(path: Option<PathBuf>, limit: usize) -> Self {
        Self {
            path,
            entries: Vec::new(),
            limit: normalize_limit(limit),
        }
    }

    /// Persist the supplied entries if this store has a backing path.
    fn save_entries(&self, entries: &[String]) -> Result<()> {
        let Some(path) = self.path.as_ref() else {
            return Ok(());
        };
        let payload = PromptHistoryFile {
            entries: entries.to_vec(),
        };
        write_history_atomically(path, serde_json::to_string_pretty(&payload)?.as_bytes())
    }
}

/// App-level helpers for interacting with prompt history.
impl App {
    /// Record a prompt in the session-independent prompt history store.
    pub(crate) fn record_prompt_history(&mut self, prompt: &str) {
        let _ = self.prompt_history.record(prompt);
    }
}

/// Normalize the history limit so callers cannot create an unusable zero-length store.
fn normalize_limit(limit: usize) -> usize {
    limit.max(1)
}

/// Trim a prompt and reject empty values before they enter history.
fn normalize_prompt(prompt: &str) -> Option<String> {
    let prompt = prompt.trim();
    (!prompt.is_empty()).then(|| prompt.to_string())
}

/// Drop oldest entries from a history vector until it fits within a normalized limit.
fn enforce_limit_for_entries(entries: &mut Vec<String>, limit: usize) {
    let limit = normalize_limit(limit);
    if entries.len() > limit {
        let drop_count = entries.len() - limit;
        entries.drain(0..drop_count);
    }
}

/// Remove older copies of a prompt before a fresh occurrence is appended.
fn remove_prompt_duplicates(entries: &mut Vec<String>, prompt: &str) {
    entries.retain(|entry| entry != prompt);
}

/// Parse history entries from any accepted disk format.
fn parse_entries(raw: &str, limit: usize) -> Vec<String> {
    let entries = serde_json::from_str::<PromptHistoryDisk>(raw)
        .map(|disk| match disk {
            PromptHistoryDisk::Current(file) => file.entries,
            PromptHistoryDisk::Legacy(entries) => entries,
        })
        .unwrap_or_default();
    sanitize_entries(entries, limit)
}

/// Remove invalid prompts and keep only the newest entries within the limit.
fn sanitize_entries(entries: Vec<String>, limit: usize) -> Vec<String> {
    let limit = normalize_limit(limit);
    let mut unique_entries = Vec::new();
    for entry in entries
        .into_iter()
        .filter_map(|entry| normalize_prompt(&entry))
    {
        remove_prompt_duplicates(&mut unique_entries, &entry);
        unique_entries.push(entry);
    }
    enforce_limit_for_entries(&mut unique_entries, limit);
    unique_entries
}

/// Write prompt history through a temporary sibling so interrupted saves do not corrupt the file.
fn write_history_atomically(path: &Path, payload: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let temp_path = history_temp_path(path);
    let write_result = (|| -> Result<()> {
        let mut file = std::fs::File::create(&temp_path)?;
        file.write_all(payload)?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(err) = write_result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(err);
    }

    #[cfg(windows)]
    if path.exists() {
        std::fs::remove_file(path)?;
    }

    if let Err(err) = std::fs::rename(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(err.into());
    }

    Ok(())
}

/// Build a unique temporary sibling path for a prompt history write.
fn history_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("prompt_history.json");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    path.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce))
}

#[cfg(test)]
/// Tests for prompt history persistence, normalization, and app integration.
mod tests {
    use super::*;

    /// Return a unique file path under the system temp directory for store tests.
    fn temp_history_path(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "azureal-prompt-history-{}-{}-{}.json",
            name,
            std::process::id(),
            unique
        ))
    }

    /// Missing files should load as empty history while preserving the path for later writes.
    #[test]
    fn load_at_missing_file_starts_empty() {
        let path = temp_history_path("missing");
        let store = PromptHistoryStore::load_at(&path, 20).unwrap();
        assert!(store.entries().is_empty());
        assert_eq!(store.path.as_ref(), Some(&path));
    }

    /// Recording a prompt should trim it, persist it, and reload it in insertion order.
    #[test]
    fn record_persists_trimmed_prompt() {
        let path = temp_history_path("record");
        let mut store = PromptHistoryStore::load_at(&path, 20).unwrap();
        assert!(store.record("  build feature  ").unwrap());

        let reloaded = PromptHistoryStore::load_at(&path, 20).unwrap();
        assert_eq!(reloaded.entries(), &["build feature".to_string()]);

        let _ = std::fs::remove_file(path);
    }

    /// Recording over an existing history file should replace it without leaving temp files.
    #[test]
    fn record_replaces_existing_file_without_temp_artifacts() {
        let path = temp_history_path("replace");
        std::fs::write(&path, r#"{"entries":["old"]}"#).unwrap();
        let mut store = PromptHistoryStore::load_at(&path, 20).unwrap();

        assert!(store.record("new").unwrap());

        let reloaded = PromptHistoryStore::load_at(&path, 20).unwrap();
        assert_eq!(reloaded.entries(), &["old".to_string(), "new".to_string()]);

        let parent = path.parent().unwrap();
        let file_name = path.file_name().unwrap().to_string_lossy();
        let temp_prefix = format!(".{file_name}.");
        let leftovers: Vec<_> = std::fs::read_dir(parent)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.starts_with(&temp_prefix) && name.ends_with(".tmp")
            })
            .collect();
        assert!(leftovers.is_empty());

        let _ = std::fs::remove_file(path);
    }

    /// Failed persistence should leave the in-memory history unchanged.
    #[test]
    fn record_does_not_mutate_when_save_fails() {
        let path = temp_history_path("save-fails");
        std::fs::create_dir_all(&path).unwrap();
        let mut store = PromptHistoryStore {
            path: Some(path.clone()),
            entries: vec!["old".to_string()],
            limit: 20,
        };

        assert!(store.record("new").is_err());
        assert_eq!(store.entries(), &["old".to_string()]);

        let _ = std::fs::remove_dir(path);
    }

    /// Blank prompts should be ignored instead of creating empty history entries.
    #[test]
    fn record_ignores_blank_prompt() {
        let mut store = PromptHistoryStore::in_memory(20);
        assert!(!store.record(" \n\t ").unwrap());
        assert!(store.entries().is_empty());
    }

    /// Stores should drop oldest prompts once the configured limit is reached.
    #[test]
    fn record_keeps_most_recent_limit() {
        let mut store = PromptHistoryStore::in_memory(2);
        store.record("first").unwrap();
        store.record("second").unwrap();
        store.record("third").unwrap();
        assert_eq!(
            store.entries(),
            &["second".to_string(), "third".to_string()]
        );
    }

    /// Re-recording an existing prompt should move it to newest without duplicating it.
    #[test]
    fn record_moves_duplicate_prompt_to_newest() {
        let mut store = PromptHistoryStore::in_memory(20);
        store.record("build").unwrap();
        store.record("continue").unwrap();
        store.record("test").unwrap();
        store.record("continue").unwrap();

        assert_eq!(
            store.entries(),
            &[
                "build".to_string(),
                "test".to_string(),
                "continue".to_string()
            ]
        );
    }

    /// Duplicate removal should happen before limit trimming so older unique prompts survive.
    #[test]
    fn record_deduplicates_before_enforcing_limit() {
        let mut store = PromptHistoryStore::in_memory(3);
        store.record("inspect").unwrap();
        store.record("continue").unwrap();
        store.record("test").unwrap();
        store.record("continue").unwrap();

        assert_eq!(
            store.entries(),
            &[
                "inspect".to_string(),
                "test".to_string(),
                "continue".to_string()
            ]
        );
    }

    /// Corrupt JSON should be treated as empty history rather than blocking startup.
    #[test]
    fn load_at_corrupt_file_starts_empty() {
        let path = temp_history_path("corrupt");
        std::fs::write(&path, "{not json").unwrap();

        let store = PromptHistoryStore::load_at(&path, 20).unwrap();
        assert!(store.entries().is_empty());

        let _ = std::fs::remove_file(path);
    }

    /// Loading should collapse older duplicate prompts while preserving newest unique order.
    #[test]
    fn load_at_deduplicates_persisted_entries() {
        let path = temp_history_path("dedupe");
        std::fs::write(
            &path,
            r#"{"entries":["inspect","continue","test","continue"," inspect "]}"#,
        )
        .unwrap();

        let store = PromptHistoryStore::load_at(&path, 20).unwrap();
        assert_eq!(
            store.entries(),
            &[
                "test".to_string(),
                "continue".to_string(),
                "inspect".to_string()
            ]
        );

        let _ = std::fs::remove_file(path);
    }

    /// Legacy bare-array files should still load and sanitize prompt entries.
    #[test]
    fn load_at_accepts_legacy_array_format() {
        let path = temp_history_path("legacy");
        std::fs::write(&path, r#"[" first ","   ","second"]"#).unwrap();

        let store = PromptHistoryStore::load_at(&path, 20).unwrap();
        assert_eq!(
            store.entries(),
            &["first".to_string(), "second".to_string()]
        );

        let _ = std::fs::remove_file(path);
    }

    /// Zero limits should normalize to one retained prompt.
    #[test]
    fn zero_limit_keeps_one_prompt() {
        let mut store = PromptHistoryStore::in_memory(0);
        store.record("first").unwrap();
        store.record("second").unwrap();
        assert_eq!(store.entries(), &["second".to_string()]);
    }

    /// App-level recording should feed the same store used by input history navigation.
    #[test]
    fn app_record_prompt_history_adds_entry() {
        let mut app = App::new();
        app.record_prompt_history("  sent prompt  ");
        assert_eq!(app.prompt_history.entries(), &["sent prompt".to_string()]);
    }
}
