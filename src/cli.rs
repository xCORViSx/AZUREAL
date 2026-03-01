//! CLI argument definitions

use clap::{Parser, Subcommand, ValueEnum};

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
#[command(name = "azureal")]
#[command(about = "Minimal multi-session Claude Code manager with git worktrees")]
#[command(version)]
#[command(propagate_version = true)]
pub struct Cli {
    /// Output format
    #[arg(short = 'o', long, value_enum, default_value_t = OutputFormat::Table, global = true)]
    pub output: OutputFormat,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Path to config file
    #[arg(long, global = true)]
    pub config: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
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

    /// Show diff for a session's worktree
    Diff {
        /// Session ID or name
        session: String,
        /// Show stat only (files changed summary)
        #[arg(long)]
        stat: bool,
    },
}

#[derive(Subcommand)]
pub enum SessionCommands {
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

    /// Unarchive a session (recreate worktree from preserved branch)
    Unarchive {
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

    /// Show diff for a session's worktree
    Diff {
        /// Session ID or name
        session: String,
        /// Show stat only (files changed summary)
        #[arg(long)]
        stat: bool,
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
pub enum ProjectCommands {
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

    /// Show project configuration
    Config {
        /// Project path (defaults to current directory)
        #[arg(short, long)]
        project: Option<String>,

        /// Set the main branch name (shows instructions in stateless mode)
        #[arg(long)]
        main_branch: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    // ── OutputFormat ──

    #[test]
    fn test_output_format_default_is_table() {
        let f = OutputFormat::default();
        assert!(matches!(f, OutputFormat::Table));
    }

    #[test]
    fn test_output_format_debug() {
        assert_eq!(format!("{:?}", OutputFormat::Table), "Table");
        assert_eq!(format!("{:?}", OutputFormat::Json), "Json");
        assert_eq!(format!("{:?}", OutputFormat::Plain), "Plain");
    }

    #[test]
    fn test_output_format_clone() {
        let f = OutputFormat::Json;
        let f2 = f;
        assert!(matches!(f, OutputFormat::Json));
        assert!(matches!(f2, OutputFormat::Json));
    }

    // ── Cli parsing: no args ──

    #[test]
    fn test_parse_no_args() {
        let cli = Cli::try_parse_from(["azureal"]).unwrap();
        assert!(cli.command.is_none());
        assert!(!cli.verbose);
        assert!(cli.config.is_none());
        assert!(matches!(cli.output, OutputFormat::Table));
    }

    // ── Cli parsing: tui subcommand ──

    #[test]
    fn test_parse_tui() {
        let cli = Cli::try_parse_from(["azureal", "tui"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Tui)));
    }

    // ── Cli parsing: verbose flag ──

    #[test]
    fn test_parse_verbose_short() {
        let cli = Cli::try_parse_from(["azureal", "-v"]).unwrap();
        assert!(cli.verbose);
    }

    #[test]
    fn test_parse_verbose_long() {
        let cli = Cli::try_parse_from(["azureal", "--verbose"]).unwrap();
        assert!(cli.verbose);
    }

    // ── Cli parsing: output format ──

    #[test]
    fn test_parse_output_json() {
        let cli = Cli::try_parse_from(["azureal", "-o", "json"]).unwrap();
        assert!(matches!(cli.output, OutputFormat::Json));
    }

    #[test]
    fn test_parse_output_plain() {
        let cli = Cli::try_parse_from(["azureal", "-o", "plain"]).unwrap();
        assert!(matches!(cli.output, OutputFormat::Plain));
    }

    #[test]
    fn test_parse_output_table() {
        let cli = Cli::try_parse_from(["azureal", "-o", "table"]).unwrap();
        assert!(matches!(cli.output, OutputFormat::Table));
    }

    #[test]
    fn test_parse_output_long() {
        let cli = Cli::try_parse_from(["azureal", "--output", "json"]).unwrap();
        assert!(matches!(cli.output, OutputFormat::Json));
    }

    #[test]
    fn test_parse_invalid_output_format() {
        let result = Cli::try_parse_from(["azureal", "-o", "xml"]);
        assert!(result.is_err());
    }

    // ── Cli parsing: config flag ──

    #[test]
    fn test_parse_config_long() {
        let cli = Cli::try_parse_from(["azureal", "--config", "/path/to/config.toml"]).unwrap();
        assert_eq!(cli.config.as_deref(), Some("/path/to/config.toml"));
    }

    #[test]
    fn test_parse_no_config() {
        let cli = Cli::try_parse_from(["azureal"]).unwrap();
        assert!(cli.config.is_none());
    }

    // ── Cli parsing: list shortcut ──

    #[test]
    fn test_parse_list() {
        let cli = Cli::try_parse_from(["azureal", "list"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::List { .. })));
    }

    #[test]
    fn test_parse_ls_alias() {
        let cli = Cli::try_parse_from(["azureal", "ls"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::List { .. })));
    }

    #[test]
    fn test_parse_list_with_project() {
        let cli = Cli::try_parse_from(["azureal", "list", "-p", "/my/project"]).unwrap();
        if let Some(Commands::List { project, all }) = cli.command {
            assert_eq!(project.as_deref(), Some("/my/project"));
            assert!(!all);
        } else {
            panic!("expected List command");
        }
    }

    #[test]
    fn test_parse_list_with_all() {
        let cli = Cli::try_parse_from(["azureal", "list", "-a"]).unwrap();
        if let Some(Commands::List { project, all }) = cli.command {
            assert!(project.is_none());
            assert!(all);
        } else {
            panic!("expected List command");
        }
    }

    // ── Cli parsing: new shortcut ──

    #[test]
    fn test_parse_new() {
        let cli = Cli::try_parse_from(["azureal", "new", "-p", "Fix the bug"]).unwrap();
        if let Some(Commands::New { prompt, project, name }) = cli.command {
            assert_eq!(prompt, "Fix the bug");
            assert!(project.is_none());
            assert!(name.is_none());
        } else {
            panic!("expected New command");
        }
    }

    #[test]
    fn test_parse_new_with_name() {
        let cli = Cli::try_parse_from(["azureal", "new", "-p", "do stuff", "-n", "my-session"]).unwrap();
        if let Some(Commands::New { name, .. }) = cli.command {
            assert_eq!(name.as_deref(), Some("my-session"));
        } else {
            panic!("expected New command");
        }
    }

    #[test]
    fn test_parse_new_requires_prompt() {
        let result = Cli::try_parse_from(["azureal", "new"]);
        assert!(result.is_err());
    }

    // ── Cli parsing: status shortcut ──

    #[test]
    fn test_parse_status() {
        let cli = Cli::try_parse_from(["azureal", "status", "my-session"]).unwrap();
        if let Some(Commands::Status { session }) = cli.command {
            assert_eq!(session, "my-session");
        } else {
            panic!("expected Status command");
        }
    }

    // ── Cli parsing: diff shortcut ──

    #[test]
    fn test_parse_diff() {
        let cli = Cli::try_parse_from(["azureal", "diff", "feat"]).unwrap();
        if let Some(Commands::Diff { session, stat }) = cli.command {
            assert_eq!(session, "feat");
            assert!(!stat);
        } else {
            panic!("expected Diff command");
        }
    }

    #[test]
    fn test_parse_diff_stat() {
        let cli = Cli::try_parse_from(["azureal", "diff", "feat", "--stat"]).unwrap();
        if let Some(Commands::Diff { stat, .. }) = cli.command {
            assert!(stat);
        } else {
            panic!("expected Diff command");
        }
    }

    // ── Cli parsing: session subcommands ──

    #[test]
    fn test_parse_session_list() {
        let cli = Cli::try_parse_from(["azureal", "session", "list"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Session(SessionCommands::List { .. }))));
    }

