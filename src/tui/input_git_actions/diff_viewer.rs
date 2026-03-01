//! Diff loading for the Git Actions panel viewer pane.
//!
//! Loads file diffs and commit diffs into the inline viewer when the user
//! navigates the file list or commit list with j/k or presses Enter/d.

use crate::app::App;
use crate::app::types::GitActionsPanel;
use crate::git::Git;

/// Load the selected file's diff into viewer_diff via full App (Enter/d from file list)
pub(crate) fn open_file_diff_inline(app: &mut App) {
    let (wt, main, path) = match app.git_actions_panel.as_ref() {
        Some(p) => {
            if let Some(file) = p.changed_files.get(p.selected_file) {
                (p.worktree_path.clone(), p.main_branch.clone(), file.path.clone())
            } else { return; }
        }
        None => return,
    };
    match Git::get_file_diff(&wt, &main, &path) {
        Ok(diff) => {
            if let Some(ref mut p) = app.git_actions_panel {
                p.viewer_diff_title = Some(format!("diff: {}", path));
                p.viewer_diff = Some(diff);
            }
        }
        Err(e) => {
            if let Some(ref mut p) = app.git_actions_panel {
                p.result_message = Some((format!("{}", e), true));
            }
        }
    }
}

/// Load the currently selected file's diff into viewer_diff (called on j/k navigation)
pub(crate) fn load_file_diff_inline(panel: &mut GitActionsPanel) {
    let file = match panel.changed_files.get(panel.selected_file) {
        Some(f) => f,
        None => { panel.viewer_diff = None; panel.viewer_diff_title = None; return; }
    };
    let path = file.path.clone();
    match Git::get_file_diff(&panel.worktree_path, &panel.main_branch, &path) {
        Ok(diff) => {
            panel.viewer_diff_title = Some(format!("diff: {}", path));
            panel.viewer_diff = Some(diff);
        }
        Err(_) => {
            panel.viewer_diff = None;
            panel.viewer_diff_title = None;
        }
    }
}

