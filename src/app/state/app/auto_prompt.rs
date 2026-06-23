//! Per-session auto-prompt repeat state.
//!
//! Auto prompt is keyed by the SQLite session id plus the worktree path that
//! owns that store. This lets independent loops run for multiple sessions in
//! the same worktree, in different worktrees, or across projects.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::App;

/// Stable identity for one auto-prompt-enabled session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AutoPromptKey {
    worktree_path: PathBuf,
    session_id: i64,
}

/// Methods for constructing and inspecting auto-prompt session keys.
impl AutoPromptKey {
    /// Build a key for a session stored in the given worktree path.
    pub fn new(worktree_path: impl Into<PathBuf>, session_id: i64) -> Self {
        Self {
            worktree_path: worktree_path.into(),
            session_id,
        }
    }

    /// Return the worktree path that owns the session store.
    pub fn worktree_path(&self) -> &Path {
        &self.worktree_path
    }

    /// Return the SQLite session id within the worktree store.
    pub fn session_id(&self) -> i64 {
        self.session_id
    }
}

/// Spawn target metadata for a repeated prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoPromptTarget {
    key: AutoPromptKey,
    branch: String,
    project_path: Option<PathBuf>,
}

/// Methods for constructing and inspecting repeat prompt targets.
impl AutoPromptTarget {
    /// Build a target for a session in a branch and optional project.
    pub fn new(
        worktree_path: impl Into<PathBuf>,
        session_id: i64,
        branch: impl Into<String>,
        project_path: Option<PathBuf>,
    ) -> Self {
        Self {
            key: AutoPromptKey::new(worktree_path, session_id),
            branch: branch.into(),
            project_path,
        }
    }

    /// Return the key for the target session.
    pub fn key(&self) -> &AutoPromptKey {
        &self.key
    }

    /// Return the branch name used for process slot grouping.
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Return the project path that owns this target, if it is known.
    pub fn project_path(&self) -> Option<&Path> {
        self.project_path.as_deref()
    }
}

/// Auto-prompt state for one enabled session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoPromptEntry {
    target: AutoPromptTarget,
    prompt: Option<String>,
    tracked_slot: Option<String>,
    pending_after_compaction: bool,
}

/// Methods for reading and mutating one enabled auto-prompt entry.
impl AutoPromptEntry {
    /// Create an enabled entry that will capture the next submitted prompt.
    fn new(target: AutoPromptTarget) -> Self {
        Self {
            target,
            prompt: None,
            tracked_slot: None,
            pending_after_compaction: false,
        }
    }

    /// Return the target session metadata for repeats.
    pub fn target(&self) -> &AutoPromptTarget {
        &self.target
    }

    /// Return the prompt text that will repeat, if it has been captured.
    pub fn prompt(&self) -> Option<&str> {
        self.prompt.as_deref()
    }

    /// Return the agent slot currently watched for turn completion.
    pub fn tracked_slot(&self) -> Option<&str> {
        self.tracked_slot.as_deref()
    }

    /// Return whether the repeat is waiting for compaction to finish.
    pub fn is_pending_after_compaction(&self) -> bool {
        self.pending_after_compaction
    }

    /// Capture a submitted prompt and associate it with a spawned slot.
    fn capture_prompt(&mut self, target: AutoPromptTarget, prompt: &str, slot: impl Into<String>) {
        if prompt.trim().is_empty() {
            return;
        }
        self.target = target;
        self.prompt = Some(prompt.to_string());
        self.tracked_slot = Some(slot.into());
        self.pending_after_compaction = false;
    }

    /// Move tracking to a hidden continuation slot for the same visible turn.
    fn track_continuation_slot(&mut self, slot: impl Into<String>) {
        if self.prompt.is_some() && self.tracked_slot.is_some() {
            self.tracked_slot = Some(slot.into());
            self.pending_after_compaction = false;
        }
    }