    #[test]
    fn test_parse_session_ls_alias() {
        let cli = Cli::try_parse_from(["azureal", "session", "ls"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Session(SessionCommands::List { .. }))));
    }

    #[test]
    fn test_parse_session_stop() {
        let cli = Cli::try_parse_from(["azureal", "session", "stop", "feat"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Stop { session, force })) = cli.command {
            assert_eq!(session, "feat");
            assert!(!force);
        } else {
            panic!("expected Session Stop");
        }
    }

    #[test]
    fn test_parse_session_stop_force() {
        let cli = Cli::try_parse_from(["azureal", "session", "stop", "feat", "-f"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Stop { force, .. })) = cli.command {
            assert!(force);
        } else {
            panic!("expected Session Stop");
        }
    }

    #[test]
    fn test_parse_session_delete() {
        let cli = Cli::try_parse_from(["azureal", "session", "delete", "feat"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Delete { session, yes })) = cli.command {
            assert_eq!(session, "feat");
            assert!(!yes);
        } else {
            panic!("expected Session Delete");
        }
    }

    #[test]
    fn test_parse_session_delete_yes() {
        let cli = Cli::try_parse_from(["azureal", "session", "delete", "feat", "-y"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Delete { yes, .. })) = cli.command {
            assert!(yes);
        } else {
            panic!("expected Session Delete");
        }
    }

    #[test]
    fn test_parse_session_archive() {
        let cli = Cli::try_parse_from(["azureal", "session", "archive", "feat"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Session(SessionCommands::Archive { .. }))));
    }

    #[test]
    fn test_parse_session_unarchive() {
        let cli = Cli::try_parse_from(["azureal", "session", "unarchive", "feat"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Session(SessionCommands::Unarchive { .. }))));
    }

    #[test]
    fn test_parse_session_resume() {
        let cli = Cli::try_parse_from(["azureal", "session", "resume", "feat"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Resume { session, prompt })) = cli.command {
            assert_eq!(session, "feat");
            assert!(prompt.is_none());
        } else {
            panic!("expected Session Resume");
        }
    }

    #[test]
    fn test_parse_session_resume_with_prompt() {
        let cli = Cli::try_parse_from(["azureal", "session", "resume", "feat", "-p", "continue"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Resume { prompt, .. })) = cli.command {
            assert_eq!(prompt.as_deref(), Some("continue"));
        } else {
            panic!("expected Session Resume");
        }
    }

    #[test]
    fn test_parse_session_logs() {
        let cli = Cli::try_parse_from(["azureal", "session", "logs", "feat"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Logs { session, follow, lines })) = cli.command {
            assert_eq!(session, "feat");
            assert!(!follow);
            assert_eq!(lines, 50);
        } else {
            panic!("expected Session Logs");
        }
    }

    #[test]
    fn test_parse_session_logs_follow() {
        let cli = Cli::try_parse_from(["azureal", "session", "logs", "feat", "-f"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Logs { follow, .. })) = cli.command {
            assert!(follow);
        } else {
            panic!("expected Session Logs");
        }
    }

    #[test]
    fn test_parse_session_logs_custom_lines() {
        let cli = Cli::try_parse_from(["azureal", "session", "logs", "feat", "-l", "100"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Logs { lines, .. })) = cli.command {
            assert_eq!(lines, 100);
        } else {
            panic!("expected Session Logs");
        }
    }

    #[test]
    fn test_parse_session_cleanup() {
        let cli = Cli::try_parse_from(["azureal", "session", "cleanup"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Cleanup { project, delete_branches, yes, dry_run })) = cli.command {
            assert!(project.is_none());
            assert!(!delete_branches);
            assert!(!yes);
            assert!(!dry_run);
        } else {
            panic!("expected Session Cleanup");
        }
    }

    #[test]
    fn test_parse_session_cleanup_all_flags() {
        let cli = Cli::try_parse_from(["azureal", "session", "cleanup", "--delete-branches", "-y", "--dry-run"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Cleanup { delete_branches, yes, dry_run, .. })) = cli.command {
            assert!(delete_branches);
            assert!(yes);
            assert!(dry_run);
        } else {
            panic!("expected Session Cleanup");
        }
    }

    // ── Cli parsing: project subcommands ──

    #[test]
    fn test_parse_project_list() {
        let cli = Cli::try_parse_from(["azureal", "project", "list"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Project(ProjectCommands::List))));
    }

    #[test]
    fn test_parse_project_ls_alias() {
        let cli = Cli::try_parse_from(["azureal", "project", "ls"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Project(ProjectCommands::List))));
    }

    #[test]
    fn test_parse_project_show() {
        let cli = Cli::try_parse_from(["azureal", "project", "show"]).unwrap();
        if let Some(Commands::Project(ProjectCommands::Show { project })) = cli.command {
            assert!(project.is_none());
        } else {
            panic!("expected Project Show");
        }
    }

    #[test]
    fn test_parse_project_show_with_arg() {
        let cli = Cli::try_parse_from(["azureal", "project", "show", "/path"]).unwrap();
        if let Some(Commands::Project(ProjectCommands::Show { project })) = cli.command {
            assert_eq!(project.as_deref(), Some("/path"));
        } else {
            panic!("expected Project Show");
        }
    }

    #[test]
    fn test_parse_project_remove() {
        let cli = Cli::try_parse_from(["azureal", "project", "remove", "proj"]).unwrap();
        if let Some(Commands::Project(ProjectCommands::Remove { project, yes })) = cli.command {
            assert_eq!(project, "proj");
            assert!(!yes);
        } else {
            panic!("expected Project Remove");
        }
    }

    #[test]
    fn test_parse_project_config() {
        let cli = Cli::try_parse_from(["azureal", "project", "config"]).unwrap();
        if let Some(Commands::Project(ProjectCommands::Config { project, main_branch })) = cli.command {
            assert!(project.is_none());
            assert!(main_branch.is_none());
        } else {
            panic!("expected Project Config");
        }
    }

    #[test]
    fn test_parse_project_config_with_branch() {
        let cli = Cli::try_parse_from(["azureal", "project", "config", "--main-branch", "develop"]).unwrap();
        if let Some(Commands::Project(ProjectCommands::Config { main_branch, .. })) = cli.command {
            assert_eq!(main_branch.as_deref(), Some("develop"));
        } else {
            panic!("expected Project Config");
        }
    }

    // ── global flags propagate to subcommands ──

    #[test]
    fn test_global_verbose_with_subcommand() {
        let cli = Cli::try_parse_from(["azureal", "-v", "tui"]).unwrap();
        assert!(cli.verbose);
        assert!(matches!(cli.command, Some(Commands::Tui)));
    }

    #[test]
    fn test_global_output_with_subcommand() {
        let cli = Cli::try_parse_from(["azureal", "-o", "json", "list"]).unwrap();
        assert!(matches!(cli.output, OutputFormat::Json));
    }

    // ── invalid subcommands ──

    #[test]
    fn test_parse_invalid_subcommand() {
        let result = Cli::try_parse_from(["azureal", "invalid"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_session_invalid_subcommand() {
        let result = Cli::try_parse_from(["azureal", "session", "invalid"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_project_invalid_subcommand() {
        let result = Cli::try_parse_from(["azureal", "project", "invalid"]);
        assert!(result.is_err());
    }
}
