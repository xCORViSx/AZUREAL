//! Self-install logic — copies binary to PATH on first run

use std::path::{Path, PathBuf};

/// Check if the binary needs installation. If so, install and return true (caller should exit).
/// Returns false for normal startup (already installed or running from cargo build dir).
pub fn maybe_self_install() -> bool {
    let exe = match std::env::current_exe().and_then(|p| dunce::canonicalize(&p).map_err(Into::into))
    {
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

    // Skip if `azureal` is already findable in PATH (installed elsewhere)
    let bin_name = if cfg!(windows) { "azureal.exe" } else { "azureal" };
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
    println!("  \x1b[36m╔══════════════════════════════════════╗\x1b[0m");
    println!("  \x1b[36m║\x1b[0m     \x1b[1;36mAZUREAL\x1b[0m — First Run Setup     \x1b[36m║\x1b[0m");
    println!("  \x1b[36m╚══════════════════════════════════════╝\x1b[0m");
    println!();

    let install_dir = pick_install_dir();
    let install_path = install_dir.join(if cfg!(windows) { "azureal.exe" } else { "azureal" });

    println!("  Installing to: \x1b[1m{}\x1b[0m", install_path.display());
    println!();

    // Create install directory
    if !install_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&install_dir) {
            eprintln!("  \x1b[31mFailed to create directory: {}\x1b[0m", e);
            wait_for_enter();
            return true;
        }
    }

    // Copy binary to install location
    if !try_copy(exe, &install_path) {
        wait_for_enter();
        return true;
    }

    // Set executable permission on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&install_path, std::fs::Permissions::from_mode(0o755));
    }

    // Ensure install dir is in PATH
    let path_updated = ensure_in_path(&install_dir);

    println!("  \x1b[32m✓ Installed successfully!\x1b[0m");
    println!();

    if path_updated {
        println!("  \x1b[33mPATH has been updated. Restart your terminal, then run:\x1b[0m");
    } else {
        println!("  You can now run:");
    }
    println!();
    println!("    \x1b[1;36mazureal\x1b[0m");
    println!();

    wait_for_enter();
    true
}

/// Pick the best install directory for this platform.
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
                    eprintln!(
                        "    sudo cp {} {}",
                        src.display(),
                        dst.display()
                    );
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

#[cfg(not(windows))]
fn add_to_windows_path(_dir: &str) -> bool {
    false
}

/// Append an export line to the user's shell profile (~/.zshrc, ~/.bashrc, etc.).
fn add_to_shell_profile(dir: &Path) -> bool {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return false,
    };

    let dir_str = dir.to_string_lossy();
    let export_line = format!("\n# Added by AZUREAL installer\nexport PATH=\"{}:$PATH\"\n", dir_str);

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
