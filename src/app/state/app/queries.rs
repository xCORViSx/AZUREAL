//! Session status queries and project/worktree accessors

use super::App;
use crate::models::{Project, Worktree};

impl App {
    pub fn current_project(&self) -> Option<&Project> { self.project.as_ref() }

    /// When browsing main, returns the separate main_worktree; otherwise indexes into worktrees vec
    pub fn current_worktree(&self) -> Option<&Worktree> {
        if self.browsing_main { return self.main_worktree.as_ref(); }
        self.selected_worktree.and_then(|idx| self.worktrees.get(idx))
    }

    /// True if ANY Claude process is running on this branch (any slot)
    pub fn is_session_running(&self, branch_name: &str) -> bool {
        self.branch_slots.get(branch_name)
            .map(|slots| slots.iter().any(|s| self.running_sessions.contains(s)))
            .unwrap_or(false)
    }

    /// True if the ACTIVE slot (the one feeding display_events) is running
    pub fn is_active_slot_running(&self) -> bool {
        self.current_worktree().and_then(|s| {
            self.active_slot.get(&s.branch_name)
                .map(|slot| self.running_sessions.contains(slot))
        }).unwrap_or(false)
    }

    /// Look up which branch a slot_id belongs to (reverse lookup)
    pub fn branch_for_slot(&self, slot_id: &str) -> Option<String> {
        self.branch_slots.iter()
            .find(|(_, slots)| slots.contains(&slot_id.to_string()))
            .map(|(branch, _)| branch.clone())
    }

    /// Check if a Claude session UUID has a running process (for status dots in session list)
    pub fn is_claude_session_running(&self, claude_session_id: &str) -> bool {
        self.claude_session_ids.iter()
            .any(|(slot, sid)| sid == claude_session_id && self.running_sessions.contains(slot))
    }

    pub fn set_status(&mut self, msg: impl Into<String>) { self.status_message = Some(msg.into()); }
    pub fn clear_status(&mut self) { self.status_message = None; }

    /// Open a full-width table popup for the given raw markdown table text.
    /// Re-renders the table at near-terminal width so columns aren't truncated.
    pub fn open_table_popup(&mut self, raw_markdown: &str) {
        let term_width = crossterm::terminal::size().map(|(w, _)| w as usize).unwrap_or(120);
        let popup_width = term_width.saturating_sub(8).max(60);
        let lines = crate::tui::render_markdown::render_table_for_popup(raw_markdown, popup_width);
        let total_lines = lines.len();
        self.table_popup = Some(crate::app::types::TablePopup { lines, scroll: 0, total_lines });
    }
}
