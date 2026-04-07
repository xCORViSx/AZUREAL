//! Self-install logic — copies binary to PATH on first run

use std::path::Path;
#[cfg(not(target_os = "macos"))]
use std::path::PathBuf;

/// Check if the binary needs installation. If so, install and return true (caller should exit).
/// Returns false for normal startup (already installed or running from cargo build dir).
pub fn maybe_self_install() -> bool {
    let exe =
        match std::env::current_exe().and_then(|p| dunce::canonicalize(&p).map_err(Into::into)) {
            Ok(p) => p,
            Err(_) => return false,
        };

    // Skip if running from a cargo build directory (development)
    let exe_str = exe.to_string_lossy();
    if exe_str.contains("target/debug")
        || exe_str.contains("target/release")
        || exe_str.contains("target\\debug")
        || exe_str.contains("target\\release")
    {
        return false;
    }

    // Skip if already in a PATH directory
    if is_in_path(&exe) {
        return false;
    }

    // Skip if already at the install destination (exec'd after first install,
    // but PATH hasn't been sourced yet in this shell session)
    #[cfg(not(target_os = "macos"))]
    {
        let install_dir = pick_install_dir();
        let install_path = install_dir.join(if cfg!(windows) {
            "azureal.exe"
        } else {
            "azureal"
        });
        if exe == install_path {
            return false;
        }
        // Also check with canonicalization for symlinks
        if install_path.exists() {
            if let Ok(canon) = dunce::canonicalize(&install_path) {
                if exe == canon {
                    return false;
                }
            }
        }
    }

    // Skip if `azureal` is already findable in PATH (installed elsewhere)
    let bin_name = if cfg!(windows) {
        "azureal.exe"
    } else {
        "azureal"
    };
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            if dir.join(bin_name).is_file() {
                return false;
            }
        }
    }

    do_install(&exe)
}

/// Check if the exe's parent directory is already in PATH.
fn is_in_path(exe: &Path) -> bool {
    let exe_dir = match exe.parent() {
        Some(d) => d,
        None => return false,
    };

    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            // Compare canonical paths to handle symlinks
            if let (Ok(a), Ok(b)) = (dunce::canonicalize(&dir), dunce::canonicalize(exe_dir)) {
                if a == b {
                    return true;
                }
            }
        }
    }
    false
}

/// Perform the self-installation. Returns true (caller should exit after).
fn do_install(exe: &Path) -> bool {
    println!();
    //                  1234567890123456789012345678901234567890
    println!("  \x1b[36m╔════════════════════════════════════════╗\x1b[0m");
    println!(
        "  \x1b[36m║\x1b[0m       \x1b[1;36mAZUREAL\x1b[0m  —  First Run Setup      \x1b[36m║\x1b[0m"
    );
    println!("  \x1b[36m╚════════════════════════════════════════╝\x1b[0m");
    println!();

    // macOS: install binary into .app bundle, write shell script trampoline to PATH.
    // The real binary lives only inside the bundle so Activity Monitor shows our icon
    // via proc_pidpath() → .app bundle lookup. The PATH entry is a ~100 byte script.
    #[cfg(target_os = "macos")]
    {
        if !install_macos(exe) {
            wait_for_enter();
            return true;
        }
        println!("  \x1b[32m✓ Installed successfully!\x1b[0m");
        println!();
        println!("  \x1b[36mLaunching AZUREAL...\x1b[0m");
        println!();

        let bundle_exec = dirs::home_dir()
            .unwrap_or_default()
            .join(".azureal/AZUREAL.app/Contents/MacOS/azureal");
        let args: Vec<String> = std::env::args().skip(1).collect();
        let err = exec_binary(&bundle_exec, &args);
        eprintln!("  \x1b[31mFailed to launch: {}\x1b[0m", err);
        println!("  \x1b[33mRun manually:\x1b[0m  azureal");
        println!();
        wait_for_enter();
        return true;
    }

    // Windows/Linux: copy binary directly to PATH
    #[cfg(not(target_os = "macos"))]
    {
        let install_dir = pick_install_dir();
        let install_path = install_dir.join(if cfg!(windows) {
            "azureal.exe"
        } else {
            "azureal"
        });

        println!("  Installing to: \x1b[1m{}\x1b[0m", install_path.display());
        println!();

        if !install_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&install_dir) {
                eprintln!("  \x1b[31mFailed to create directory: {}\x1b[0m", e);
                wait_for_enter();
                return true;
            }
        }

        if !try_copy(exe, &install_path) {
            wait_for_enter();
            return true;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&install_path, std::fs::Permissions::from_mode(0o755));
        }

        let _path_updated = ensure_in_path(&install_dir);
        let in_path_now = is_in_path_dir(&install_dir);

        println!("  \x1b[32m✓ Installed successfully!\x1b[0m");
        println!();

        println!();

        // Exec the installed binary directly — the user ran us expecting the app,
        // so launch it instead of making them type the command again.
        // The new process runs from the install dir, so is_in_path() returns true
        // and it skips installation, proceeding to normal TUI startup.
        println!("  \x1b[36mLaunching AZUREAL...\x1b[0m");
        println!();

        let args: Vec<String> = std::env::args().skip(1).collect();
        let err = exec_binary(&install_path, &args);
        // exec_binary only returns on failure
        eprintln!("  \x1b[31mFailed to launch: {}\x1b[0m", err);

        if !in_path_now {
            let shell = std::env::var("SHELL").unwrap_or_default();
            let profile_name = if shell.contains("zsh") {
                ".zshrc"
            } else if shell.contains("fish") {
                ".profile"
            } else {
                ".bashrc"
            };
            println!();
            println!(
                "  \x1b[33mTo activate PATH, run:\x1b[0m  source ~/{}",
                profile_name
            );
        }
        println!("  \x1b[33mThen run:\x1b[0m  azureal");
        println!();

        wait_for_enter();
        true
    }
}

