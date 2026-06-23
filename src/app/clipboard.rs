//! Clipboard bridge for system and in-app copy/paste operations.
//!
//! Azureal keeps an internal clipboard as a fallback for paste operations inside
//! the app, but user-facing copy actions must also reach the operating system
//! clipboard so text can be pasted into other applications.

use std::io::Write;
use std::process::{Command, Stdio};

use super::App;

/// A command-line program that writes stdin to the platform clipboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClipboardWriteCommand {
    /// Executable name resolved through the current process PATH.
    program: &'static str,
    /// Arguments passed to the executable before the clipboard text is written.
    args: &'static [&'static str],
}

/// A command-line program that prints the platform clipboard to stdout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClipboardReadCommand {
    /// Executable name resolved through the current process PATH.
    program: &'static str,
    /// Arguments passed to the executable before stdout is captured.
    args: &'static [&'static str],
}

/// Write text to the system clipboard through `arboard` or a platform command.
pub(super) fn write_system_clipboard(
    clipboard: &mut Option<arboard::Clipboard>,
    text: &str,
) -> bool {
    if write_arboard_clipboard(clipboard, text) {
        return true;
    }

    write_platform_clipboard(text)
}

/// Read text from the system clipboard through `arboard` or a platform command.
pub(super) fn read_system_clipboard(clipboard: &mut Option<arboard::Clipboard>) -> Option<String> {
    if let Some(cb) = clipboard.as_mut() {
        if let Ok(text) = cb.get_text() {
            return Some(text);
        }
    } else {
        return None;
    }

    *clipboard = arboard::Clipboard::new().ok();
    if let Some(cb) = clipboard.as_mut() {
        if let Ok(text) = cb.get_text() {
            return Some(text);
        }
    }

    read_platform_clipboard()
}

/// Clipboard status helpers for user-facing copy actions.
impl App {
    /// Show an accurate status after a copy action attempts the system clipboard.
    pub fn set_clipboard_copy_status(&mut self, copied_to_system: bool, success_message: &str) {
        if copied_to_system {
            self.set_status(success_message);
        } else {
            self.set_status("Copy failed: system clipboard unavailable");
        }
    }
}

/// Try `arboard`, recreating its handle once if the persistent handle is stale.
fn write_arboard_clipboard(clipboard: &mut Option<arboard::Clipboard>, text: &str) -> bool {
    if clipboard.is_none() {
        *clipboard = arboard::Clipboard::new().ok();
    }

    if let Some(cb) = clipboard.as_mut() {
        if cb.set_text(text.to_string()).is_ok() {
            return true;
        }
    }

    *clipboard = arboard::Clipboard::new().ok();
    clipboard
        .as_mut()
        .map(|cb| cb.set_text(text.to_string()).is_ok())
        .unwrap_or(false)
}

/// Try the platform clipboard writer commands available on the current OS.
fn write_platform_clipboard(text: &str) -> bool {
    platform_clipboard_write_commands()
        .iter()
        .any(|command| run_clipboard_write_command(*command, text))
}

/// Try the platform clipboard reader commands available on the current OS.
fn read_platform_clipboard() -> Option<String> {
    platform_clipboard_read_commands()
        .iter()
        .find_map(|command| run_clipboard_read_command(*command))
}

/// Return fallback clipboard writer commands for the target operating system.
fn platform_clipboard_write_commands() -> &'static [ClipboardWriteCommand] {
    #[cfg(target_os = "macos")]
    {
        &[ClipboardWriteCommand {
            program: "pbcopy",
            args: &[],
        }]
    }

    #[cfg(target_os = "windows")]
    {
        &[ClipboardWriteCommand {
            program: "clip",
            args: &[],
        }]
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        &[
            ClipboardWriteCommand {
                program: "wl-copy",
                args: &[],
            },
            ClipboardWriteCommand {
                program: "xclip",
                args: &["-selection", "clipboard"],
            },
            ClipboardWriteCommand {
                program: "xsel",
                args: &["--clipboard", "--input"],
            },
        ]
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        &[]
    }
}

/// Return fallback clipboard reader commands for the target operating system.
fn platform_clipboard_read_commands() -> &'static [ClipboardReadCommand] {
    #[cfg(target_os = "macos")]
    {
        &[ClipboardReadCommand {
            program: "pbpaste",
            args: &[],
        }]
    }

    #[cfg(target_os = "windows")]
    {
        &[ClipboardReadCommand {
            program: "powershell",
            args: &["-NoProfile", "-Command", "Get-Clipboard"],
        }]
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        &[
            ClipboardReadCommand {
                program: "wl-paste",
                args: &[],
            },
            ClipboardReadCommand {
                program: "xclip",
                args: &["-selection", "clipboard", "-o"],
            },
            ClipboardReadCommand {
                program: "xsel",
                args: &["--clipboard", "--output"],
            },
        ]
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        &[]
    }
}

/// Run one clipboard writer command and feed it the copied text on stdin.
fn run_clipboard_write_command(command: ClipboardWriteCommand, text: &str) -> bool {
    let mut child = match Command::new(command.program)
        .args(command.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };

    let wrote = child
        .stdin
        .take()
        .map(|mut stdin| stdin.write_all(text.as_bytes()).is_ok())
        .unwrap_or(false);
    if !wrote {
        let _ = child.kill();
        let _ = child.wait();
        return false;
    }

    child.wait().map(|status| status.success()).unwrap_or(false)
}

/// Run one clipboard reader command and return UTF-8 stdout when it succeeds.
fn run_clipboard_read_command(command: ClipboardReadCommand) -> Option<String> {
    let output = Command::new(command.program)
        .args(command.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The current platform exposes a static command list without allocation.
    #[test]
    fn write_commands_are_static() {
        let commands = platform_clipboard_write_commands();
        assert!(commands.len() <= 3);
    }

    /// The current platform exposes a static read command list without allocation.
    #[test]
    fn read_commands_are_static() {
        let commands = platform_clipboard_read_commands();
        assert!(commands.len() <= 3);
    }

    /// Clipboard write commands carry their executable and argument metadata.
    #[test]
    fn write_command_metadata_is_accessible() {
        let command = ClipboardWriteCommand {
            program: "copy-tool",
            args: &["--clipboard"],
        };
        assert_eq!(command.program, "copy-tool");
        assert_eq!(command.args, &["--clipboard"]);
    }

    /// Clipboard read commands carry their executable and argument metadata.
    #[test]
    fn read_command_metadata_is_accessible() {
        let command = ClipboardReadCommand {
            program: "paste-tool",
            args: &["--clipboard"],
        };
        assert_eq!(command.program, "paste-tool");
        assert_eq!(command.args, &["--clipboard"]);
    }
}