    /// Mark this entry as blocked by a pending compaction lifecycle.
    fn defer_for_compaction(&mut self) {
        if self.prompt.is_some() && self.tracked_slot.is_some() {
            self.pending_after_compaction = true;
        }
    }

    /// Clear the watched slot without forgetting the captured prompt.
    fn clear_tracked_turn(&mut self) {
        self.tracked_slot = None;
        self.pending_after_compaction = false;
    }

    /// Return the captured prompt and clear the completed tracked turn.
    fn take_repeat_prompt(&mut self) -> Option<String> {
        let prompt = self.prompt.clone();
        self.clear_tracked_turn();
        prompt
    }
}

/// Registry of auto-prompt entries keyed by session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoPromptState {
    entries: HashMap<AutoPromptKey, AutoPromptEntry>,
}

/// Methods for toggling, capturing, and scheduling per-session auto prompts.
impl AutoPromptState {
    /// Return true when the target session currently has auto prompt enabled.
    #[cfg(test)]
    pub fn is_enabled_for(&self, key: &AutoPromptKey) -> bool {
        self.entries.contains_key(key)
    }

    /// Return the enabled entry for a target session, if present.
    pub fn entry_for(&self, key: &AutoPromptKey) -> Option<&AutoPromptEntry> {
        self.entries.get(key)
    }

    /// Toggle auto prompt for one target and return the new enabled state.
    pub fn toggle(&mut self, target: AutoPromptTarget) -> bool {
        let key = target.key().clone();
        if self.entries.remove(&key).is_some() {
            return false;
        }
        self.entries.insert(key, AutoPromptEntry::new(target));
        true
    }

    /// Capture a visible prompt for the target session if auto prompt is enabled there.
    pub fn capture_prompt(
        &mut self,
        target: AutoPromptTarget,
        prompt: &str,
        slot: impl Into<String>,
    ) {
        if let Some(entry) = self.entries.get_mut(target.key()) {
            entry.capture_prompt(target, prompt, slot);
        }
    }

    /// Move a target session to a hidden continuation slot after compaction.
    pub fn track_continuation_slot(&mut self, key: &AutoPromptKey, slot: impl Into<String>) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.track_continuation_slot(slot);
        }
    }

    /// Mark one target session as waiting on compaction before its repeat.
    pub fn defer_for_compaction(&mut self, key: &AutoPromptKey) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.defer_for_compaction();
        }
    }

    /// Clear the watched slot for one target while keeping the captured prompt.
    pub fn clear_tracked_turn(&mut self, key: &AutoPromptKey) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.clear_tracked_turn();
        }
    }

    /// Return the captured repeat prompt for one target and clear its watched slot.
    pub fn take_repeat_prompt(&mut self, key: &AutoPromptKey) -> Option<String> {
        self.entries
            .get_mut(key)
            .and_then(AutoPromptEntry::take_repeat_prompt)
    }

    /// Return keys that currently watch an agent slot for completion.
    pub fn tracked_keys(&self) -> Vec<AutoPromptKey> {
        self.entries
            .iter()
            .filter_map(|(key, entry)| entry.tracked_slot().map(|_| key.clone()))
            .collect()
    }
}

/// Auto-prompt helpers that need access to the current app session.
impl App {
    /// Build an auto-prompt target for the currently viewed session.
    pub(crate) fn current_auto_prompt_target(&self) -> Option<AutoPromptTarget> {
        let worktree = self.current_worktree()?;
        let worktree_path = worktree.worktree_path.clone()?;
        let session_id = self.current_session_id?;
        let project_path = self.project.as_ref().map(|project| project.path.clone());
        Some(AutoPromptTarget::new(
            worktree_path,
            session_id,
            worktree.branch_name.clone(),
            project_path,
        ))
    }

