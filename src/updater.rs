//! Auto-update checker and installer
//!
//! Checks GitHub releases for newer versions during startup (background thread).
//! Downloads and replaces the binary when the user accepts the update dialog.

use std::sync::mpsc::Sender;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const GITHUB_API_URL: &str =
    "https://api.github.com/repos/xCORViSx/AZUREAL/releases/latest";
const CHECK_INTERVAL_SECS: u64 = 86400; // 24 hours

/// Information about an available update.
#[derive(Clone)]
pub struct UpdateInfo {
    pub version: String,
    pub download_url: String,
    #[allow(dead_code)]
    pub release_url: String,
}

/// Result of checking for updates.
pub enum UpdateCheckResult {
    Available(UpdateInfo),
    UpToDate,
    #[allow(dead_code)]
    Skipped,
    RateLimited,
    Failed(#[allow(dead_code)] String),
}

/// Progress events sent during download+install.
pub enum UpdateProgress {
    Downloading(u8),
    Installing,
    Complete,
    Failed(String),
}

/// Parse "X.Y.Z" (with optional leading "v") into a comparable tuple.
fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.strip_prefix('v').unwrap_or(s);
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

/// Select the correct asset name for this platform.
fn platform_asset_name() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("azureal-macos-arm64.tar.gz"),
        ("macos", "x86_64") => Some("azureal-macos-x64.tar.gz"),
        ("linux", "x86_64") => Some("azureal-linux-x86_64.tar.gz"),
        ("windows", "x86_64") => Some("azureal-windows-x64.tar.gz"),
        _ => None,
    }
}

/// Check GitHub for a newer release.
pub fn check_for_update(
    skip_version: Option<&str>,
    last_check: Option<u64>,
) -> UpdateCheckResult {
    // Rate limit: once per 24 hours
    if let Some(last) = last_check {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if now.saturating_sub(last) < CHECK_INTERVAL_SECS {
            return UpdateCheckResult::RateLimited;
        }
    }

    // Fetch latest release from GitHub API
    let response = match ureq::get(GITHUB_API_URL)
        .header("User-Agent", &format!("azureal/{}", CURRENT_VERSION))
        .header("Accept", "application/vnd.github.v3+json")
        .call()
    {
        Ok(r) => r,
        Err(e) => return UpdateCheckResult::Failed(format!("HTTP error: {}", e)),
    };

    let body: String = match response.into_body().read_to_string() {
        Ok(s) => s,
        Err(e) => return UpdateCheckResult::Failed(format!("Read error: {}", e)),
    };

    let json: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => return UpdateCheckResult::Failed(format!("JSON parse error: {}", e)),
    };

    let tag = match json["tag_name"].as_str() {
        Some(t) => t,
        None => return UpdateCheckResult::Failed("No tag_name in response".into()),
    };

    // Compare versions
    let remote = match parse_version(tag) {
        Some(v) => v,
        None => return UpdateCheckResult::Failed(format!("Cannot parse version: {}", tag)),
    };
    let local = match parse_version(CURRENT_VERSION) {
        Some(v) => v,
        None => return UpdateCheckResult::Failed("Cannot parse local version".into()),
    };

    if remote <= local {
        return UpdateCheckResult::UpToDate;
    }

    let version = tag.strip_prefix('v').unwrap_or(tag).to_string();

    // Check if user chose to skip this version
    if let Some(skip) = skip_version {
        if skip == version {
            return UpdateCheckResult::Skipped;
        }
    }

    // Find the platform-specific download URL
    let asset_name = match platform_asset_name() {
        Some(n) => n,
        None => return UpdateCheckResult::Failed("Unsupported platform".into()),
    };

    let download_url = json["assets"]
        .as_array()
        .and_then(|assets| {
            assets.iter().find_map(|a| {
                let name = a["name"].as_str()?;
                if name == asset_name {
                    a["browser_download_url"].as_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
        });

    let download_url = match download_url {
        Some(u) => u,
        None => {
            return UpdateCheckResult::Failed(format!(
                "No asset '{}' in release",
                asset_name
            ))
        }
    };

    let release_url = json["html_url"]
        .as_str()
        .unwrap_or("")
        .to_string();

    UpdateCheckResult::Available(UpdateInfo {
        version,
        download_url,
        release_url,
    })
}

/// Download the update and replace the installed binary.
pub fn download_and_install(info: &UpdateInfo, progress: Sender<UpdateProgress>) {
    let result = download_and_install_inner(info, &progress);
    if let Err(e) = result {
        let _ = progress.send(UpdateProgress::Failed(e));
    }
}

fn download_and_install_inner(
    info: &UpdateInfo,
    progress: &Sender<UpdateProgress>,
) -> Result<(), String> {
    let _ = progress.send(UpdateProgress::Downloading(0));

    // Resolve where the current binary is installed
    let exe = std::env::current_exe()
        .and_then(|p| dunce::canonicalize(&p).map_err(Into::into))
        .map_err(|e| format!("Cannot find current exe: {}", e))?;

    let install_dir = exe.parent().ok_or("Cannot find exe parent dir")?;
    let tmp_path = install_dir.join(if cfg!(windows) {
        "azureal_update.tmp"
    } else {
        ".azureal_update.tmp"
    });

    // Download the tar.gz
    let response = ureq::get(&info.download_url)
        .header("User-Agent", &format!("azureal/{}", CURRENT_VERSION))
        .call()
        .map_err(|e| format!("Download failed: {}", e))?;

    let content_length = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    // Read full body into memory (tar.gz is ~10-30MB, fine for RAM)
    let mut body = Vec::new();
    let mut reader = response.into_body().into_reader();
    let mut buf = [0u8; 65536];
    let mut downloaded: u64 = 0;

    loop {
        match std::io::Read::read(&mut reader, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                body.extend_from_slice(&buf[..n]);
                downloaded += n as u64;
                if let Some(total) = content_length {
                    let pct = ((downloaded * 100) / total).min(99) as u8;
                    let _ = progress.send(UpdateProgress::Downloading(pct));
                }
            }
            Err(e) => return Err(format!("Download read error: {}", e)),
        }
    }

    let _ = progress.send(UpdateProgress::Downloading(100));
    let _ = progress.send(UpdateProgress::Installing);

    // Extract binary from tar.gz
    let extracted = extract_binary_from_targz(&body)?;

    // Write extracted binary to temp file
    std::fs::write(&tmp_path, &extracted)
        .map_err(|e| format!("Write temp file failed: {}", e))?;

    // Platform-specific binary replacement
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755));

        // Try atomic rename (same filesystem)
        if std::fs::rename(&tmp_path, &exe).is_err() {
            // Fallback: sudo cp
            let status = std::process::Command::new("sudo")
                .args(["cp", &tmp_path.to_string_lossy(), &exe.to_string_lossy()])
                .status();
            let _ = std::fs::remove_file(&tmp_path);
            match status {
                Ok(s) if s.success() => {
                    let _ = std::process::Command::new("sudo")
                        .args(["chmod", "+x", &exe.to_string_lossy()])
                        .status();
                }
                _ => return Err("Failed to replace binary (try running with sudo)".into()),
            }
        }
    }

    #[cfg(windows)]
    {
        let old_path = exe.with_extension("exe.old");
        // Clean up previous .old if it exists
        let _ = std::fs::remove_file(&old_path);
        // Rename running binary away (Windows allows rename but not overwrite)
        std::fs::rename(&exe, &old_path)
            .map_err(|e| format!("Cannot rename running binary: {}", e))?;
        // Move new binary into place
        if let Err(e) = std::fs::rename(&tmp_path, &exe) {
            // Rollback: restore old binary
            let _ = std::fs::rename(&old_path, &exe);
            return Err(format!("Cannot install new binary: {}", e));
        }
    }

    let _ = progress.send(UpdateProgress::Complete);
    Ok(())
}

