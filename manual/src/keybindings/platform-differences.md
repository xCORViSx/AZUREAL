# Platform Differences

AZUREAL runs on macOS, Windows, and Linux. The core keybindings are the same across platforms, but **modifier keys differ** because each platform has different conventions for system-level shortcuts.

---

## Modifier Key Mapping

| Action | macOS | Windows / Linux |
|--------|-------|-----------------|
| Copy selection | `Cmd+C` | `Ctrl+C` |
| Cancel agent | `Ctrl+C` | `Alt+C` |
| Archive worktree | `Cmd+A` | `Alt+A` |
| Delete worktree | `Cmd+D` | `Alt+D` |
| Select all | `Cmd+A` | `Ctrl+A` |
| Save file | `Cmd+S` | `Ctrl+S` |
| Undo | `Cmd+Z` | `Ctrl+Z` |
| Redo | `Cmd+Shift+Z` | `Ctrl+Y` |
| STT (edit mode) | `Ctrl+S` | `Alt+S` |

The pattern is straightforward: where macOS uses `Cmd` as the primary modifier, Windows and Linux use `Ctrl`. Where macOS uses `Ctrl` for secondary actions (like cancelling an agent), Windows and Linux use `Alt` to avoid collision with `Ctrl+C` being mapped to copy.

---

## Why Copy and Cancel Differ

On macOS, `Cmd+C` is the system copy shortcut, so AZUREAL maps copy to `Cmd+C` and uses `Ctrl+C` (which has no system meaning on macOS) for cancelling the running agent.

On Windows and Linux, `Ctrl+C` is both the system copy shortcut and the traditional terminal interrupt signal. AZUREAL maps it to copy (matching user expectation), and moves agent cancellation to `Alt+C` to avoid the collision.

---

## Modifier Symbol Display

AZUREAL renders modifier keys differently depending on the platform:

**macOS** uses standard Apple modifier symbols:

| Symbol | Modifier |
|--------|----------|
| `⌃` | Control |
| `⌥` | Option |
| `⇧` | Shift |
| `⌘` | Command |

**Windows and Linux** use text labels:

| Label | Modifier |
|-------|----------|
| `Ctrl+` | Control |
| `Alt+` | Alt |
| `Shift+` | Shift |

These symbols and labels appear in the help overlay, status bar hints, and panel footers.

---

## macOS Option Key Workaround

On macOS, pressing `Option` + a letter key produces a Unicode character instead of sending the key combination directly. For example:

| Key Combination | Character Produced |
|-----------------|--------------------|
| `Option+C` | c-cedilla |
| `Option+D` | partial differential |
| `Option+A` | a-ring |
| `Option+S` | sharp-s |

This is standard macOS behavior, but it means raw terminal input for `Option+letter` arrives as a Unicode character, not as an `Alt+letter` event.

AZUREAL handles this internally by mapping these Unicode characters back to their intended `Option+letter` combinations. The keybinding system recognizes both the Unicode character and the intended modifier combination, so bindings work correctly regardless.

The help overlay filters out these Unicode alternative representations. You will only see the human-readable `Option+letter` form, not the raw Unicode character it produces.

---

## Summary

If you are switching between platforms:

- **Non-modifier keys** (`p`, `j`, `k`, `T`, `G`, `?`, etc.) are identical everywhere.
- **Modifier keys** follow each platform's conventions (`Cmd` on macOS, `Ctrl` on Windows/Linux).
- **Agent cancel** is always one step removed from copy (`Ctrl+C` on macOS, `Alt+C` elsewhere).
- **The help overlay** (`?`) always shows the correct bindings for your current platform.

When in doubt, press `?` to see exactly what each key does on your system.
