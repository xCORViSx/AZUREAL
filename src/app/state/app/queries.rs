//! Session status queries and project/worktree accessors

use super::App;
use crate::models::{Project, Worktree, WorktreeStatus};
use std::path::Path;

/// Query and status helpers for current project/worktree application state.
impl App {
    /// Return the currently loaded project, if any.
    pub fn current_project(&self) -> Option<&Project> {
        self.project.as_ref()
    }

    /// When browsing main, returns the separate main_worktree; otherwise indexes into worktrees vec
    pub fn current_worktree(&self) -> Option<&Worktree> {
        if self.browsing_main {
            return self.main_worktree.as_ref();
        }
        self.selected_worktree
            .and_then(|idx| self.worktrees.get(idx))
    }

    /// True if ANY Claude process is running on this branch (any slot)
    pub fn is_session_running(&self, branch_name: &str) -> bool {
        self.branch_slots
            .get(branch_name)
            .map(|slots| slots.iter().any(|s| self.running_sessions.contains(s)))
            .unwrap_or(false)
    }

    /// True if the ACTIVE slot (the one feeding display_events) is running
    pub fn is_active_slot_running(&self) -> bool {
        self.current_worktree()
            .and_then(|s| {
                self.active_slot
                    .get(&s.branch_name)
                    .map(|slot| self.running_sessions.contains(slot))
            })
            .unwrap_or(false)
    }

    /// Return true when the session pane is showing the given store session
    /// from the given worktree path.
    pub fn is_viewing_session_target(&self, session_id: i64, wt_path: &Path) -> bool {
        self.current_session_id == Some(session_id)
            && self
                .current_worktree()
                .and_then(|worktree| worktree.worktree_path.as_deref())
                == Some(wt_path)
    }

    /// Return true when the session pane is showing the store session owned by
    /// the given process slot.
    pub fn is_viewing_slot_target(&self, slot_id: &str) -> bool {
        self.pid_session_target
            .get(slot_id)
            .map(|(session_id, wt_path, _, _)| self.is_viewing_session_target(*session_id, wt_path))
            .unwrap_or(false)
    }

    /// Return true when the active RCR workflow belongs to the currently shown
    /// worktree and store session.
    pub fn rcr_session_is_visible(&self) -> bool {
        let Some(rcr) = self.rcr_session.as_ref() else {
            return false;
        };
        if self.pid_session_target.contains_key(&rcr.slot_id) {
            return self.is_viewing_slot_target(&rcr.slot_id);
        }

        self.current_session_id.is_none()
            && self
                .current_worktree()
                .and_then(|worktree| worktree.worktree_path.as_deref())
                == Some(rcr.worktree_path.as_path())
    }

    /// Look up which branch a slot_id belongs to (reverse lookup)
    pub fn branch_for_slot(&self, slot_id: &str) -> Option<String> {
        self.branch_slots
            .iter()
            .find(|(_, slots)| slots.iter().any(|slot| slot == slot_id))
            .map(|(branch, _)| branch.clone())
    }

    /// Backend used by a running slot.
    pub fn backend_for_slot(&self, slot_id: &str) -> crate::backend::Backend {
        if self.codex_slot_started_at.contains_key(slot_id) {
            crate::backend::Backend::Codex
        } else {
            crate::backend::Backend::Claude
        }
    }

    /// Model label used by a running slot. Falls back to the currently selected
    /// display model only for legacy slots registered before this map existed.
    pub fn model_for_slot(&self, slot_id: &str) -> String {
        self.agent_slot_models
            .get(slot_id)
            .cloned()
            .unwrap_or_else(|| self.display_model_name().to_string())
    }

    /// Check if a Claude session UUID has a running process (for status dots in session list)
    pub fn is_claude_session_running(&self, claude_session_id: &str) -> bool {
        self.agent_session_ids
            .iter()
            .any(|(slot, sid)| sid == claude_session_id && self.running_sessions.contains(slot))
    }

    /// True when no worktrees exist and main is not being browsed — the welcome
    /// modal should block all input except Browse Main, Add Worktree, and Quit.
    pub fn needs_welcome_modal(&self) -> bool {
        self.project.is_some()
            && self.worktrees.is_empty()
            && !self.browsing_main
            && !self.is_projects_panel_active()
            && self.branch_dialog.is_none()
    }

