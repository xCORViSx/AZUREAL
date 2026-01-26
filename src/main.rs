mod app;
mod claude;
mod config;
mod db;
mod git;
mod migrations;
mod models;
mod session;
mod syntax;
mod tui;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable table format
    #[default]
    Table,
    /// JSON output for scripting
    Json,
    /// Plain text, one item per line
    Plain,
}

#[derive(Parser)]
#[command(name = "crystal")]
#[command(about = "Minimal multi-session Claude Code manager with git worktrees")]
#[command(version)]
#[command(propagate_version = true)]
struct Cli {
    /// Output format
    #[arg(short = 'o', long, value_enum, default_value_t = OutputFormat::Table, global = true)]
    output: OutputFormat,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Path to config file
    #[arg(long, global = true)]
    config: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the TUI interface
    Tui,

    /// Session management commands
    #[command(subcommand)]
    Session(SessionCommands),

    /// Project management commands
    #[command(subcommand)]
    Project(ProjectCommands),

    // Shortcuts for common session operations
    /// List all sessions (shortcut for 'session list')
    #[command(alias = "ls")]
    List {
        /// Filter by project path
        #[arg(short, long)]
        project: Option<String>,
        /// Show archived sessions too
        #[arg(short, long)]
        all: bool,
    },

    /// Create a new session (shortcut for 'session new')
    New {
        /// Initial prompt for Claude
        #[arg(short, long)]
        prompt: String,
        /// Project path (defaults to current directory)
        #[arg(short = 'd', long)]
        project: Option<String>,
        /// Custom session name
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Show session status (shortcut for 'session status')
    Status {
        /// Session ID or name
        session: String,
    },

    /// Generate or show diff for a session's worktree
    Diff {
        /// Session ID or name
        session: String,
        /// Output format (unified, stat, patch, json)
        #[arg(short, long, default_value = "unified")]
        format: String,
        /// Save diff to file
        #[arg(short, long)]
        output: Option<String>,
        /// Save snapshot to database
        #[arg(long)]
        save: bool,
        /// Show diff history
        #[arg(long)]
        history: bool,
    },
}

#[derive(Subcommand)]
enum SessionCommands {
    /// List all sessions
    #[command(alias = "ls")]
    List {
        /// Filter by project path
        #[arg(short, long)]
        project: Option<String>,
        /// Show archived sessions too
        #[arg(short, long)]
        all: bool,
    },

