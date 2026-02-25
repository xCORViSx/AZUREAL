//! Input handler for the AZUREAL++ developer hub panel.
//! Full-screen modal overlay — consumes all input when active.
//! Sub-states (naming input, issue create, PR create) intercept keys before
//! the keybinding system resolves the rest.

use anyhow::Result;
use crossterm::event::{self, KeyCode, KeyModifiers};

use crate::app::App;
use crate::app::types::{AzurealTab, IssueCreateState, PrCreateState};
use super::keybindings::{lookup_azureal_action, Action};

/// Handle keyboard input when the AZUREAL++ panel is active.
pub fn handle_azureal_input(key: event::KeyEvent, app: &mut App) -> Result<()> {
    // Sub-state: debug dump naming input
    if app.azureal_panel.as_ref().is_some_and(|p| p.dump_naming.is_some()) {
        return handle_dump_naming(key, app);
    }
    // Sub-state: issue creation form
    if app.azureal_panel.as_ref().is_some_and(|p| p.issue_create.is_some()) {
        return handle_issue_create(key, app);
    }
    // Sub-state: PR creation form
    if app.azureal_panel.as_ref().is_some_and(|p| p.pr_create.is_some()) {
        return handle_pr_create(key, app);
    }
    // Sub-state: issue detail view (Esc goes back to list)
    if app.azureal_panel.as_ref().is_some_and(|p| p.issue_detail_view) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                if let Some(ref mut p) = app.azureal_panel { p.issue_detail_view = false; p.issue_detail_scroll = 0; }
            }
            KeyCode::Char('o') => { open_current_issue_browser(app); }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(ref mut p) = app.azureal_panel { p.issue_detail_scroll += 1; }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(ref mut p) = app.azureal_panel { p.issue_detail_scroll = p.issue_detail_scroll.saturating_sub(1); }
            }
            _ => {}
        }
        return Ok(());
    }
    // Sub-state: issue filter input
    if app.azureal_panel.as_ref().is_some_and(|p| p.issue_filter.is_some()) {
        return handle_issue_filter(key, app);
    }

    let tab = match app.azureal_panel {
        Some(ref p) => p.tab,
        None => return Ok(()),
    };

    let Some(action) = lookup_azureal_action(tab, key.modifiers, key.code) else {
        // Enter also triggers dump debug on Debug tab
        if tab == AzurealTab::Debug && key.code == KeyCode::Enter {
            if let Some(ref mut p) = app.azureal_panel {
                p.dump_naming = Some(String::new());
            }
            return Ok(());
        }
        return Ok(());
    };

    match action {
        // ── Shared ──
        Action::Escape => { app.close_azureal_panel(); }
        Action::AzurealSwitchTab => {
            if let Some(ref mut p) = app.azureal_panel {
                p.tab = match p.tab {
                    AzurealTab::Debug => AzurealTab::Issues,
                    AzurealTab::Issues => AzurealTab::PullRequests,
                    AzurealTab::PullRequests => AzurealTab::Debug,
                };
                // Trigger lazy loading when switching to Issues/PRs for the first time
                maybe_start_loading(app);
            }
        }
        Action::AzurealRefresh => { refresh_current_tab(app); }

        // ── Debug tab ──
        Action::AzurealDumpDebug => {
            if let Some(ref mut p) = app.azureal_panel {
                p.dump_naming = Some(String::new());
            }
        }
        Action::AzurealViewDump => { view_selected_dump(app); }
        Action::AzurealDeleteDump => { delete_selected_dump(app); }

        // ── Issues tab ──
        Action::Confirm => {
            match tab {
                AzurealTab::Issues => {
                    if let Some(ref mut p) = app.azureal_panel {
                        if !p.issues.is_empty() {
                            p.issue_detail_view = true;
                            p.issue_detail_scroll = 0;
                        }
                    }
                }
                _ => {}
            }
        }
        Action::AzurealCreateIssue => {
            if let Some(ref mut p) = app.azureal_panel {
                p.issue_create = Some(IssueCreateState {
                    title: String::new(),
                    body: String::new(),
                    cursor_in_title: true,
                    cursor: 0,
                });
            }
        }
        Action::AzurealToggleClosed => {
            if let Some(ref mut p) = app.azureal_panel {
                p.show_closed = !p.show_closed;
            }
            refresh_current_tab(app);
        }
        Action::AzurealOpenInBrowser => { open_in_browser(app); }
        Action::AzurealFilter => {
            if let Some(ref mut p) = app.azureal_panel {
                p.issue_filter = Some(String::new());
            }
        }

        // ── PRs tab ──
        Action::AzurealCreatePR => {
            if let Some(ref p) = app.azureal_panel {
                let branch = app.current_worktree()
                    .map(|w| w.branch_name.clone())
                    .unwrap_or_default();
                let head = if let Some(ref owner) = p.fork_owner {
                    format!("{}:{}", owner, branch)
                } else {
                    branch.clone()
                };
                if let Some(ref mut p) = app.azureal_panel {
                    p.pr_create = Some(PrCreateState {
                        title: String::new(),
                        body: String::new(),
                        cursor_in_title: true,
                        cursor: 0,
                        head_branch: head,
                    });
                }
            }
        }

        // ── Navigation ──
        Action::NavDown => { nav_down(app); }
        Action::NavUp => { nav_up(app); }

        _ => {}
    }

    Ok(())
}