/// macOS install: copy binary into .app bundle, write shell script to PATH.
#[cfg(target_os = "macos")]
fn install_macos(exe: &Path) -> bool {
    let config_dir = dirs::home_dir().unwrap_or_default().join(".azureal");
    let bundle_dir = config_dir.join("AZUREAL.app");
    let contents = bundle_dir.join("Contents");
    let bundle_exec = contents.join("MacOS/azureal");

    println!("  Installing to: \x1b[1m{}\x1b[0m", bundle_exec.display());
    println!("  Trampoline:    \x1b[1m/usr/local/bin/azureal\x1b[0m");
    println!();

    // Create bundle structure
    if let Err(e) = std::fs::create_dir_all(contents.join("MacOS")) {
        eprintln!("  \x1b[31mFailed to create bundle: {}\x1b[0m", e);
        return false;
    }
    let _ = std::fs::create_dir_all(contents.join("Resources"));

    // Write bundle metadata
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
            "\t<key>CFBundleIconFile</key>\n\t<string>AZUREAL</string>\n",
            "\t<key>CFBundlePackageType</key>\n\t<string>APPL</string>\n",
            "\t<key>LSUIElement</key>\n\t<true/>\n",
            "</dict>\n</plist>\n",
        ),
    );

    // Copy binary into bundle
    if !try_copy(exe, &bundle_exec) {
        return false;
    }
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&bundle_exec, std::fs::Permissions::from_mode(0o755));
    }

    // Codesign and register with LaunchServices
    let _ = std::process::Command::new("codesign")
        .args(["--force", "--sign", "-", &bundle_dir.to_string_lossy()])
        .output();
    let _ = std::process::Command::new(
        "/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister"
    ).args(["-f", &bundle_dir.to_string_lossy()]).output();

    // Write shell script trampoline to PATH
    let trampoline = format!("#!/bin/sh\nexec \"{}\" \"$@\"\n", bundle_exec.display());
    let trampoline_path = Path::new("/usr/local/bin/azureal");

    // Try direct write, fall back to sudo
    let wrote = if std::fs::write(trampoline_path, &trampoline).is_ok() {
        true
    } else {
        println!("  \x1b[33mRequires elevated permissions. Requesting sudo...\x1b[0m");
        let status = std::process::Command::new("sudo")
            .args(["tee", &trampoline_path.to_string_lossy()])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(ref mut stdin) = child.stdin {
                    stdin.write_all(trampoline.as_bytes())?;
                }
                child.wait()
            });
        matches!(status, Ok(s) if s.success())
    };

    if !wrote {
        eprintln!("  \x1b[31mFailed to write trampoline to /usr/local/bin/azureal\x1b[0m");
        eprintln!("  Create it manually:");
        eprintln!("    echo '#!/bin/sh' | sudo tee /usr/local/bin/azureal");
        eprintln!(
            "    echo 'exec \"{}\" \"$@\"' | sudo tee -a /usr/local/bin/azureal",
            bundle_exec.display()
        );
        eprintln!("    sudo chmod +x /usr/local/bin/azureal");
        return false;
    }

    // Make trampoline executable
    let _ = std::process::Command::new("sudo")
        .args(["chmod", "+x", &trampoline_path.to_string_lossy()])
        .status();

    true
}

