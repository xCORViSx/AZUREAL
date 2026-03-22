# Completion Notifications

AZUREAL sends native desktop notifications when agent sessions complete. This
lets you work in other applications while agents run and get notified the moment
a response is ready -- without polling the terminal.

---

## What Triggers a Notification

A notification fires for **every session exit**, not just the currently focused
session. If you have three agents running in parallel across different worktrees,
you receive a notification when each one finishes.

The notification content varies based on the exit condition:

| Condition | Body Text |
|-----------|-----------|
| Context compaction | "Compacting context" |
| Normal completion | "Response complete" |
| Error exit | "Exited with error" |
| Process terminated | "Process terminated" |

The notification **title** follows the format **`worktree:session_name`**,
making it easy to identify which agent finished from the notification banner
alone.

---

## Platform Details

### macOS

- Notifications sent via the `notify-rust` crate.
- An `.icns` icon file is **embedded in the binary** at compile time via
  `include_bytes!()`.
- On first launch, AZUREAL creates a minimal `.app` bundle at
  **`~/.azureal/AZUREAL.app`** containing the extracted icon. This bundle is
  what macOS associates with the notification, allowing the branded icon to
  appear.
- Notification permissions are automatically enabled by writing to the
  `ncprefs.plist` file, avoiding the need for users to manually grant
  permissions through System Settings on first use.
- The `.app` bundle is created once and reused on subsequent launches.
- Notification sound: "Glass".

### Windows

- Notifications are sent via **PowerShell WinRT toast APIs**. The `notify-rust`
  crate is not used on Windows because its custom `.app_id()` requires a
  registered AppUserModelID (AUMID), which silently drops toasts when
  unregistered.
- PowerShell's own pre-registered AUMID is used instead, ensuring reliable
  delivery.
- `CREATE_NO_WINDOW` (`0x08000000`) prevents a console window from flashing
  when the PowerShell process spawns.
- Toast XML uses `appLogoOverride` with `~/.azureal/AZUREAL_toast.png` for a
  crisp branded icon (PNG renders clearly in toasts; `.ico` renders blurry).

### Linux

- Notifications sent via the `notify-rust` crate (same as macOS).

---

## Implementation

Each notification is dispatched on a **fire-and-forget background thread** that
never blocks the event loop. If the notification system is unavailable or the
send fails, the failure is silently ignored -- notifications are a convenience
feature and must never interfere with the core TUI experience.

---

## Quick Reference

| Detail | Value |
|--------|-------|
| Trigger | Every session exit (all sessions, not just focused) |
| Title format | `worktree:session_name` |
| macOS | `notify-rust` + `.app` bundle + Glass sound |
| Windows | PowerShell WinRT toast + PNG icon |
| Linux | `notify-rust` |
| Threading | Fire-and-forget background thread |
| Platform | macOS, Windows, and Linux |
