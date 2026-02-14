//! UI state management: focus, dialogs, menus, wizard, rebase, run commands

use crate::app::types::{BranchDialog, ContextMenu, Focus, GitActionsPanel, GitChangedFile, PresetPrompt, PresetPromptDialog, PresetPromptPicker, ProjectsPanel, RunCommand, RunCommandDialog, RunCommandPicker, SessionAction, ViewMode};
use crate::config::{config_dir, ensure_config_dir, ensure_project_data_dir, load_projects, project_data_dir};
use crate::git::Git;
use crate::models::{Project, RebaseStatus};

use super::App;

impl App {
    pub fn focus_next(&mut self) {
        // Close overlays when cycling focus (clean slate)
        self.show_file_tree = false;
        self.show_session_list = false;
        self.focus = match self.focus {
            Focus::Worktrees => Focus::Viewer,
            Focus::Viewer => Focus::Output,
            Focus::Output => Focus::Input,
            Focus::Input => Focus::Worktrees,
            // FileTree focus only active when overlay is open — cycle out to Worktrees
            Focus::FileTree => Focus::Viewer,
            Focus::WorktreeCreation | Focus::BranchDialog => self.focus,
        };
    }

    pub fn focus_prev(&mut self) {
        // If file tree is open and we'd land on Worktrees, go to FileTree instead
        // (keeps the overlay open so you can Shift+Tab back into it)
        let file_tree_open = self.show_file_tree;
        self.show_session_list = false;
        self.focus = match self.focus {
            Focus::Worktrees => { self.show_file_tree = false; Focus::Input }
            Focus::Viewer if file_tree_open => Focus::FileTree,
            Focus::Viewer => { self.show_file_tree = false; Focus::Worktrees }
            Focus::Output => { self.show_file_tree = false; Focus::Viewer }
            Focus::Input => { self.show_file_tree = false; Focus::Output }
            Focus::FileTree => { self.show_file_tree = false; Focus::Worktrees }
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

    /// Open the Git Actions panel for the currently selected worktree.
    /// Gathers branch info and changed files via `git diff --name-status` + `--numstat`.
    pub fn open_git_actions_panel(&mut self) {
        let session = match self.current_session() {
            Some(s) => s,
            None => { self.set_status("No worktree selected"); return; }
        };
        let wt_path = match session.worktree_path.as_ref() {
            Some(p) => p.clone(),
            None => { self.set_status("No worktree path"); return; }
        };
        let worktree_name = session.branch_name.clone();
        let main_branch = self.project.as_ref()
            .map(|p| p.main_branch.clone())
            .unwrap_or_else(|| "main".to_string());

        // Load changed files — typically <100ms, fine for modal open
        let changed_files = match Git::get_diff_files(&wt_path, &main_branch) {
            Ok(files) => files.into_iter().map(|(path, status, add, del)| {
                GitChangedFile { path, status, additions: add, deletions: del }
            }).collect(),
            Err(_) => Vec::new(),
        };

        self.git_actions_panel = Some(GitActionsPanel {
            worktree_name,
            worktree_path: wt_path,
            main_branch,
            changed_files,
            selected_file: 0,
            file_scroll: 0,
            actions_focused: true,
            selected_action: 0,
            result_message: None,
        });
    }

    /// Close the Git Actions panel
    pub fn close_git_actions_panel(&mut self) {
        self.git_actions_panel = None;
    }

    /// Load a file into the viewer for Read/Write tool clicks (no diff overlay).
    /// Opens the file with syntax highlighting at the top of the file.
    pub fn load_file_at_path(&mut self, file_path: &str) {
        use std::path::PathBuf;
        let path = PathBuf::from(file_path);
        if let Ok(content) = std::fs::read_to_string(&path) {
            self.viewer_content = Some(content);
            self.viewer_path = Some(path);
            self.viewer_mode = crate::app::ViewerMode::File;
            self.viewer_edit_diff = None;
            self.viewer_edit_diff_line = None;
            self.viewer_scroll = 0;
            self.viewer_lines_dirty = true;
            self.focus = Focus::Viewer;
        } else {
            self.set_status(&format!("Cannot read file: {}", file_path));
        }
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
            let status = session.status(self.is_session_running(&session.branch_name));
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

    /// Save run commands — globals to ~/.azureal/, locals to .azureal/
    pub fn save_run_commands(&self) -> anyhow::Result<()> {
        // Split by scope
        let (globals, locals): (Vec<_>, Vec<_>) = self.run_commands.iter().partition(|c| c.global);

        // Write global run commands to ~/.azureal/runcmds
        let _ = ensure_config_dir();
        let global_path = config_dir().join("runcmds");
        let global_json: Vec<_> = globals.iter().map(|c| serde_json::json!({"name": c.name, "command": c.command})).collect();
        std::fs::write(&global_path, serde_json::to_string_pretty(&global_json)?)?;

        // Write project-local run commands to .azureal/runcmds
        if let Some(dir) = ensure_project_data_dir()? {
            let project_path = dir.join("runcmds");
            let project_json: Vec<_> = locals.iter().map(|c| serde_json::json!({"name": c.name, "command": c.command})).collect();
            std::fs::write(&project_path, serde_json::to_string_pretty(&project_json)?)?;
        }
        Ok(())
    }

    /// Load run commands — merges globals (~/.azureal/) then project-locals (.azureal/)
    pub fn load_run_commands(&mut self) {
        self.run_commands.clear();

        // Load global run commands from ~/.azureal/runcmds
        let global_path = config_dir().join("runcmds");
        if let Ok(content) = std::fs::read_to_string(&global_path) {
            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                self.run_commands.extend(arr.iter().filter_map(|v| {
                    let name = v.get("name")?.as_str()?.to_string();
                    let command = v.get("command")?.as_str()?.to_string();
                    Some(RunCommand::new(name, command, true))
                }));
            }
        }

        // Load project-local run commands from .azureal/runcmds
        if let Some(dir) = project_data_dir() {
            let project_path = dir.join("runcmds");
            if let Ok(content) = std::fs::read_to_string(&project_path) {
                if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                    self.run_commands.extend(arr.iter().filter_map(|v| {
                        let name = v.get("name")?.as_str()?.to_string();
                        let command = v.get("command")?.as_str()?.to_string();
                        Some(RunCommand::new(name, command, false))
                    }));
                }
            }
        }
    }

    // ── Preset prompts ──

    /// Open preset prompt picker — if no presets exist, open add dialog directly
    pub fn open_preset_prompt_picker(&mut self) {
        if self.preset_prompts.is_empty() {
            self.preset_prompt_dialog = Some(PresetPromptDialog::new());
        } else {
            self.preset_prompt_picker = Some(PresetPromptPicker::new());
        }
    }

    /// Apply a preset prompt: populate input box, enter prompt mode, close picker
    pub fn select_preset_prompt(&mut self, idx: usize) {
        let Some(preset) = self.preset_prompts.get(idx) else { return };
        self.input = preset.prompt.clone();
        self.input_cursor = self.input.chars().count();
        self.prompt_mode = true;
        self.focus = Focus::Input;
        self.preset_prompt_picker = None;
        self.set_status(format!("Loaded preset: {}", preset.name));
    }

    /// Save preset prompts — globals to ~/.azureal/, project-locals to .azureal/
    /// Each file stores only its own scope; both are always overwritten on save.
    pub fn save_preset_prompts(&self) -> anyhow::Result<()> {
        // Split presets by scope
        let (globals, locals): (Vec<_>, Vec<_>) = self.preset_prompts.iter()
            .partition(|p| p.global);

        // Write global presets to ~/.azureal/presetprompts
        let _ = ensure_config_dir();
        let global_path = config_dir().join("presetprompts");
        let global_json: Vec<_> = globals.iter().map(|p| serde_json::json!({"name": p.name, "prompt": p.prompt})).collect();
        std::fs::write(&global_path, serde_json::to_string_pretty(&global_json)?)?;

        // Write project presets to .azureal/presetprompts
        if let Some(dir) = ensure_project_data_dir()? {
            let project_path = dir.join("presetprompts");
            let project_json: Vec<_> = locals.iter().map(|p| serde_json::json!({"name": p.name, "prompt": p.prompt})).collect();
            std::fs::write(&project_path, serde_json::to_string_pretty(&project_json)?)?;
        }
        Ok(())
    }

    /// Load preset prompts — merges globals (~/.azureal/) then project-locals (.azureal/)
    pub fn load_preset_prompts(&mut self) {
        self.preset_prompts.clear();

        // Load global presets from ~/.azureal/presetprompts
        let global_path = config_dir().join("presetprompts");
        if let Ok(content) = std::fs::read_to_string(&global_path) {
            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                self.preset_prompts.extend(arr.iter().filter_map(|v| {
                    let name = v.get("name")?.as_str()?.to_string();
                    let prompt = v.get("prompt")?.as_str()?.to_string();
                    Some(PresetPrompt::new(name, prompt, true))
                }));
            }
        }

        // Load project-local presets from .azureal/presetprompts
        if let Some(dir) = project_data_dir() {
            let project_path = dir.join("presetprompts");
            if let Ok(content) = std::fs::read_to_string(&project_path) {
                if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                    self.preset_prompts.extend(arr.iter().filter_map(|v| {
                        let name = v.get("name")?.as_str()?.to_string();
                        let prompt = v.get("prompt")?.as_str()?.to_string();
                        Some(PresetPrompt::new(name, prompt, false))
                    }));
                }
            }
        }
    }

    // Viewer tabs
    pub fn viewer_tab_current(&mut self) {
        // 2 rows × 6 tabs per row = 12 max
        const MAX_TABS: usize = 12;
        // Save current viewer state to a new tab (if we have content)
        if self.viewer_content.is_some() || self.viewer_path.is_some() {
            if self.viewer_tabs.len() >= MAX_TABS {
                self.status_message = Some(format!("Max {} tabs reached", MAX_TABS));
                return;
            }
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

    // ── Projects panel ──

    /// Open the Projects panel overlay (loads entries from ~/.azureal/projects.txt)
    pub fn open_projects_panel(&mut self) {
        let entries = load_projects();
        self.projects_panel = Some(ProjectsPanel::new(entries));
    }

    /// Close the Projects panel and return focus to Worktrees
    pub fn close_projects_panel(&mut self) {
        self.projects_panel = None;
        self.focus = Focus::Worktrees;
    }

    pub fn is_projects_panel_active(&self) -> bool { self.projects_panel.is_some() }

    /// Switch to a different project by path. Kills all Claude processes,
    /// clears all session/render state, and reloads everything for the new project.
    pub fn switch_project(&mut self, path: std::path::PathBuf) {
        // Kill all running Claude processes first
        self.cancel_all_claude();

        // Clear all session and render state
        self.sessions.clear();
        self.selected_worktree = None;
        self.display_events.clear();
        self.output_lines.clear();
        self.output_buffer.clear();
        self.output_scroll = usize::MAX;
        self.pending_user_message = None;
        self.staged_prompt = None;
        self.event_parser = crate::events::EventParser::new();
        self.selected_event = None;
        self.session_file_path = None;
        self.session_file_modified = None;
        self.session_file_size = 0;
        self.session_file_parse_offset = 0;
        self.session_file_dirty = false;
        self.pending_tool_calls.clear();
        self.failed_tool_calls.clear();
        self.claude_session_ids.clear();
        self.claude_exit_codes.clear();
        self.session_files.clear();
        self.session_selected_file_idx.clear();
        self.file_tree_entries.clear();
        self.file_tree_selected = None;
        self.file_tree_expanded.clear();
        self.viewer_content = None;
        self.viewer_path = None;
        self.viewer_tabs.clear();
        self.title_session_name.clear();
        self.current_todos.clear();
        self.subagent_todos.clear();
        self.invalidate_render_cache();
        self.invalidate_sidebar();
        self.invalidate_file_tree();
        self.rendered_events_count = 0;
        self.rendered_content_line_count = 0;
        self.rendered_events_start = 0;

        // Set the new project
        let main_branch = Git::get_main_branch(&path).unwrap_or_else(|_| "main".to_string());
        self.project = Some(Project::from_path(path, main_branch));

        // Reload sessions and output
        let _ = self.load_sessions();
        self.load_session_output();
        self.load_run_commands();
        self.load_preset_prompts();

        // Close the panel and return focus
        self.projects_panel = None;
        self.focus = Focus::Worktrees;
    }

    /// Set the OS terminal window title to reflect the current state.
    /// "AZUREAL" when no project, "AZUREAL @ project : branch" when loaded.
    pub fn update_terminal_title(&self) {
        let title = match (&self.project, self.current_session()) {
            (Some(project), Some(session)) => {
                let branch = session.branch_name.strip_prefix("azureal/")
                    .unwrap_or(&session.branch_name);
                format!("AZUREAL @ {} : {}", project.name, branch)
            }
            (Some(project), None) => format!("AZUREAL @ {}", project.name),
            _ => "AZUREAL".to_string(),
        };
        let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::SetTitle(title));
    }

    /// Kill all running Claude processes across all sessions.
    /// Slot keys ARE PID strings — parse each back to u32 for kill.
    pub fn cancel_all_claude(&mut self) {
        let slots: Vec<String> = self.running_sessions.drain().collect();
        for slot in &slots {
            if let Ok(pid) = slot.parse::<u32>() {
                #[cfg(unix)]
                { let _ = std::process::Command::new("kill").arg(pid.to_string()).status(); }
                #[cfg(windows)]
                { let _ = std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/F"]).status(); }
            }
            self.claude_receivers.remove(slot);
            self.interactive_sessions.remove(slot);
        }
        self.branch_slots.clear();
        self.active_slot.clear();
    }
}
