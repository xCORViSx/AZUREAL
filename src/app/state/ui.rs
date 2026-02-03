//! UI state management: focus, dialogs, menus, wizard, rebase, run commands

use crate::app::types::{BranchDialog, ContextMenu, Focus, RunCommand, RunCommandDialog, RunCommandPicker, SessionAction, ViewMode};
use crate::config::{ensure_project_data_dir, project_data_dir};
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
            Focus::WorktreeCreation | Focus::BranchDialog => self.focus,
        };
    }

    pub fn focus_prev(&mut self) {
        self.focus = match self.focus {
            Focus::Worktrees => Focus::Input,
            Focus::FileTree => Focus::Worktrees,
            Focus::Viewer => Focus::FileTree,
            Focus::Output => Focus::Viewer,
            Focus::Input => Focus::Output,
            Focus::WorktreeCreation | Focus::BranchDialog => self.focus,
        };
    }

    pub fn toggle_help(&mut self) { self.show_help = !self.show_help; }
    pub fn toggle_terminal(&mut self) {
        if self.terminal_mode { self.close_terminal(); } else { self.open_terminal(); }
    }

    pub fn exit_worktree_creation_mode(&mut self) {
        self.focus = Focus::Worktrees;
        self.clear_worktree_creation_input();
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

    // Diff view - loads git diff into Viewer pane
    pub fn load_diff(&mut self) -> anyhow::Result<()> {
        if let Some(session) = self.current_session() {
            if let Some(ref wt_path) = session.worktree_path {
                if let Some(project) = self.current_project() {
                    let diff = Git::get_diff(wt_path, &project.main_branch)?;
                    self.load_diff_into_viewer(&diff.diff_text, Some(session.name().to_string()));
                    return Ok(());
                }
            }
        }
        anyhow::bail!("No active session with worktree")
    }

    /// Load diff content into the Viewer pane
    pub fn load_diff_into_viewer(&mut self, diff_text: &str, title: Option<String>) {
        self.viewer_content = Some(diff_text.to_string());
        self.viewer_mode = crate::app::ViewerMode::Diff;
        self.viewer_path = title.map(std::path::PathBuf::from);
        self.viewer_scroll = 0;
        self.viewer_lines_dirty = true;
        self.focus = Focus::Viewer;
    }

    /// Load a file into the viewer with inline Edit diff highlighting
    /// Shows the full file with syntax highlighting, scrolled to the edit location
    /// The edit region is highlighted with red/green diff backgrounds
    pub fn load_file_with_edit_diff(&mut self, file_path: &str, old_string: &str, new_string: &str) {
        use std::path::PathBuf;

        let path = PathBuf::from(file_path);
        if let Ok(content) = std::fs::read_to_string(&path) {
            // Save previous viewer state if not already in Edit diff view (for Esc restoration)
            if self.viewer_edit_diff.is_none() {
                self.viewer_prev_state = Some((
                    self.viewer_content.clone(),
                    self.viewer_path.clone(),
                    self.viewer_scroll,
                ));
            }

            // Find edit location using progressively broader searches
            let edit_line = Self::find_edit_line(&content, old_string, new_string);

            self.viewer_content = Some(content);
            self.viewer_path = Some(path);
            self.viewer_mode = crate::app::ViewerMode::File;
            self.viewer_edit_diff = Some((old_string.to_string(), new_string.to_string()));
            self.viewer_edit_diff_line = Some(edit_line);
            self.viewer_scroll = edit_line.saturating_sub(3); // Scroll to show edit with 3 lines context above
            self.viewer_lines_dirty = true;
            self.focus = Focus::Viewer;
        } else {
            self.set_status(&format!("Cannot read file: {}", file_path));
        }
    }

    /// Find the line number where an edit occurs using multiple search strategies
    fn find_edit_line(content: &str, old_string: &str, new_string: &str) -> usize {
        // Helper to count newlines before a position
        let line_at = |pos: usize| content[..pos].chars().filter(|&c| c == '\n').count();

        // Strategy 1: Search for full new_string (most accurate when edit is applied)
        if !new_string.is_empty() {
            if let Some(pos) = content.find(new_string) {
                return line_at(pos);
            }
        }

        // Strategy 2: Search for full old_string (when edit not yet applied, or viewing history)
        if !old_string.is_empty() {
            if let Some(pos) = content.find(old_string) {
                return line_at(pos);
            }
        }

        // Strategy 3: Search for significant lines from new_string (exact match)
        if !new_string.is_empty() {
            let significant_lines: Vec<&str> = new_string.lines()
                .filter(|l| l.trim().len() > 3)
                .take(3)
                .collect();
            for line in &significant_lines {
                if let Some(pos) = content.find(*line) {
                    return line_at(pos);
                }
            }
        }

        // Strategy 4: Same for old_string
        if !old_string.is_empty() {
            let significant_lines: Vec<&str> = old_string.lines()
                .filter(|l| l.trim().len() > 3)
                .take(3)
                .collect();
            for line in &significant_lines {
                if let Some(pos) = content.find(*line) {
                    return line_at(pos);
                }
            }
        }

        // Strategy 5: Search for trimmed lines (handles whitespace/indent differences)
        // Match trimmed content against trimmed lines in file
        let find_trimmed_line = |search_str: &str| -> Option<usize> {
            for search_line in search_str.lines() {
                let trimmed = search_line.trim();
                if trimmed.len() <= 5 { continue; } // Skip short lines
                for (line_num, content_line) in content.lines().enumerate() {
                    if content_line.trim() == trimmed {
                        return Some(line_num);
                    }
                }
            }
            None
        };

        if let Some(line) = find_trimmed_line(new_string) {
            return line;
        }
        if let Some(line) = find_trimmed_line(old_string) {
            return line;
        }

        // Strategy 6: Look for unique identifiers (function names, variable names)
        let find_by_identifier = |s: &str| -> Option<usize> {
            let mut words: Vec<&str> = s.split(|c: char| !c.is_alphanumeric() && c != '_')
                .filter(|w| w.len() >= 6)
                .collect();
            words.sort_by(|a, b| b.len().cmp(&a.len()));
            for word in words.iter().take(5) {
                if let Some(pos) = content.find(*word) {
                    return Some(line_at(pos));
                }
            }
            None
        };

        if let Some(line) = find_by_identifier(new_string) {
            return line;
        }
        if let Some(line) = find_by_identifier(old_string) {
            return line;
        }

        0 // Fallback to top of file
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
        self.creation_wizard = Some(crate::wizard::CreationWizard::new_single_project(self.project.as_ref()));
        self.focus = Focus::Input;
    }

    pub fn cancel_wizard(&mut self) {
        self.creation_wizard = None;
        self.focus = Focus::Worktrees;
    }

    pub fn is_wizard_active(&self) -> bool { self.creation_wizard.is_some() }

    // Run commands
    pub fn open_run_command_dialog(&mut self) {
        self.run_command_dialog = Some(RunCommandDialog::new());
    }

    pub fn open_run_command_picker(&mut self) {
        if self.run_commands.is_empty() {
            self.set_status("No run commands. Press ⌥r to add one.");
            return;
        }
        if self.run_commands.len() == 1 {
            self.execute_run_command(0);
        } else {
            self.run_command_picker = Some(RunCommandPicker::new());
        }
    }

    pub fn execute_run_command(&mut self, idx: usize) {
        let Some(cmd) = self.run_commands.get(idx) else { return };
        let command = cmd.command.clone();
        let name = cmd.name.clone();

        // Open terminal if not open and send command
        if !self.terminal_mode {
            self.open_terminal();
        }
        if let Some(ref mut writer) = self.terminal_writer {
            let _ = writer.write_all(command.as_bytes());
            let _ = writer.write_all(b"\n");
            let _ = writer.flush();
        }
        self.set_status(format!("Running: {}", name));
    }

    /// Save run commands to project data directory (.azureal/run_commands.json)
    pub fn save_run_commands(&self) -> anyhow::Result<()> {
        let Some(dir) = ensure_project_data_dir()? else { return Ok(()); };
        let path = dir.join("run_commands.json");
        let json: Vec<_> = self.run_commands.iter().map(|c| serde_json::json!({"name": c.name, "command": c.command})).collect();
        std::fs::write(&path, serde_json::to_string_pretty(&json)?)?;
        Ok(())
    }

    /// Load run commands from project data directory
    pub fn load_run_commands(&mut self) {
        let Some(dir) = project_data_dir() else { return; };
        let path = dir.join("run_commands.json");
        if !path.exists() { return; }
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                self.run_commands = arr.iter().filter_map(|v| {
                    let name = v.get("name")?.as_str()?.to_string();
                    let command = v.get("command")?.as_str()?.to_string();
                    Some(RunCommand::new(name, command))
                }).collect();
            }
        }
    }

    // Viewer tabs
    pub fn viewer_tab_current(&mut self) {
        // Save current viewer state to a new tab (if we have content)
        if self.viewer_content.is_some() || self.viewer_path.is_some() {
            use crate::app::types::ViewerTab;
            let title = self.viewer_path.as_ref()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Untitled".to_string());
            let tab = ViewerTab {
                path: self.viewer_path.clone(),
                content: self.viewer_content.clone(),
                scroll: self.viewer_scroll,
                mode: self.viewer_mode,
                title,
            };
            self.viewer_tabs.push(tab);
            self.viewer_active_tab = self.viewer_tabs.len() - 1;
        }
    }

    pub fn toggle_viewer_tab_dialog(&mut self) {
        self.viewer_tab_dialog = !self.viewer_tab_dialog;
    }

    pub fn viewer_next_tab(&mut self) {
        if !self.viewer_tabs.is_empty() {
            self.viewer_active_tab = (self.viewer_active_tab + 1) % self.viewer_tabs.len();
            self.load_tab_to_viewer();
        }
    }

    pub fn viewer_prev_tab(&mut self) {
        if !self.viewer_tabs.is_empty() {
            self.viewer_active_tab = if self.viewer_active_tab == 0 {
                self.viewer_tabs.len() - 1
            } else {
                self.viewer_active_tab - 1
            };
            self.load_tab_to_viewer();
        }
    }

    pub fn viewer_close_current_tab(&mut self) {
        if self.viewer_tabs.is_empty() { return; }
        self.viewer_tabs.remove(self.viewer_active_tab);
        if self.viewer_active_tab >= self.viewer_tabs.len() && !self.viewer_tabs.is_empty() {
            self.viewer_active_tab = self.viewer_tabs.len() - 1;
        }
        if self.viewer_tabs.is_empty() {
            self.viewer_content = None;
            self.viewer_path = None;
            self.viewer_mode = crate::app::ViewerMode::Empty;
            self.viewer_lines_dirty = true;
        } else {
            self.load_tab_to_viewer();
        }
    }

    pub fn load_tab_to_viewer(&mut self) {
        if let Some(tab) = self.viewer_tabs.get(self.viewer_active_tab) {
            self.viewer_content = tab.content.clone();
            self.viewer_path = tab.path.clone();
            self.viewer_scroll = tab.scroll;
            self.viewer_mode = tab.mode;
            self.viewer_lines_dirty = true;
        }
    }
}
