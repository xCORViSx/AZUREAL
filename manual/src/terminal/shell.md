# Shell Integration

AZUREAL automatically detects and launches the appropriate shell for your
platform. No manual configuration is required -- the terminal works out of the
box on macOS, Linux, and Windows.

---

## Shell Detection

### Unix (macOS and Linux)

On Unix systems, AZUREAL reads the `SHELL` environment variable to determine
which shell to launch. If `SHELL` is not set, it falls back to `/bin/bash`.

Most systems set `SHELL` to the user's login shell (e.g., `/bin/zsh` on modern
macOS, `/bin/bash` on most Linux distributions). The terminal inherits this
shell along with its configuration files (`.bashrc`, `.zshrc`, etc.).

### Windows

Windows shell detection follows a prioritized chain:

1. **`pwsh.exe`** (PowerShell 7+) -- tried first.
2. **`powershell.exe`** (Windows PowerShell 5.1) -- tried if `pwsh.exe` is not
   found.
3. **`COMSPEC` / `cmd.exe`** -- final fallback.

Each candidate is verified by checking its exit status before accepting it. If
a shell binary exists but fails to start, detection moves to the next candidate.

PowerShell is launched with the `-NoLogo` flag to suppress the startup banner,
keeping the terminal output clean.

---

## Environment Setup

Regardless of platform, AZUREAL sets one environment variable before spawning
the shell:

```text
TERM=xterm-256color
```

This tells programs running inside the terminal that 256-color output is
supported. Most modern CLI tools (ls with colors, git diff, syntax
highlighters) respect this variable and produce colored output automatically.

---

## Per-Worktree Shells

Each worktree runs its own independent shell instance. The shell's working
directory is set to the worktree's root directory at launch:

| Worktree | Shell CWD |
|----------|-----------|
| main | `<repo>/` |
| feature-auth | `<repo>/worktrees/feature-auth/` |
| bugfix-render | `<repo>/worktrees/bugfix-render/` |

When you switch between worktrees, the terminal switches to that worktree's
shell. If the worktree does not have a shell running yet, one is spawned on
first access. Shells persist for the lifetime of the worktree session -- if you
switch away and come back, your shell history and state are still there.

---

## PTY Initialization Order

The embedded terminal uses `portable-pty` to manage the pseudo-terminal. The
PTY setup follows a specific initialization order that is critical for
reliability:

1. **Clone the reader** -- `try_clone_reader()` obtains a handle for reading
   PTY output.
2. **Take the writer** -- `take_writer()` obtains a handle for sending input to
   the PTY.
3. **Spawn the shell** -- `spawn_command()` launches the detected shell inside
   the PTY.
4. **Drop the slave** -- `drop(pair.slave)` releases the slave side of the PTY
   pair.

Steps 1 and 2 must happen before step 3. Attempting to clone the reader or take
the writer after the command has been spawned can lead to race conditions or
missed output. Dropping the slave after spawn ensures the PTY correctly
detects when the shell exits.