    /// Create a new session
    New {
        /// Initial prompt for Claude
        #[arg(short, long)]
        prompt: String,
        /// Project path (defaults to current directory)
        #[arg(short = 'd', long)]
        project: Option<String>,
        /// Custom session name
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Show session status
    Status {
        /// Session ID or name
        session: String,
    },

    /// Stop a running session
    Stop {
        /// Session ID or name
        session: String,
        /// Force stop (SIGKILL instead of SIGTERM)
        #[arg(short, long)]
        force: bool,
    },

    /// Delete a session and its worktree
    Delete {
        /// Session ID or name
        session: String,
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },

    /// Archive a session (keeps worktree, marks as archived)
    Archive {
        /// Session ID or name
        session: String,
    },

    /// Resume a stopped or waiting session
    Resume {
        /// Session ID or name
        session: String,
        /// Additional prompt to send
        #[arg(short, long)]
        prompt: Option<String>,
    },

    /// Show session logs/output
    Logs {
        /// Session ID or name
        session: String,
        /// Follow output in real-time
        #[arg(short, long)]
        follow: bool,
        /// Number of lines to show
        #[arg(short, long, default_value = "50")]
        lines: usize,
    },

    /// Generate or show diff for a session
    Diff {
        /// Session ID or name
        session: String,
        /// Output format (unified, stat, patch, json)
        #[arg(short, long, default_value = "unified")]
        format: String,
        /// Save diff to file
        #[arg(short, long)]
        output: Option<String>,
        /// Save snapshot to database
        #[arg(long)]
        save: bool,
        /// Show diff history
        #[arg(long)]
        history: bool,
    },
    /// Clean up worktrees from completed/failed/archived sessions
    Cleanup {
        /// Project path (defaults to current directory)
        #[arg(short = 'd', long)]
        project: Option<String>,
        /// Also delete the associated git branches
        #[arg(long)]
        delete_branches: bool,
        /// Perform cleanup without confirmation
        #[arg(short = 'y', long)]
        yes: bool,
        /// Only show what would be cleaned up (dry run)
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum ProjectCommands {
    /// List all projects
    #[command(alias = "ls")]
    List,

    /// Show project details
    Show {
        /// Project path or ID
        project: Option<String>,
    },

    /// Remove a project from tracking (does not delete files)
    Remove {
        /// Project path or ID
        project: String,
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },

    /// Set project configuration
    Config {
        /// Project path (defaults to current directory)
        #[arg(short, long)]
        project: Option<String>,

        /// Set the main branch name
        #[arg(long)]
        main_branch: Option<String>,

        /// Set a system prompt for all sessions
        #[arg(long)]
        system_prompt: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging based on verbose flag
    let log_level = if cli.verbose {
        "crystal=debug"
    } else {
        "crystal=info"
    };

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| log_level.to_string()),
        ))
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    // Ensure config directory exists
    config::ensure_config_dir()?;

    // Initialize database
    let db = db::Database::open()?;
    db.migrate()?;

    let output_format = cli.output;

    match cli.command {
        // TUI (default)
        Some(Commands::Tui) | None => {
            tui::run(db).await?;
        }

        // Session shortcuts
        Some(Commands::List { project, all }) => {
            handle_session_list(&db, project, all, output_format)?;
        }
        Some(Commands::New { prompt, project, name }) => {
            handle_session_new(&db, prompt, project, name, output_format)?;
        }
        Some(Commands::Status { session }) => {
            handle_session_status(&db, &session, output_format)?;
        }
        Some(Commands::Diff { session, format, output, save, history }) => {
            handle_session_diff(&db, &session, &format, output, save, history)?;
        }

        // Session subcommands
        Some(Commands::Session(cmd)) => match cmd {
            SessionCommands::List { project, all } => {
                handle_session_list(&db, project, all, output_format)?;
            }
            SessionCommands::New { prompt, project, name } => {
                handle_session_new(&db, prompt, project, name, output_format)?;
            }
            SessionCommands::Status { session } => {
                handle_session_status(&db, &session, output_format)?;
            }
            SessionCommands::Stop { session, force } => {
                handle_session_stop(&db, &session, force)?;
            }
            SessionCommands::Delete { session, yes } => {
                handle_session_delete(&db, &session, yes)?;
            }
            SessionCommands::Archive { session } => {
                handle_session_archive(&db, &session)?;
            }
            SessionCommands::Resume { session, prompt } => {
                handle_session_resume(&db, &session, prompt)?;
            }
            SessionCommands::Logs { session, follow, lines } => {
                handle_session_logs(&db, &session, follow, lines)?;
            }
            SessionCommands::Diff { session, format, output, save, history } => {
                handle_session_diff(&db, &session, &format, output, save, history)?;
            }
            SessionCommands::Cleanup {
                project,
                delete_branches,
                yes,
                dry_run,
            } => {
                handle_session_cleanup(&db, project, delete_branches, yes, dry_run)?;
            }
        },

        // Project subcommands
        Some(Commands::Project(cmd)) => match cmd {
            ProjectCommands::List => {
                handle_project_list(&db, output_format)?;
            }
            ProjectCommands::Show { project } => {
                handle_project_show(&db, project, output_format)?;
            }
            ProjectCommands::Remove { project, yes } => {
                handle_project_remove(&db, &project, yes)?;
            }
            ProjectCommands::Config {
                project,
                main_branch,
                system_prompt,
            } => {
                handle_project_config(&db, project, main_branch, system_prompt)?;
            }
        },
    }

    Ok(())
}

// ==================== Session Handlers ====================

fn handle_session_list(
    db: &db::Database,
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
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&sessions)?);
        }
        OutputFormat::Plain => {
            for session in &sessions {
                println!("{}\t{}\t{}", session.id, session.name, session.status.as_str());
            }
        }
        OutputFormat::Table => {
            if sessions.is_empty() {
                println!("No sessions found.");
            } else {
                println!(
                    "{:<36} {:<25} {:<12} {}",
                    "ID", "NAME", "STATUS", "WORKTREE"
                );
                println!("{}", "-".repeat(90));
                for session in sessions {
                    println!(
                        "{:<36} {:<25} {:<12} {}",
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

fn handle_session_new(
    db: &db::Database,
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
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&session)?);
        }
        OutputFormat::Plain => {
            println!("{}", session.id);
        }
        OutputFormat::Table => {
            println!("Created session: {} ({})", session.name, session.id);
            println!("Worktree: {}", session.worktree_path.display());
            println!("Branch: {}", session.branch_name);
        }
    }
    Ok(())
}

