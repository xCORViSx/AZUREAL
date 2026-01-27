//! Session command handlers

use anyhow::Result;

use crate::cli::OutputFormat;
use crate::db::Database;
use crate::git::Git;
use crate::models;
use crate::session;

/// Truncate string with ellipsis
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len { s.to_string() }
    else { format!("{}...", &s[..max_len.saturating_sub(3)]) }
}

/// Find session by ID or partial match
pub fn find_session(db: &Database, session_id: &str) -> Result<models::Session> {
    if let Some(session) = db.get_session(session_id)? { return Ok(session); }

    let sessions = db.list_sessions()?;
    let matches: Vec<_> = sessions.iter()
        .filter(|s| s.id.starts_with(session_id) || s.name.to_lowercase().contains(&session_id.to_lowercase()))
        .collect();

    match matches.len() {
        0 => anyhow::bail!("Session not found: {}", session_id),
        1 => Ok(matches[0].clone()),
        _ => {
            eprintln!("Multiple sessions match '{}':", session_id);
            for s in &matches { eprintln!("  {} - {}", s.id, s.name); }
            anyhow::bail!("Please specify a more precise session ID or name");
        }
    }
}

pub fn handle_session_list(
    db: &Database,
    project_filter: Option<String>,
    _all: bool,
    output_format: OutputFormat,
) -> Result<()> {
    let sessions = if let Some(project_path) = project_filter {
        let path = std::path::PathBuf::from(&project_path);
        if let Some(project) = db.get_project_by_path(&path)? {
            db.list_sessions_for_project(project.id)?
        } else {
            println!("Project not found: {}", project_path);
            return Ok(());
        }
    } else {
        db.list_sessions()?
    };

    match output_format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&sessions)?),
        OutputFormat::Plain => {
            for session in &sessions {
                println!("{}\t{}\t{}", session.id, session.name, session.status.as_str());
            }
        }
        OutputFormat::Table => {
            if sessions.is_empty() {
                println!("No sessions found.");
            } else {
                println!("{:<36} {:<25} {:<12} {}", "ID", "NAME", "STATUS", "WORKTREE");
                println!("{}", "-".repeat(90));
                for session in sessions {
                    println!("{:<36} {:<25} {:<12} {}",
                        session.id,
                        truncate(&session.name, 25),
                        session.status.as_str(),
                        truncate(&session.worktree_path.to_string_lossy(), 30)
                    );
                }
            }
        }
    }
    Ok(())
}

pub fn handle_session_new(
    db: &Database,
    prompt: String,
    project_path: Option<String>,
    _name: Option<String>,
    output_format: OutputFormat,
) -> Result<()> {
    let path = project_path
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));

    let project = db.get_or_create_project(&path)?;
    let session = session::SessionManager::new(db).create_session(&project, &prompt)?;

    match output_format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&session)?),
        OutputFormat::Plain => println!("{}", session.id),
        OutputFormat::Table => {
            println!("Created session: {} ({})", session.name, session.id);
            println!("Worktree: {}", session.worktree_path.display());
            println!("Branch: {}", session.branch_name);
        }
    }
    Ok(())
}

