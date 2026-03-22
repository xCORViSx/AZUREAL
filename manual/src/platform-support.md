# Platform Support

AZUREAL runs on macOS, Linux, and Windows. The core feature set is identical
across all three platforms -- agent sessions, worktrees, the session pane, the
file viewer, the git panel, and the embedded terminal all work everywhere. The
differences are in input handling, GPU availability, terminal protocol support,
and platform-specific integrations.

---

## Recommended Terminals

| Platform | Recommended | Also tested |
|----------|-------------|-------------|
| macOS | [Kitty](https://sw.kovidgoyal.net/kitty/) | Ghostty, Alacritty, WezTerm, Terminal.app |
| Linux | [Kitty](https://sw.kovidgoyal.net/kitty/) | Ghostty, Alacritty, WezTerm, Konsole |
| Windows | [Windows Terminal](https://aka.ms/terminal) | — |

**Kitty** and **Windows Terminal** deliver the best overall experience -- both
for input handling and for rendering fidelity. They produce the cleanest
interpretation of AZUREAL's box-drawing characters, Unicode glyphs, and styled
borders, resulting in pixel-perfect pane separators, tab bars, and dialog
frames. Kitty also provides the Kitty keyboard protocol on macOS/Linux for
unambiguous key reporting, while Windows Terminal provides full ConPTY support
with reliable mouse and key event handling. Other listed terminals work well --
AZUREAL automatically provides `Alt+` fallback bindings for key combinations
that require the Kitty protocol, though some terminals may show minor visual
artifacts in complex border intersections or half-block character rendering.

---

## Platform Matrix

| Feature | macOS | Linux | Windows |
|---------|-------|-------|---------|
| Agent sessions (Claude + Codex) | Yes | Yes | Yes |
| Git worktrees | Yes | Yes | Yes |
| Session store (SQLite) | Yes | Yes | Yes |
| File watcher backend | kqueue | inotify | ReadDirectoryChangesW |
| Embedded terminal | PTY | PTY | ConPTY |
| Speech-to-text (Whisper) | Metal GPU | CPU only | CUDA GPU |
| Kitty keyboard protocol | Yes | Yes | No |
| `fast_draw_input()` | Yes | No | No |
| `.app` bundle | Yes | N/A | N/A |
| Notifications | NSUserNotification | D-Bus (notify-rust) | Toast (PowerShell) |
| Modifier key for destructive actions | Cmd | Ctrl | Alt |

---

## Key Bindings by Platform

AZUREAL adapts its modifier key usage to each platform's conventions:

| Action | macOS | Linux | Windows |
|--------|-------|-------|---------|
| Copy | Cmd+C | Ctrl+C | Ctrl+C |
| Save | Cmd+S | Ctrl+S | Ctrl+S |
| Undo | Cmd+Z | Ctrl+Z | Ctrl+Z |
| Cancel agent | -- | -- | Alt+C |
| Archive worktree | -- | -- | Alt+A |

On macOS, Cmd is the primary modifier. On Linux, Ctrl fills the same role. On
Windows, destructive actions use Alt to avoid conflicts with the console input
system. See [Platform Differences](./keybindings/platform-differences.md) for
the full keybinding mapping.

---

## Build Dependencies

All platforms require the Rust toolchain, LLVM/Clang, and CMake for the Whisper
speech-to-text dependency. Platform-specific build requirements:

| Platform | Additional Requirements |
|----------|----------------------|
| macOS | Xcode Command Line Tools |
| Linux | `libclang-dev`, `cmake` |
| Windows | `LLVM.LLVM`, `CMake`, `Ninja`, `LIBCLANG_PATH` environment variable, NVIDIA CUDA Toolkit |

See [Requirements](./getting-started/requirements.md) for installation
instructions.

---

## Terminal Protocol Support

AZUREAL uses the **Kitty keyboard protocol** on macOS and Linux for improved
key event accuracy. This protocol distinguishes between key press and key
release events and provides unambiguous reporting of modifier combinations.

On Windows, the Kitty protocol is **not enabled** because it conflicts with mouse
event handling in Windows Terminal. Windows uses the standard console input API
instead, which provides adequate key reporting for all supported features.

---

## Chapter Contents

- **[macOS](./platform-support/macos.md)** -- Primary platform: Metal GPU,
  `.app` bundle, notifications, Cmd key bindings, fast-path input, and
  Option+letter remapping.
- **[Linux](./platform-support/linux.md)** -- Full support: CPU-only Whisper,
  Ctrl key bindings, build dependencies, and Kitty protocol.
- **[Windows](./platform-support/windows.md)** -- ConPTY terminal, PowerShell
  shell detection, Alt key bindings, MSVC Whisper fixes, path canonicalization,
  and NTFS junctions.
