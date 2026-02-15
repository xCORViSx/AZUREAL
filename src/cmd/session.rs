//! Session command handlers (stateless - derived from git)

use anyhow::Result;

use crate::cli::OutputFormat;
use crate::git::Git;
use crate::models::{Project, Worktree};

/// Truncate string with ellipsis
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len { s.to_string() }
    else { format!("{}...", &s[..max_len.saturating_sub(3)]) }
}

/// Discover project from current directory
fn discover_project() -> Result<Project> {
    let cwd = std::env::current_dir()?;
    if !Git::is_git_repo(&cwd) {
        anyhow::bail!("Not in a git repository");
    }
    let repo_root = Git::repo_root(&cwd)?;
    let main_branch = Git::get_main_branch(&repo_root)?;
    Ok(Project::from_path(repo_root, main_branch))
}

/// Discover sessions from git worktrees and branches
fn discover_worktrees(project: &Project) -> Result<Vec<Worktree>> {
    let worktrees = Git::list_worktrees_detailed(&project.path)?;
    let azureal_branches = Git::list_azureal_branches(&project.path)?;

    let mut sessions = Vec::new();
    let mut active_branches: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Add all worktrees as sessions
    for wt in &worktrees {
        let branch_name = wt.branch.clone().unwrap_or_else(|| project.main_branch.clone());
        let claude_id = crate::config::find_latest_claude_session(&wt.path);

        sessions.push(Worktree {
            branch_name: branch_name.clone(),
            worktree_path: Some(wt.path.clone()),
            claude_session_id: claude_id,
            archived: false,
        });
        active_branches.insert(branch_name);
    }

    // Add archived sessions (azureal/* branches without worktrees)
    for branch in azureal_branches {
        if !active_branches.contains(&branch) {
            sessions.push(Worktree {
                branch_name: branch,
                worktree_path: None,
                claude_session_id: None,
                archived: true,
            });
        }
    }

    Ok(sessions)
}

/// Find worktree by name or branch
fn find_worktree(worktrees: &[Worktree], query: &str) -> Result<Worktree> {
    // Exact branch match
    if let Some(s) = worktrees.iter().find(|s| s.branch_name == query) {
        return Ok(s.clone());
    }

    // Match by name (without azureal/ prefix)
    if let Some(s) = worktrees.iter().find(|s| s.name() == query) {
        return Ok(s.clone());
    }

    // Partial match
    let matches: Vec<_> = worktrees.iter()
        .filter(|s| s.branch_name.contains(query) || s.name().to_lowercase().contains(&query.to_lowercase()))
        .collect();

    match matches.len() {
        0 => anyhow::bail!("Worktree not found: {}", query),
        1 => Ok(matches[0].clone()),
        _ => {
            eprintln!("Multiple worktrees match '{}':", query);
            for s in &matches { eprintln!("  {}", s.branch_name); }
            anyhow::bail!("Please specify a more precise name");
        }
    }
}

pub fn handle_session_list(
    _project_filter: Option<String>,
    _all: bool,
    output_format: OutputFormat,
) -> Result<()> {
    let project = discover_project()?;
    let sessions = discover_worktrees(&project)?;
    match output_format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&sessions)?),
        OutputFormat::Plain => {
            for session in &sessions {
                // CLI has no process tracking — pass false for is_running
                println!("{}\t{}", session.branch_name, session.status(false).as_str());
            }
        }
        OutputFormat::Table => {
            if sessions.is_empty() {
                println!("No sessions found.");
            } else {
                println!("{:<40} {:<12} WORKTREE", "BRANCH", "STATUS");
                println!("{}", "-".repeat(90));
                for session in sessions {
                    let wt = session.worktree_path.as_ref()
                        .map(|p| truncate(&p.to_string_lossy(), 35))
                        .unwrap_or_else(|| "(archived)".to_string());
                    println!("{:<40} {:<12} {}",
                        truncate(&session.branch_name, 40),
                        session.status(false).as_str(),
                        wt
                    );
                }
            }
        }
    }
    Ok(())
}

