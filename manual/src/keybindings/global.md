# Global Keybindings

These keybindings are available in **command mode** (red border). They are suppressed during prompt mode, terminal mode, edit mode, and active filter inputs. See [Vim-Style Modes](./vim-modes.md) for details on when globals are active.

---

## Mode Switching

| Key | Action |
|-----|--------|
| `p` | Enter prompt mode |
| `T` (Shift+T) | Toggle terminal |
| `Esc` | Return to command mode |

---

## Scrolling (Per-Pane)

| Key | Action |
|-----|--------|
| `j` | Scroll down one line |
| `k` | Scroll up one line |
| `J` (Shift+J) | Page scroll down |
| `K` (Shift+K) | Page scroll up |

These are defined per-pane (FileTree, Viewer, Session, Terminal) rather than as global bindings, but they behave consistently across all scrollable panes. The scroll targets whichever pane currently has focus.

---

## Pane Focus

| Key | Action |
|-----|--------|
| `Tab` | Cycle pane focus forward |
| `Shift+Tab` | Cycle pane focus backward |

Focus cycles through the four panes: FileTree, Viewer, Session, and Input.

---

## Panels & Overlays

| Key | Action |
|-----|--------|
| `G` (Shift+G) | Open git actions panel |
| `H` (Shift+H) | Open health panel |
| `M` (Shift+M) | Browse main branch |
| `P` (Shift+P) | Open projects panel |
| `?` | Open help overlay |

Panels are modal overlays that take focus when opened. Each panel defines its own keybindings for navigation within the panel (see panel-specific chapters). Press `Esc` to close a panel and return to command mode.

---

## Worktree Tabs

| Key | Action |
|-----|--------|
| `[` | Switch to previous worktree tab |
| `]` | Switch to next worktree tab |

These keys cycle through the worktree tab row at the top of the interface. The active worktree determines which branch, session, and terminal are shown.

---

## Run Commands

| Key | Action |
|-----|--------|
| `r` | Execute a run command |
| `R` (Shift+R) | Add a new run command |

Run commands are predefined shell commands (build, test, lint, etc.) that execute in the active worktree. See [Run Commands](../run-commands.md) for configuration details.

---

## Clipboard & Agent Control

| Key | Action |
|-----|--------|
| `Cmd+C` / `Ctrl+C` | Copy selection (platform-dependent) |
| `Ctrl+C` / `Alt+C` | Cancel running agent (platform-dependent) |
| `Ctrl+M` | Cycle model (Claude Opus, Sonnet, Haiku, Codex models) |
| `Ctrl+Q` | Quit the application |

The copy and cancel keys differ between macOS and other platforms. See [Platform Differences](./platform-differences.md) for the full mapping.

---

## Quick Reference

The complete table in one place:

| Key | Action |
|-----|--------|
| `p` | Enter prompt mode |
| `T` | Toggle terminal |
| `Esc` | Return to command mode (per-pane) |
| `Tab` / `Shift+Tab` | Cycle pane focus |
| `G` | GitView panel |
| `H` | Health panel |
| `M` | Browse main branch |
| `P` | Projects panel |
| `r` | Run command |
| `R` | Add run command |
| `[` / `]` | Switch worktree tab |
| `?` | Help overlay |
| `Cmd+C` / `Ctrl+C` | Copy selection |
| `Ctrl+C` / `Alt+C` | Cancel agent |
| `Ctrl+M` | Cycle model |
| `Ctrl+Q` | Quit |
