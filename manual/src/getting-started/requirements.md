# Requirements

AZUREAL has a small set of hard requirements and a few optional dependencies
that unlock additional features.

## Platform Support

| Platform | Status | Notes |
|----------|--------|-------|
| macOS | **Primary** | Metal GPU acceleration for Whisper; `.app` bundle icon support |
| Linux | Supported | CPU-based Whisper; all features functional |
| Windows | Supported | ConPTY terminal backend; `cmd.exe` and PowerShell shells; CUDA GPU-accelerated Whisper |

## Terminal Emulator

AZUREAL requires a modern terminal emulator with true-color and mouse support.
For the best experience:

| Platform | Recommended | Also tested |
|----------|-------------|-------------|
| macOS | [Kitty](https://sw.kovidgoyal.net/kitty/) | Ghostty, Alacritty, WezTerm, Terminal.app |
| Linux | [Kitty](https://sw.kovidgoyal.net/kitty/) | Ghostty, Alacritty, WezTerm, Konsole |
| Windows | [Windows Terminal](https://aka.ms/terminal) | — |

**Kitty** and **Windows Terminal** deliver the best overall experience for two
reasons:

1. **Input protocol support.** Kitty provides the
   [Kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)
   on macOS and Linux, which allows AZUREAL to distinguish ambiguous key
   combinations (e.g., `Tab` vs `Ctrl+I`, `Enter` vs `Ctrl+M`). Windows
   Terminal provides full ConPTY support with reliable mouse and key reporting.

2. **Rendering fidelity.** Both terminals produce the cleanest interpretation
   of AZUREAL's box-drawing characters (`│`, `┌`, `╔`, `█`, `▓`, etc.),
   Unicode glyphs, and styled borders. Pane separators, tab bars, dialog
   frames, and half-block splash art all render pixel-perfect with correct
   line joining and consistent glyph widths.

Other listed terminals work well -- the main difference is that terminals
without the Kitty keyboard protocol trigger `Alt+` fallback bindings for
certain shortcuts, and some terminals may show minor visual artifacts in
complex border intersections or half-block character rendering. See
[Platform Differences](../keybindings/platform-differences.md) for details.

The legacy `conhost.exe` on Windows is not recommended due to limited ANSI
support.

## Required

### Git 2.15+

AZUREAL relies heavily on `git worktree` commands, which require Git 2.15 or
later. Verify your version:

```sh
git --version
```

### At Least One Agent CLI

You need at least one of the following agent backends installed. Both can be
active simultaneously -- AZUREAL derives the backend from whichever model you
select.

**Claude Code CLI** (for Claude models):

```sh
# macOS / Linux
curl -fsSL https://claude.ai/install.sh | bash

# Windows (PowerShell)
irm https://claude.ai/install.ps1 | iex
```

**Codex CLI** (for OpenAI models -- optional):

```sh
npm install -g @openai/codex
```

### Rust (Latest Stable)

Required only if building from source. Install via [rustup](https://rustup.rs/):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Pre-built binaries do not require Rust.

### LLVM/Clang + CMake

Build dependency for `whisper-rs`, which provides speech-to-text support. Even
if you do not plan to use speech input, the crate is compiled as part of the
build.

**macOS:**

Install Xcode Command Line Tools (includes Clang and required headers):

```sh
xcode-select --install
```

CMake, if not already present:

```sh
brew install cmake
```

**Linux (Debian/Ubuntu):**

```sh
sudo apt install libclang-dev cmake
```

**Windows:**

```powershell
winget install LLVM.LLVM Kitware.CMake
```

After installing LLVM on Windows, set the `LIBCLANG_PATH` environment variable
to the LLVM `bin` directory (e.g., `C:\Program Files\LLVM\bin`).

### NVIDIA CUDA Toolkit (Windows)

Required on Windows for GPU-accelerated Whisper inference. Without CUDA, Whisper
falls back to CPU-only mode which is significantly slower.

```powershell
winget install Nvidia.CUDA
```

Restart your terminal after installation so the `CUDA_PATH` environment variable
is available. The build system reads `CUDA_PATH` to locate the CUDA libraries
automatically.

> **Note:** This requirement applies only to Windows. macOS uses Metal for GPU
> acceleration (handled automatically), and Linux uses CPU inference.

## Optional

### Nerd Font

AZUREAL uses Nerd Font glyphs for file tree icons (file type icons, folder
icons, git status indicators). When a Nerd Font is not detected, the application
falls back to emoji-based icons automatically.

Recommended fonts:

- [JetBrainsMono Nerd Font](https://www.nerdfonts.com/font-downloads)
- [FiraCode Nerd Font](https://www.nerdfonts.com/font-downloads)
- [Hack Nerd Font](https://www.nerdfonts.com/font-downloads)

Configure your terminal emulator to use the installed Nerd Font.

### Whisper Model (Speech-to-Text)

To use the built-in speech-to-text feature, download a Whisper model to the
expected path:

```
~/.azureal/speech/ggml-small.en.bin
```

Models are available from
[HuggingFace](https://huggingface.co/ggerganov/whisper.cpp/tree/main). The
`ggml-small.en.bin` model offers a good balance of accuracy and speed. Larger
models (medium, large) improve accuracy at the cost of higher latency and memory
usage.

On macOS, Whisper runs on the Metal GPU for faster inference. On Windows, Whisper
uses CUDA GPU acceleration (requires NVIDIA GPU + CUDA Toolkit). Linux uses CPU
inference.

## Quick Checklist

Before proceeding to installation, confirm:

- [ ] Git 2.15+ is installed
- [ ] At least one agent CLI is installed (Claude Code or Codex)
- [ ] LLVM/Clang and CMake are available (if building from source)
- [ ] NVIDIA CUDA Toolkit is installed (Windows only, if building from source)
- [ ] A Nerd Font is configured in your terminal (recommended)