/// Pick the best install directory for this platform.
#[cfg(not(target_os = "macos"))]
fn pick_install_dir() -> PathBuf {
    if cfg!(windows) {
        // Windows: ~/.azureal/bin/ (user-writable, no UAC)
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("C:\\"))
            .join(".azureal")
            .join("bin")
    } else {
        // macOS/Linux: prefer /usr/local/bin if writable, else ~/.local/bin
        let usr_local = PathBuf::from("/usr/local/bin");
        if usr_local.exists() && is_dir_writable(&usr_local) {
            usr_local
        } else {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".local")
                .join("bin")
        }
    }
}

#[cfg(not(target_os = "macos"))]
/// Test if a directory is writable by creating a temp file.
fn is_dir_writable(path: &Path) -> bool {
    let test = path.join(".azureal_write_test");
    match std::fs::write(&test, b"") {
        Ok(_) => {
            let _ = std::fs::remove_file(&test);
            true
        }
        Err(_) => false,
    }
}

/// Try to copy the binary. Falls back to sudo on Unix if direct copy fails.
fn try_copy(src: &Path, dst: &Path) -> bool {
    match std::fs::copy(src, dst) {
        Ok(_) => true,
        Err(_) if cfg!(unix) => {
            println!("  \x1b[33mRequires elevated permissions. Requesting sudo...\x1b[0m");
            let status = std::process::Command::new("sudo")
                .args(["cp", &src.to_string_lossy(), &dst.to_string_lossy()])
                .status();
            match status {
                Ok(s) if s.success() => {
                    // Also set permissions via sudo
                    let _ = std::process::Command::new("sudo")
                        .args(["chmod", "+x", &dst.to_string_lossy()])
                        .status();
                    true
                }
                _ => {
                    eprintln!("  \x1b[31mInstallation failed. Try manually:\x1b[0m");
                    eprintln!("    sudo cp {} {}", src.display(), dst.display());
                    false
                }
            }
        }
        Err(e) => {
            eprintln!("  \x1b[31mInstallation failed: {}\x1b[0m", e);
            false
        }
    }
}

/// Check if a directory is currently in PATH (runtime check).
fn is_in_path_dir(dir: &Path) -> bool {
    if let Some(path_var) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path_var) {
            if let (Ok(a), Ok(b)) = (dunce::canonicalize(&d), dunce::canonicalize(dir)) {
                if a == b {
                    return true;
                }
            }
            // Also check without canonicalization (dir may not exist yet)
            if d == dir {
                return true;
            }
        }
    }
    false
}

#[cfg(not(target_os = "macos"))]
/// Ensure the install directory is in PATH. Returns true if PATH was modified.
fn ensure_in_path(dir: &Path) -> bool {
    // Check if already in PATH
    let dir_str = dir.to_string_lossy();
    if let Some(path_var) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path_var) {
            if d == dir {
                return false;
            }
        }
    }

    if cfg!(windows) {
        add_to_windows_path(&dir_str)
    } else {
        add_to_shell_profile(dir)
    }
}

