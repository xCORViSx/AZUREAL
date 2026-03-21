# macOS

macOS is AZUREAL's primary development platform. It receives the most testing,
has the most platform-specific optimizations, and is the only platform with GPU-
accelerated speech-to-text and native notification support.

---

## Metal GPU for Whisper

On macOS, the Whisper speech-to-text engine runs on the **Metal GPU**. This
provides significantly faster transcription compared to CPU inference on Linux
and Windows. The Metal backend is selected automatically when available -- no
configuration is required.

GPU inference means lower latency for speech-to-text: dictating a prompt and
getting the transcription back feels near-instant on Apple Silicon machines.

---

## .app Bundle

AZUREAL creates a `.app` bundle at `~/.azureal/AZUREAL.app`. This bundle is
auto-created on first launch and serves two purposes:

### Activity Monitor Integration

macOS identifies processes by their bundle. Without a `.app` bundle, AZUREAL
appears in Activity Monitor under whatever terminal emulator launched it (e.g.,
"Terminal" or "iTerm2"). With the bundle, it appears as **AZUREAL** with its own
branded icon, making it easy to identify in process lists and the Dock.

### Process Identity via proc_pidpath()

macOS uses `proc_pidpath()` to resolve a process's executable path for display
purposes. AZUREAL re-execs itself through the `.app` bundle so that
`proc_pidpath()` returns the bundle path rather than a bare binary path. This
ensures consistent process identification across Activity Monitor, `lsof`, and
other system tools.

The bundle contains a branded icon and a minimal `Info.plist`. It is regenerated
if missing or outdated.

---

## Notifications

Completion notifications are delivered via **NSUserNotification**. When an agent
session completes while AZUREAL is not the focused application, a system
notification appears with the session name and completion status.

Permission is requested automatically on first use. If the user has disabled
notifications for AZUREAL in System Preferences, the notification call silently
fails with no error.

---

## Cmd Key Bindings

macOS uses Cmd as the primary modifier, matching platform conventions:

| Keybinding | Action |
|------------|--------|
| Cmd+C | Copy selected text to clipboard |
| Cmd+S | Save current file (edit mode) |
| Cmd+Z | Undo last edit (edit mode) |

These bindings feel native to macOS users and do not conflict with terminal
control sequences (Ctrl+C sends SIGINT, which is a different action).

---

## Option+Letter Unicode Remapping

On macOS, pressing Option+letter produces Unicode characters (e.g., Option+A
produces "a" with a diacritical mark). AZUREAL remaps these back to their ASCII
equivalents when processing input, so Option+letter keybindings work as expected
rather than inserting Unicode characters into the input field.

---

## fast_draw_input()

The fast-path input rendering optimization is **macOS-only**. When the user
types in the input field and no other part of the screen needs updating,
`fast_draw_input()` writes the input field directly to the terminal via VT
escape sequences, bypassing ratatui's full `terminal.draw()` call.

| Path | Latency |
|------|---------|
| `fast_draw_input()` | ~0.1ms |
| `terminal.draw()` | ~18ms |

This gives macOS users the most responsive typing experience. The optimization
relies on direct VT writes that work reliably with macOS terminal emulators
(Terminal.app, iTerm2, Kitty, Alacritty, WezTerm) but conflict with the Windows
console input parser, which is why it is not available on Windows.

---

## Kitty Keyboard Protocol

AZUREAL enables the **Kitty keyboard protocol** on macOS for improved key event
reporting. This protocol provides:

- Unambiguous key identification (distinguishing Tab from Ctrl+I, Enter from
  Ctrl+M, etc.).
- Separate key press and release events.
- Accurate modifier reporting.

The recommended terminal on macOS is **Kitty**, which has full protocol support.
AZUREAL has also been tested in **Ghostty**, **Alacritty**, **WezTerm**, and
**Terminal.app**. Terminal.app does not support the Kitty protocol, but AZUREAL
falls back gracefully to standard key reporting with `Alt+` alternatives for
affected bindings.

---

## Build Notes

macOS builds require Xcode Command Line Tools for Clang headers and CMake for
the Whisper build:

```sh
xcode-select --install
brew install cmake
```

On Apple Silicon (M1/M2/M3/M4), builds produce ARM64 binaries natively. No
Rosetta translation is needed.