/// Extract the binary from a tar.gz archive.
/// Expects a single file named "azureal" or "azureal.exe" in the archive.
fn extract_binary_from_targz(data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;

    let gz = flate2::read::GzDecoder::new(data);
    let mut archive = tar::Archive::new(gz);

    let bin_name = if cfg!(windows) {
        "azureal.exe"
    } else {
        "azureal"
    };

    for entry in archive.entries().map_err(|e| format!("tar error: {}", e))? {
        let mut entry = entry.map_err(|e| format!("tar entry error: {}", e))?;
        let path = entry
            .path()
            .map_err(|e| format!("tar path error: {}", e))?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if name == bin_name {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| format!("tar read error: {}", e))?;
            return Ok(buf);
        }
    }

    Err(format!("'{}' not found in archive", bin_name))
}

/// Delete leftover .old binary from a previous Windows update.
pub fn cleanup_old_binary() {
    #[cfg(windows)]
    {
        if let Ok(exe) = std::env::current_exe()
            .and_then(|p| dunce::canonicalize(&p).map_err(Into::into))
        {
            let old = exe.with_extension("exe.old");
            if old.exists() {
                let _ = std::fs::remove_file(&old);
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_basic() {
        assert_eq!(parse_version("1.0.77"), Some((1, 0, 77)));
    }

    #[test]
    fn parse_version_with_v() {
        assert_eq!(parse_version("v1.0.77"), Some((1, 0, 77)));
    }

    #[test]
    fn parse_version_invalid() {
        assert_eq!(parse_version("1.0"), None);
        assert_eq!(parse_version("abc"), None);
    }

    #[test]
    fn platform_asset_exists() {
        // Should return Some on any supported dev machine
        let name = platform_asset_name();
        assert!(name.is_some(), "unsupported platform for update");
        assert!(name.unwrap().ends_with(".tar.gz"));
    }

    #[test]
    fn current_version_parses() {
        assert!(parse_version(CURRENT_VERSION).is_some());
    }
}