pub fn handle_session_new(
    prompt: String,
    _project_path: Option<String>,
    _name: Option<String>,
    output_format: OutputFormat,
) -> Result<()> {
    let project = discover_project()?;

    // Generate session name from prompt
    let name = generate_session_name(&prompt);
    let worktree_name = sanitize_for_branch(&name);
    let branch_name = format!("azureal/{}", worktree_name);
    let worktree_path = project.worktrees_dir().join(&worktree_name);

    if worktree_path.exists() {
        anyhow::bail!("Worktree already exists: {}", worktree_path.display());
    }

    // Create git worktree
    Git::create_worktree(&project.path, &worktree_path, &branch_name)?;

    let session = Worktree {
        branch_name: branch_name.clone(),
        worktree_path: Some(worktree_path.clone()),
        claude_session_id: None,
        archived: false,
    };

    match output_format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&session)?),
        OutputFormat::Plain => println!("{}", branch_name),
        OutputFormat::Table => {
            println!("Created session: {} ({})", session.name(), branch_name);
            println!("Worktree: {}", worktree_path.display());
        }
    }
    Ok(())
}

pub fn handle_session_status(session_query: &str, output_format: OutputFormat) -> Result<()> {
    let project = discover_project()?;
    let sessions = discover_worktrees(&project)?;
    let session = find_worktree(&sessions, session_query)?;
    match output_format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&session)?),
        // CLI has no process tracking — pass false for is_running
        OutputFormat::Plain => println!("{}\t{}", session.branch_name, session.status(false).as_str()),
        OutputFormat::Table => {
            println!("Session: {}", session.name());
            println!("Branch: {}", session.branch_name);
            println!("Status: {}", session.status(false).as_str());
            if let Some(ref wt) = session.worktree_path {
                println!("Worktree: {}", wt.display());
                if let Ok(status) = Git::status(wt) {
                    if !status.trim().is_empty() {
                        println!("\nGit status:\n{}", status);
                    }
                }
            } else {
                println!("Worktree: (archived)");
            }
            if let Some(ref id) = session.claude_session_id {
                println!("Claude session: {}", id);
            }
        }
    }
    Ok(())
}

pub fn handle_session_stop(session_query: &str, _force: bool) -> Result<()> {
    let project = discover_project()?;
    let sessions = discover_worktrees(&project)?;
    let session = find_worktree(&sessions, session_query)?;

    // Note: In stateless mode, we can't track PIDs
    // User should use `pkill` or similar to stop Claude processes
    println!("Session: {}", session.name());
    println!("To stop a Claude process, use: pkill -f 'claude.*{}'", session.name());
    Ok(())
}

pub fn handle_session_delete(session_query: &str, skip_confirm: bool) -> Result<()> {
    let project = discover_project()?;
    let sessions = discover_worktrees(&project)?;
    let session = find_worktree(&sessions, session_query)?;

    if !skip_confirm {
        if let Some(ref wt) = session.worktree_path {
            println!("Delete session '{}' and worktree at {}?", session.name(), wt.display());
        } else {
            println!("Delete archived branch '{}'?", session.branch_name);
        }
        print!("Type 'yes' to confirm: ");
        use std::io::Write;
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if input.trim() != "yes" {
            println!("Cancelled.");
            return Ok(());
        }
    }

    // Remove worktree if exists
    if let Some(ref wt) = session.worktree_path {
        Git::remove_worktree(&project.path, wt)?;
    }

    // Delete the branch
    Git::delete_branch(&project.path, &session.branch_name)?;

    println!("Deleted session: {}", session.name());
    Ok(())
}

pub fn handle_session_archive(session_query: &str) -> Result<()> {
    let project = discover_project()?;
    let sessions = discover_worktrees(&project)?;
    let session = find_worktree(&sessions, session_query)?;

    if session.archived {
        println!("Session is already archived: {}", session.name());
        return Ok(());
    }

    if let Some(ref wt) = session.worktree_path {
        Git::remove_worktree(&project.path, wt)?;
        println!("Archived session: {} (branch preserved)", session.name());
    } else {
        println!("Session has no worktree to archive");
    }
    Ok(())
}

pub fn handle_session_resume(session_query: &str, _prompt: Option<String>) -> Result<()> {
    let project = discover_project()?;
    let sessions = discover_worktrees(&project)?;
    let session = find_worktree(&sessions, session_query)?;

    println!("Session: {}", session.name());
    if let Some(ref wt) = session.worktree_path {
        println!("Worktree: {}", wt.display());
        println!("\nTo resume this session, run Claude in the worktree:");
        println!("  cd {} && claude", wt.display());
    } else {
        println!("Session is archived. Create a new worktree first:");
        println!("  git worktree add worktrees/{} {}", session.name(), session.branch_name);
    }
    Ok(())
}

