// AZUREAL: multi-agent development environment
mod app;
mod azufig;
mod claude;
mod cli;
mod cmd;
mod config;
mod events;
mod git;
mod github;
mod models;
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
    let cli = Cli::parse();

    let log_level = if cli.verbose { "azureal=debug" } else { "azureal=info" };

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
            .and_then(|p| p.canonicalize())
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
                let dst_mod = std::fs::metadata(&bundle_exec).and_then(|m| m.modified()).ok();
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
                contents.join("Resources/Azureal.icns"),
                include_bytes!("../resources/Azureal.icns"),
            );
            let _ = std::fs::write(contents.join("Info.plist"), concat!(
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
            ));
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
        if !already_in_bundle && bundle_exec.exists() && std::env::var_os("AZUREAL_REEXEC").is_none() {
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
        struct ProcessSerialNumber { high: u32, low: u32 }
        extern "C" {
            fn TransformProcessType(psn: *mut ProcessSerialNumber, ty: u32) -> i32;
        }
        unsafe {
            let mut psn = ProcessSerialNumber { high: 0, low: kCurrentProcess };
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
                .args(["-c", concat!(
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
                )])
                .output();
            let _ = std::fs::write(&notif_marker, "1");
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
            SessionCommands::Unarchive { session } => cmd::handle_session_unarchive(&session)?,
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
