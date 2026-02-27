//! Deferred action execution
//!
//! Runs actions after their loading indicator has rendered on-screen.
//! Each DeferredAction variant delegates to the same method that would
//! have been called synchronously before the deferred pattern.

use crate::app::App;

/// Execute a deferred action after its loading indicator has rendered on-screen.
/// Called from the event loop's post-draw section. Each variant delegates to the
/// same method that would have been called synchronously before the deferred pattern.
pub fn execute_deferred_action(app: &mut App, action: crate::app::DeferredAction) {
    use crate::app::DeferredAction;
    match action {
        DeferredAction::LoadSession { branch, idx } => {
            app.save_current_terminal();
            app.select_session_file(&branch, idx);
            app.show_session_list = false;
            app.session_filter.clear();
            app.session_filter_active = false;
            app.session_content_search = false;
            app.session_search_results.clear();
            app.invalidate_sidebar();
        }
        DeferredAction::LoadFile { path } => {
            app.load_file_by_path(&path);
        }
        DeferredAction::OpenHealthPanel => {
            app.open_health_panel();
        }
        DeferredAction::SwitchProject { path } => {
            app.switch_project(path);
        }
        DeferredAction::RescanHealthScope { dirs } => {
            app.rescan_health_with_dirs(&dirs);
        }
        DeferredAction::GitCommit { worktree, message } => {
            if let Some(ref mut p) = app.git_actions_panel {
                match crate::git::Git::commit(&worktree, &message) {
                    Ok(out) => {
                        let first = out.lines().next().unwrap_or(&out);
                        p.result_message = Some((format!("Committed: {}", first), false));
                        crate::tui::input_git_actions::refresh_changed_files(p);
                        crate::tui::input_git_actions::refresh_commit_log(p);
                    }
                    Err(e) => { p.result_message = Some((format!("{}", e), true)); }
                }
            }
        }
        DeferredAction::GitCommitAndPush { worktree, message } => {
            if let Some(ref mut p) = app.git_actions_panel {
                match crate::git::Git::commit(&worktree, &message) {
                    Ok(_) => {
                        match crate::git::Git::push(&worktree) {
                            Ok(_) => {
                                p.result_message = Some(("Committed and pushed".into(), false));
                            }
                            Err(e) => {
                                p.result_message = Some((format!("Committed but push failed: {}", e), true));
                            }
                        }
                        crate::tui::input_git_actions::refresh_changed_files(p);
                        crate::tui::input_git_actions::refresh_commit_log(p);
                    }
                    Err(e) => { p.result_message = Some((format!("{}", e), true)); }
                }
            }
        }
    }
}