/// Add a directory to the Windows user PATH via PowerShell registry API.
#[cfg(windows)]
fn add_to_windows_path(dir: &str) -> bool {
    // Use PowerShell to safely modify user PATH without the setx 1024-char limit
    let script = format!(
        "$p = [Environment]::GetEnvironmentVariable('PATH', 'User'); \
         if ($p -notlike '*{}*') {{ \
             [Environment]::SetEnvironmentVariable('PATH', \"$p;{}\", 'User') \
         }}",
        dir, dir
    );
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status();
    match status {
        Ok(s) if s.success() => {
            println!("  Added to user PATH: {}", dir);
            true
        }
        _ => {
            eprintln!("  \x1b[33mCould not update PATH automatically.\x1b[0m");
            eprintln!("  Add this directory to your PATH manually: {}", dir);
            true
        }
    }
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn add_to_windows_path(_dir: &str) -> bool {
    false
}

/// Append an export line to the user's shell profile (~/.zshrc, ~/.bashrc, etc.).
#[cfg(not(target_os = "macos"))]
fn add_to_shell_profile(dir: &Path) -> bool {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return false,
    };

    let dir_str = dir.to_string_lossy();
    let export_line = format!(
        "\n# Added by AZUREAL installer\nexport PATH=\"{}:$PATH\"\n",
        dir_str
    );

    // Pick the right shell profile
    let shell = std::env::var("SHELL").unwrap_or_default();
    let profile = if shell.contains("zsh") {
        home.join(".zshrc")
    } else if shell.contains("fish") {
        // fish uses different syntax — fall back to .profile
        home.join(".profile")
    } else {
        home.join(".bashrc")
    };

    // Check if already present
    if let Ok(content) = std::fs::read_to_string(&profile) {
        if content.contains(&*dir_str) {
            return false;
        }
    }

    // Append export line
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&profile)
    {
        use std::io::Write;
        if f.write_all(export_line.as_bytes()).is_ok() {
            println!("  Added to {}", profile.display());
            return true;
        }
    }

    false
}

/// Replace the current process with the installed binary.
/// On Unix, uses execv (never returns on success). On Windows, spawns and exits.
fn exec_binary(path: &Path, args: &[String]) -> String {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let c_path = match CString::new(path.as_os_str().as_bytes()) {
            Ok(p) => p,
            Err(e) => return format!("invalid path: {}", e),
        };

        let mut c_args = vec![c_path.clone()];
        for arg in args {
            match CString::new(arg.as_bytes()) {
                Ok(a) => c_args.push(a),
                Err(e) => return format!("invalid arg: {}", e),
            }
        }

        // execv replaces this process entirely — only returns on error
        nix_execv(&c_path, &c_args)
    }

    #[cfg(windows)]
    {
        match std::process::Command::new(path).args(args).status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(e) => format!("{}", e),
        }
    }
}

/// Wrapper for libc::execv to keep unsafe contained.
#[cfg(unix)]
fn nix_execv(path: &std::ffi::CString, args: &[std::ffi::CString]) -> String {
    let c_ptrs: Vec<*const libc::c_char> = args
        .iter()
        .map(|a| a.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect();

    unsafe {
        libc::execv(path.as_ptr(), c_ptrs.as_ptr());
    }
    // execv only returns on error
    std::io::Error::last_os_error().to_string()
}

/// Block until the user presses Enter.
fn wait_for_enter() {
    use std::io::Read;
    println!("  Press Enter to exit...");
    let _ = std::io::stdin().read(&mut [0u8]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_in_path_with_current_exe() {
        // The test binary IS in the cargo target dir which IS in PATH for the test runner
        // Just verify the function doesn't panic
        let exe = std::env::current_exe().unwrap();
        let _ = is_in_path(&exe);
    }

    #[test]
    fn test_pick_install_dir_returns_absolute_path() {
        let dir = pick_install_dir();
        assert!(dir.is_absolute());
    }

    #[test]
    fn test_pick_install_dir_unix_paths() {
        if !cfg!(windows) {
            let dir = pick_install_dir();
            let s = dir.to_string_lossy();
            assert!(
                s.contains("/usr/local/bin") || s.contains(".local/bin"),
                "unexpected install dir: {}",
                s
            );
        }
    }

    #[test]
    fn test_is_dir_writable_temp() {
        let tmp = std::env::temp_dir();
        assert!(is_dir_writable(&tmp));
    }

    #[test]
    fn test_is_dir_writable_nonexistent() {
        assert!(!is_dir_writable(Path::new("/nonexistent/path/xyz")));
    }

    #[test]
    fn test_maybe_self_install_skips_in_cargo_target() {
        // When running tests, exe is in target/debug — should skip
        assert!(!maybe_self_install());
    }
}
