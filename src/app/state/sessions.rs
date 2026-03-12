//! Session navigation and CRUD operations

use std::sync::mpsc;
use crate::app::types::{Focus, BackgroundOpProgress, BackgroundOpOutcome};
use crate::git::Git;
use crate::models::Worktree;

use super::App;

impl App {
    pub fn select_next_session(&mut self) {
        if self.worktrees.is_empty() { return; }
        let next = match self.selected_worktree {
            Some(i) if i + 1 < self.worktrees.len() => i + 1,
            Some(_) => 0, // wrap to first
            None => 0,
        };
        self.save_current_terminal();
        self.selected_worktree = Some(next);
        self.load_session_output();
        self.invalidate_sidebar();
    }

    pub fn select_prev_session(&mut self) {
        if self.worktrees.is_empty() { return; }
        let prev = match self.selected_worktree {
            Some(0) => self.worktrees.len() - 1, // wrap to last
            Some(i) => i - 1,
            None => self.worktrees.len() - 1,
        };
        self.save_current_terminal();
        self.selected_worktree = Some(prev);
        self.load_session_output();
        self.invalidate_sidebar();
    }

    /// Create a new git worktree with a custom name
    pub fn create_new_worktree_with_name(&mut self, worktree_name: String, _prompt: String) -> anyhow::Result<Worktree> {
        let Some(project) = self.project.clone() else {
            anyhow::bail!("No project loaded")
        };

        let branch_name = format!("{}/{}", crate::models::BRANCH_PREFIX, worktree_name);
        let worktree_path = project.worktrees_dir().join(&worktree_name);

        if worktree_path.exists() {
            anyhow::bail!("Worktree already exists: {}", worktree_path.display());
        }

        let project_path = project.path.clone();
        let wt_path = worktree_path.clone();
        let branch_clone = branch_name.clone();
        let (tx, rx) = mpsc::channel();
        self.loading_indicator = Some("Creating worktree...".into());
        self.background_op_receiver = Some(rx);
        self.save_current_terminal();
        std::thread::spawn(move || {
            let outcome = match Git::create_worktree(&project_path, &wt_path, &branch_clone) {
                Ok(()) => BackgroundOpOutcome::Created { branch: branch_clone },
                Err(e) => BackgroundOpOutcome::Failed(format!("Create failed: {}", e)),
            };
            let _ = tx.send(BackgroundOpProgress { phase: String::new(), outcome: Some(outcome) });
        });

        // Return a placeholder — the real worktree is set up when the background op completes
        Ok(Worktree {
            branch_name,
            worktree_path: Some(worktree_path),
            claude_session_id: None,
            archived: false,
        })
    }

    pub fn archive_current_worktree(&mut self) -> anyhow::Result<()> {
        let session = match self.current_worktree() {
            Some(s) => s,
            None => return Ok(()),
        };
        if let Some(project) = &self.project {
            if session.branch_name == project.main_branch {
                self.set_status("Cannot archive main branch");
                return Ok(());
            }
        }
        let wt_path = match session.worktree_path.clone() {
            Some(p) => p,
            None => return Ok(()),
        };
        let project_path = match self.project.as_ref() {
            Some(p) => p.path.clone(),
            None => return Ok(()),
        };
        let (tx, rx) = mpsc::channel();
        self.loading_indicator = Some("Archiving worktree...".into());
        self.background_op_receiver = Some(rx);
        std::thread::spawn(move || {
            let outcome = match Git::remove_worktree(&project_path, &wt_path) {
                Ok(()) => BackgroundOpOutcome::Archived,
                Err(e) => BackgroundOpOutcome::Failed(format!("Archive failed: {}", e)),
            };
            let _ = tx.send(BackgroundOpProgress { phase: String::new(), outcome: Some(outcome) });
        });
        Ok(())
    }