    /// Set the transient status message shown in the status bar.
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }

    /// Clear any transient status message from the status bar.
    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    /// Open a full-width table popup for the given raw markdown table text.
    /// Re-renders the table at near-terminal width so columns aren't truncated.
    pub fn open_table_popup(&mut self, raw_markdown: &str) {
        let term_width = crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(120);
        let popup_width = term_width.saturating_sub(8).max(60);
        let lines = crate::tui::render_markdown::render_table_for_popup(raw_markdown, popup_width);
        let total_lines = lines.len();
        self.table_popup = Some(crate::app::types::TablePopup {
            lines,
            scroll: 0,
            total_lines,
        });
    }

    /// Compute the aggregate worktree activity status for a project path.
    /// Returns the highest-priority status across all worktrees:
    /// Running > Failed > Waiting > Pending > Stopped
    pub fn project_status(&self, project_path: &Path) -> WorktreeStatus {
        let is_current = self
            .project
            .as_ref()
            .map(|p| p.path == project_path)
            .unwrap_or(false);

        if is_current {
            // Active project — check live worktree statuses
            self.worktrees
                .iter()
                .map(|wt| wt.status(self.is_session_running(&wt.branch_name)))
                .max_by_key(status_priority)
                .unwrap_or(WorktreeStatus::Pending)
        } else if let Some(snapshot) = self.project_snapshots.get(project_path) {
            // Background project — check saved worktrees against global running_sessions
            snapshot
                .worktrees
                .iter()
                .map(|wt| {
                    let running = snapshot
                        .branch_slots
                        .get(&wt.branch_name)
                        .map(|slots| slots.iter().any(|s| self.running_sessions.contains(s)))
                        .unwrap_or(false);
                    wt.status(running)
                })
                .max_by_key(status_priority)
                .unwrap_or(WorktreeStatus::Pending)
        } else {
            // No data for this project
            WorktreeStatus::Pending
        }
    }
}

/// Priority ordering for aggregate status: higher = takes precedence
fn status_priority(status: &WorktreeStatus) -> u8 {
    match status {
        WorktreeStatus::Running => 5,
        WorktreeStatus::Failed => 4,
        WorktreeStatus::Waiting => 3,
        WorktreeStatus::Pending => 2,
        WorktreeStatus::Completed => 1,
        WorktreeStatus::Stopped => 0,
    }
}

#[cfg(test)]
/// Tests for project, worktree, slot, and status query helpers.
mod tests {
    use super::*;
    use crate::app::types::RcrSession;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// Slot reverse lookup should find the owning branch without allocating a probe String.
    #[test]
    fn branch_for_slot_finds_existing_slot() {
        let mut app = App::new();
        app.branch_slots.insert(
            "feature".into(),
            vec!["slot-a".to_string(), "slot-b".to_string()],
        );

        assert_eq!(app.branch_for_slot("slot-b"), Some("feature".into()));
    }

    /// Slot reverse lookup returns none when no branch owns the slot.
    #[test]
    fn branch_for_slot_returns_none_for_missing_slot() {
        let mut app = App::new();
        app.branch_slots = HashMap::from([("feature".into(), vec!["slot-a".to_string()])]);

        assert_eq!(app.branch_for_slot("slot-z"), None);
    }

    /// Status helpers update and clear the status bar message.
    #[test]
    fn status_helpers_update_message() {
        let mut app = App::new();

        app.set_status("ready");
        assert_eq!(app.status_message.as_deref(), Some("ready"));

        app.clear_status();
        assert!(app.status_message.is_none());
    }

    /// RCR visibility requires the selected worktree and store session to match.
    #[test]
    fn rcr_session_is_visible_for_matching_target() {
        let mut app = App::new();
        let wt_path = PathBuf::from("/tmp/rcr-visible");
        app.worktrees.push(Worktree {
            branch_name: "feature".into(),
            worktree_path: Some(wt_path.clone()),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.current_session_id = Some(7);
        app.pid_session_target
            .insert("42".into(), (7, wt_path.clone(), 0, 0));
        app.rcr_session = Some(RcrSession {
            branch: "feature".into(),
            display_name: "feature".into(),
            worktree_path: wt_path,
            repo_root: PathBuf::from("/tmp/repo"),
            slot_id: "42".into(),
            session_id: None,
            approval_pending: false,
            continue_with_merge: false,
        });

        assert!(app.rcr_session_is_visible());
        assert!(app.is_viewing_slot_target("42"));
    }

    /// RCR visibility is false when the user selects a different session.
    #[test]
    fn rcr_session_is_hidden_for_different_store_session() {
        let mut app = App::new();
        let wt_path = PathBuf::from("/tmp/rcr-hidden");
        app.worktrees.push(Worktree {
            branch_name: "feature".into(),
            worktree_path: Some(wt_path.clone()),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app.current_session_id = Some(8);
        app.pid_session_target
            .insert("42".into(), (7, wt_path.clone(), 0, 0));
        app.rcr_session = Some(RcrSession {
            branch: "feature".into(),
            display_name: "feature".into(),
            worktree_path: wt_path,
            repo_root: PathBuf::from("/tmp/repo"),
            slot_id: "42".into(),
            session_id: None,
            approval_pending: false,
            continue_with_merge: false,
        });

        assert!(!app.rcr_session_is_visible());
        assert!(!app.is_viewing_slot_target("42"));
    }
}
