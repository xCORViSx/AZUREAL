# Completion Notifications

AZUREAL sends native macOS notifications when agent sessions complete. This lets
you work in other applications while agents run and get notified the moment a
response is ready -- without polling the terminal.

---

## What Triggers a Notification

A notification fires for **every session exit**, not just the currently focused
session. If you have three agents running in parallel across different worktrees,
you receive a notification when each one finishes.

The notification content varies based on the exit condition:

| Condition | Body Text |
|-----------|-----------|
| Normal completion | "Response complete" |
| Error exit | "Exited with error" |
| Process terminated | "Process terminated" |

The notification **title** follows the format **`worktree:session_name`**,
making it easy to identify which agent finished from the notification banner
alone.

---

## Branding

Notifications display the AZUREAL icon. The icon infrastructure is set up
automatically:

- An `.icns` icon file is **embedded in the binary** at compile time via
  `include_bytes!()`.
- On first launch, AZUREAL creates a minimal `.app` bundle at
  **`~/.azureal/AZUREAL.app`** containing the extracted icon. This bundle is
  what macOS associates with the notification, allowing the branded icon to
  appear.

The `.app` bundle is created once and reused on subsequent launches.

---

## Notification Permissions

AZUREAL automatically enables notification permissions by writing to the
`ncprefs.plist` file. This avoids the need for users to manually grant
notification permissions through System Settings on first use.

---

## Implementation

Notifications are sent via the `notify-rust` crate. Each notification is
dispatched on a **fire-and-forget background thread** that never blocks the
event loop. If the notification system is unavailable or the send fails, the
failure is silently ignored -- notifications are a convenience feature and must
never interfere with the core TUI experience.

---

## Platform Note

Completion notifications currently target **macOS only**. The `.app` bundle
mechanism and `ncprefs.plist` integration are macOS-specific. On other platforms,
notifications are not sent.

---

## Quick Reference

| Detail | Value |
|--------|-------|
| Trigger | Every session exit (all sessions, not just focused) |
| Title format | `worktree:session_name` |
| Notification library | notify-rust |
| Icon delivery | `include_bytes!()` embedded `.icns` |
| App bundle | `~/.azureal/AZUREAL.app` (auto-created on first launch) |
| Threading | Fire-and-forget background thread |
| Platform | macOS only |