    /// Restore an archived worktree by recreating its git worktree from the preserved branch
    pub fn unarchive_current_worktree(&mut self) -> anyhow::Result<()> {
        let session = self.current_worktree().ok_or_else(|| anyhow::anyhow!("No worktree selected"))?;
        if !session.archived {
            anyhow::bail!("Worktree is not archived");
        }
        let branch = session.branch_name.clone();
        let worktree_name = session.name().to_string();
        let project = self.project.clone().ok_or_else(|| anyhow::anyhow!("No project loaded"))?;
        let worktree_path = project.worktrees_dir().join(&worktree_name);
        let project_path = project.path.clone();
        let (tx, rx) = mpsc::channel();
        self.loading_indicator = Some("Unarchiving worktree...".into());
        self.background_op_receiver = Some(rx);
        let branch_clone = branch.clone();
        let name_clone = worktree_name.clone();
        std::thread::spawn(move || {
            let outcome = match Git::create_worktree_from_branch(&project_path, &worktree_path, &branch_clone) {
                Ok(()) => BackgroundOpOutcome::Unarchived { branch: branch_clone, display_name: name_clone },
                Err(e) => BackgroundOpOutcome::Failed(format!("Unarchive failed: {}", e)),
            };
            let _ = tx.send(BackgroundOpProgress { phase: String::new(), outcome: Some(outcome) });
        });
        Ok(())
    }

    /// Delete the current worktree AND its branch permanently
    pub fn delete_current_worktree(&mut self) -> anyhow::Result<()> {
        let wt = self.current_worktree().ok_or_else(|| anyhow::anyhow!("No worktree selected"))?;
        let project = self.project.clone().ok_or_else(|| anyhow::anyhow!("No project loaded"))?;
        if wt.branch_name == project.main_branch {
            anyhow::bail!("Cannot delete main branch");
        }
        let branch = wt.branch_name.clone();
        let name = wt.name().to_string();
        let wt_path = wt.worktree_path.clone();
        let project_path = project.path.clone();
        let prev_idx = self.selected_worktree.unwrap_or(0);

        // Clean up auto-rebase config (fast, can stay on main thread)
        self.auto_rebase_enabled.remove(&branch);
        crate::azufig::set_auto_rebase(&project_path, &branch, false);
        // Clean up stale session state immediately
        self.session_files.remove(&branch);
        self.session_selected_file_idx.remove(&branch);
        self.claude_session_ids.retain(|k, _| k != &branch);
        self.unread_sessions.remove(&branch);
        if let Some(slots) = self.branch_slots.remove(&branch) {
            for slot in &slots {
                self.running_sessions.remove(slot);
                self.claude_receivers.remove(slot);
                self.claude_exit_codes.remove(slot);
                self.claude_session_ids.remove(slot);
            }
        }
        self.active_slot.remove(&branch);

        let (tx, rx) = mpsc::channel();
        self.loading_indicator = Some("Deleting worktree...".into());
        self.background_op_receiver = Some(rx);
        let branch_clone = branch.clone();
        let name_clone = name.clone();
        std::thread::spawn(move || {
            // Remove worktree directory
            if let Some(ref path) = wt_path {
                let _ = Git::remove_worktree(&project_path, path);
            }
            // Delete git branch (local + remote + tracking ref)
            let _ = Git::delete_branch(&project_path, &branch_clone);
            let _ = tx.send(BackgroundOpProgress {
                phase: String::new(),
                outcome: Some(BackgroundOpOutcome::Deleted {
                    display_name: name_clone,
                    prev_idx,
                }),
            });
        });
        Ok(())
    }

    /// Select a specific session file by index
    pub fn select_session_file(&mut self, branch_name: &str, idx: usize) {
        if let Some(files) = self.session_files.get(branch_name) {
            if idx < files.len() {
                self.session_selected_file_idx.insert(branch_name.to_string(), idx);
                // Load the selected session file
                self.load_session_output();
                self.invalidate_sidebar();
            }
        }
    }

    /// Jump to first session
    pub fn select_first_session(&mut self) {
        if self.worktrees.is_empty() { return; }
        if self.selected_worktree != Some(0) {
            self.save_current_terminal();
            self.selected_worktree = Some(0);
            self.load_session_output();
            self.invalidate_sidebar();
        }
    }

