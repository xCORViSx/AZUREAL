mod app;
mod claude;
mod cli;
mod cmd;
mod config;
mod events;
mod git;
mod models;
mod stt;
mod syntax;
mod tui;
mod wizard;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use cli::{Cli, Commands, ProjectCommands, SessionCommands};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let log_level = if cli.verbose { "azureal=debug" } else { "azureal=info" };

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| log_level.to_string()),
        ))
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    config::ensure_config_dir()?;

    let output_format = cli.output;

    match cli.command {
        Some(Commands::Tui) | None => tui::run().await?,

        // Session shortcuts
        Some(Commands::List { project, all }) => cmd::handle_session_list(project, all, output_format)?,
        Some(Commands::New { prompt, project, name }) => cmd::handle_session_new(prompt, project, name, output_format)?,
        Some(Commands::Status { session }) => cmd::handle_session_status(&session, output_format)?,
        Some(Commands::Diff { session, stat }) => cmd::handle_session_diff(&session, stat)?,

        // Session subcommands
        Some(Commands::Session(cmd)) => match cmd {
            SessionCommands::List { project, all } => cmd::handle_session_list(project, all, output_format)?,
            SessionCommands::New { prompt, project, name } => cmd::handle_session_new(prompt, project, name, output_format)?,
            SessionCommands::Status { session } => cmd::handle_session_status(&session, output_format)?,
            SessionCommands::Stop { session, force } => cmd::handle_session_stop(&session, force)?,
            SessionCommands::Delete { session, yes } => cmd::handle_session_delete(&session, yes)?,
            SessionCommands::Archive { session } => cmd::handle_session_archive(&session)?,
            SessionCommands::Resume { session, prompt } => cmd::handle_session_resume(&session, prompt)?,
            SessionCommands::Logs { session, follow, lines } => cmd::handle_session_logs(&session, follow, lines)?,
            SessionCommands::Diff { session, stat } => cmd::handle_session_diff(&session, stat)?,
            SessionCommands::Cleanup { project, delete_branches, yes, dry_run } => cmd::handle_session_cleanup(project, delete_branches, yes, dry_run)?,
        },

        // Project subcommands
        Some(Commands::Project(cmd)) => match cmd {
            ProjectCommands::List => cmd::handle_project_list(output_format)?,
            ProjectCommands::Show { project } => cmd::handle_project_show(project, output_format)?,
            ProjectCommands::Remove { project, yes } => cmd::handle_project_remove(&project, yes)?,
            ProjectCommands::Config { project, main_branch } => cmd::handle_project_config(project, main_branch)?,
        },
    }

    Ok(())
}
