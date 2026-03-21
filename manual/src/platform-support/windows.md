# Windows

Windows is a supported platform with full feature parity, though several
subsystems require platform-specific implementations and workarounds due to
differences in the terminal model, filesystem, and input handling.

---

## ConPTY Terminal Backend

The embedded terminal uses **ConPTY** (Windows Console Pseudo Terminal) via the
`portable-pty` crate. ConPTY is the modern Windows equivalent of Unix PTY,
available on Windows 10 1809+ and all Windows 11 versions.

### Shell Detection

AZUREAL detects the available shell in preference order:

1. `pwsh.exe` -- PowerShell 7+ (cross-platform PowerShell)
2. `powershell.exe` -- Windows PowerShell 5.1 (ships with Windows)
3. `cmd.exe` -- Command Prompt (fallback)

The first available shell is used. PowerShell 7+ is preferred because it
supports modern terminal features and ANSI escape sequences more reliably than
the legacy Windows PowerShell.

### Enter Key Behavior

PowerShell expects **carriage return** (`\r`) rather than **line feed** (`\n`)
for line submission. AZUREAL sends `\r` when the Enter key is pressed in the
embedded terminal on Windows. This is transparent to the user but differs from
the Unix behavior where Enter sends `\n`.

### Terminal Title Reassertion

Claude Code CLI calls `SetConsoleTitle()` during execution, which overwrites
AZUREAL's terminal title. After each agent process exits, AZUREAL reasserts its
own title (`AZUREAL @ <project> : <branch>`) to restore the expected title bar
content.

---

## CPU-Only Whisper with MSVC Fixes

Whisper runs on the CPU on Windows, as on Linux. However, the `whisper-rs` crate
requires additional patches to compile with the MSVC toolchain:

- **Layout tests disabled** -- MSVC produces different struct layouts than GCC/
  Clang, causing layout assertion failures in the vendored whisper.cpp bindings.
  These tests are disabled on Windows.
- **Enum type fixes** -- Certain C enum types require explicit size annotations
  to match MSVC's default enum representation.

These fixes are applied in AZUREAL's vendored copy of `whisper-rs` and do not
affect runtime behavior.

### Build Dependencies

```powershell
winget install LLVM.LLVM Kitware.CMake
```

After installing LLVM, set the `LIBCLANG_PATH` environment variable:

```powershell
[Environment]::SetEnvironmentVariable("LIBCLANG_PATH", "C:\Program Files\LLVM\bin", "User")
```

Restart your terminal after setting this variable.

---

## Alt Key Bindings

Windows uses **Alt** as the modifier for destructive or significant actions,
rather than Cmd (macOS) or Ctrl (Linux). This avoids conflicts with the Windows
console input system, where Ctrl+C is intercepted by the console host before it
reaches the application.

| Keybinding | Action |
|------------|--------|
| Alt+C | Cancel running agent |
| Alt+A | Archive current worktree |

Standard Ctrl bindings that do not conflict with the console (Ctrl+S, Ctrl+Z)
work normally.

---

## No Kitty Keyboard Protocol

The Kitty keyboard protocol is **not enabled** on Windows. Windows Terminal's
implementation of the protocol conflicts with mouse event reporting, causing
mouse clicks and scrolling to be misinterpreted. AZUREAL uses the standard Win32
console input API instead, which provides adequate key and mouse event reporting
for all features.

This means that certain ambiguous key combinations (Tab vs Ctrl+I, Enter vs
Ctrl+M) cannot be distinguished on Windows. AZUREAL's keybinding system accounts
for this by avoiding these ambiguous bindings.

---

## No fast_draw_input()

The `fast_draw_input()` optimization (direct VT escape sequence writes to bypass
ratatui's draw call) is **not available** on Windows. Direct VT writes conflict
with the Windows console input parser, which processes VT sequences differently
than Unix terminal emulators.

All rendering on Windows goes through the standard `terminal.draw()` path. The
~18ms draw latency is acceptable on Windows because there is no competing
fast-path expectation.

---

## Path Canonicalization

Windows paths returned by `std::fs::canonicalize()` include the `\\?\` extended-
length prefix (e.g., `\\?\C:\Users\name\project`). This prefix is valid but
causes issues with tools that do not expect it.

AZUREAL uses the [dunce](https://docs.rs/dunce/) crate to strip the `\\?\`
prefix from canonicalized paths, producing standard `C:\Users\name\project`
paths throughout the application. This ensures compatibility with git, Claude
Code CLI, and any other external tools invoked by AZUREAL.

---

## NTFS Junctions for Session Linking

On Unix systems, AZUREAL uses symbolic links for session file references between
worktrees. Windows NTFS does not support unprivileged symlink creation (it
requires the `SeCreateSymbolicLinkPrivilege` unless Developer Mode is enabled).

AZUREAL uses **NTFS junctions** instead, which do not require elevated
privileges. Junctions function identically to directory symlinks for AZUREAL's
purposes -- the file watcher and session parser follow them transparently.

---

## File Watcher

The file watcher uses **ReadDirectoryChangesW** on Windows, the native Win32
API for monitoring directory changes. This is event-driven and efficient, similar
to inotify on Linux and kqueue on macOS.

Unlike inotify, ReadDirectoryChangesW does not have a configurable watch limit
-- it is bounded by available memory. The graceful fallback to stat()-based
polling is still implemented but rarely needed on Windows.

---

## Recommended Terminal

The recommended terminal on Windows is **[Windows Terminal](https://aka.ms/terminal)**,
which ships with Windows 11 and is available on Windows 10 via the Microsoft
Store. It provides full ConPTY support, true-color output, and reliable mouse
event reporting.

The legacy `conhost.exe` (the default console host prior to Windows Terminal) has
limited ANSI support and is not recommended.
