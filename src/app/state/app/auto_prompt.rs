//! Auto-prompt repeat state.
//!
//! Tracks whether automatic prompt repetition is enabled, which prompt should
//! repeat, and which agent slot must finish before the next repeat can be
//! queued.

/// State for repeating the latest captured prompt after completed turns.
#[derive(Debug, Clone, Default)]
pub struct AutoPromptState {
    enabled: bool,
    prompt: Option<String>,
    tracked_slot: Option<String>,
    branch: Option<String>,
    session_id: Option<i64>,
    pending_after_compaction: bool,
}

/// Methods for toggling, capturing, and staging auto-prompt repeats.
impl AutoPromptState {
    /// Return whether auto prompt is currently enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Return the prompt text that will be repeated, if one has been captured.
    pub fn prompt(&self) -> Option<&str> {
        self.prompt.as_deref()
    }

    /// Return the agent slot being watched for a completed turn.
    pub fn tracked_slot(&self) -> Option<&str> {
        self.tracked_slot.as_deref()
    }

    /// Return the branch that owns the tracked turn.
    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    /// Return the store session id that owns the tracked turn.
    pub fn session_id(&self) -> Option<i64> {
        self.session_id
    }

    /// Return whether a repeat is waiting for compaction to finish first.
    pub fn is_pending_after_compaction(&self) -> bool {
        self.pending_after_compaction
    }

    /// Toggle auto prompt on or off and return the new enabled state.
    ///
    /// Enabling starts fresh so the next prompt sent by the user becomes the
    /// repeat source. Disabling clears all captured and pending repeat state.
    pub fn toggle(&mut self) -> bool {
        if self.enabled {
            self.clear_all();
            return false;
        }
        self.enabled = true;
        self.prompt = None;
        self.clear_tracked_turn();
        true
    }

    /// Capture a visible prompt and associate it with the spawned agent slot.
    ///
    /// Empty prompts are ignored. The prompt is stored exactly as submitted so
    /// multiline repeat prompts preserve their original content.
    pub fn capture_prompt(
        &mut self,
        prompt: &str,
        slot: impl Into<String>,
        branch: impl Into<String>,
        session_id: Option<i64>,
    ) {
        if !self.enabled || prompt.trim().is_empty() {
            return;
        }
        self.prompt = Some(prompt.to_string());
        self.tracked_slot = Some(slot.into());
        self.branch = Some(branch.into());
        self.session_id = session_id;
        self.pending_after_compaction = false;
    }

    /// Move tracking to a hidden continuation slot for the same visible turn.
    ///
    /// This is used when compaction pauses a turn and the event loop sends a
    /// hidden continuation prompt. The repeat should wait for that continuation
    /// to finish rather than firing on the pause.
    pub fn track_continuation_slot(&mut self, slot: impl Into<String>) {
        if self.enabled && self.prompt.is_some() && self.tracked_slot.is_some() {
            self.tracked_slot = Some(slot.into());
            self.pending_after_compaction = false;
        }
    }

    /// Mark the current repeat as waiting for compaction to complete.
    pub fn defer_for_compaction(&mut self) {
        if self.enabled && self.prompt.is_some() && self.tracked_slot.is_some() {
            self.pending_after_compaction = true;
        }
    }

    /// Clear the tracked turn while keeping the captured prompt for the indicator.
    pub fn clear_tracked_turn(&mut self) {
        self.tracked_slot = None;
        self.branch = None;
        self.session_id = None;
        self.pending_after_compaction = false;
    }

    /// Prepare the captured prompt for a repeat send and clear the completed turn.
    pub fn take_repeat_prompt(&mut self) -> Option<String> {
        let prompt = self.prompt.clone();
        self.clear_tracked_turn();
        prompt
    }

    /// Clear all auto-prompt state and turn the feature off.
    fn clear_all(&mut self) {
        self.enabled = false;
        self.prompt = None;
        self.clear_tracked_turn();
    }
}

#[cfg(test)]
/// Tests for auto-prompt state transitions.
mod tests {
    use super::*;

    /// New state starts disabled with no captured prompt.
    #[test]
    fn default_is_disabled() {
        let state = AutoPromptState::default();
        assert!(!state.is_enabled());
        assert!(state.prompt().is_none());
    }

    /// Toggling on arms the feature without carrying a stale prompt.
    #[test]
    fn toggle_on_arms_next_prompt() {
        let mut state = AutoPromptState::default();
        assert!(state.toggle());
        assert!(state.is_enabled());
        assert!(state.prompt().is_none());
    }

    /// Toggling off clears captured text and tracked turn metadata.
    #[test]
    fn toggle_off_clears_state() {
        let mut state = AutoPromptState::default();
        state.toggle();
        state.capture_prompt("again", "42", "feature", Some(7));

        assert!(!state.toggle());

        assert!(!state.is_enabled());
        assert!(state.prompt().is_none());
        assert!(state.tracked_slot().is_none());
        assert!(!state.is_pending_after_compaction());
    }

    /// Capturing a prompt records the spawned slot and session identity.
    #[test]
    fn capture_prompt_records_turn_identity() {
        let mut state = AutoPromptState::default();
        state.toggle();

        state.capture_prompt("run checks", "88", "feature/a", Some(12));

        assert_eq!(state.prompt(), Some("run checks"));
        assert_eq!(state.tracked_slot(), Some("88"));
        assert_eq!(state.branch(), Some("feature/a"));
        assert_eq!(state.session_id(), Some(12));
    }

    /// Hidden continuation slots replace the paused slot without changing text.
    #[test]
    fn continuation_slot_keeps_prompt_text() {
        let mut state = AutoPromptState::default();
        state.toggle();
        state.capture_prompt("continue work", "10", "main", Some(1));

        state.track_continuation_slot("11");

        assert_eq!(state.prompt(), Some("continue work"));
        assert_eq!(state.tracked_slot(), Some("11"));
        assert_eq!(state.branch(), Some("main"));
    }

    /// Preparing a repeat keeps the prompt available for the next spawned turn.
    #[test]
    fn take_repeat_prompt_keeps_captured_text() {
        let mut state = AutoPromptState::default();
        state.toggle();
        state.capture_prompt("loop", "10", "main", Some(1));

        assert_eq!(state.take_repeat_prompt(), Some("loop".to_string()));
        assert_eq!(state.prompt(), Some("loop"));
        assert!(state.tracked_slot().is_none());
    }
}