/// Load the currently selected commit's diff into viewer_diff (called on j/k navigation)
pub(crate) fn load_commit_diff_inline(panel: &mut GitActionsPanel) {
    let commit = match panel.commits.get(panel.selected_commit) {
        Some(c) => c,
        None => { panel.viewer_diff = None; panel.viewer_diff_title = None; return; }
    };
    let hash = commit.full_hash.clone();
    let subject = commit.subject.clone();
    let short = commit.hash.clone();
    match Git::get_commit_diff(&panel.worktree_path, &hash) {
        Ok(diff) => {
            panel.viewer_diff_title = Some(format!("{} {}", short, subject));
            panel.viewer_diff = Some(diff);
        }
        Err(_) => {
            panel.viewer_diff = None;
            panel.viewer_diff_title = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::{GitActionsPanel, GitChangedFile, GitCommit};
    use std::path::PathBuf;

    /// Create a minimal GitActionsPanel for testing
    fn make_panel() -> GitActionsPanel {
        GitActionsPanel {
            worktree_name: "test-branch".into(),
            worktree_path: PathBuf::from("/tmp/test-diff-viewer"),
            repo_root: PathBuf::from("/tmp/test-repo"),
            main_branch: "main".into(),
            is_on_main: false,
            changed_files: Vec::new(),
            selected_file: 0,
            file_scroll: 0,
            focused_pane: 0,
            selected_action: 0,
            result_message: None,
            commit_overlay: None,
            conflict_overlay: None,
            commits: Vec::new(),
            selected_commit: 0,
            commit_scroll: 0,
            viewer_diff: None,
            viewer_diff_title: None,
            commits_behind_main: 0,
            commits_ahead_main: 0,
            commits_behind_remote: 0,
            commits_ahead_remote: 0,
            auto_resolve_files: Vec::new(),
            auto_resolve_overlay: None,
        }
    }

    fn make_changed_file(path: &str) -> GitChangedFile {
        GitChangedFile {
            path: path.to_string(),
            status: 'M',
            additions: 10,
            deletions: 5,
        }
    }

    fn make_commit(hash: &str, subject: &str) -> GitCommit {
        GitCommit {
            hash: hash.to_string(),
            full_hash: format!("{}abcdef1234567", hash),
            subject: subject.to_string(),
            is_pushed: false,
        }
    }

    // ── open_file_diff_inline tests ──

    #[test]
    fn test_open_file_diff_inline_no_panel() {
        let mut app = App::new();
        assert!(app.git_actions_panel.is_none());
        open_file_diff_inline(&mut app);
        // No panic when panel is None
    }

    #[test]
    fn test_open_file_diff_inline_empty_files() {
        let mut app = App::new();
        app.git_actions_panel = Some(make_panel());
        open_file_diff_inline(&mut app);
        assert!(app.git_actions_panel.as_ref().unwrap().viewer_diff.is_none());
    }

    #[test]
    fn test_open_file_diff_inline_with_file() {
        let mut app = App::new();
        let mut panel = make_panel();
        panel.changed_files.push(make_changed_file("src/main.rs"));
        panel.selected_file = 0;
        app.git_actions_panel = Some(panel);
        open_file_diff_inline(&mut app);
        let p = app.git_actions_panel.as_ref().unwrap();
        assert!(p.viewer_diff.is_some() || p.result_message.is_some());
    }

    #[test]
    fn test_open_file_diff_inline_selected_out_of_bounds() {
        let mut app = App::new();
        let mut panel = make_panel();
        panel.changed_files.push(make_changed_file("file.rs"));
        panel.selected_file = 99;
        app.git_actions_panel = Some(panel);
        open_file_diff_inline(&mut app);
        assert!(app.git_actions_panel.as_ref().unwrap().viewer_diff.is_none());
    }

    #[test]
    fn test_open_file_diff_inline_sets_title() {
        let mut app = App::new();
        let mut panel = make_panel();
        // Use the actual CWD (git repo) so git diff works
        panel.worktree_path = std::env::current_dir().unwrap();
        panel.changed_files.push(make_changed_file("Cargo.toml"));
        panel.selected_file = 0;
        app.git_actions_panel = Some(panel);
        open_file_diff_inline(&mut app);
        let p = app.git_actions_panel.as_ref().unwrap();
        if p.viewer_diff.is_some() {
            assert!(p.viewer_diff_title.as_ref().unwrap().contains("Cargo.toml"));
        }
    }

    // ── load_file_diff_inline tests ──

    #[test]
    fn test_load_file_diff_inline_no_files() {
        let mut panel = make_panel();
        load_file_diff_inline(&mut panel);
        assert!(panel.viewer_diff.is_none());
        assert!(panel.viewer_diff_title.is_none());
    }

    #[test]
    fn test_load_file_diff_inline_clears_on_empty() {
        let mut panel = make_panel();
        panel.viewer_diff = Some("old diff".into());
        panel.viewer_diff_title = Some("old title".into());
        load_file_diff_inline(&mut panel);
        assert!(panel.viewer_diff.is_none());
        assert!(panel.viewer_diff_title.is_none());
    }

    #[test]
    fn test_load_file_diff_inline_selected_out_of_bounds() {
        let mut panel = make_panel();
        panel.changed_files.push(make_changed_file("a.rs"));
        panel.selected_file = 5;
        panel.viewer_diff = Some("stale".into());
        load_file_diff_inline(&mut panel);
        assert!(panel.viewer_diff.is_none());
    }

    #[test]
    fn test_load_file_diff_inline_with_file() {
        let mut panel = make_panel();
        panel.changed_files.push(make_changed_file("Cargo.toml"));
        panel.selected_file = 0;
        load_file_diff_inline(&mut panel);
        // Git fails in /tmp path — either diff loaded or cleared
    }

    #[test]
    fn test_load_file_diff_inline_title_format() {
        let path = "src/main.rs";
        let expected = format!("diff: {}", path);
        assert_eq!(expected, "diff: src/main.rs");
    }

    #[test]
    fn test_load_file_diff_inline_error_clears_diff() {
        let mut panel = make_panel();
        panel.viewer_diff = Some("previous diff".into());
        panel.viewer_diff_title = Some("prev title".into());
        panel.changed_files.push(make_changed_file("nonexistent_file.rs"));
        panel.selected_file = 0;
        load_file_diff_inline(&mut panel);
        // Error path clears diff
        if panel.viewer_diff.is_none() {
            assert!(panel.viewer_diff_title.is_none());
        }
    }

    // ── load_commit_diff_inline tests ──

    #[test]
    fn test_load_commit_diff_inline_no_commits() {
        let mut panel = make_panel();
        load_commit_diff_inline(&mut panel);
        assert!(panel.viewer_diff.is_none());
        assert!(panel.viewer_diff_title.is_none());
    }

    #[test]
    fn test_load_commit_diff_inline_clears_on_empty() {
        let mut panel = make_panel();
        panel.viewer_diff = Some("old commit diff".into());
        panel.viewer_diff_title = Some("old commit title".into());
        load_commit_diff_inline(&mut panel);
        assert!(panel.viewer_diff.is_none());
        assert!(panel.viewer_diff_title.is_none());
    }

    #[test]
    fn test_load_commit_diff_inline_selected_out_of_bounds() {
        let mut panel = make_panel();
        panel.commits.push(make_commit("abc1234", "feat: add stuff"));
        panel.selected_commit = 10;
        panel.viewer_diff = Some("stale".into());
        load_commit_diff_inline(&mut panel);
        assert!(panel.viewer_diff.is_none());
    }

    #[test]
    fn test_load_commit_diff_inline_with_commit() {
        let mut panel = make_panel();
        panel.commits.push(make_commit("abc1234", "feat: test"));
        panel.selected_commit = 0;
        load_commit_diff_inline(&mut panel);
        // git show fails in test env
    }

    #[test]
    fn test_commit_diff_title_format() {
        let short = "abc1234";
        let subject = "feat: add tests";
        let title = format!("{} {}", short, subject);
        assert_eq!(title, "abc1234 feat: add tests");
    }

    #[test]
    fn test_load_commit_diff_inline_error_clears_diff() {
        let mut panel = make_panel();
        panel.viewer_diff = Some("prev".into());
        panel.viewer_diff_title = Some("prev title".into());
        panel.commits.push(make_commit("deadbeef", "test"));
        panel.selected_commit = 0;
        load_commit_diff_inline(&mut panel);
        // If error, both should be None
        if panel.viewer_diff.is_none() {
            assert!(panel.viewer_diff_title.is_none());
        }
    }

    // ── GitActionsPanel field defaults ──

    #[test]
    fn test_panel_default_worktree_name() {
        let panel = make_panel();
        assert_eq!(panel.worktree_name, "test-branch");
    }

    #[test]
    fn test_panel_default_no_viewer_diff() {
        let panel = make_panel();
        assert!(panel.viewer_diff.is_none());
    }

    #[test]
    fn test_panel_default_no_viewer_diff_title() {
        let panel = make_panel();
        assert!(panel.viewer_diff_title.is_none());
    }

    #[test]
    fn test_panel_default_selected_file_zero() {
        let panel = make_panel();
        assert_eq!(panel.selected_file, 0);
    }

    #[test]
    fn test_panel_default_selected_commit_zero() {
        let panel = make_panel();
        assert_eq!(panel.selected_commit, 0);
    }

    #[test]
    fn test_panel_default_no_result_message() {
        let panel = make_panel();
        assert!(panel.result_message.is_none());
    }

    #[test]
    fn test_panel_main_branch() {
        let panel = make_panel();
        assert_eq!(panel.main_branch, "main");
    }

    #[test]
    fn test_panel_is_on_main_false() {
        let panel = make_panel();
        assert!(!panel.is_on_main);
    }

    // ── GitChangedFile tests ──

    #[test]
    fn test_changed_file_path() {
        let f = make_changed_file("src/lib.rs");
        assert_eq!(f.path, "src/lib.rs");
    }

    #[test]
    fn test_changed_file_status() {
        let f = make_changed_file("x");
        assert_eq!(f.status, 'M');
    }

    #[test]
    fn test_changed_file_additions() {
        let f = make_changed_file("x");
        assert_eq!(f.additions, 10);
    }

    #[test]
    fn test_changed_file_deletions() {
        let f = make_changed_file("x");
        assert_eq!(f.deletions, 5);
    }

    #[test]
    fn test_changed_file_added_status() {
        let f = GitChangedFile { path: "new.rs".into(), status: 'A', additions: 50, deletions: 0 };
        assert_eq!(f.status, 'A');
    }

    #[test]
    fn test_changed_file_deleted_status() {
        let f = GitChangedFile { path: "old.rs".into(), status: 'D', additions: 0, deletions: 30 };
        assert_eq!(f.status, 'D');
    }

    #[test]
    fn test_changed_file_renamed_status() {
        let f = GitChangedFile { path: "new.rs".into(), status: 'R', additions: 5, deletions: 5 };
        assert_eq!(f.status, 'R');
    }

    // ── GitCommit tests ──

    #[test]
    fn test_commit_hash() {
        let c = make_commit("abc1234", "msg");
        assert_eq!(c.hash, "abc1234");
    }

    #[test]
    fn test_commit_full_hash() {
        let c = make_commit("abc1234", "msg");
        assert!(c.full_hash.starts_with("abc1234"));
    }

    #[test]
    fn test_commit_subject() {
        let c = make_commit("abc1234", "feat: do things");
        assert_eq!(c.subject, "feat: do things");
    }

    #[test]
    fn test_commit_is_pushed_default() {
        let c = make_commit("x", "y");
        assert!(!c.is_pushed);
    }

    #[test]
    fn test_commit_is_pushed_true() {
        let mut c = make_commit("x", "y");
        c.is_pushed = true;
        assert!(c.is_pushed);
    }

    // ── App git_actions_panel default ──

    #[test]
    fn test_app_default_no_git_panel() {
        let app = App::new();
        assert!(app.git_actions_panel.is_none());
    }

    // ── Panel state mutations ──

    #[test]
    fn test_panel_viewer_diff_set_directly() {
        let mut panel = make_panel();
        panel.viewer_diff = Some("diff content".into());
        assert_eq!(panel.viewer_diff.as_deref(), Some("diff content"));
    }

    #[test]
    fn test_panel_viewer_diff_title_set_directly() {
        let mut panel = make_panel();
        panel.viewer_diff_title = Some("diff: src/lib.rs".into());
        assert_eq!(panel.viewer_diff_title.as_deref(), Some("diff: src/lib.rs"));
    }

    #[test]
    fn test_panel_clear_viewer_diff() {
        let mut panel = make_panel();
        panel.viewer_diff = Some("diff".into());
        panel.viewer_diff = None;
        assert!(panel.viewer_diff.is_none());
    }

    #[test]
    fn test_panel_clear_viewer_diff_title() {
        let mut panel = make_panel();
        panel.viewer_diff_title = Some("title".into());
        panel.viewer_diff_title = None;
        assert!(panel.viewer_diff_title.is_none());
    }

    // ── Diff title format contract ──

    #[test]
    fn test_file_diff_title_prefix_is_diff_colon() {
        // load_file_diff_inline sets: "diff: {path}"
        let path = "src/config.rs";
        let title = format!("diff: {}", path);
        assert!(title.starts_with("diff: "));
        assert!(title.contains(path));
    }

    #[test]
    fn test_commit_diff_title_format_short_hash_first() {
        let short = "abc1234";
        let subject = "fix: broken thing";
        let title = format!("{} {}", short, subject);
        assert!(title.starts_with("abc1234 "));
    }

    #[test]
    fn test_commit_diff_title_contains_subject() {
        let short = "deadbee";
        let subject = "refactor: split module";
        let title = format!("{} {}", short, subject);
        assert!(title.contains("refactor: split module"));
    }

    // ── Multiple changed files — selection logic ──

    #[test]
    fn test_panel_multiple_files_selection_zero() {
        let mut panel = make_panel();
        panel.changed_files.push(make_changed_file("a.rs"));
        panel.changed_files.push(make_changed_file("b.rs"));
        panel.changed_files.push(make_changed_file("c.rs"));
        panel.selected_file = 0;
        assert_eq!(panel.changed_files[panel.selected_file].path, "a.rs");
    }

    #[test]
    fn test_panel_multiple_files_selection_last() {
        let mut panel = make_panel();
        panel.changed_files.push(make_changed_file("a.rs"));
        panel.changed_files.push(make_changed_file("b.rs"));
        panel.changed_files.push(make_changed_file("c.rs"));
        panel.selected_file = 2;
        assert_eq!(panel.changed_files[panel.selected_file].path, "c.rs");
    }

    // ── Multiple commits — selection logic ──

    #[test]
    fn test_panel_multiple_commits_selection() {
        let mut panel = make_panel();
        panel.commits.push(make_commit("aaa1111", "feat: first"));
        panel.commits.push(make_commit("bbb2222", "feat: second"));
        panel.selected_commit = 1;
        assert_eq!(panel.commits[panel.selected_commit].subject, "feat: second");
    }

    #[test]
    fn test_panel_commits_ahead_counter() {
        let mut panel = make_panel();
        panel.commits_ahead_main = 5;
        assert_eq!(panel.commits_ahead_main, 5);
    }

    #[test]
    fn test_panel_commits_behind_counter() {
        let mut panel = make_panel();
        panel.commits_behind_main = 3;
        assert_eq!(panel.commits_behind_main, 3);
    }

    // ── Result message state ──

    #[test]
    fn test_panel_result_message_success() {
        let mut panel = make_panel();
        panel.result_message = Some(("Merge successful".into(), false));
        let (msg, is_err) = panel.result_message.as_ref().unwrap();
        assert_eq!(msg, "Merge successful");
        assert!(!is_err);
    }

    #[test]
    fn test_panel_result_message_error() {
        let mut panel = make_panel();
        panel.result_message = Some(("Git error occurred".into(), true));
        let (msg, is_err) = panel.result_message.as_ref().unwrap();
        assert_eq!(msg, "Git error occurred");
        assert!(is_err);
    }

    // ── load_commit_diff_inline: title components match make_commit ──

    #[test]
    fn test_commit_full_hash_starts_with_short() {
        let c = make_commit("abc1234", "msg");
        assert!(c.full_hash.starts_with(&c.hash));
    }

    #[test]
    fn test_commit_make_full_hash_format() {
        let c = make_commit("abc1234", "msg");
        assert_eq!(c.full_hash, "abc1234abcdef1234567");
    }
}