    /// Start a fresh Claude session on the current worktree (clears state, enters prompt mode)
    pub fn start_new_session(&mut self) {
        if let Some(wt) = self.current_worktree().cloned() {
            if wt.archived {
                let key = if cfg!(target_os = "macos") { "⌘a" } else { "Ctrl+Shift+A" };
                self.set_status(&format!("Worktree is archived — unarchive first ({key})"));
            } else if wt.worktree_path.is_some() {
                let branch = wt.branch_name.clone();
                // Clear session ID from both the branch-key fallback AND the
                // active slot's key so get_claude_session_id() returns None
                // and the next prompt starts a fresh conversation.
                if let Some(slot) = self.active_slot.get(&branch) {
                    let slot = slot.clone();
                    self.claude_session_ids.remove(&slot);
                }
                self.claude_session_ids.remove(&branch);
                self.display_events.clear();
                self.session_lines.clear();
                self.session_buffer.clear();
                self.session_scroll = usize::MAX;
                self.session_file_parse_offset = 0;
                self.rendered_events_count = 0;
                self.rendered_content_line_count = 0;
                self.rendered_events_start = 0;
                self.event_parser = crate::events::EventParser::new();
                self.selected_event = None;
                self.current_todos.clear();
                self.subagent_todos.clear();
                self.session_tokens = None;
                self.token_badge_cache = None;
                self.invalidate_render_cache();
                self.focus = Focus::Input;
                self.prompt_mode = true;
                self.set_status("Add session — type your prompt and press Enter");
            }
        }
    }