// ── Sub-state handlers ──

fn handle_dump_naming(key: event::KeyEvent, app: &mut App) -> Result<()> {
    match key.code {
        KeyCode::Enter => {
            let name = app.azureal_panel.as_mut()
                .and_then(|p| p.dump_naming.take())
                .unwrap_or_default();
            app.dump_debug_output(&name);
            // Refresh the dump files list
            if let Some(ref mut p) = app.azureal_panel {
                let project_path = app.project.as_ref()
                    .map(|pr| pr.path.clone())
                    .unwrap_or_else(|| std::path::PathBuf::from("."));
                p.dump_files = crate::app::scan_debug_dumps_pub(&project_path);
                p.dump_saving = false;
            }
        }
        KeyCode::Esc => {
            if let Some(ref mut p) = app.azureal_panel { p.dump_naming = None; }
        }
        KeyCode::Backspace => {
            if let Some(ref mut p) = app.azureal_panel {
                if let Some(ref mut s) = p.dump_naming { s.pop(); }
            }
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(ref mut p) = app.azureal_panel {
                if let Some(ref mut s) = p.dump_naming { s.push(c); }
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_issue_create(key: event::KeyEvent, app: &mut App) -> Result<()> {
    if key.code == KeyCode::Esc {
        if let Some(ref mut p) = app.azureal_panel { p.issue_create = None; }
        return Ok(());
    }
    // ⌃Enter submits
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Enter {
        return submit_issue(app);
    }
    if key.code == KeyCode::Tab {
        if let Some(ref mut p) = app.azureal_panel {
            if let Some(ref mut c) = p.issue_create {
                c.cursor_in_title = !c.cursor_in_title;
                c.cursor = if c.cursor_in_title { c.title.len() } else { c.body.len() };
            }
        }
        return Ok(());
    }
    // Text editing in the active field
    if let Some(ref mut p) = app.azureal_panel {
        if let Some(ref mut c) = p.issue_create {
            let field = if c.cursor_in_title { &mut c.title } else { &mut c.body };
            match key.code {
                KeyCode::Char(ch) => { field.push(ch); c.cursor = field.len(); }
                KeyCode::Backspace => { field.pop(); c.cursor = field.len(); }
                KeyCode::Enter => {
                    if !c.cursor_in_title { field.push('\n'); c.cursor = field.len(); }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn handle_pr_create(key: event::KeyEvent, app: &mut App) -> Result<()> {
    if key.code == KeyCode::Esc {
        if let Some(ref mut p) = app.azureal_panel { p.pr_create = None; }
        return Ok(());
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Enter {
        return submit_pr(app);
    }
    if key.code == KeyCode::Tab {
        if let Some(ref mut p) = app.azureal_panel {
            if let Some(ref mut c) = p.pr_create {
                c.cursor_in_title = !c.cursor_in_title;
                c.cursor = if c.cursor_in_title { c.title.len() } else { c.body.len() };
            }
        }
        return Ok(());
    }
    if let Some(ref mut p) = app.azureal_panel {
        if let Some(ref mut c) = p.pr_create {
            let field = if c.cursor_in_title { &mut c.title } else { &mut c.body };
            match key.code {
                KeyCode::Char(ch) => { field.push(ch); c.cursor = field.len(); }
                KeyCode::Backspace => { field.pop(); c.cursor = field.len(); }
                KeyCode::Enter => {
                    if !c.cursor_in_title { field.push('\n'); c.cursor = field.len(); }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn handle_issue_filter(key: event::KeyEvent, app: &mut App) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            if let Some(ref mut p) = app.azureal_panel { p.issue_filter = None; }
        }
        KeyCode::Enter => {
            // Keep filter active, just exit filter input mode
            if let Some(ref mut p) = app.azureal_panel {
                if p.issue_filter.as_ref().is_some_and(|f| f.is_empty()) {
                    p.issue_filter = None;
                }
            }
        }
        KeyCode::Backspace => {
            if let Some(ref mut p) = app.azureal_panel {
                if let Some(ref mut f) = p.issue_filter { f.pop(); }
            }
        }
        KeyCode::Char(c) => {
            if let Some(ref mut p) = app.azureal_panel {
                if let Some(ref mut f) = p.issue_filter { f.push(c); }
            }
        }
        _ => {}
    }
    Ok(())
}

// ── Action implementations ──

fn nav_down(app: &mut App) {
    if let Some(ref mut p) = app.azureal_panel {
        match p.tab {
            AzurealTab::Debug => {
                if !p.dump_files.is_empty() && p.dump_selected + 1 < p.dump_files.len() {
                    p.dump_selected += 1;
                }
            }
            AzurealTab::Issues => {
                let filtered_len = filtered_issue_count(p);
                if filtered_len > 0 && p.issue_selected + 1 < filtered_len {
                    p.issue_selected += 1;
                }
            }
            AzurealTab::PullRequests => {
                if !p.prs.is_empty() && p.pr_selected + 1 < p.prs.len() {
                    p.pr_selected += 1;
                }
            }
        }
    }
}

fn nav_up(app: &mut App) {
    if let Some(ref mut p) = app.azureal_panel {
        match p.tab {
            AzurealTab::Debug => {
                p.dump_selected = p.dump_selected.saturating_sub(1);
            }
            AzurealTab::Issues => {
                p.issue_selected = p.issue_selected.saturating_sub(1);
            }
            AzurealTab::PullRequests => {
                p.pr_selected = p.pr_selected.saturating_sub(1);
            }
        }
    }
}

fn filtered_issue_count(panel: &crate::app::types::AzurealPlusPlusPanel) -> usize {
    if let Some(ref filter) = panel.issue_filter {
        if !filter.is_empty() {
            let f = filter.to_lowercase();
            return panel.issues.iter().filter(|i| i.title.to_lowercase().contains(&f)).count();
        }
    }
    panel.issues.len()
}

fn view_selected_dump(app: &mut App) {
    let (path, filename) = match app.azureal_panel.as_ref() {
        Some(p) if !p.dump_files.is_empty() => {
            let name = &p.dump_files[p.dump_selected].0;
            let project = app.project.as_ref().map(|pr| pr.path.clone()).unwrap_or_else(|| std::path::PathBuf::from("."));
            (project.join(".azureal").join(name), name.clone())
        }
        _ => return,
    };
    // Load into viewer and close panel
    if let Ok(content) = std::fs::read_to_string(&path) {
        app.close_azureal_panel();
        app.viewer_content = Some(content);
        app.viewer_path = Some(path);
        app.viewer_mode = crate::app::types::ViewerMode::File;
        app.viewer_scroll = 0;
        app.viewer_lines_dirty = true;
        app.focus = crate::app::types::Focus::Viewer;
        app.set_status(format!("Viewing {}", filename));
    }
}

fn delete_selected_dump(app: &mut App) {
    let path = match app.azureal_panel.as_ref() {
        Some(p) if !p.dump_files.is_empty() => {
            let name = &p.dump_files[p.dump_selected].0;
            let project = app.project.as_ref().map(|pr| pr.path.clone()).unwrap_or_else(|| std::path::PathBuf::from("."));
            project.join(".azureal").join(name)
        }
        _ => return,
    };
    let _ = std::fs::remove_file(&path);
    if let Some(ref mut p) = app.azureal_panel {
        let project_path = app.project.as_ref().map(|pr| pr.path.clone()).unwrap_or_else(|| std::path::PathBuf::from("."));
        p.dump_files = crate::app::scan_debug_dumps_pub(&project_path);
        if p.dump_selected >= p.dump_files.len() {
            p.dump_selected = p.dump_files.len().saturating_sub(1);
        }
    }
}

fn open_current_issue_browser(app: &mut App) {
    if let Some(ref p) = app.azureal_panel {
        if let Some(issue) = p.issues.get(p.issue_selected) {
            let _ = crate::github::open_issue_in_browser(&p.upstream_repo, issue.number);
        }
    }
}

fn open_in_browser(app: &mut App) {
    if let Some(ref p) = app.azureal_panel {
        match p.tab {
            AzurealTab::Issues => {
                if let Some(issue) = p.issues.get(p.issue_selected) {
                    let _ = crate::github::open_issue_in_browser(&p.upstream_repo, issue.number);
                }
            }
            AzurealTab::PullRequests => {
                if let Some(pr) = p.prs.get(p.pr_selected) {
                    let _ = crate::github::open_pr_in_browser(&p.upstream_repo, pr.number);
                }
            }
            _ => {}
        }
    }
}

fn submit_issue(app: &mut App) -> Result<()> {
    let (repo, title, body) = match app.azureal_panel.as_ref() {
        Some(p) => {
            if let Some(ref c) = p.issue_create {
                if c.title.trim().is_empty() {
                    app.set_status("Issue title cannot be empty");
                    return Ok(());
                }
                (p.upstream_repo.clone(), c.title.clone(), c.body.clone())
            } else { return Ok(()); }
        }
        None => return Ok(()),
    };
    match crate::github::create_issue(&repo, &title, &body) {
        Ok(num) => {
            app.set_status(format!("Created issue #{}", num));
            if let Some(ref mut p) = app.azureal_panel {
                p.issue_create = None;
            }
            refresh_current_tab(app);
        }
        Err(e) => { app.set_status(format!("Failed to create issue: {}", e)); }
    }
    Ok(())
}

fn submit_pr(app: &mut App) -> Result<()> {
    let (repo, head, title, body) = match app.azureal_panel.as_ref() {
        Some(p) => {
            if let Some(ref c) = p.pr_create {
                if c.title.trim().is_empty() {
                    app.set_status("PR title cannot be empty");
                    return Ok(());
                }
                (p.upstream_repo.clone(), c.head_branch.clone(), c.title.clone(), c.body.clone())
            } else { return Ok(()); }
        }
        None => return Ok(()),
    };
    match crate::github::create_pr(&repo, &head, &title, &body) {
        Ok(url) => {
            app.set_status(format!("Created PR: {}", url));
            if let Some(ref mut p) = app.azureal_panel {
                p.pr_create = None;
            }
            refresh_current_tab(app);
        }
        Err(e) => { app.set_status(format!("Failed to create PR: {}", e)); }
    }
    Ok(())
}

/// Trigger background loading for the current tab if it hasn't loaded yet
pub fn maybe_start_loading(app: &mut App) {
    let (tab, repo, show_closed) = match app.azureal_panel.as_ref() {
        Some(p) => (p.tab, p.upstream_repo.clone(), p.show_closed),
        None => return,
    };
    if repo.is_empty() { return; }

    match tab {
        AzurealTab::Issues => {
            let panel = app.azureal_panel.as_ref().unwrap();
            if panel.issues.is_empty() && !panel.issues_loading && panel.issues_receiver.is_none() {
                let (tx, rx) = std::sync::mpsc::channel();
                let repo_clone = repo;
                std::thread::spawn(move || {
                    let result = crate::github::fetch_issues(&repo_clone, show_closed);
                    let _ = tx.send(result);
                });
                if let Some(ref mut p) = app.azureal_panel {
                    p.issues_loading = true;
                    p.issues_receiver = Some(rx);
                }
            }
        }
        AzurealTab::PullRequests => {
            let panel = app.azureal_panel.as_ref().unwrap();
            if panel.prs.is_empty() && !panel.prs_loading && panel.prs_receiver.is_none() {
                let (tx, rx) = std::sync::mpsc::channel();
                let repo_clone = repo;
                std::thread::spawn(move || {
                    let result = crate::github::fetch_prs(&repo_clone);
                    let _ = tx.send(result);
                });
                if let Some(ref mut p) = app.azureal_panel {
                    p.prs_loading = true;
                    p.prs_receiver = Some(rx);
                }
            }
        }
        _ => {}
    }
}

fn refresh_current_tab(app: &mut App) {
    if let Some(ref mut p) = app.azureal_panel {
        match p.tab {
            AzurealTab::Debug => {
                let project_path = app.project.as_ref().map(|pr| pr.path.clone()).unwrap_or_else(|| std::path::PathBuf::from("."));
                p.dump_files = crate::app::scan_debug_dumps_pub(&project_path);
            }
            AzurealTab::Issues => {
                p.issues.clear();
                p.issues_loading = false;
                p.issues_receiver = None;
                p.issue_selected = 0;
            }
            AzurealTab::PullRequests => {
                p.prs.clear();
                p.prs_loading = false;
                p.prs_receiver = None;
                p.pr_selected = 0;
            }
        }
    }
    maybe_start_loading(app);
}