fn handle_session_status(db: &db::Database, session_id: &str, output_format: OutputFormat) -> Result<()> {
    let session = find_session(db, session_id)?;

    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&session)?);
        }
        OutputFormat::Plain => {
            println!("{}\t{}\t{}", session.id, session.name, session.status.as_str());
        }
        OutputFormat::Table => {
            println!("Session: {}", session.name);
            println!("ID: {}", session.id);
            println!("Status: {}", session.status.as_str());
            println!("Worktree: {}", session.worktree_path.display());
            println!("Branch: {}", session.branch_name);
            println!("Created: {}", session.created_at.format("%Y-%m-%d %H:%M:%S UTC"));
            println!("Updated: {}", session.updated_at.format("%Y-%m-%d %H:%M:%S UTC"));
            if let Some(pid) = session.pid {
                println!("PID: {}", pid);
            }
            if let Some(code) = session.exit_code {
                println!("Exit code: {}", code);
            }

            // Show git status if worktree exists
            if session.worktree_path.exists() {
                if let Ok(status) = git::Git::status(&session.worktree_path) {
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

fn handle_session_stop(db: &db::Database, session_id: &str, force: bool) -> Result<()> {
    let session = find_session(db, session_id)?;

    if let Some(pid) = session.pid {
        let signal = if force { "SIGKILL" } else { "SIGTERM" };
        println!("Sending {} to process {}", signal, pid);

        #[cfg(unix)]
        {
            let sig = if force { 9 } else { 15 }; // SIGKILL = 9, SIGTERM = 15
            unsafe {
                libc::kill(pid as i32, sig);
            }
        }

        #[cfg(not(unix))]
        {
            println!("Process termination not supported on this platform");
        }

        db.update_session_status(&session.id, models::SessionStatus::Stopped)?;
        println!("Session stopped: {}", session.name);
    } else {
        println!("Session has no running process: {}", session.name);
    }
    Ok(())
}

fn handle_session_delete(db: &db::Database, session_id: &str, skip_confirm: bool) -> Result<()> {
    let session = find_session(db, session_id)?;

    if !skip_confirm {
        println!(
            "Delete session '{}' and worktree at {}?",
            session.name,
            session.worktree_path.display()
        );
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

    let project = db
        .get_project(session.project_id)?
        .ok_or_else(|| anyhow::anyhow!("Project not found"))?;

    session::SessionManager::new(db).delete_session(&session, &project)?;
    println!("Deleted session: {}", session.name);
    Ok(())
}

fn handle_session_archive(db: &db::Database, session_id: &str) -> Result<()> {
    let session = find_session(db, session_id)?;
    db.archive_session(&session.id)?;
    println!("Archived session: {}", session.name);
    Ok(())
}

fn handle_session_resume(_db: &db::Database, session_id: &str, _prompt: Option<String>) -> Result<()> {
    // TODO: Implement actual resume logic with ClaudeProcess
    println!("Resume not yet implemented for session: {}", session_id);
    println!("Use the TUI to interact with sessions.");
    Ok(())
}

fn handle_session_logs(db: &db::Database, session_id: &str, _follow: bool, lines: usize) -> Result<()> {
    let session = find_session(db, session_id)?;
    let outputs = db.get_session_outputs(&session.id)?;

    let to_show = if outputs.len() > lines {
        &outputs[outputs.len() - lines..]
    } else {
        &outputs
    };

    for output in to_show {
        println!(
            "[{}] {}: {}",
            output.timestamp.format("%H:%M:%S"),
            output.output_type.as_str(),
            output.data
        );
    }

    if outputs.is_empty() {
        println!("No output recorded for session: {}", session.name);
    }

    // TODO: Implement --follow with tail-like behavior
    Ok(())
}

fn handle_session_diff(
    db: &db::Database,
    session_id: &str,
    format: &str,
    output: Option<String>,
    save: bool,
    history: bool,
) -> Result<()> {
    let session = find_session(db, session_id)?;

    // Get project for main branch info
    let project = db
        .get_project(session.project_id)?
        .ok_or_else(|| anyhow::anyhow!("Project not found for session"))?;

    if history {
        // Show diff history
        let diffs = db.get_diff_history(&session.id)?;
        if diffs.is_empty() {
            println!("No saved diffs for this session.");
        } else {
            println!("Diff history for session: {}\n", session.name);
            for (i, diff) in diffs.iter().enumerate() {
                println!(
                    "{}. {} - {} ({} files, +{} -{})",
                    i + 1,
                    diff.timestamp.format("%Y-%m-%d %H:%M"),
                    diff.head_commit
                        .as_ref()
                        .map(|c| git::Git::short_hash(c))
                        .unwrap_or_else(|| "unknown".to_string()),
                    diff.files_changed.len(),
                    diff.additions,
                    diff.deletions
                );
            }
        }
        return Ok(());
    }

    if !session.worktree_path.exists() {
        anyhow::bail!("Worktree does not exist: {}", session.worktree_path.display());
    }

    // Generate current diff
    let mut diff = git::Git::get_diff(&session.worktree_path, &project.main_branch)?;
    diff.session_id = session.id.clone();

    if diff.is_empty() {
        println!("No changes in this session.");
        return Ok(());
    }

    // Save to database if requested
    if save {
        db.save_diff(&diff)?;
        println!("Diff snapshot saved.");
    }

    // Format output
    let output_text = match format {
        "stat" => {
            let mut result = String::new();
            result.push_str(&format!("Session: {}\n", session.name));
            result.push_str(&format!("Branch: {}\n", session.branch_name));
            if let Some(ref base) = diff.base_commit {
                result.push_str(&format!("Base: {}\n", git::Git::short_hash(base)));
            }
            if let Some(ref head) = diff.head_commit {
                result.push_str(&format!("Head: {}\n", git::Git::short_hash(head)));
            }
            result.push_str(&format!("\nFiles changed ({}):\n", diff.files_changed.len()));
            for file in &diff.files_changed {
                result.push_str(&format!("  {}\n", file));
            }
            result.push_str(&format!(
                "\n{} insertions(+), {} deletions(-)\n",
                diff.additions, diff.deletions
            ));
            result
        }
        "patch" => git::Git::generate_patch(&session.worktree_path, &project.main_branch)?,
        "json" => serde_json::to_string_pretty(&diff)?,
        _ => diff.diff_text.clone(), // "unified" or default
    };

    // Output to file or stdout
    if let Some(path) = output {
        std::fs::write(&path, &output_text)?;
        println!("Diff written to: {}", path);
    } else {
        println!("{}", output_text);
    }

    Ok(())
}

fn handle_session_cleanup(
    db: &db::Database,
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
        println!(
            "  {} [{}] {} ({})",
            session.status.symbol(),
            session.status.as_str(),
            session.name,
            session.worktree_path.display()
        );
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

    let mut cleaned = 0;
    let mut errors = 0;
    for session in &sessions {
        match manager.cleanup_session(session, &project, delete_branches) {
            Ok(()) => {
                println!("Cleaned: {}", session.name);
                cleaned += 1;
            }
            Err(e) => {
                eprintln!("Error cleaning {}: {}", session.name, e);
                errors += 1;
            }
        }
    }

    println!("\nCleanup complete: {} cleaned, {} errors", cleaned, errors);
    Ok(())
}

// ==================== Project Handlers ====================

fn handle_project_list(db: &db::Database, output_format: OutputFormat) -> Result<()> {
    let projects = db.list_projects()?;

    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&projects)?);
        }
        OutputFormat::Plain => {
            for project in &projects {
                println!("{}\t{}\t{}", project.id, project.name, project.path.display());
            }
        }
        OutputFormat::Table => {
            if projects.is_empty() {
                println!("No projects found.");
            } else {
                println!("{:<6} {:<20} {}", "ID", "NAME", "PATH");
                println!("{}", "-".repeat(70));
                for project in projects {
                    println!(
                        "{:<6} {:<20} {}",
                        project.id,
                        truncate(&project.name, 20),
                        project.path.display()
                    );
                }
            }
        }
    }
    Ok(())
}

