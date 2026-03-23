// AZUREAL: multi-agent development environment
mod app;
mod azufig;
mod backend;
mod claude;
mod cli;
mod cmd;
mod codex;
mod config;
mod events;
mod git;
mod install;
mod models;
mod updater;
mod stt;
mod syntax;
mod tui;
mod watcher;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use cli::{Cli, Commands, ProjectCommands, SessionCommands};

#[tokio::main]
async fn main() -> Result<()> {
    // Self-install: if running from outside PATH (e.g. ~/Downloads), copy binary
    // to a PATH directory and exit. Skips for cargo builds (target/debug|release).
    if install::maybe_self_install() {
        return Ok(());
    }

    let cli = Cli::parse();

    let log_level = if cli.verbose {
        "azureal=debug"
    } else {
        "azureal=info"
    };

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| log_level.to_string()),
        ))
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    config::ensure_config_dir()?;

    // Create a .app bundle in ~/.azureal/ so macOS shows the Azureal icon
    // in notifications AND Activity Monitor. The .icns is compiled into the
    // binary via include_bytes!() — zero external files needed after install.
    //
    // Activity Monitor resolves icons by calling proc_pidpath() on the running
    // process, then walking UP the directory tree looking for a .app bundle.
    // Symlinks don't work because proc_pidpath() resolves them to the real path.
    // So we COPY the binary into the bundle and re-exec through that copy.
    // After re-exec, proc_pidpath() returns ~/.azureal/AZUREAL.app/Contents/MacOS/azureal
    // and Activity Monitor finds our icon.
    #[cfg(target_os = "macos")]
    {
        let bundle_dir = config::config_dir().join("AZUREAL.app");
        let contents = bundle_dir.join("Contents");
        let bundle_exec = contents.join("MacOS/azureal");
        let exe = std::env::current_exe()
            .and_then(|p| dunce::canonicalize(&p).map_err(Into::into))
            .unwrap_or_default();

        // Are we already running from inside the bundle? If so, skip re-exec.
        let already_in_bundle = exe.starts_with(&bundle_dir);

        // Rebuild bundle if plist is missing
        let needs_create = !contents.join("Info.plist").exists();
        // Re-copy binary if the source changed (e.g., cargo install to new location)
        // or if the bundle executable is older than the source binary
        let needs_copy = if already_in_bundle {
            false
        } else {
            !bundle_exec.exists() || {
                let src_mod = std::fs::metadata(&exe).and_then(|m| m.modified()).ok();
                let dst_mod = std::fs::metadata(&bundle_exec)
                    .and_then(|m| m.modified())
                    .ok();
                match (src_mod, dst_mod) {
                    (Some(s), Some(d)) => s > d,
                    _ => true,
                }
            }
        };

        if needs_create {
            let _ = std::fs::create_dir_all(contents.join("MacOS"));
            let _ = std::fs::create_dir_all(contents.join("Resources"));
            let _ = std::fs::write(
                contents.join("Resources/AZUREAL.icns"),
                include_bytes!("../resources/AZUREAL.icns"),
            );
            let _ = std::fs::write(
                contents.join("Info.plist"),
                concat!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                    "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ",
                    "\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
                    "<plist version=\"1.0\">\n<dict>\n",
                    "\t<key>CFBundleIdentifier</key>\n\t<string>com.xcorvisx.azureal</string>\n",
                    "\t<key>CFBundleName</key>\n\t<string>AZUREAL</string>\n",
                    "\t<key>CFBundleDisplayName</key>\n\t<string>AZUREAL</string>\n",
                    "\t<key>CFBundleExecutable</key>\n\t<string>azureal</string>\n",
                    "\t<key>CFBundleIconFile</key>\n\t<string>Azureal</string>\n",
                    "\t<key>CFBundlePackageType</key>\n\t<string>APPL</string>\n",
                    "\t<key>LSUIElement</key>\n\t<true/>\n",
                    "</dict>\n</plist>\n",
                ),
            );
        }

        // Copy the real binary into the bundle so proc_pidpath() resolves
        // to inside the .app — this is what makes Activity Monitor show our icon.
        // After copying, re-sign ad-hoc so the bundle passes codesign validation
        // (the source binary has a linker ad-hoc signature that references no
        // resources, but inside a .app bundle macOS expects consistency).
        if needs_create || needs_copy {
            let _ = std::fs::copy(&exe, &bundle_exec);
            let _ = std::process::Command::new("codesign")
                .args(["--force", "--sign", "-", &bundle_dir.to_string_lossy()])
                .output();
            let _ = std::process::Command::new(
                "/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister"
            ).args(["-f", &bundle_dir.to_string_lossy()]).output();
        }

        // Re-exec through the bundle copy so this process's proc_pidpath()
        // returns a path inside the .app — making Activity Monitor show our icon.
        // AZUREAL_REEXEC env var prevents infinite loop: the re-exec'd process
        // sees it and skips this block. Command::exec() replaces the current
        // process (unix execvp), so if it succeeds we never reach the next line.
        if !already_in_bundle
            && bundle_exec.exists()
            && std::env::var_os("AZUREAL_REEXEC").is_none()
        {
            use std::os::unix::process::CommandExt;
            let args: Vec<String> = std::env::args().skip(1).collect();
            let err = std::process::Command::new(&bundle_exec)
                .args(&args)
                .env("AZUREAL_REEXEC", "1")
                .exec();
            // exec() only returns on failure — fall through and run from original path
            tracing::debug!("re-exec failed: {}", err);
        }

        // Register with the macOS window server so NSRunningApplication (and
        // Activity Monitor) recognizes this process as a proper app with our
        // bundle's icon. Without this call, proc_pidpath() returns the right
        // path but the process isn't registered as a "user application" — so
        // Activity Monitor shows a generic icon. TransformProcessType(psn, 4)
        // registers us as a UI element app (same as LSUIElement=true: no Dock
        // icon, no menu bar, but Activity Monitor shows our icon).
        #[allow(non_upper_case_globals)]
        const kCurrentProcess: u32 = 2;
        #[repr(C)]
        struct ProcessSerialNumber {
            high: u32,
            low: u32,
        }
        extern "C" {
            fn TransformProcessType(psn: *mut ProcessSerialNumber, ty: u32) -> i32;
        }
        unsafe {
            let mut psn = ProcessSerialNumber {
                high: 0,
                low: kCurrentProcess,
            };
            // 4 = kProcessTransformToUIElementAppType
            TransformProcessType(&mut psn, 4);
        }

        // Pre-enable notification permissions on first launch so the user
        // gets completion alerts immediately. TransformProcessType (above)
        // registers us with the window server, which causes macOS to create
        // an ncprefs entry with notifications DISABLED by default. We flip
        // the flags to enabled using Python's plistlib (the only reliable
        // way to edit macOS binary plists). A marker file ensures this
        // runs only once — users who later disable notifications in System
        // Settings won't have their preference overridden.
        let notif_marker = config::config_dir().join(".notif_enabled");
        if !notif_marker.exists() {
            // Small delay so TransformProcessType's ncprefs entry is flushed
            std::thread::sleep(std::time::Duration::from_millis(200));
            let _ = std::process::Command::new("python3")
                .args([
                    "-c",
                    concat!(
                        "import plistlib,os,pathlib\n",
                        "p=pathlib.Path.home()/'Library/Preferences/com.apple.ncprefs.plist'\n",
                        "if not p.exists(): exit()\n",
                        "d=plistlib.loads(p.read_bytes())\n",
                        "bid='com.xcorvisx.azureal'\n",
                        "found=False\n",
                        "for app in d.get('apps',[]):\n",
                        "  if app.get('bundle-id')==bid:\n",
                        // 41951246 = ALLOW_NOTIFICATIONS|BANNERS|SOUND|BADGE|PREVIEW_ALWAYS|BIT_23
                        "    app['flags']=41951246;found=True;break\n",
                        "if not found:\n",
                        "  d.setdefault('apps',[]).append({'bundle-id':bid,'flags':41951246})\n",
                        "p.write_bytes(plistlib.dumps(d,fmt=plistlib.FMT_BINARY))\n",
                        "os.system('killall usernoted 2>/dev/null')\n",
                    ),
                ])
                .output();
            let _ = std::fs::write(&notif_marker, "1");
        }

        let _ = notify_rust::set_application("com.xcorvisx.azureal");
    }

    // Write the embedded .ico to ~/.azureal/ for toast notifications and
    // install a Windows Terminal profile fragment so the tab shows our icon.
    // GetConsoleWindow() returns null in WT (ConPTY pseudo-console has no
    // window), so WM_SETICON cannot work — fragments are the supported way.
    #[cfg(target_os = "windows")]
    {
        let ico_path = config::config_dir().join("AZUREAL.ico");
        if !ico_path.exists() {
            let _ = std::fs::write(&ico_path, include_bytes!("../resources/AZUREAL.ico"));
        }

        // Extract toast notification icon (PNG renders crisper than .ico in toasts)
        let toast_png = config::config_dir().join("AZUREAL_toast.png");
        if !toast_png.exists() {
            let _ = std::fs::write(&toast_png, include_bytes!("../resources/AZUREAL_toast.png"));
        }

        // Install WT profile fragment (always rewritten to pick up icon/exe changes)
        if let Some(local_app) = std::env::var_os("LOCALAPPDATA") {
            let frag_dir = std::path::PathBuf::from(local_app)
                .join("Microsoft")
                .join("Windows Terminal")
                .join("Fragments")
                .join("Azureal");
            let frag_path = frag_dir.join("azureal.json");
            let icon = toast_png.to_string_lossy().replace('\\', "/");
            let exe = std::env::current_exe()
                .unwrap_or_default()
                .to_string_lossy()
                .replace('\\', "/");
            let json = format!(
                r#"{{"profiles":[{{"name":"Azureal","commandline":"\"{}\"","icon":"{}","hidden":false}}]}}"#,
                exe, icon
            );
            let _ = std::fs::create_dir_all(&frag_dir);
            let _ = std::fs::write(&frag_path, json);
        }
    }

    let output_format = cli.output;

    match cli.command {
        Some(Commands::Tui) | None => tui::run().await?,

        // Session shortcuts
        Some(Commands::List { project, all }) => {
            cmd::handle_session_list(project, all, output_format)?
        }
        Some(Commands::New {
            prompt,
            project,
            name,
        }) => cmd::handle_session_new(prompt, project, name, output_format)?,
        Some(Commands::Status { session }) => cmd::handle_session_status(&session, output_format)?,
        Some(Commands::Diff { session, stat }) => cmd::handle_session_diff(&session, stat)?,

        // Session subcommands
        Some(Commands::Session(cmd)) => match cmd {
            SessionCommands::List { project, all } => {
                cmd::handle_session_list(project, all, output_format)?
            }
            SessionCommands::New {
                prompt,
                project,
                name,
            } => cmd::handle_session_new(prompt, project, name, output_format)?,
            SessionCommands::Status { session } => {
                cmd::handle_session_status(&session, output_format)?
            }
            SessionCommands::Stop { session, force } => cmd::handle_session_stop(&session, force)?,
            SessionCommands::Delete { session, yes } => cmd::handle_session_delete(&session, yes)?,
            SessionCommands::Archive { session } => cmd::handle_session_archive(&session)?,
            SessionCommands::Unarchive { session } => cmd::handle_session_unarchive(&session)?,
            SessionCommands::Resume { session, prompt } => {
                cmd::handle_session_resume(&session, prompt)?
            }
            SessionCommands::Logs {
                session,
                follow,
                lines,
            } => cmd::handle_session_logs(&session, follow, lines)?,
            SessionCommands::Diff { session, stat } => cmd::handle_session_diff(&session, stat)?,
            SessionCommands::Cleanup {
                project,
                delete_branches,
                yes,
                dry_run,
            } => cmd::handle_session_cleanup(project, delete_branches, yes, dry_run)?,
        },

        // Project subcommands
        Some(Commands::Project(cmd)) => match cmd {
            ProjectCommands::List => cmd::handle_project_list(output_format)?,
            ProjectCommands::Show { project } => cmd::handle_project_show(project, output_format)?,
            ProjectCommands::Remove { project, yes } => cmd::handle_project_remove(&project, yes)?,
            ProjectCommands::Config {
                project,
                main_branch,
            } => cmd::handle_project_config(project, main_branch)?,
        },
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use clap::Parser as _;

    // ── CLI parsing: subcommand routing ──

    #[test]
    fn test_cli_no_args_defaults_to_none_command() {
        let cli = Cli::try_parse_from(["azureal"]).unwrap();
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_tui_command() {
        let cli = Cli::try_parse_from(["azureal", "tui"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Tui)));
    }

    #[test]
    fn test_cli_list_command() {
        let cli = Cli::try_parse_from(["azureal", "list"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::List { .. })));
    }

    #[test]
    fn test_cli_list_alias_ls() {
        let cli = Cli::try_parse_from(["azureal", "ls"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::List { .. })));
    }

    #[test]
    fn test_cli_new_command() {
        let cli = Cli::try_parse_from(["azureal", "new", "--prompt", "test prompt"]).unwrap();
        if let Some(Commands::New { prompt, .. }) = cli.command {
            assert_eq!(prompt, "test prompt");
        } else {
            panic!("expected New command");
        }
    }

    #[test]
    fn test_cli_status_command() {
        let cli = Cli::try_parse_from(["azureal", "status", "my-session"]).unwrap();
        if let Some(Commands::Status { session }) = cli.command {
            assert_eq!(session, "my-session");
        } else {
            panic!("expected Status command");
        }
    }

    #[test]
    fn test_cli_diff_command() {
        let cli = Cli::try_parse_from(["azureal", "diff", "my-session"]).unwrap();
        if let Some(Commands::Diff { session, stat }) = cli.command {
            assert_eq!(session, "my-session");
            assert!(!stat);
        } else {
            panic!("expected Diff command");
        }
    }

    #[test]
    fn test_cli_diff_with_stat_flag() {
        let cli = Cli::try_parse_from(["azureal", "diff", "my-session", "--stat"]).unwrap();
        if let Some(Commands::Diff { stat, .. }) = cli.command {
            assert!(stat);
        }
    }

    // ── CLI parsing: session subcommands ──

    #[test]
    fn test_cli_session_list() {
        let cli = Cli::try_parse_from(["azureal", "session", "list"]).unwrap();
        if let Some(Commands::Session(SessionCommands::List { .. })) = cli.command {
            // ok
        } else {
            panic!("expected Session List");
        }
    }

    #[test]
    fn test_cli_session_new() {
        let cli = Cli::try_parse_from(["azureal", "session", "new", "--prompt", "build feature"])
            .unwrap();
        if let Some(Commands::Session(SessionCommands::New { prompt, .. })) = cli.command {
            assert_eq!(prompt, "build feature");
        } else {
            panic!("expected Session New");
        }
    }

    #[test]
    fn test_cli_session_stop() {
        let cli = Cli::try_parse_from(["azureal", "session", "stop", "my-session"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Stop { session, force })) = cli.command {
            assert_eq!(session, "my-session");
            assert!(!force);
        } else {
            panic!("expected Session Stop");
        }
    }

    #[test]
    fn test_cli_session_stop_force() {
        let cli =
            Cli::try_parse_from(["azureal", "session", "stop", "my-session", "--force"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Stop { force, .. })) = cli.command {
            assert!(force);
        }
    }

    #[test]
    fn test_cli_session_delete() {
        let cli = Cli::try_parse_from(["azureal", "session", "delete", "my-session"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Delete { session, yes })) = cli.command {
            assert_eq!(session, "my-session");
            assert!(!yes);
        } else {
            panic!("expected Session Delete");
        }
    }

    #[test]
    fn test_cli_session_delete_yes() {
        let cli =
            Cli::try_parse_from(["azureal", "session", "delete", "my-session", "--yes"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Delete { yes, .. })) = cli.command {
            assert!(yes);
        }
    }

    #[test]
    fn test_cli_session_archive() {
        let cli = Cli::try_parse_from(["azureal", "session", "archive", "my-session"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Archive { session })) = cli.command {
            assert_eq!(session, "my-session");
        } else {
            panic!("expected Session Archive");
        }
    }

    #[test]
    fn test_cli_session_unarchive() {
        let cli = Cli::try_parse_from(["azureal", "session", "unarchive", "my-session"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Unarchive { session })) = cli.command {
            assert_eq!(session, "my-session");
        } else {
            panic!("expected Session Unarchive");
        }
    }

    #[test]
    fn test_cli_session_resume() {
        let cli = Cli::try_parse_from(["azureal", "session", "resume", "my-session"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Resume { session, prompt })) = cli.command {
            assert_eq!(session, "my-session");
            assert!(prompt.is_none());
        } else {
            panic!("expected Session Resume");
        }
    }

    #[test]
    fn test_cli_session_logs() {
        let cli = Cli::try_parse_from(["azureal", "session", "logs", "my-session"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Logs {
            session, follow, ..
        })) = cli.command
        {
            assert_eq!(session, "my-session");
            assert!(!follow);
        } else {
            panic!("expected Session Logs");
        }
    }

    #[test]
    fn test_cli_session_diff() {
        let cli = Cli::try_parse_from(["azureal", "session", "diff", "my-session"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Diff { session, stat })) = cli.command {
            assert_eq!(session, "my-session");
            assert!(!stat);
        } else {
            panic!("expected Session Diff");
        }
    }

    #[test]
    fn test_cli_session_cleanup() {
        let cli = Cli::try_parse_from(["azureal", "session", "cleanup"]).unwrap();
        if let Some(Commands::Session(SessionCommands::Cleanup {
            delete_branches,
            yes,
            dry_run,
            ..
        })) = cli.command
        {
            assert!(!delete_branches);
            assert!(!yes);
            assert!(!dry_run);
        } else {
            panic!("expected Session Cleanup");
        }
    }

    #[test]
    fn test_cli_session_cleanup_with_flags() {
        let cli = Cli::try_parse_from([
            "azureal",
            "session",
            "cleanup",
            "--delete-branches",
            "--yes",
            "--dry-run",
        ])
        .unwrap();
        if let Some(Commands::Session(SessionCommands::Cleanup {
            delete_branches,
            yes,
            dry_run,
            ..
        })) = cli.command
        {
            assert!(delete_branches);
            assert!(yes);
            assert!(dry_run);
        }
    }

    // ── CLI parsing: project subcommands ──

    #[test]
    fn test_cli_project_list() {
        let cli = Cli::try_parse_from(["azureal", "project", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Project(ProjectCommands::List))
        ));
    }

    #[test]
    fn test_cli_project_show() {
        let cli = Cli::try_parse_from(["azureal", "project", "show"]).unwrap();
        if let Some(Commands::Project(ProjectCommands::Show { project })) = cli.command {
            assert!(project.is_none());
        } else {
            panic!("expected Project Show");
        }
    }

    #[test]
    fn test_cli_project_show_with_path() {
        let cli = Cli::try_parse_from(["azureal", "project", "show", "/tmp/project"]).unwrap();
        if let Some(Commands::Project(ProjectCommands::Show { project })) = cli.command {
            assert_eq!(project, Some("/tmp/project".to_string()));
        }
    }

    #[test]
    fn test_cli_project_remove() {
        let cli = Cli::try_parse_from(["azureal", "project", "remove", "my-project"]).unwrap();
        if let Some(Commands::Project(ProjectCommands::Remove { project, yes })) = cli.command {
            assert_eq!(project, "my-project");
            assert!(!yes);
        } else {
            panic!("expected Project Remove");
        }
    }

    #[test]
    fn test_cli_project_config() {
        let cli = Cli::try_parse_from(["azureal", "project", "config"]).unwrap();
        if let Some(Commands::Project(ProjectCommands::Config {
            project,
            main_branch,
        })) = cli.command
        {
            assert!(project.is_none());
            assert!(main_branch.is_none());
        } else {
            panic!("expected Project Config");
        }
    }

    // ── CLI: global flags ──

    #[test]
    fn test_cli_verbose_flag() {
        let cli = Cli::try_parse_from(["azureal", "--verbose"]).unwrap();
        assert!(cli.verbose);
    }

    #[test]
    fn test_cli_verbose_default() {
        let cli = Cli::try_parse_from(["azureal"]).unwrap();
        assert!(!cli.verbose);
    }

    #[test]
    fn test_cli_output_format_default() {
        let cli = Cli::try_parse_from(["azureal"]).unwrap();
        assert!(matches!(cli.output, cli::OutputFormat::Table));
    }

    #[test]
    fn test_cli_output_format_json() {
        let cli = Cli::try_parse_from(["azureal", "--output", "json"]).unwrap();
        assert!(matches!(cli.output, cli::OutputFormat::Json));
    }

    #[test]
    fn test_cli_output_format_plain() {
        let cli = Cli::try_parse_from(["azureal", "--output", "plain"]).unwrap();
        assert!(matches!(cli.output, cli::OutputFormat::Plain));
    }

    #[test]
    fn test_cli_output_format_table() {
        let cli = Cli::try_parse_from(["azureal", "--output", "table"]).unwrap();
        assert!(matches!(cli.output, cli::OutputFormat::Table));
    }

    // ── CLI: command + global flag combinations ──

    #[test]
    fn test_cli_verbose_with_list() {
        let cli = Cli::try_parse_from(["azureal", "--verbose", "list"]).unwrap();
        assert!(cli.verbose);
        assert!(matches!(cli.command, Some(Commands::List { .. })));
    }

    #[test]
    fn test_cli_output_json_with_session_list() {
        let cli = Cli::try_parse_from(["azureal", "--output", "json", "session", "list"]).unwrap();
        assert!(matches!(cli.output, cli::OutputFormat::Json));
        if let Some(Commands::Session(SessionCommands::List { .. })) = cli.command {
            // ok
        } else {
            panic!("expected Session List");
        }
    }

    #[test]
    fn test_cli_list_all_flag() {
        let cli = Cli::try_parse_from(["azureal", "list", "--all"]).unwrap();
        if let Some(Commands::List { all, .. }) = cli.command {
            assert!(all);
        }
    }

    #[test]
    fn test_cli_list_project_filter() {
        let cli = Cli::try_parse_from(["azureal", "list", "--project", "my-proj"]).unwrap();
        if let Some(Commands::List { project, .. }) = cli.command {
            assert_eq!(project, Some("my-proj".to_string()));
        }
    }

    // ── CLI: invalid subcommands ──

    #[test]
    fn test_cli_invalid_subcommand() {
        let result = Cli::try_parse_from(["azureal", "nonexistent"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_session_invalid_subcommand() {
        let result = Cli::try_parse_from(["azureal", "session", "invalid"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_project_invalid_subcommand() {
        let result = Cli::try_parse_from(["azureal", "project", "invalid"]);
        assert!(result.is_err());
    }

    // ── CLI: missing required args ──

    #[test]
    fn test_cli_new_missing_prompt() {
        let result = Cli::try_parse_from(["azureal", "new"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_status_missing_session() {
        let result = Cli::try_parse_from(["azureal", "status"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_diff_missing_session() {
        let result = Cli::try_parse_from(["azureal", "diff"]);
        assert!(result.is_err());
    }

    // ── Module existence verification ──
    // These tests verify that all expected modules are accessible

    #[test]
    fn test_models_module_accessible() {
        let _ = models::BRANCH_PREFIX;
    }

    #[test]
    fn test_config_module_accessible() {
        let _ = config::Config::default();
    }

    #[test]
    fn test_cli_module_accessible() {
        let _ = cli::OutputFormat::default();
    }

    #[test]
    fn test_models_strip_branch_prefix() {
        assert_eq!(models::strip_branch_prefix("azureal/test"), "test");
    }

    #[test]
    fn test_config_dir_exists() {
        let dir = config::config_dir();
        assert!(dir.to_string_lossy().contains(".azureal"));
    }

    #[test]
    fn test_cli_version_flag() {
        let result = Cli::try_parse_from(["azureal", "--version"]);
        assert!(result.is_err()); // --version causes early exit
    }

    #[test]
    fn test_cli_unknown_subcommand() {
        let result = Cli::try_parse_from(["azureal", "nonexistent"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_dir_is_absolute() {
        let dir = config::config_dir();
        assert!(dir.is_absolute());
    }
}
