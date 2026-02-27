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