    /// Jump to last session
    pub fn select_last_session(&mut self) {
        if self.worktrees.is_empty() { return; }
        let last = self.worktrees.len() - 1;
        if self.selected_worktree != Some(last) {
            self.save_current_terminal();
            self.selected_worktree = Some(last);
            self.load_session_output();
            self.invalidate_sidebar();
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Create a Worktree with a given branch name
    fn wt(name: &str) -> Worktree {
        Worktree {
            branch_name: format!("azureal/{}", name),
            worktree_path: Some(PathBuf::from(format!("/tmp/wt/{}", name))),
            claude_session_id: None,
            archived: false,
        }
    }

    /// Create an App with N worktrees
    fn app_with_worktrees(count: usize) -> App {
        let mut app = App::new();
        for i in 0..count {
            app.worktrees.push(wt(&format!("wt-{}", i)));
        }
        if count > 0 {
            app.selected_worktree = Some(0);
        }
        app
    }

    // ── select_next_session ──

    #[test]
    fn test_next_session_from_first() {
        let mut app = app_with_worktrees(3);
        app.selected_worktree = Some(0);
        app.select_next_session();
        assert_eq!(app.selected_worktree, Some(1));
    }

    #[test]
    fn test_next_session_from_middle() {
        let mut app = app_with_worktrees(5);
        app.selected_worktree = Some(2);
        app.select_next_session();
        assert_eq!(app.selected_worktree, Some(3));
    }

    #[test]
    fn test_next_session_wraps_from_last() {
        let mut app = app_with_worktrees(3);
        app.selected_worktree = Some(2); // last
        app.select_next_session();
        assert_eq!(app.selected_worktree, Some(0)); // wraps to first
    }

    #[test]
    fn test_next_session_empty_worktrees() {
        let mut app = App::new();
        app.select_next_session();
        assert_eq!(app.selected_worktree, None);
    }

    #[test]
    fn test_next_session_from_none() {
        let mut app = app_with_worktrees(3);
        app.selected_worktree = None;
        app.select_next_session();
        assert_eq!(app.selected_worktree, Some(0));
    }

    #[test]
    fn test_next_session_single_worktree() {
        let mut app = app_with_worktrees(1);
        app.selected_worktree = Some(0);
        app.select_next_session();
        assert_eq!(app.selected_worktree, Some(0)); // wraps to self
    }

    // ── select_prev_session ──

    #[test]
    fn test_prev_session_from_last() {
        let mut app = app_with_worktrees(3);
        app.selected_worktree = Some(2);
        app.select_prev_session();
        assert_eq!(app.selected_worktree, Some(1));
    }

    #[test]
    fn test_prev_session_from_middle() {
        let mut app = app_with_worktrees(5);
        app.selected_worktree = Some(3);
        app.select_prev_session();
        assert_eq!(app.selected_worktree, Some(2));
    }

    #[test]
    fn test_prev_session_wraps_from_first() {
        let mut app = app_with_worktrees(3);
        app.selected_worktree = Some(0);
        app.select_prev_session();
        assert_eq!(app.selected_worktree, Some(2)); // wraps to last
    }

    #[test]
    fn test_prev_session_empty_worktrees() {
        let mut app = App::new();
        app.select_prev_session();
        assert_eq!(app.selected_worktree, None);
    }

    #[test]
    fn test_prev_session_from_none() {
        let mut app = app_with_worktrees(3);
        app.selected_worktree = None;
        app.select_prev_session();
        assert_eq!(app.selected_worktree, Some(2)); // wraps to last
    }

    #[test]
    fn test_prev_session_single_worktree() {
        let mut app = app_with_worktrees(1);
        app.selected_worktree = Some(0);
        app.select_prev_session();
        assert_eq!(app.selected_worktree, Some(0)); // wraps to self
    }

    // ── select_first_session ──

    #[test]
    fn test_first_session_from_end() {
        let mut app = app_with_worktrees(5);
        app.selected_worktree = Some(4);
        app.select_first_session();
        assert_eq!(app.selected_worktree, Some(0));
    }

    #[test]
    fn test_first_session_already_first() {
        let mut app = app_with_worktrees(3);
        app.selected_worktree = Some(0);
        app.select_first_session();
        assert_eq!(app.selected_worktree, Some(0));
    }

    #[test]
    fn test_first_session_empty_worktrees() {
        let mut app = App::new();
        app.select_first_session();
        assert_eq!(app.selected_worktree, None);
    }

    #[test]
    fn test_first_session_from_middle() {
        let mut app = app_with_worktrees(5);
        app.selected_worktree = Some(3);
        app.select_first_session();
        assert_eq!(app.selected_worktree, Some(0));
    }

    // ── select_last_session ──

    #[test]
    fn test_last_session_from_start() {
        let mut app = app_with_worktrees(5);
        app.selected_worktree = Some(0);
        app.select_last_session();
        assert_eq!(app.selected_worktree, Some(4));
    }

    #[test]
    fn test_last_session_already_last() {
        let mut app = app_with_worktrees(3);
        app.selected_worktree = Some(2);
        app.select_last_session();
        assert_eq!(app.selected_worktree, Some(2));
    }

    #[test]
    fn test_last_session_empty_worktrees() {
        let mut app = App::new();
        app.select_last_session();
        assert_eq!(app.selected_worktree, None);
    }

    #[test]
    fn test_last_session_from_middle() {
        let mut app = app_with_worktrees(5);
        app.selected_worktree = Some(2);
        app.select_last_session();
        assert_eq!(app.selected_worktree, Some(4));
    }

    // ── select_session_file ──

    #[test]
    fn test_select_session_file_valid_idx() {
        let mut app = app_with_worktrees(1);
        let branch = "azureal/wt-0".to_string();
        app.session_files.insert(branch.clone(), vec![
            ("sess-0".to_string(), PathBuf::from("/sess0.json"), "10:00".to_string()),
            ("sess-1".to_string(), PathBuf::from("/sess1.json"), "11:00".to_string()),
        ]);
        app.select_session_file(&branch, 1);
        assert_eq!(app.session_selected_file_idx.get(&branch), Some(&1));
    }

    #[test]
    fn test_select_session_file_out_of_bounds() {
        let mut app = app_with_worktrees(1);
        let branch = "azureal/wt-0".to_string();
        app.session_files.insert(branch.clone(), vec![
            ("sess-0".to_string(), PathBuf::from("/sess0.json"), "10:00".to_string()),
        ]);
        app.select_session_file(&branch, 5); // out of bounds
        assert!(app.session_selected_file_idx.get(&branch).is_none());
    }

    #[test]
    fn test_select_session_file_unknown_branch() {
        let mut app = app_with_worktrees(1);
        app.select_session_file("unknown/branch", 0);
        assert!(app.session_selected_file_idx.get("unknown/branch").is_none());
    }

    #[test]
    fn test_select_session_file_first_idx() {
        let mut app = app_with_worktrees(1);
        let branch = "azureal/wt-0".to_string();
        app.session_files.insert(branch.clone(), vec![
            ("a".to_string(), PathBuf::from("/a"), "09:00".to_string()),
            ("b".to_string(), PathBuf::from("/b"), "10:00".to_string()),
        ]);
        app.select_session_file(&branch, 0);
        assert_eq!(app.session_selected_file_idx.get(&branch), Some(&0));
    }

    // ── Wrap-around consistency ──

    #[test]
    fn test_next_then_prev_returns_to_same() {
        let mut app = app_with_worktrees(5);
        app.selected_worktree = Some(2);
        app.select_next_session();
        app.select_prev_session();
        assert_eq!(app.selected_worktree, Some(2));
    }

    #[test]
    fn test_prev_then_next_returns_to_same() {
        let mut app = app_with_worktrees(5);
        app.selected_worktree = Some(2);
        app.select_prev_session();
        app.select_next_session();
        assert_eq!(app.selected_worktree, Some(2));
    }

    #[test]
    fn test_next_wraps_full_cycle() {
        let mut app = app_with_worktrees(3);
        app.selected_worktree = Some(0);
        app.select_next_session(); // 1
        app.select_next_session(); // 2
        app.select_next_session(); // 0 (wrap)
        assert_eq!(app.selected_worktree, Some(0));
    }

    #[test]
    fn test_prev_wraps_full_cycle() {
        let mut app = app_with_worktrees(3);
        app.selected_worktree = Some(0);
        app.select_prev_session(); // 2 (wrap)
        app.select_prev_session(); // 1
        app.select_prev_session(); // 0
        assert_eq!(app.selected_worktree, Some(0));
    }

    // ── Two-worktree cases ──

    #[test]
    fn test_next_two_worktrees_toggles() {
        let mut app = app_with_worktrees(2);
        app.selected_worktree = Some(0);
        app.select_next_session();
        assert_eq!(app.selected_worktree, Some(1));
        app.select_next_session();
        assert_eq!(app.selected_worktree, Some(0));
    }

    #[test]
    fn test_prev_two_worktrees_toggles() {
        let mut app = app_with_worktrees(2);
        app.selected_worktree = Some(0);
        app.select_prev_session();
        assert_eq!(app.selected_worktree, Some(1));
        app.select_prev_session();
        assert_eq!(app.selected_worktree, Some(0));
    }

    // ── archive_current_worktree: guard against main branch ──

    #[test]
    fn test_archive_main_branch_blocked() {
        let mut app = App::new();
        app.project = Some(crate::models::Project {
            name: "test".to_string(),
            path: PathBuf::from("/tmp/project"),
            main_branch: "main".to_string(),
        });
        app.worktrees.push(Worktree {
            branch_name: "main".to_string(),
            worktree_path: Some(PathBuf::from("/tmp/project")),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        let result = app.archive_current_worktree();
        assert!(result.is_ok()); // returns Ok but does nothing
        assert!(app.status_message.as_ref().unwrap().contains("Cannot archive main branch"));
    }

    // ── delete_current_worktree: guard against main branch ──

    #[test]
    fn test_delete_main_branch_blocked() {
        let mut app = App::new();
        app.project = Some(crate::models::Project {
            name: "test".to_string(),
            path: PathBuf::from("/tmp/project"),
            main_branch: "main".to_string(),
        });
        app.worktrees.push(Worktree {
            branch_name: "main".to_string(),
            worktree_path: Some(PathBuf::from("/tmp/project")),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        let result = app.delete_current_worktree();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Cannot delete main branch"));
    }

    // ── delete_current_worktree: no worktree selected ──

    #[test]
    fn test_delete_no_worktree_selected() {
        let mut app = App::new();
        let result = app.delete_current_worktree();
        assert!(result.is_err());
    }

    // ── archive_current_worktree: no worktree selected ──

    #[test]
    fn test_archive_no_worktree_selected() {
        let mut app = App::new();
        let result = app.archive_current_worktree();
        assert!(result.is_ok()); // returns Ok(()) when no worktree
    }

    // ── unarchive_current_worktree: not archived ──

    #[test]
    fn test_unarchive_not_archived_errors() {
        let mut app = App::new();
        app.worktrees.push(Worktree {
            branch_name: "azureal/active".to_string(),
            worktree_path: Some(PathBuf::from("/tmp/active")),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        let result = app.unarchive_current_worktree();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not archived"));
    }

    // ── unarchive_current_worktree: no selection ──

    #[test]
    fn test_unarchive_no_selection_errors() {
        let mut app = App::new();
        let result = app.unarchive_current_worktree();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No worktree selected"));
    }

    // ── create_new_worktree_with_name: no project ──

    #[test]
    fn test_create_worktree_no_project_errors() {
        let mut app = App::new();
        let result = app.create_new_worktree_with_name("test-wt".to_string(), "prompt".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No project loaded"));
    }

    // ── Large index consistency ──

    #[test]
    fn test_many_worktrees_next_prev() {
        let mut app = app_with_worktrees(100);
        app.selected_worktree = Some(50);
        for _ in 0..10 {
            app.select_next_session();
        }
        assert_eq!(app.selected_worktree, Some(60));
        for _ in 0..20 {
            app.select_prev_session();
        }
        assert_eq!(app.selected_worktree, Some(40));
    }

    #[test]
    fn test_first_last_session_large_list() {
        let mut app = app_with_worktrees(50);
        app.selected_worktree = Some(25);
        app.select_first_session();
        assert_eq!(app.selected_worktree, Some(0));
        app.select_last_session();
        assert_eq!(app.selected_worktree, Some(49));
    }

    // ── Worktree navigation: state preservation ──

    #[test]
    fn test_next_session_preserves_worktrees_vec() {
        let mut app = app_with_worktrees(3);
        let names_before: Vec<_> = app.worktrees.iter().map(|w| w.branch_name.clone()).collect();
        app.select_next_session();
        let names_after: Vec<_> = app.worktrees.iter().map(|w| w.branch_name.clone()).collect();
        assert_eq!(names_before, names_after);
    }

    #[test]
    fn test_prev_session_preserves_worktrees_vec() {
        let mut app = app_with_worktrees(3);
        let count_before = app.worktrees.len();
        app.select_prev_session();
        assert_eq!(app.worktrees.len(), count_before);
    }

    #[test]
    fn test_first_session_preserves_worktrees_vec() {
        let mut app = app_with_worktrees(5);
        app.selected_worktree = Some(3);
        let count = app.worktrees.len();
        app.select_first_session();
        assert_eq!(app.worktrees.len(), count);
    }

    #[test]
    fn test_last_session_preserves_worktrees_vec() {
        let mut app = app_with_worktrees(5);
        app.selected_worktree = Some(1);
        let count = app.worktrees.len();
        app.select_last_session();
        assert_eq!(app.worktrees.len(), count);
    }

    // ── Rapid navigation patterns ──

    #[test]
    fn test_next_five_times_from_zero() {
        let mut app = app_with_worktrees(10);
        app.selected_worktree = Some(0);
        for _ in 0..5 {
            app.select_next_session();
        }
        assert_eq!(app.selected_worktree, Some(5));
    }

    #[test]
    fn test_prev_five_times_from_nine() {
        let mut app = app_with_worktrees(10);
        app.selected_worktree = Some(9);
        for _ in 0..5 {
            app.select_prev_session();
        }
        assert_eq!(app.selected_worktree, Some(4));
    }

    #[test]
    fn test_next_across_wrap_boundary() {
        let mut app = app_with_worktrees(4);
        app.selected_worktree = Some(3); // last
        app.select_next_session();
        assert_eq!(app.selected_worktree, Some(0)); // wraps
        app.select_next_session();
        assert_eq!(app.selected_worktree, Some(1)); // continues
    }

    #[test]
    fn test_prev_across_wrap_boundary() {
        let mut app = app_with_worktrees(4);
        app.selected_worktree = Some(0); // first
        app.select_prev_session();
        assert_eq!(app.selected_worktree, Some(3)); // wraps to last
        app.select_prev_session();
        assert_eq!(app.selected_worktree, Some(2)); // continues
    }

    // ── select_session_file edge cases ──

    #[test]
    fn test_select_session_file_empty_list() {
        let mut app = app_with_worktrees(1);
        let branch = "azureal/wt-0".to_string();
        app.session_files.insert(branch.clone(), vec![]);
        app.select_session_file(&branch, 0); // out of bounds for empty list
        assert!(app.session_selected_file_idx.get(&branch).is_none());
    }

    #[test]
    fn test_select_session_file_last_valid_idx() {
        let mut app = app_with_worktrees(1);
        let branch = "azureal/wt-0".to_string();
        app.session_files.insert(branch.clone(), vec![
            ("a".to_string(), PathBuf::from("/a"), "1".to_string()),
            ("b".to_string(), PathBuf::from("/b"), "2".to_string()),
            ("c".to_string(), PathBuf::from("/c"), "3".to_string()),
        ]);
        app.select_session_file(&branch, 2); // last valid
        assert_eq!(app.session_selected_file_idx.get(&branch), Some(&2));
    }

    #[test]
    fn test_select_session_file_overwrite_previous_selection() {
        let mut app = app_with_worktrees(1);
        let branch = "azureal/wt-0".to_string();
        app.session_files.insert(branch.clone(), vec![
            ("a".to_string(), PathBuf::from("/a"), "1".to_string()),
            ("b".to_string(), PathBuf::from("/b"), "2".to_string()),
        ]);
        app.select_session_file(&branch, 0);
        assert_eq!(app.session_selected_file_idx.get(&branch), Some(&0));
        app.select_session_file(&branch, 1);
        assert_eq!(app.session_selected_file_idx.get(&branch), Some(&1));
    }

    // ── delete_current_worktree: error message contents ──

    #[test]
    fn test_delete_no_project_errors() {
        let mut app = App::new();
        app.worktrees.push(Worktree {
            branch_name: "azureal/test".to_string(),
            worktree_path: Some(PathBuf::from("/tmp/wt")),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        let result = app.delete_current_worktree();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No project loaded"));
    }

    // ── first/last idempotency ──

    #[test]
    fn test_first_session_idempotent() {
        let mut app = app_with_worktrees(5);
        app.selected_worktree = Some(3);
        app.select_first_session();
        assert_eq!(app.selected_worktree, Some(0));
        app.select_first_session();
        assert_eq!(app.selected_worktree, Some(0)); // no change
    }

    #[test]
    fn test_last_session_idempotent() {
        let mut app = app_with_worktrees(5);
        app.selected_worktree = Some(1);
        app.select_last_session();
        assert_eq!(app.selected_worktree, Some(4));
        app.select_last_session();
        assert_eq!(app.selected_worktree, Some(4)); // no change
    }

    // ── start_new_session ──

    #[test]
    fn test_start_new_session_enters_prompt_mode() {
        let mut app = app_with_worktrees(1);
        app.start_new_session();
        assert!(app.prompt_mode);
        assert_eq!(app.focus, Focus::Input);
        assert_eq!(app.session_scroll, usize::MAX);
    }

    #[test]
    fn test_start_new_session_archived_blocked() {
        let mut app = App::new();
        app.worktrees.push(Worktree {
            branch_name: "azureal/archived".to_string(),
            worktree_path: None,
            claude_session_id: None,
            archived: true,
        });
        app.selected_worktree = Some(0);
        app.start_new_session();
        assert!(app.status_message.as_ref().unwrap().contains("archived"));
        assert!(!app.prompt_mode);
    }

    #[test]
    fn test_start_new_session_no_worktree() {
        let mut app = App::new();
        app.start_new_session();
        assert!(!app.prompt_mode);
    }
}
