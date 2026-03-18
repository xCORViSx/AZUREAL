# Linux

Linux is a fully supported platform. Every feature in AZUREAL works identically
to macOS, with two exceptions: Whisper runs on the CPU rather than a GPU, and
`fast_draw_input()` is not enabled (the standard draw path is used for all
rendering). There are no missing features or degraded capabilities.

---

## CPU-Only Whisper

The Whisper speech-to-text engine runs on the CPU on Linux. There is no Metal
equivalent, and CUDA/Vulkan GPU backends are not currently integrated.
Transcription is slower than on macOS with Metal but remains usable for typical
prompt dictation.

For best results, use the `ggml-small.en.bin` model. Larger models (medium,
large) produce better transcriptions but increase latency on CPU inference.

---

## Build Dependencies

Linux builds require `libclang-dev` and `cmake`:

**Debian/Ubuntu:**

```sh
sudo apt install libclang-dev cmake
```

**Fedora/RHEL:**

```sh
sudo dnf install clang-devel cmake
```

**Arch Linux:**

```sh
sudo pacman -S clang cmake
```

These are needed for the `whisper-rs` crate, which compiles whisper.cpp from
source during the Rust build.

---

## Ctrl Key Bindings

Linux uses Ctrl as the primary modifier, following standard terminal conventions:

| Keybinding | Action |
|------------|--------|
| Ctrl+C | Copy selected text to clipboard |
| Ctrl+S | Save current file (edit mode) |
| Ctrl+Z | Undo last edit (edit mode) |

These are the same bindings as macOS with Cmd replaced by Ctrl. The full
keybinding set is otherwise identical across both platforms.

---

## Kitty Keyboard Protocol

AZUREAL enables the **Kitty keyboard protocol** on Linux, just as on macOS. The
protocol is supported by:

- Kitty
- WezTerm
- Alacritty
- Ghostty
- Foot

Terminal emulators that do not support the protocol (e.g., GNOME Terminal, older
xterm) fall back to standard key reporting with no loss of functionality. The
only difference is that certain ambiguous key combinations (like Tab vs Ctrl+I)
may not be distinguishable without the protocol.

---

## File Watcher

The file watcher uses **inotify** on Linux. This is efficient and event-driven
but subject to a system-wide watch limit. If AZUREAL logs a message about watch
initialization failure and falls back to polling, increase the inotify limit:

```sh
# Check current limit
cat /proc/sys/fs/inotify/max_user_watches

# Increase temporarily
sudo sysctl fs.inotify.max_user_watches=524288

# Increase permanently
echo "fs.inotify.max_user_watches=524288" | sudo tee -a /etc/sysctl.conf
sudo sysctl -p
```

The default limit on many distributions (8192) is insufficient for large projects
with deep directory trees. The recommended value of 524288 is used by VS Code,
JetBrains IDEs, and other file-watching applications.

---

## Embedded Terminal

The embedded terminal uses standard PTY (pseudo-terminal) on Linux, the same as
macOS. Shell detection follows `$SHELL` and defaults to `/bin/bash` if unset.
All terminal features -- color support, click-to-position, mouse drag selection,
auto-scroll, and mouse wheel scrolling -- work identically to macOS.

---

## Notifications

System notifications are not currently implemented on Linux. Agent completion
events are reflected in the status bar and session list but do not produce
desktop notifications. This is a planned feature.

---

## Distribution Notes

AZUREAL is tested on Ubuntu 22.04+ and Fedora 38+. It should work on any modern
Linux distribution with glibc 2.31+ and a terminal emulator that supports 256
colors. Musl-based distributions (Alpine) are untested.
