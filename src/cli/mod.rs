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
#[command(name = "azural")]
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