    /// Toggle auto prompt for the currently viewed session.
    pub(crate) fn toggle_auto_prompt_for_current_session(&mut self) -> Result<bool, &'static str> {
        let Some(target) = self.current_auto_prompt_target() else {
            return Err("Auto prompt needs an active session");
        };
        Ok(self.auto_prompt.toggle(target))
    }

    /// Return the auto-prompt entry for the currently viewed session.
    pub(crate) fn current_auto_prompt_entry(&self) -> Option<&AutoPromptEntry> {
        let target = self.current_auto_prompt_target()?;
        self.auto_prompt.entry_for(target.key())
    }
}

#[cfg(test)]
/// Tests for per-session auto-prompt state transitions.
mod tests {
    use super::*;

    /// Build a repeat target with a unique session id.
    fn target(session_id: i64) -> AutoPromptTarget {
        AutoPromptTarget::new(
            format!("/tmp/worktree-{session_id}"),
            session_id,
            format!("feature/{session_id}"),
            Some(PathBuf::from(format!("/tmp/project-{session_id}"))),
        )
    }

    /// New state starts with no enabled sessions.
    #[test]
    fn default_has_no_enabled_sessions() {
        let state = AutoPromptState::default();
        assert!(!state.is_enabled_for(target(1).key()));
    }

    /// Toggling one target does not enable a different session.
    #[test]
    fn toggle_is_scoped_to_one_session_key() {
        let mut state = AutoPromptState::default();
        let first = target(1);
        let second = target(2);

        assert!(state.toggle(first.clone()));

        assert!(state.is_enabled_for(first.key()));
        assert!(!state.is_enabled_for(second.key()));
    }

    /// Toggling an enabled target removes only that target's entry.
    #[test]
    fn toggle_off_clears_only_matching_session() {
        let mut state = AutoPromptState::default();
        let first = target(1);
        let second = target(2);
        state.toggle(first.clone());
        state.toggle(second.clone());
        state.capture_prompt(first.clone(), "again", "42");

        assert!(!state.toggle(first.clone()));

        assert!(!state.is_enabled_for(first.key()));
        assert!(state.is_enabled_for(second.key()));
    }

    /// Capturing a prompt records the spawned slot for the matching target.
    #[test]
    fn capture_prompt_records_per_session_turn_identity() {
        let mut state = AutoPromptState::default();
        let first = target(1);
        let second = target(2);
        state.toggle(first.clone());
        state.toggle(second.clone());

        state.capture_prompt(first.clone(), "run checks", "88");

        let first_entry = state.entry_for(first.key()).unwrap();
        let second_entry = state.entry_for(second.key()).unwrap();
        assert_eq!(first_entry.prompt(), Some("run checks"));
        assert_eq!(first_entry.tracked_slot(), Some("88"));
        assert_eq!(first_entry.target().branch(), "feature/1");
        assert!(second_entry.prompt().is_none());
        assert!(second_entry.tracked_slot().is_none());
    }

    /// Hidden continuation slots replace only the target session's watched slot.
    #[test]
    fn continuation_slot_is_per_session() {
        let mut state = AutoPromptState::default();
        let first = target(1);
        let second = target(2);
        state.toggle(first.clone());
        state.toggle(second.clone());
        state.capture_prompt(first.clone(), "continue work", "10");
        state.capture_prompt(second.clone(), "other loop", "20");

        state.track_continuation_slot(first.key(), "11");

        assert_eq!(
            state.entry_for(first.key()).unwrap().tracked_slot(),
            Some("11")
        );
        assert_eq!(
            state.entry_for(second.key()).unwrap().tracked_slot(),
            Some("20")
        );
    }

    /// Preparing a repeat leaves the prompt text available for the next loop.
    #[test]
    fn take_repeat_prompt_keeps_captured_text() {
        let mut state = AutoPromptState::default();
        let first = target(1);
        state.toggle(first.clone());
        state.capture_prompt(first.clone(), "loop", "10");

        assert_eq!(
            state.take_repeat_prompt(first.key()),
            Some("loop".to_string())
        );
        let entry = state.entry_for(first.key()).unwrap();
        assert_eq!(entry.prompt(), Some("loop"));
        assert!(entry.tracked_slot().is_none());
    }
}
