//! UI state management: focus, dialogs, menus, wizard, rebase, run commands

use crate::app::types::{BranchDialog, Focus, GitActionsPanel, GitChangedFile, PresetPrompt, PresetPromptDialog, PresetPromptPicker, ProjectsPanel, RunCommand, RunCommandDialog, RunCommandPicker};
use crate::config::load_projects;
use crate::git::Git;
use crate::models::Project;

use super::{App, DeferredAction};

impl App {
    pub fn focus_next(&mut self) {
        // FileTree is always visible — full cycle includes it
        self.show_session_list = false;
        self.focus = match self.focus {
            Focus::Worktrees => Focus::FileTree,
            Focus::FileTree => Focus::Viewer,
            Focus::Viewer => Focus::Session,
            Focus::Session => Focus::Input,
            Focus::Input => Focus::Worktrees,
            Focus::BranchDialog => self.focus,
        };
    }

    pub fn focus_prev(&mut self) {
        self.show_session_list = false;
        self.focus = match self.focus {
            Focus::Worktrees => Focus::Input,
            Focus::Input => Focus::Session,
            Focus::Session => Focus::Viewer,
            Focus::Viewer => Focus::FileTree,
            Focus::FileTree => Focus::Worktrees,
            Focus::BranchDialog => self.focus,
        };
    }

    pub fn toggle_help(&mut self) { self.show_help = !self.show_help; }
    pub fn toggle_terminal(&mut self) {
        if self.terminal_mode { self.close_terminal(); } else { self.open_terminal(); }
    }

    pub fn open_branch_dialog(&mut self, branches: Vec<String>, checked_out: Vec<String>, worktree_counts: Vec<usize>) {
        self.branch_dialog = Some(BranchDialog::new(branches, checked_out, worktree_counts));
        self.focus = Focus::BranchDialog;
    }

    pub fn close_branch_dialog(&mut self) {
        self.branch_dialog = None;
        self.focus = Focus::Worktrees;
    }

    /// Open the Git Actions panel for the currently selected worktree.
    /// Gathers branch info and changed files via `git diff --name-status` + `--numstat`.
    pub fn open_git_actions_panel(&mut self) {
        let session = match self.current_worktree() {
            Some(s) => s,
            None => { self.set_status("No worktree selected"); return; }
        };
        let wt_path = match session.worktree_path.as_ref() {
            Some(p) => p.clone(),
            None => { self.set_status("No worktree path"); return; }
        };
        let worktree_name = session.branch_name.clone();
        let project = match self.project.as_ref() {
            Some(p) => p,
            None => { self.set_status("No project loaded"); return; }
        };
        let main_branch = project.main_branch.clone();
        let repo_root = project.path.clone();

        // Load changed files — typically <100ms, fine for modal open
        let changed_files = match Git::get_diff_files(&wt_path, &main_branch) {
            Ok(files) => files.into_iter().map(|(path, status, add, del)| {
                GitChangedFile { path, status, additions: add, deletions: del }
            }).collect(),
            Err(_) => Vec::new(),
        };

        let is_on_main = worktree_name == main_branch;

        // Load commit log — feature branches show only branch-specific commits
        let log_main = if is_on_main { None } else { Some(main_branch.as_str()) };
        let commits = Git::get_commit_log(&wt_path, 200, log_main)
            .unwrap_or_default()
            .into_iter()
            .map(|(hash, full_hash, subject, is_pushed)| {
                crate::app::types::GitCommit { hash, full_hash, subject, is_pushed }
            })
            .collect();

        let auto_resolve_files = crate::azufig::load_auto_resolve_files(&repo_root);
        let (commits_behind_main, commits_ahead_main) = if is_on_main { (0, 0) } else {
            Git::get_main_divergence(&wt_path, &main_branch)
        };
        let (commits_behind_remote, commits_ahead_remote) = Git::get_remote_divergence(&wt_path);

        self.git_actions_panel = Some(GitActionsPanel {
            worktree_name,
            worktree_path: wt_path,
            repo_root,
            main_branch,
            is_on_main,
            changed_files,
            selected_file: 0,
            file_scroll: 0,
            focused_pane: 0,
            selected_action: 0,
            result_message: None,
            commit_overlay: None,
            conflict_overlay: None,
            commits,
            selected_commit: 0,
            commit_scroll: 0,
            viewer_diff: None,
            viewer_diff_title: None,
            commits_behind_main,
            commits_ahead_main,
            commits_behind_remote,
            commits_ahead_remote,
            auto_resolve_files,
            auto_resolve_overlay: None,
            squash_merge_receiver: None,
        });
    }