fn handle_project_show(
    db: &db::Database,
    project_arg: Option<String>,
    output_format: OutputFormat,
) -> Result<()> {
    let project = match project_arg {
        Some(arg) => {
            // Try as ID first, then as path
            if let Ok(id) = arg.parse::<i64>() {
                db.get_project(id)?
            } else {
                db.get_project_by_path(&std::path::PathBuf::from(&arg))?
            }
        }
        None => {
            let cwd = std::env::current_dir()?;
            db.get_project_by_path(&cwd)?
        }
    };

    let project = project.ok_or_else(|| anyhow::anyhow!("Project not found"))?;

    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&project)?);
        }
        OutputFormat::Plain => {
            println!("{}\t{}\t{}", project.id, project.name, project.path.display());
        }
        OutputFormat::Table => {
            println!("Project: {}", project.name);
            println!("ID: {}", project.id);
            println!("Path: {}", project.path.display());
            println!("Main branch: {}", project.main_branch);
            println!("Created: {}", project.created_at.format("%Y-%m-%d %H:%M:%S UTC"));

            if let Some(prompt) = &project.system_prompt {
                println!("System prompt: {}", truncate(prompt, 60));
            }

            // Show session count
            let sessions = db.list_sessions_for_project(project.id)?;
            println!("\nSessions: {}", sessions.len());
            if !sessions.is_empty() {
                for session in sessions.iter().take(5) {
                    println!(
                        "  {} {} - {}",
                        session.status.symbol(),
                        truncate(&session.name, 30),
                        session.status.as_str()
                    );
                }
                if sessions.len() > 5 {
                    println!("  ... and {} more", sessions.len() - 5);
                }
            }
        }
    }
    Ok(())
}

