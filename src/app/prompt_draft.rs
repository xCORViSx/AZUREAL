//! Worktree-scoped prompt draft storage for the shared input box.
//!
//! The renderer and input handlers still edit `App::input` directly. This
//! module snapshots that active buffer when the viewed worktree changes so a
//! half-typed prompt stays with the worktree where it was started.

use super::App;

/// Snapshot of the prompt box state for one worktree.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PromptInputDraft {
    /// Prompt text currently shown in the input box.
    input: String,
    /// Cursor position as a character index into `input`.
    input_cursor: usize,
    /// Optional selection range as character indices into `input`.
    input_selection: Option<(usize, usize)>,
    /// Prompt-history cursor active while browsing previous prompts.
    prompt_history_idx: Option<usize>,
    /// User text saved before prompt-history browsing began.
    prompt_history_temp: Option<String>,
}

/// Prompt draft construction and restoration helpers.
impl PromptInputDraft {
    /// Capture the active prompt box state from the application.
    fn capture(app: &App) -> Self {
        Self {
            input: app.input.clone(),
            input_cursor: app.input_cursor,
            input_selection: app.input_selection,
            prompt_history_idx: app.prompt_history_idx,
            prompt_history_temp: app.prompt_history_temp.clone(),
        }
    }

    /// Return true when the draft carries no visible or transient input state.
    fn is_empty(&self) -> bool {
        self.input.is_empty()
            && self.input_cursor == 0
            && self.input_selection.is_none()
            && self.prompt_history_idx.is_none()
            && self.prompt_history_temp.is_none()
    }

    /// Restore this draft into the active prompt box, clamping stale indices.
    fn restore(&self, app: &mut App) {
        app.input = self.input.clone();
        let char_count = app.input.chars().count();
        app.input_cursor = self.input_cursor.min(char_count);
        app.input_selection = self.input_selection.map(|(start, end)| {
            let start = start.min(char_count);
            let end = end.min(char_count);
            (start, end)
        });
        app.prompt_history_idx = self.prompt_history_idx;
        app.prompt_history_temp = self.prompt_history_temp.clone();
    }
}

/// Worktree-scoped prompt draft reconciliation methods.
impl App {
    /// Build the stable key used to associate a prompt draft with the visible worktree.
    fn current_prompt_draft_key(&self) -> Option<String> {
        let worktree = self.current_worktree()?;
        if let Some(path) = worktree.worktree_path.as_ref() {
            return Some(path.to_string_lossy().into_owned());
        }

        let project_key = self
            .project
            .as_ref()
            .map(|project| project.path.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("<no-project>"));
        Some(format!("{project_key}::{}", worktree.branch_name))
    }

    /// Store the currently active prompt buffer under its active worktree key.
    fn store_active_prompt_draft(&mut self) {
        let Some(key) = self.active_prompt_draft_key.clone() else {
            return;
        };
        let draft = PromptInputDraft::capture(self);
        if draft.is_empty() {
            self.prompt_drafts.remove(&key);
        } else {
            self.prompt_drafts.insert(key, draft);
        }
    }

    /// Save the old worktree prompt and load the prompt for the current worktree.
    ///
    /// Returns true when the active input buffer changed and the UI should redraw.
    pub(crate) fn sync_prompt_draft_to_current_worktree(&mut self) -> bool {
        let next_key = self.current_prompt_draft_key();
        if self.active_prompt_draft_key == next_key {
            self.store_active_prompt_draft();
            return false;
        }

        let before = PromptInputDraft::capture(self);
        if self.active_prompt_draft_key.is_none() && !before.is_empty() {
            self.active_prompt_draft_key = next_key;
            self.store_active_prompt_draft();
            return false;
        }

        self.store_active_prompt_draft();

        let draft = next_key
            .as_ref()
            .and_then(|key| self.prompt_drafts.get(key))
            .cloned()
            .unwrap_or_default();
        draft.restore(self);
        self.active_prompt_draft_key = next_key;

        draft != before
    }
}

#[cfg(test)]
/// Tests for worktree-scoped prompt draft reconciliation.
mod tests {
    use super::*;
    use crate::models::Worktree;
    use std::path::PathBuf;

    /// Build a test worktree with a stable path-backed draft key.
    fn worktree(branch: &str, path: &str) -> Worktree {
        Worktree {
            branch_name: branch.to_string(),
            worktree_path: Some(PathBuf::from(path)),
            claude_session_id: None,
            archived: false,
        }
    }

    /// Switching worktrees saves and restores independent prompt text.
    #[test]
    fn worktree_switch_restores_independent_prompt_text() {
        let mut app = App::new();
        app.worktrees = vec![
            worktree("feature/a", "/tmp/azureal-a"),
            worktree("feature/b", "/tmp/azureal-b"),
        ];
        app.selected_worktree = Some(0);
        app.sync_prompt_draft_to_current_worktree();

        app.input = "prompt for a".to_string();
        app.input_cursor = app.input.chars().count();

        app.selected_worktree = Some(1);
        assert!(app.sync_prompt_draft_to_current_worktree());
        assert!(app.input.is_empty());

        app.input = "prompt for b".to_string();
        app.input_cursor = app.input.chars().count();

        app.selected_worktree = Some(0);
        assert!(app.sync_prompt_draft_to_current_worktree());
        assert_eq!(app.input, "prompt for a");
        assert_eq!(app.input_cursor, "prompt for a".chars().count());

        app.selected_worktree = Some(1);
        assert!(app.sync_prompt_draft_to_current_worktree());
        assert_eq!(app.input, "prompt for b");
    }

    /// Draft restoration preserves cursor, selection, and history-browse state.
    #[test]
    fn worktree_switch_restores_prompt_editing_state() {
        let mut app = App::new();
        app.worktrees = vec![
            worktree("feature/a", "/tmp/azureal-a"),
            worktree("feature/b", "/tmp/azureal-b"),
        ];
        app.selected_worktree = Some(0);
        app.sync_prompt_draft_to_current_worktree();

        app.input = "abcdef".to_string();
        app.input_cursor = 4;
        app.input_selection = Some((1, 4));
        app.prompt_history_idx = Some(2);
        app.prompt_history_temp = Some("draft before history".to_string());

        app.selected_worktree = Some(1);
        app.sync_prompt_draft_to_current_worktree();
        app.selected_worktree = Some(0);
        app.sync_prompt_draft_to_current_worktree();

        assert_eq!(app.input, "abcdef");
        assert_eq!(app.input_cursor, 4);
        assert_eq!(app.input_selection, Some((1, 4)));
        assert_eq!(app.prompt_history_idx, Some(2));
        assert_eq!(
            app.prompt_history_temp.as_deref(),
            Some("draft before history")
        );
    }

    /// Clearing a submitted prompt removes the saved draft for that worktree.
    #[test]
    fn clear_input_removes_saved_worktree_draft() {
        let mut app = App::new();
        app.worktrees = vec![
            worktree("feature/a", "/tmp/azureal-a"),
            worktree("feature/b", "/tmp/azureal-b"),
        ];
        app.selected_worktree = Some(0);
        app.sync_prompt_draft_to_current_worktree();
        app.input = "submitted prompt".to_string();
        app.input_cursor = app.input.chars().count();
        app.sync_prompt_draft_to_current_worktree();

        app.clear_input();
        app.sync_prompt_draft_to_current_worktree();
        app.selected_worktree = Some(1);
        app.sync_prompt_draft_to_current_worktree();
        app.selected_worktree = Some(0);
        app.sync_prompt_draft_to_current_worktree();

        assert!(app.input.is_empty());
        assert_eq!(app.input_cursor, 0);
    }
}