    /// Close the Git Actions panel. If a conflict overlay is open (in-progress
    /// rebase on the feature branch), abort the rebase to leave the branch clean.
    pub fn close_git_actions_panel(&mut self) {
        if let Some(ref panel) = self.git_actions_panel {
            if panel.conflict_overlay.is_some() {
                // Abort rebase on the feature branch (if conflict came from rebase)
                let _ = Git::rebase_abort(&panel.worktree_path);
                // Clean up squash merge state on main (if conflict came from
                // squash merge — no MERGE_HEAD, so merge --abort won't work)
                Git::cleanup_squash_merge_state(&panel.repo_root);
            }
        }
        self.git_actions_panel = None;
        self.git_status_selected = false;
        // Session pane visible again — clear unread for the viewed session, then
        // recompute branch-level unread (remove branch if no more unread UUIDs)
        if let Some(wt) = self.current_worktree() {
            let branch = wt.branch_name.clone();
            if let Some(viewed_id) = self.viewed_session_id(&branch) {
                self.unread_session_ids.remove(&viewed_id);
            }
            let still_unread = self.session_files.get(&branch)
                .map(|files| files.iter().any(|(uuid, _, _)| self.unread_session_ids.contains(uuid)))
                .unwrap_or(false);
            if !still_unread {
                self.unread_sessions.remove(&branch);
            }
        }
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

    // ── Main branch browse mode ──

    /// Enter read-only main branch browse mode. Saves current selection,
    /// switches to main_worktree, opens file tree, and loads main's session.
    pub fn enter_main_browse(&mut self) {
        if self.main_worktree.is_none() {
            self.set_status("No main worktree found");
            return;
        }
        self.save_current_terminal();
        self.pre_main_browse_selection = self.selected_worktree;
        self.browsing_main = true;
        // current_worktree() now returns main_worktree — load its session + file tree
        self.load_session_output();
        self.focus = Focus::FileTree;
        self.invalidate_sidebar();
    }

    /// Exit main branch browse mode. Restores previous worktree selection and focus.
    pub fn exit_main_browse(&mut self) {
        self.browsing_main = false;
        self.selected_worktree = self.pre_main_browse_selection.take();
        self.focus = Focus::Worktrees;
        self.load_session_output();
        self.invalidate_sidebar();
    }

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

    /// Save run commands — globals to `[runcmds]` in global azufig,
    /// locals to `[runcmds]` in project azufig (load-modify-save).
    /// Format: N_name = "command" where N is the 1-based position (quick-select number)
    pub fn save_run_commands(&self) -> anyhow::Result<()> {
        let (globals, locals): (Vec<_>, Vec<_>) = self.run_commands.iter().partition(|c| c.global);

        // Write global run commands — enumerate with 1-based prefix to preserve order
        crate::azufig::update_global_azufig(|az| {
            az.runcmds = globals.iter().enumerate()
                .map(|(i, c)| (format!("{}_{}", i + 1, c.name), c.command.clone())).collect();
        });

        // Write project-local run commands — same numbering (continues from globals in the Vec,
        // but each scope has its own 1-based numbering)
        if let Some(ref project) = self.project {
            crate::azufig::update_project_azufig(&project.path, |az| {
                az.runcmds = locals.iter().enumerate()
                    .map(|(i, c)| (format!("{}_{}", i + 1, c.name), c.command.clone())).collect();
            });
        }
        Ok(())
    }

    /// Load run commands — merges globals then project-locals from azufig.
    /// Format: N_name = "command" — sorted by N to restore saved order, prefix stripped
    pub fn load_run_commands(&mut self) {
        self.run_commands.clear();

        // Load global run commands, sorted by numeric prefix
        let global = crate::azufig::load_global_azufig();
        self.run_commands.extend(load_ordered_map(&global.runcmds, true));

        // Load project-local run commands, sorted by numeric prefix
        if let Some(ref project) = self.project {
            let local = crate::azufig::load_project_azufig(&project.path);
            self.run_commands.extend(load_ordered_map(&local.runcmds, false));
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

    /// Save preset prompts — globals to `[presetprompts]` in global azufig,
    /// locals to `[presetprompts]` in project azufig (load-modify-save).
    /// Format: N_name = "prompt text" where N is the 1-based position
    pub fn save_preset_prompts(&self) -> anyhow::Result<()> {
        let (globals, locals): (Vec<_>, Vec<_>) = self.preset_prompts.iter()
            .partition(|p| p.global);

        // Write global presets — enumerate with 1-based prefix to preserve order
        crate::azufig::update_global_azufig(|az| {
            az.presetprompts = globals.iter().enumerate()
                .map(|(i, p)| (format!("{}_{}", i + 1, p.name), p.prompt.clone())).collect();
        });

        // Write project-local presets
        if let Some(ref project) = self.project {
            crate::azufig::update_project_azufig(&project.path, |az| {
                az.presetprompts = locals.iter().enumerate()
                    .map(|(i, p)| (format!("{}_{}", i + 1, p.name), p.prompt.clone())).collect();
            });
        }
        Ok(())
    }

    /// Load preset prompts — merges globals then project-locals from azufig.
    /// Format: N_name = "prompt text" — sorted by N to restore saved order, prefix stripped
    pub fn load_preset_prompts(&mut self) {
        self.preset_prompts.clear();

        // Load global presets, sorted by numeric prefix
        let global = crate::azufig::load_global_azufig();
        self.preset_prompts.extend(load_ordered_presets(&global.presetprompts, true));

        // Load project-local presets, sorted by numeric prefix
        if let Some(ref project) = self.project {
            let local = crate::azufig::load_project_azufig(&project.path);
            self.preset_prompts.extend(load_ordered_presets(&local.presetprompts, false));
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

    /// Returns true if any git operation is in progress that could corrupt the
    /// repo if interrupted (commit, push, rebase, RCR, commit message generation).
    pub fn git_action_in_progress(&self) -> bool {
        // Deferred git commit or commit+push about to execute
        if matches!(self.deferred_action,
            Some(DeferredAction::GitCommit { .. } | DeferredAction::GitCommitAndPush { .. })) {
            return true;
        }
        // RCR session active (Claude resolving rebase conflicts on a worktree)
        if self.rcr_session.is_some() {
            return true;
        }
        // Commit message being generated (Claude one-shot running)
        if let Some(ref panel) = self.git_actions_panel {
            if let Some(ref overlay) = panel.commit_overlay {
                if overlay.generating { return true; }
            }
            // Squash merge running on background thread
            if panel.squash_merge_receiver.is_some() { return true; }
        }
        // Loading indicator for a git operation (e.g. "Committing...")
        if self.loading_indicator.is_some() && matches!(self.deferred_action,
            Some(DeferredAction::GitCommit { .. } | DeferredAction::GitCommitAndPush { .. })) {
            return true;
        }
        false
    }

    /// Switch to a different project by path. Kills all Claude processes,
    /// clears all session/render state, and reloads everything for the new project.
    pub fn switch_project(&mut self, path: std::path::PathBuf) {
        // Kill all running Claude processes first
        self.cancel_all_claude();

        // Clear all session and render state
        self.browsing_main = false;
        self.pre_main_browse_selection = None;
        self.main_worktree = None;
        self.worktrees.clear();
        self.selected_worktree = None;
        self.display_events.clear();
        self.session_lines.clear();
        self.session_buffer.clear();
        self.session_scroll = usize::MAX;
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
        self.unread_sessions.clear();
        self.unread_session_ids.clear();
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
        self.project = Some(Project::from_path(path.clone(), main_branch));

        // Reload filetree hidden dirs from the new project's azufig
        let az = crate::azufig::load_project_azufig(&path);
        self.file_tree_hidden_dirs = az.filetree.hidden.into_iter().collect();

        // Reload sessions and output
        let _ = self.load_worktrees();
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
        let title = match (&self.project, self.current_worktree()) {
            (Some(project), Some(session)) => {
                let branch = crate::models::strip_branch_prefix(&session.branch_name);
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
        }
        self.branch_slots.clear();
        self.active_slot.clear();
    }
}

/// Parse a "N_name" key: extract the numeric prefix for sorting and strip it to get the clean name.
/// Keys without a valid prefix get sort key usize::MAX (appended at end) and are used as-is.
fn parse_ordered_key(key: &str) -> (usize, String) {
    if let Some(idx) = key.find('_') {
        if let Ok(n) = key[..idx].parse::<usize>() {
            return (n, key[idx + 1..].to_string());
        }
    }
    (usize::MAX, key.to_string())
}

/// Load a HashMap of "N_name" = "value" entries as an ordered Vec of RunCommands.
/// Sorts by numeric prefix, strips prefix from name.
fn load_ordered_map(map: &std::collections::HashMap<String, String>, global: bool) -> Vec<RunCommand> {
    let mut entries: Vec<_> = map.iter()
        .map(|(k, v)| { let (ord, name) = parse_ordered_key(k); (ord, name, v.clone()) })
        .collect();
    entries.sort_by_key(|(ord, _, _)| *ord);
    entries.into_iter().map(|(_, name, cmd)| RunCommand::new(name, cmd, global)).collect()
}

/// Load a HashMap of "N_name" = "value" entries as an ordered Vec of PresetPrompts.
/// Sorts by numeric prefix, strips prefix from name.
fn load_ordered_presets(map: &std::collections::HashMap<String, String>, global: bool) -> Vec<PresetPrompt> {
    let mut entries: Vec<_> = map.iter()
        .map(|(k, v)| { let (ord, name) = parse_ordered_key(k); (ord, name, v.clone()) })
        .collect();
    entries.sort_by_key(|(ord, _, _)| *ord);
    entries.into_iter().map(|(_, name, prompt)| PresetPrompt::new(name, prompt, global)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ── focus_next ──

    #[test]
    fn focus_next_worktrees_to_filetree() {
        let mut app = App::new();
        app.focus = Focus::Worktrees;
        app.focus_next();
        assert_eq!(app.focus, Focus::FileTree);
    }

    #[test]
    fn focus_next_filetree_to_viewer() {
        let mut app = App::new();
        app.focus = Focus::FileTree;
        app.focus_next();
        assert_eq!(app.focus, Focus::Viewer);
    }

    #[test]
    fn focus_next_viewer_to_session() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.focus_next();
        assert_eq!(app.focus, Focus::Session);
    }

    #[test]
    fn focus_next_session_to_input() {
        let mut app = App::new();
        app.focus = Focus::Session;
        app.focus_next();
        assert_eq!(app.focus, Focus::Input);
    }

    #[test]
    fn focus_next_input_to_worktrees() {
        let mut app = App::new();
        app.focus = Focus::Input;
        app.focus_next();
        assert_eq!(app.focus, Focus::Worktrees);
    }

    #[test]
    fn focus_next_full_cycle() {
        let mut app = App::new();
        app.focus = Focus::Worktrees;
        let expected = [Focus::FileTree, Focus::Viewer, Focus::Session, Focus::Input, Focus::Worktrees];
        for &exp in &expected {
            app.focus_next();
            assert_eq!(app.focus, exp);
        }
    }

    #[test]
    fn focus_next_branch_dialog_stays() {
        let mut app = App::new();
        app.focus = Focus::BranchDialog;
        app.focus_next();
        assert_eq!(app.focus, Focus::BranchDialog);
    }

    #[test]
    fn focus_next_clears_session_list() {
        let mut app = App::new();
        app.show_session_list = true;
        app.focus = Focus::Worktrees;
        app.focus_next();
        assert!(!app.show_session_list);
    }

    // ── focus_prev ──

    #[test]
    fn focus_prev_worktrees_to_input() {
        let mut app = App::new();
        app.focus = Focus::Worktrees;
        app.focus_prev();
        assert_eq!(app.focus, Focus::Input);
    }

    #[test]
    fn focus_prev_input_to_session() {
        let mut app = App::new();
        app.focus = Focus::Input;
        app.focus_prev();
        assert_eq!(app.focus, Focus::Session);
    }

    #[test]
    fn focus_prev_session_to_viewer() {
        let mut app = App::new();
        app.focus = Focus::Session;
        app.focus_prev();
        assert_eq!(app.focus, Focus::Viewer);
    }

    #[test]
    fn focus_prev_viewer_to_filetree() {
        let mut app = App::new();
        app.focus = Focus::Viewer;
        app.focus_prev();
        assert_eq!(app.focus, Focus::FileTree);
    }

    #[test]
    fn focus_prev_filetree_to_worktrees() {
        let mut app = App::new();
        app.focus = Focus::FileTree;
        app.focus_prev();
        assert_eq!(app.focus, Focus::Worktrees);
    }

    #[test]
    fn focus_prev_full_cycle() {
        let mut app = App::new();
        app.focus = Focus::Worktrees;
        let expected = [Focus::Input, Focus::Session, Focus::Viewer, Focus::FileTree, Focus::Worktrees];
        for &exp in &expected {
            app.focus_prev();
            assert_eq!(app.focus, exp);
        }
    }

    #[test]
    fn focus_prev_branch_dialog_stays() {
        let mut app = App::new();
        app.focus = Focus::BranchDialog;
        app.focus_prev();
        assert_eq!(app.focus, Focus::BranchDialog);
    }

    #[test]
    fn focus_prev_clears_session_list() {
        let mut app = App::new();
        app.show_session_list = true;
        app.focus = Focus::Session;
        app.focus_prev();
        assert!(!app.show_session_list);
    }

    // ── focus_next and focus_prev are inverses ──

    #[test]
    fn focus_next_prev_roundtrip() {
        let mut app = App::new();
        app.focus = Focus::Worktrees;
        app.focus_next();
        app.focus_prev();
        assert_eq!(app.focus, Focus::Worktrees);
    }

    #[test]
    fn focus_prev_next_roundtrip() {
        let mut app = App::new();
        app.focus = Focus::Session;
        app.focus_prev();
        app.focus_next();
        assert_eq!(app.focus, Focus::Session);
    }

    // ── toggle_help ──

    #[test]
    fn toggle_help_on() {
        let mut app = App::new();
        assert!(!app.show_help);
        app.toggle_help();
        assert!(app.show_help);
    }

    #[test]
    fn toggle_help_off() {
        let mut app = App::new();
        app.show_help = true;
        app.toggle_help();
        assert!(!app.show_help);
    }

    #[test]
    fn toggle_help_double_toggle() {
        let mut app = App::new();
        app.toggle_help();
        app.toggle_help();
        assert!(!app.show_help);
    }

    // ── open_branch_dialog ──

    #[test]
    fn open_branch_dialog_empty_branches_still_opens() {
        let mut app = App::new();
        app.open_branch_dialog(vec![], vec![], vec![]);
        // Dialog opens even with no branches (has "[+] Create new" row)
        assert!(app.branch_dialog.is_some());
        assert_eq!(app.focus, Focus::BranchDialog);
    }

    #[test]
    fn open_branch_dialog_with_branches() {
        let mut app = App::new();
        app.open_branch_dialog(
            vec!["main".to_string(), "dev".to_string()],
            vec!["main".to_string()],
            vec![0, 1],
        );
        assert!(app.branch_dialog.is_some());
        assert_eq!(app.focus, Focus::BranchDialog);
        let dialog = app.branch_dialog.as_ref().unwrap();
        assert_eq!(dialog.branches.len(), 2);
        assert_eq!(dialog.checked_out.len(), 1);
    }

    // ── close_branch_dialog ──

    #[test]
    fn close_branch_dialog_clears_and_refocuses() {
        let mut app = App::new();
        app.branch_dialog = Some(BranchDialog::new(
            vec!["branch".to_string()],
            vec![],
            vec![0],
        ));
        app.focus = Focus::BranchDialog;
        app.close_branch_dialog();
        assert!(app.branch_dialog.is_none());
        assert_eq!(app.focus, Focus::Worktrees);
    }

    // ── close_projects_panel ──

    #[test]
    fn close_projects_panel_clears() {
        let mut app = App::new();
        app.projects_panel = Some(ProjectsPanel::new(vec![]));
        app.focus = Focus::Input;
        app.close_projects_panel();
        assert!(app.projects_panel.is_none());
        assert_eq!(app.focus, Focus::Worktrees);
    }

    // ── is_projects_panel_active ──

    #[test]
    fn is_projects_panel_active_false_default() {
        let app = App::new();
        assert!(!app.is_projects_panel_active());
    }

    #[test]
    fn is_projects_panel_active_true() {
        let mut app = App::new();
        app.projects_panel = Some(ProjectsPanel::new(vec![]));
        assert!(app.is_projects_panel_active());
    }

    // ── open_run_command_dialog ──

    #[test]
    fn open_run_command_dialog_creates_dialog() {
        let mut app = App::new();
        assert!(app.run_command_dialog.is_none());
        app.open_run_command_dialog();
        assert!(app.run_command_dialog.is_some());
    }

    // ── open_run_command_picker ──

    #[test]
    fn open_run_command_picker_empty_commands_sets_status() {
        let mut app = App::new();
        app.open_run_command_picker();
        assert!(app.run_command_picker.is_none());
        assert!(app.status_message.is_some());
    }

    #[test]
    fn open_run_command_picker_single_command_executes() {
        let mut app = App::new();
        app.run_commands.push(RunCommand::new("test", "cargo test", false));
        app.open_run_command_picker();
        // With a single command, no picker is opened — it executes directly
        assert!(app.run_command_picker.is_none());
    }

    #[test]
    fn open_run_command_picker_multiple_commands_opens_picker() {
        let mut app = App::new();
        app.run_commands.push(RunCommand::new("build", "cargo build", false));
        app.run_commands.push(RunCommand::new("test", "cargo test", false));
        app.open_run_command_picker();
        assert!(app.run_command_picker.is_some());
    }

    // ── open_preset_prompt_picker ──

    #[test]
    fn open_preset_prompt_picker_no_presets_opens_dialog() {
        let mut app = App::new();
        app.open_preset_prompt_picker();
        assert!(app.preset_prompt_dialog.is_some());
        assert!(app.preset_prompt_picker.is_none());
    }

    #[test]
    fn open_preset_prompt_picker_with_presets_opens_picker() {
        let mut app = App::new();
        app.preset_prompts.push(PresetPrompt::new("quick", "do it fast", true));
        app.open_preset_prompt_picker();
        assert!(app.preset_prompt_picker.is_some());
        assert!(app.preset_prompt_dialog.is_none());
    }

    // ── select_preset_prompt ──

    #[test]
    fn select_preset_prompt_valid_index() {
        let mut app = App::new();
        app.preset_prompts.push(PresetPrompt::new("fix", "Fix the bug in main.rs", false));
        app.select_preset_prompt(0);
        assert_eq!(app.input, "Fix the bug in main.rs");
        assert_eq!(app.input_cursor, "Fix the bug in main.rs".chars().count());
        assert!(app.prompt_mode);
        assert_eq!(app.focus, Focus::Input);
        assert!(app.preset_prompt_picker.is_none());
    }

    #[test]
    fn select_preset_prompt_invalid_index_noop() {
        let mut app = App::new();
        app.preset_prompts.push(PresetPrompt::new("a", "b", false));
        let old_input = app.input.clone();
        app.select_preset_prompt(5); // out of bounds
        assert_eq!(app.input, old_input);
    }

    #[test]
    fn select_preset_prompt_sets_status() {
        let mut app = App::new();
        app.preset_prompts.push(PresetPrompt::new("my-preset", "prompt text", true));
        app.select_preset_prompt(0);
        let status = app.status_message.as_deref().unwrap();
        assert!(status.contains("my-preset"));
    }

    // ── viewer_tab_current ──

    #[test]
    fn viewer_tab_current_no_content_noop() {
        let mut app = App::new();
        app.viewer_content = None;
        app.viewer_path = None;
        app.viewer_tab_current();
        assert!(app.viewer_tabs.is_empty());
    }

    #[test]
    fn viewer_tab_current_adds_tab() {
        let mut app = App::new();
        app.viewer_content = Some("file content".to_string());
        app.viewer_path = Some(std::path::PathBuf::from("/tmp/test.rs"));
        app.viewer_scroll = 10;
        app.viewer_mode = crate::app::types::ViewerMode::File;
        app.viewer_tab_current();
        assert_eq!(app.viewer_tabs.len(), 1);
        assert_eq!(app.viewer_active_tab, 0);
        let tab = &app.viewer_tabs[0];
        assert_eq!(tab.title, "test.rs");
        assert_eq!(tab.scroll, 10);
    }

    #[test]
    fn viewer_tab_current_max_tabs() {
        let mut app = App::new();
        for i in 0..12 {
            app.viewer_content = Some(format!("content {}", i));
            app.viewer_path = Some(std::path::PathBuf::from(format!("/tmp/file{}.rs", i)));
            app.viewer_tab_current();
        }
        assert_eq!(app.viewer_tabs.len(), 12);
        // Try adding a 13th — should be rejected
        app.viewer_content = Some("extra".to_string());
        app.viewer_path = Some(std::path::PathBuf::from("/tmp/extra.rs"));
        app.viewer_tab_current();
        assert_eq!(app.viewer_tabs.len(), 12);
        assert!(app.status_message.as_deref().unwrap().contains("Max"));
    }

    // ── toggle_viewer_tab_dialog ──

    #[test]
    fn toggle_viewer_tab_dialog() {
        let mut app = App::new();
        assert!(!app.viewer_tab_dialog);
        app.toggle_viewer_tab_dialog();
        assert!(app.viewer_tab_dialog);
        app.toggle_viewer_tab_dialog();
        assert!(!app.viewer_tab_dialog);
    }

    // ── viewer_close_current_tab ──

    #[test]
    fn viewer_close_current_tab_empty_noop() {
        let mut app = App::new();
        app.viewer_close_current_tab(); // should not panic
        assert!(app.viewer_tabs.is_empty());
    }

    #[test]
    fn viewer_close_current_tab_clears_viewer_when_last() {
        let mut app = App::new();
        app.viewer_content = Some("content".to_string());
        app.viewer_path = Some(std::path::PathBuf::from("/tmp/f.rs"));
        app.viewer_tab_current();
        assert_eq!(app.viewer_tabs.len(), 1);
        app.viewer_close_current_tab();
        assert!(app.viewer_tabs.is_empty());
        assert!(app.viewer_content.is_none());
        assert!(app.viewer_path.is_none());
        assert_eq!(app.viewer_mode, crate::app::types::ViewerMode::Empty);
    }

    // ── load_tab_to_viewer ──

    #[test]
    fn load_tab_to_viewer_restores_state() {
        let mut app = App::new();
        // Create two tabs
        app.viewer_content = Some("content A".to_string());
        app.viewer_path = Some(std::path::PathBuf::from("/a.rs"));
        app.viewer_scroll = 5;
        app.viewer_mode = crate::app::types::ViewerMode::File;
        app.viewer_tab_current();
        app.viewer_content = Some("content B".to_string());
        app.viewer_path = Some(std::path::PathBuf::from("/b.rs"));
        app.viewer_scroll = 20;
        app.viewer_tab_current();
        // Switch to tab 0
        app.viewer_active_tab = 0;
        app.load_tab_to_viewer();
        assert_eq!(app.viewer_content.as_deref(), Some("content A"));
        assert_eq!(app.viewer_scroll, 5);
    }

    // ── enter_main_browse / exit_main_browse ──

    #[test]
    fn enter_main_browse_no_main_worktree_sets_status() {
        let mut app = App::new();
        app.main_worktree = None;
        app.enter_main_browse();
        assert!(!app.browsing_main);
        assert!(app.status_message.as_deref().unwrap().contains("No main worktree"));
    }

    #[test]
    fn exit_main_browse_restores_selection() {
        let mut app = App::new();
        app.browsing_main = true;
        app.pre_main_browse_selection = Some(2);
        app.exit_main_browse();
        assert!(!app.browsing_main);
        assert_eq!(app.selected_worktree, Some(2));
        assert_eq!(app.focus, Focus::Worktrees);
    }

    // ── git_action_in_progress (from ui.rs context) ──

    #[test]
    fn git_action_in_progress_rcr_session() {
        let mut app = App::new();
        app.rcr_session = Some(crate::app::types::RcrSession {
            branch: "feature/test".to_string(),
            display_name: "test".to_string(),
            worktree_path: std::path::PathBuf::from("/wt"),
            repo_root: std::path::PathBuf::from("/repo"),
            slot_id: "pid-1".to_string(),
            session_id: None,
            approval_pending: false,
            continue_with_merge: false,
        });
        assert!(app.git_action_in_progress());
    }

    // ── parse_ordered_key ──

    #[test]
    fn parse_ordered_key_valid() {
        let (n, name) = parse_ordered_key("1_build");
        assert_eq!(n, 1);
        assert_eq!(name, "build");
    }

    #[test]
    fn parse_ordered_key_large_number() {
        let (n, name) = parse_ordered_key("42_deploy");
        assert_eq!(n, 42);
        assert_eq!(name, "deploy");
    }

    #[test]
    fn parse_ordered_key_no_underscore() {
        let (n, name) = parse_ordered_key("justname");
        assert_eq!(n, usize::MAX);
        assert_eq!(name, "justname");
    }

    #[test]
    fn parse_ordered_key_non_numeric_prefix() {
        let (n, name) = parse_ordered_key("abc_test");
        assert_eq!(n, usize::MAX);
        assert_eq!(name, "abc_test");
    }

    #[test]
    fn parse_ordered_key_zero_prefix() {
        let (n, name) = parse_ordered_key("0_first");
        assert_eq!(n, 0);
        assert_eq!(name, "first");
    }

    #[test]
    fn parse_ordered_key_multiple_underscores() {
        let (n, name) = parse_ordered_key("3_my_cool_cmd");
        assert_eq!(n, 3);
        assert_eq!(name, "my_cool_cmd");
    }

    #[test]
    fn parse_ordered_key_empty_name_after_prefix() {
        let (n, name) = parse_ordered_key("5_");
        assert_eq!(n, 5);
        assert_eq!(name, "");
    }

    #[test]
    fn parse_ordered_key_empty_string() {
        let (n, name) = parse_ordered_key("");
        assert_eq!(n, usize::MAX);
        assert_eq!(name, "");
    }

    // ── load_ordered_map ──

    #[test]
    fn load_ordered_map_empty() {
        let map = HashMap::new();
        let result = load_ordered_map(&map, true);
        assert!(result.is_empty());
    }

    #[test]
    fn load_ordered_map_single_entry() {
        let mut map = HashMap::new();
        map.insert("1_build".to_string(), "cargo build".to_string());
        let result = load_ordered_map(&map, true);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "build");
        assert_eq!(result[0].command, "cargo build");
        assert!(result[0].global);
    }

    #[test]
    fn load_ordered_map_preserves_order() {
        let mut map = HashMap::new();
        map.insert("3_third".to_string(), "cmd3".to_string());
        map.insert("1_first".to_string(), "cmd1".to_string());
        map.insert("2_second".to_string(), "cmd2".to_string());
        let result = load_ordered_map(&map, false);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].name, "first");
        assert_eq!(result[1].name, "second");
        assert_eq!(result[2].name, "third");
        assert!(!result[0].global);
    }

    #[test]
    fn load_ordered_map_no_prefix() {
        let mut map = HashMap::new();
        map.insert("raw_cmd".to_string(), "echo hi".to_string());
        let result = load_ordered_map(&map, true);
        assert_eq!(result.len(), 1);
        // "raw" is not numeric, so name stays "raw_cmd"
        assert_eq!(result[0].name, "raw_cmd");
    }

    // ── load_ordered_presets ──

    #[test]
    fn load_ordered_presets_empty() {
        let map = HashMap::new();
        let result = load_ordered_presets(&map, false);
        assert!(result.is_empty());
    }

    #[test]
    fn load_ordered_presets_sorted() {
        let mut map = HashMap::new();
        map.insert("2_fix".to_string(), "Fix this bug".to_string());
        map.insert("1_review".to_string(), "Review this code".to_string());
        let result = load_ordered_presets(&map, true);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "review");
        assert_eq!(result[0].prompt, "Review this code");
        assert!(result[0].global);
        assert_eq!(result[1].name, "fix");
        assert_eq!(result[1].prompt, "Fix this bug");
    }

    // ── find_edit_line (static method on App) ──

    #[test]
    fn find_edit_line_new_string_found() {
        let content = "line 0\nline 1\nthe target line\nline 3\n";
        let line = App::find_edit_line(content, "", "the target line");
        assert_eq!(line, 2);
    }

    #[test]
    fn find_edit_line_old_string_found() {
        let content = "line 0\nold code here\nline 2\n";
        let line = App::find_edit_line(content, "old code here", "");
        assert_eq!(line, 1);
    }

    #[test]
    fn find_edit_line_both_empty_returns_zero() {
        let content = "anything\nhere\n";
        let line = App::find_edit_line(content, "", "");
        assert_eq!(line, 0);
    }

    #[test]
    fn find_edit_line_new_preferred_over_old() {
        let content = "line 0\nnew stuff\nold stuff\nline 3\n";
        let line = App::find_edit_line(content, "old stuff", "new stuff");
        // new_string is checked first, so should find "new stuff" at line 1
        assert_eq!(line, 1);
    }

    #[test]
    fn find_edit_line_significant_lines_fallback() {
        let content = "line 0\nfn important_function() {\nline 2\nline 3\n";
        // new_string not found as a whole, but contains a significant line
        let line = App::find_edit_line(content, "", "something\nfn important_function() {\nsomething else");
        assert_eq!(line, 1);
    }

    #[test]
    fn find_edit_line_trimmed_match() {
        let content = "    fn hello() {\n        world();\n    }\n";
        // Exact match won't work because of different indentation
        let line = App::find_edit_line(content, "", "fn hello_world() {\n  fn hello() {");
        // Should find "fn hello() {" via trimmed matching (strategy 5)
        assert_eq!(line, 0);
    }

    #[test]
    fn find_edit_line_identifier_fallback() {
        let content = "some code\nfn calculate_total_price() {\n  return 0;\n}\n";
        // Search for identifier that exists in the content
        let line = App::find_edit_line(content, "", "x = calculate_total_price() + y");
        // "calculate_total_price" is a long identifier — should be found at line 1
        assert_eq!(line, 1);
    }

    #[test]
    fn find_edit_line_no_match_returns_zero() {
        let content = "aaa\nbbb\nccc\n";
        let line = App::find_edit_line(content, "zzz", "yyy");
        assert_eq!(line, 0);
    }

    #[test]
    fn find_edit_line_empty_content() {
        let line = App::find_edit_line("", "old", "new");
        assert_eq!(line, 0);
    }

    #[test]
    fn find_edit_line_first_line() {
        let content = "target\nother\n";
        let line = App::find_edit_line(content, "", "target");
        assert_eq!(line, 0);
    }
}