pub fn handle_session_logs(session_query: &str, _follow: bool, _lines: usize) -> Result<()> {
    let project = discover_project()?;
    let sessions = discover_worktrees(&project)?;
    let session = find_worktree(&sessions, session_query)?;

    println!("Session: {}", session.name());

    if let Some(ref id) = session.claude_session_id {
        println!("Claude session ID: {}", id);
        if let Some(ref wt) = session.worktree_path {
            if let Some(file) = crate::config::claude_session_file(wt, id) {
                println!("Session file: {}", file.display());
            }
        }
    } else {
        println!("No Claude session ID found");
    }

    println!("\nUse the TUI for interactive session viewing.");
    Ok(())
}

pub fn handle_session_diff(session_query: &str, stat_only: bool) -> Result<()> {
    let project = discover_project()?;
    let sessions = discover_worktrees(&project)?;
    let session = find_worktree(&sessions, session_query)?;

    let Some(ref wt) = session.worktree_path else {
        anyhow::bail!("Session is archived (no worktree)");
    };

    let diff_info = Git::get_diff(wt, &project.main_branch)?;

    if stat_only {
        if diff_info.files_changed.is_empty() {
            println!("No changes");
        } else {
            println!("Files changed:");
            for file in &diff_info.files_changed { println!("  {}", file); }
            println!("\n{} files, +{} -{} lines", diff_info.files_changed.len(), diff_info.additions, diff_info.deletions);
        }
    } else if diff_info.diff_text.is_empty() {
        println!("No changes");
    } else {
        println!("{}", diff_info.diff_text);
    }
    Ok(())
}

pub fn handle_session_cleanup(
    _project_path: Option<String>,
    delete_branches: bool,
    skip_confirm: bool,
    dry_run: bool,
) -> Result<()> {
    let project = discover_project()?;
    let sessions = discover_worktrees(&project)?;

    // Find cleanable sessions (archived ones)
    let cleanable: Vec<_> = sessions.iter().filter(|s| s.archived).collect();

    if cleanable.is_empty() {
        println!("No archived sessions to clean up.");
        return Ok(());
    }

    println!("Archived sessions eligible for cleanup:");
    println!("{}", "-".repeat(60));
    for session in &cleanable {
        println!("  {} (branch only)", session.branch_name);
    }
    println!("{}", "-".repeat(60));
    println!("Total: {} session(s)", cleanable.len());

    if dry_run {
        println!("\nDry run - no changes made.");
        return Ok(());
    }

    if !delete_branches {
        println!("\nNo worktrees to remove (all are already archived).");
        println!("Use --delete-branches to remove the git branches.");
        return Ok(());
    }

    if !skip_confirm {
        print!("\nDelete {} branch(es)? [y/N] ", cleanable.len());
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cleanup cancelled.");
            return Ok(());
        }
    }

    let (mut cleaned, mut errors) = (0, 0);
    for session in &cleanable {
        match Git::delete_branch(&project.path, &session.branch_name) {
            Ok(()) => { println!("Deleted: {}", session.branch_name); cleaned += 1; }
            Err(e) => { eprintln!("Error deleting {}: {}", session.branch_name, e); errors += 1; }
        }
    }

    println!("\nCleanup complete: {} deleted, {} errors", cleaned, errors);
    Ok(())
}

/// Generate a session name from the prompt
fn generate_session_name(prompt: &str) -> String {
    let name: String = prompt
        .chars()
        .take(40)
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-' || *c == '_')
        .collect();

    let name = name.trim();

    if name.is_empty() {
        format!("session-{}", &uuid::Uuid::new_v4().to_string()[..8])
    } else {
        let name = if name.len() > 30 {
            if let Some(pos) = name[..30].rfind(' ') { &name[..pos] }
            else { &name[..30] }
        } else { name };
        name.to_string()
    }
}

/// Sanitize a string for use as a git branch name
fn sanitize_for_branch(s: &str) -> String {
    let sanitized: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();

    let mut result = String::new();
    let mut last_was_dash = false;

    for c in sanitized.chars() {
        if c == '-' {
            if !last_was_dash && !result.is_empty() {
                result.push(c);
                last_was_dash = true;
            }
        } else {
            result.push(c);
            last_was_dash = false;
        }
    }

    result.trim_end_matches('-').to_string()
}