fn handle_project_remove(db: &db::Database, project_arg: &str, skip_confirm: bool) -> Result<()> {
    let project = if let Ok(id) = project_arg.parse::<i64>() {
        db.get_project(id)?
    } else {
        db.get_project_by_path(&std::path::PathBuf::from(project_arg))?
    };

    let project = project.ok_or_else(|| anyhow::anyhow!("Project not found"))?;

    if !skip_confirm {
        println!(
            "Remove project '{}' from tracking?",
            project.name
        );
        println!("This will NOT delete the project files or worktrees.");
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

    // Note: This will cascade delete sessions due to foreign key
    db.delete_project(project.id)?;
    println!("Removed project: {}", project.name);
    Ok(())
}

fn handle_project_config(
    db: &db::Database,
    project_arg: Option<String>,
    main_branch: Option<String>,
    system_prompt: Option<String>,
) -> Result<()> {
    let path = project_arg
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));

    let project = db
        .get_project_by_path(&path)?
        .ok_or_else(|| anyhow::anyhow!("Project not found: {}", path.display()))?;

    if main_branch.is_none() && system_prompt.is_none() {
        // Show current config
        println!("Project: {}", project.name);
        println!("Main branch: {}", project.main_branch);
        if let Some(prompt) = &project.system_prompt {
            println!("System prompt: {}", prompt);
        } else {
            println!("System prompt: (not set)");
        }
        return Ok(());
    }

    if let Some(branch) = main_branch {
        db.update_project_main_branch(project.id, &branch)?;
        println!("Updated main branch to: {}", branch);
    }

    if let Some(prompt) = system_prompt {
        db.update_project_system_prompt(project.id, Some(&prompt))?;
        println!("Updated system prompt");
    }

    Ok(())
}

// ==================== Utilities ====================

fn find_session(db: &db::Database, session_id: &str) -> Result<models::Session> {
    // Try exact ID match first
    if let Some(session) = db.get_session(session_id)? {
        return Ok(session);
    }

    // Try partial ID match or name match
    let sessions = db.list_sessions()?;

    // Partial ID match
    let matches: Vec<_> = sessions
        .iter()
        .filter(|s| s.id.starts_with(session_id) || s.name.to_lowercase().contains(&session_id.to_lowercase()))
        .collect();

    match matches.len() {
        0 => anyhow::bail!("Session not found: {}", session_id),
        1 => Ok(matches[0].clone()),
        _ => {
            eprintln!("Multiple sessions match '{}':", session_id);
            for s in &matches {
                eprintln!("  {} - {}", s.id, s.name);
            }
            anyhow::bail!("Please specify a more precise session ID or name");
        }
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
