//! Session status queries and project/worktree accessors

use super::App;
use crate::models::{Project, Worktree, WorktreeStatus};
use std::path::Path;

impl App {
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

    /// Look up which branch a slot_id belongs to (reverse lookup)
    pub fn branch_for_slot(&self, slot_id: &str) -> Option<String> {
        self.branch_slots
            .iter()
            .find(|(_, slots)| slots.contains(&slot_id.to_string()))
            .map(|(branch, _)| branch.clone())
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
        self.project.is_some() && self.worktrees.is_empty() && !self.browsing_main
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }
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
