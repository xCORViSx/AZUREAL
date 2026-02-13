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
mod watcher;
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

    // Create and register a minimal .app bundle in ~/.azureal/ so macOS
    // notifications show the Azureal icon. The .icns is compiled into the
    // binary via include_bytes!() — zero external files needed after install.
    // Only writes files if the bundle doesn't exist yet (first launch).
    #[cfg(target_os = "macos")]
    {
        let bundle_dir = config::config_dir().join("Azureal.app");
        if !bundle_dir.join("Contents/Info.plist").exists() {
            let contents = bundle_dir.join("Contents");
            let _ = std::fs::create_dir_all(contents.join("MacOS"));
            let _ = std::fs::create_dir_all(contents.join("Resources"));
            // Stub executable — macOS requires a valid executable in the bundle
            let _ = std::fs::write(contents.join("MacOS/Azureal"), "#!/bin/sh\nexit 0\n");
            #[cfg(unix)] {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(
                    contents.join("MacOS/Azureal"),
                    std::fs::Permissions::from_mode(0o755),
                );
            }
            // Icon embedded at compile time (~629KB)
            let _ = std::fs::write(
                contents.join("Resources/Azureal.icns"),
                include_bytes!("../resources/Azureal.icns"),
            );
            // Info.plist with our bundle identifier
            let _ = std::fs::write(contents.join("Info.plist"), concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
                "<plist version=\"1.0\">\n<dict>\n",
                "\t<key>CFBundleIdentifier</key>\n\t<string>com.xcorvisx.azureal</string>\n",
                "\t<key>CFBundleName</key>\n\t<string>Azureal</string>\n",
                "\t<key>CFBundleExecutable</key>\n\t<string>Azureal</string>\n",
                "\t<key>CFBundleIconFile</key>\n\t<string>Azureal</string>\n",
                "\t<key>CFBundlePackageType</key>\n\t<string>APPL</string>\n",
                "\t<key>LSUIElement</key>\n\t<true/>\n",
                "</dict>\n</plist>\n",
            ));
            // Register with Launch Services so macOS knows about the bundle
            let _ = std::process::Command::new(
                "/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister"
            ).args(["-R", "-f", &bundle_dir.to_string_lossy()]).output();
        }
        let _ = notify_rust::set_application("com.xcorvisx.azureal");
    }

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
