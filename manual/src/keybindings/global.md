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

## Scrolling

| Key | Action |
|-----|--------|
| `j` | Scroll down one line |
| `k` | Scroll up one line |
| `J` (Shift+J) | Page scroll down |
| `K` (Shift+K) | Page scroll up |

Scroll targets the currently focused pane. In the session pane, this scrolls the conversation. In the file viewer, this scrolls the file content.

---

## Pane Focus

| Key | Action |
|-----|--------|
| `Tab` | Cycle pane focus forward |
| `Shift+Tab` | Cycle pane focus backward |

Focus cycles through the visible panes: file tree, file viewer, and session pane. Hidden panes (e.g., when the file tree is toggled off) are skipped.

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

## Toggles

| Key | Action |
|-----|--------|
| `f` | Toggle file tree visibility |
| `s` | Toggle session list visibility |

---

## Search

| Key | Action |
|-----|--------|
| `/` | Activate search / filter |

The behavior of `/` depends on the focused pane. In the session pane, it opens the session search. In the file tree, it opens the file filter. Once a search/filter is active, letter keys type into the filter field -- global bindings are suppressed until `Esc` dismisses the filter.

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
| `Esc` | Return to command mode |
| `j` / `k` | Scroll line down / up |
| `J` / `K` | Page scroll down / up |
| `Tab` / `Shift+Tab` | Cycle pane focus |
| `G` | Git actions panel |
| `H` | Health panel |
| `M` | Browse main branch |
| `P` | Projects panel |
| `r` | Run command |
| `R` | Add run command |
| `[` / `]` | Switch worktree tab |
| `f` | Toggle file tree |
| `s` | Toggle session list |
| `/` | Search / filter |
| `?` | Help overlay |
| `Cmd+C` / `Ctrl+C` | Copy selection |
| `Ctrl+C` / `Alt+C` | Cancel agent |
| `Ctrl+M` | Cycle model |
| `Ctrl+Q` | Quit |
