mod app;
mod config;
mod db;
mod git;
mod models;
mod session;
mod tui;
mod claude;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "crystal")]
#[command(about = "Minimal multi-session Claude Code manager with git worktrees")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the TUI interface
    Tui,
    /// List all sessions
    List,
    /// Create a new session
    New {
        /// Initial prompt for Claude
        #[arg(short, long)]
        prompt: String,
        /// Project path (defaults to current directory)
        #[arg(short = 'd', long)]
        project: Option<String>,
    },
    /// Show session status
    Status {
        /// Session ID or name
        session: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "crystal=info".to_string()),
        ))
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    let cli = Cli::parse();

    // Ensure config directory exists
    config::ensure_config_dir()?;

    // Initialize database
    let db = db::Database::open()?;
    db.migrate()?;

    match cli.command {
        Some(Commands::List) => {
            let sessions = db.list_sessions()?;
            if sessions.is_empty() {
                println!("No sessions found.");
            } else {
                println!("{:<36} {:<20} {:<12} {}", "ID", "NAME", "STATUS", "PROJECT");
                println!("{}", "-".repeat(80));
                for session in sessions {
                    println!(
                        "{:<36} {:<20} {:<12} {}",
                        session.id,
                        truncate(&session.name, 20),
                        session.status.as_str(),
                        session.project_id
                    );
                }
            }
        }
        Some(Commands::New { prompt, project }) => {
            let project_path = project
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));

            // Ensure project exists in DB
            let project = db.get_or_create_project(&project_path)?;

            // Create session
            let session = session::SessionManager::new(&db)
                .create_session(&project, &prompt)?;

            println!("Created session: {} ({})", session.name, session.id);
            println!("Worktree: {}", session.worktree_path.display());
        }
        Some(Commands::Status { session: session_id }) => {
            if let Some(session) = db.get_session(&session_id)? {
                println!("Session: {}", session.name);
                println!("ID: {}", session.id);
                println!("Status: {}", session.status.as_str());
                println!("Worktree: {}", session.worktree_path.display());
                println!("Created: {}", session.created_at);
            } else {
                println!("Session not found: {}", session_id);
            }
        }
        Some(Commands::Tui) | None => {
            // Launch TUI
            tui::run(db).await?;
        }
    }

    Ok(())
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
