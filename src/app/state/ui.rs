//! UI state management: focus, dialogs, menus, wizard, rebase

use crate::app::types::{BranchDialog, ContextMenu, Focus, SessionAction, ViewMode};
use crate::git::Git;
use crate::models::RebaseStatus;

use super::App;

impl App {
    pub fn focus_next(&mut self) {
        self.focus = match self.focus {
            Focus::Worktrees => Focus::FileTree,
            Focus::FileTree => Focus::Viewer,
            Focus::Viewer => Focus::Output,
            Focus::Output => Focus::Input,
            Focus::Input => Focus::Worktrees,
            Focus::SessionCreation | Focus::BranchDialog => self.focus,
        };
    }

    pub fn focus_prev(&mut self) {
        self.focus = match self.focus {
            Focus::Worktrees => Focus::Input,
            Focus::FileTree => Focus::Worktrees,
            Focus::Viewer => Focus::FileTree,
            Focus::Output => Focus::Viewer,
            Focus::Input => Focus::Output,
            Focus::SessionCreation | Focus::BranchDialog => self.focus,
        };
    }

    pub fn toggle_help(&mut self) { self.show_help = !self.show_help; }
    pub fn toggle_terminal(&mut self) {
        if self.terminal_mode { self.close_terminal(); } else { self.open_terminal(); }
    }

    pub fn exit_session_creation_mode(&mut self) {
        self.focus = Focus::Worktrees;
        self.clear_session_creation_input();
        self.clear_status();
    }

    // Branch dialog
    pub fn open_branch_dialog(&mut self, branches: Vec<String>) {
        if branches.is_empty() {
            self.set_status("No available branches to checkout");
            return;
        }
        self.branch_dialog = Some(BranchDialog::new(branches));
        self.focus = Focus::BranchDialog;
    }

    pub fn close_branch_dialog(&mut self) {
        self.branch_dialog = None;
        self.focus = Focus::Worktrees;
    }

    // Diff view
    pub fn load_diff(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_session() {
            if let Some(ref wt_path) = session.worktree_path {
                if let Some(project) = self.current_project() {
                    let diff = Git::get_diff(wt_path, &project.main_branch)?;
                    self.diff_text = Some(diff.diff_text);
                    self.diff_lines_dirty = true;
                    self.view_mode = ViewMode::Diff;
                    self.focus = Focus::Output;
                    return Ok(());
                }
            }
        }
        anyhow::bail!("No active session with worktree")
    }

    // Rebase status
    pub fn set_rebase_status(&mut self, status: RebaseStatus) {
        self.rebase_status = Some(status);
        self.selected_conflict = if self.rebase_status.as_ref().is_some_and(|s| !s.conflicted_files.is_empty()) { Some(0) } else { None };
        self.view_mode = ViewMode::Rebase;
        self.focus = Focus::Output;
    }

    pub fn clear_rebase_status(&mut self) {
        self.rebase_status = None;
        self.selected_conflict = None;
        if self.view_mode == ViewMode::Rebase { self.view_mode = ViewMode::Output; }
    }

    pub fn select_next_conflict(&mut self) {
        if let Some(ref status) = self.rebase_status {
            if let Some(idx) = self.selected_conflict {
                if idx + 1 < status.conflicted_files.len() { self.selected_conflict = Some(idx + 1); }
            }
        }
    }

    pub fn select_prev_conflict(&mut self) {
        if let Some(idx) = self.selected_conflict {
            if idx > 0 { self.selected_conflict = Some(idx - 1); }
        }
    }

    pub fn current_conflict_file(&self) -> Option<&str> {
        self.rebase_status.as_ref().and_then(|status| {
            self.selected_conflict.and_then(|idx| status.conflicted_files.get(idx).map(|s| s.as_str()))
        })
    }

    // Context menu
    pub fn open_context_menu(&mut self) {
        if let Some(session) = self.current_session() {
            let status = session.status(&self.running_sessions);
            let actions = SessionAction::available_for_status(status);
            if !actions.is_empty() { self.context_menu = Some(ContextMenu { actions, selected: 0 }); }
        }
    }

    pub fn close_context_menu(&mut self) { self.context_menu = None; }

    pub fn context_menu_next(&mut self) {
        if let Some(ref mut menu) = self.context_menu {
            if menu.selected + 1 < menu.actions.len() { menu.selected += 1; }
        }
    }

    pub fn context_menu_prev(&mut self) {
        if let Some(ref mut menu) = self.context_menu {
            if menu.selected > 0 { menu.selected -= 1; }
        }
    }

    pub fn selected_action(&self) -> Option<SessionAction> {
        self.context_menu.as_ref().map(|menu| menu.actions[menu.selected].clone())
    }

    // Wizard
    pub fn start_wizard(&mut self) {
        self.creation_wizard = Some(crate::wizard::SessionCreationWizard::new_single_project(self.project.as_ref()));
        self.focus = Focus::Input;
    }

    pub fn cancel_wizard(&mut self) {
        self.creation_wizard = None;
        self.focus = Focus::Worktrees;
    }

    pub fn is_wizard_active(&self) -> bool { self.creation_wizard.is_some() }
}