pub fn handle_session_status(db: &Database, session_id: &str, output_format: OutputFormat) -> Result<()> {
    let session = find_session(db, session_id)?;

    match output_format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&session)?),
        OutputFormat::Plain => println!("{}\t{}\t{}", session.id, session.name, session.status.as_str()),
        OutputFormat::Table => {
            println!("Session: {}", session.name);
            println!("ID: {}", session.id);
            println!("Status: {}", session.status.as_str());
            println!("Worktree: {}", session.worktree_path.display());
            println!("Branch: {}", session.branch_name);
            println!("Created: {}", session.created_at.format("%Y-%m-%d %H:%M:%S UTC"));
            println!("Updated: {}", session.updated_at.format("%Y-%m-%d %H:%M:%S UTC"));
            if let Some(pid) = session.pid { println!("PID: {}", pid); }
            if let Some(code) = session.exit_code { println!("Exit code: {}", code); }

            if session.worktree_path.exists() {
                if let Ok(status) = Git::status(&session.worktree_path) {
                    if !status.trim().is_empty() {
                        println!("\nGit status:");
                        println!("{}", status);
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn handle_session_stop(db: &Database, session_id: &str, force: bool) -> Result<()> {
    let session = find_session(db, session_id)?;

    if let Some(pid) = session.pid {
        let signal = if force { "SIGKILL" } else { "SIGTERM" };
        println!("Sending {} to process {}", signal, pid);

        #[cfg(unix)]
        {
            let sig = if force { 9 } else { 15 };
            unsafe { libc::kill(pid as i32, sig); }
        }

        #[cfg(not(unix))]
        println!("Process termination not supported on this platform");

        db.update_session_status(&session.id, models::SessionStatus::Stopped)?;
        println!("Session stopped: {}", session.name);
    } else {
        println!("Session has no running process: {}", session.name);
    }
    Ok(())
}

pub fn handle_session_delete(db: &Database, session_id: &str, skip_confirm: bool) -> Result<()> {
    let session = find_session(db, session_id)?;

    if !skip_confirm {
        println!("Delete session '{}' and worktree at {}?", session.name, session.worktree_path.display());
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

    let project = db.get_project(session.project_id)?
        .ok_or_else(|| anyhow::anyhow!("Project not found"))?;

    session::SessionManager::new(db).delete_session(&session, &project)?;
    println!("Deleted session: {}", session.name);
    Ok(())
}

pub fn handle_session_archive(db: &Database, session_id: &str) -> Result<()> {
    let session = find_session(db, session_id)?;
    db.archive_session(&session.id)?;
    println!("Archived session: {}", session.name);
    Ok(())
}

pub fn handle_session_resume(_db: &Database, session_id: &str, _prompt: Option<String>) -> Result<()> {
    println!("Resume not yet implemented for session: {}", session_id);
    println!("Use the TUI to interact with sessions.");
    Ok(())
}

pub fn handle_session_logs(db: &Database, session_id: &str, _follow: bool, lines: usize) -> Result<()> {
    let session = find_session(db, session_id)?;
    let outputs = db.get_session_outputs(&session.id)?;

    let to_show = if outputs.len() > lines { &outputs[outputs.len() - lines..] } else { &outputs };

    for output in to_show {
        println!("[{}] {}: {}", output.timestamp.format("%H:%M:%S"), output.output_type.as_str(), output.data);
    }

    if outputs.is_empty() { println!("No output recorded for session: {}", session.name); }
    Ok(())
}

pub fn handle_session_diff(db: &Database, session_id: &str, stat_only: bool) -> Result<()> {
    let session = find_session(db, session_id)?;

    if !session.worktree_path.exists() {
        anyhow::bail!("Worktree does not exist: {}", session.worktree_path.display());
    }

    let project = db.get_project(session.project_id)?
        .ok_or_else(|| anyhow::anyhow!("Project not found"))?;

    let diff_info = Git::get_diff(&session.worktree_path, &project.main_branch)?;

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
    db: &Database,
    project_path: Option<String>,
    delete_branches: bool,
    skip_confirm: bool,
    dry_run: bool,
) -> Result<()> {
    let path = project_path
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));

    let project = db.get_or_create_project(&path)?;
    let manager = session::SessionManager::new(db);
    let sessions = manager.list_cleanable_sessions(project.id)?;

    if sessions.is_empty() {
        println!("No sessions to clean up.");
        return Ok(());
    }

    println!("Sessions eligible for cleanup:");
    println!("{}", "-".repeat(80));
    for session in &sessions {
        println!("  {} [{}] {} ({})", session.status.symbol(), session.status.as_str(), session.name, session.worktree_path.display());
    }
    println!("{}", "-".repeat(80));
    println!("Total: {} session(s)", sessions.len());

    if dry_run {
        println!("\nDry run - no changes made.");
        return Ok(());
    }

    if !skip_confirm {
        print!("\nProceed with cleanup? [y/N] ");
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
    for session in &sessions {
        match manager.cleanup_session(session, &project, delete_branches) {
            Ok(()) => { println!("Cleaned: {}", session.name); cleaned += 1; }
            Err(e) => { eprintln!("Error cleaning {}: {}", session.name, e); errors += 1; }
        }
    }

    println!("\nCleanup complete: {} cleaned, {} errors", cleaned, errors);
    Ok(())
}
