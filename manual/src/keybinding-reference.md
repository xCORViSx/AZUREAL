# Complete Keybinding Reference

This page is the canonical reference for every keybinding in AZUREAL. It is
organized by context -- the mode or panel that must be active for the keybinding
to apply. For conceptual explanations of the modal input system, see
[Keybindings & Input Modes](./keybindings.md).

> **Platform note:** This reference uses macOS modifier notation (`Cmd`, `Ctrl`,
> `Alt/Option`). On Windows and Linux, substitute `Ctrl` for `Cmd` and `Alt` for
> `Ctrl` in most cases. See
> [Platform Differences](./keybindings/platform-differences.md) for the full
> mapping table.

---

## Global (Command Mode)

These keybindings are available in command mode across the main worktree view.
They do **not** fire during text input (prompt mode, edit mode, terminal type
mode, or active filter/search inputs).

| Key | Action | Notes |
|-----|--------|-------|
| `Ctrl+Q` | Quit | Prompts confirmation if agents are running |
| `Ctrl+D` | Debug dump | Opens naming dialog, then writes state snapshot to file |
| `Ctrl+C` / `Alt+C` | Cancel agent | Kills the active agent in the current slot. macOS uses `Ctrl+C`; Windows/Linux uses `Alt+C` |
| `Cmd+C` / `Ctrl+C` | Copy selection | Copies from whichever pane has an active text selection. macOS uses `Cmd+C`; Windows/Linux uses `Ctrl+C` |
| `Ctrl+M` | Cycle model | Rotates through available models: opus, sonnet, haiku, gpt-5.4, and others |
| `?` | Help overlay | Opens the help overlay showing all keybindings for the current context |
| `p` | Enter prompt mode | Focuses the input area for typing a prompt. Closes the terminal if it is open |
| `T` | Toggle terminal | Opens or closes the embedded terminal pane |
| `G` | Git panel | Opens the git panel overlay |
| `H` | Health panel | Opens the health panel overlay |
| `M` | Browse main branch | Switches view to the main branch (read-only browse) |
| `P` | Projects panel | Opens the projects panel overlay |
| `r` | Run command | Opens the run-command picker, or executes directly if only one command is registered |
| `R` | Add run command | Opens the dialog to register a new run command |
| `[` / `]` | Switch worktree tab | Moves to the previous/next worktree tab. Wraps around at both ends |
| `f` | Toggle file tree | Shows or hides the file tree pane |
| `s` | Toggle session list | Opens the session list overlay when the session pane is focused |
| `/` | Search / filter | Context-dependent: filters the file tree, searches session text, or filters session list depending on focused pane |
| `Tab` / `Shift+Tab` | Cycle pane focus | Cycles focus through FileTree, Viewer, Session, and Input in order |

---

## Worktree Actions

Worktree operations use either a `W` leader key sequence (press `W`, then the
action key) or a direct key when the worktree tab row is focused. See
[Leader Sequences](./keybindings/leader-sequences.md) for details on the leader
key mechanism.

| Key | Action |
|-----|--------|
| `Wa` / `a` | Create new worktree |
| `Wr` / `r` | Rename worktree |
| `Wx` / `x` | Archive or unarchive worktree (toggles based on current state) |
| `Wd` / `d` | Delete worktree |

---

## Input / Prompt Mode

Active when the input area is focused for typing a prompt to an agent. The input
area border turns yellow to indicate prompt mode.

| Key | Action |
|-----|--------|
| `Enter` | Submit prompt | Sends the current text to the active agent |
| `Shift+Enter` | Insert newline | Adds a line break without submitting |
| `Esc` | Return to command mode | Exits prompt mode without sending |
| `Ctrl+S` / `Alt+S` | Toggle speech-to-text | Starts or stops audio capture for transcription. macOS uses `Ctrl+S`; Windows/Linux uses `Alt+S` |
| `Alt+P` | Preset prompts picker | Opens the preset prompt selection dialog |
| `Alt+1` through `Alt+9`, `Alt+0` | Quick-select preset | Inserts preset prompt 1-9 or 10 directly without opening the picker |
| `Up` / `Down` | Cursor navigation | Moves the text cursor up/down within multi-line input |
| `Left` / `Right` | Character navigation | Moves the cursor one character left/right |
| `Home` / `End` | Line start/end | Jumps to the beginning or end of the current line |
| `Ctrl+U` | Clear input | Deletes all text in the input area |

---

## Session Pane (Command Mode)

These keybindings apply when the session pane is focused and you are in command
mode (not typing a prompt).

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll line | Scroll session content down/up by one line |
| `J` / `K` | Page scroll | Scroll session content down/up by one page |
| `s` | Toggle session list | Opens the session list overlay |
| `/` | Search text | Enters search mode to find text within the session |
| `n` / `N` | Next/prev search match | Jumps to the next or previous occurrence of the search term |

---

## Session List Overlay

The session list is a modal overlay for browsing, loading, and managing saved
sessions. It appears over the session pane when `s` is pressed.

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate | Move selection up/down through the session list |
| `J` / `K` | Page scroll | Jump selection up/down by one page |
| `Enter` | Load session | Load the selected session into the session pane |
| `a` | New session | Create a new empty session |
| `r` | Rename session | Open rename dialog for the selected session |
| `/` | Filter by name | Enter filter mode to narrow the list by session name |
| `//` | Content search | Enter content search mode to search across session bodies |
| `s` / `Esc` | Close | Close the session list overlay |

---

## File Tree

These keybindings apply when the file tree pane is focused.

| Key | Action |
|-----|--------|
| `j` / `k` / `Up` / `Down` | Navigate | Move selection through the file tree |
| `Enter` | Open file / expand directory | Opens the selected file in the viewer, or expands/collapses a directory |
| `Backspace` | Collapse / go to parent | Collapses the current directory, or moves selection to the parent directory |
| `a` | Add file/directory | Opens dialog to create a new file or directory at the current location |
| `d` | Delete | Delete the selected file or directory |
| `r` | Rename | Rename the selected file or directory |
| `c` | Copy (clipboard mode) | Marks the selected item for copying; navigate to destination and paste |
| `m` | Move (clipboard mode) | Marks the selected item for moving; navigate to destination and paste |
| `O` | Options overlay | Opens the file tree options overlay |
| `/` | Filter | Enter filter mode to narrow the tree by filename |

---

## Viewer

These keybindings apply when the file viewer pane is focused in its default
read-only mode.

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll line | Scroll the file content down/up by one line |
| `J` / `K` | Page scroll | Scroll the file content down/up by one page |
| `e` | Enter edit mode | Switch to edit mode for the current file |
| `t` | Save to tab | Save the current file view as a persistent tab |
| `Alt+t` | Tab picker dialog | Open the tab picker to select from open tabs |
| `[` / `]` | Navigate tabs | Cycle through open viewer tabs |
| `x` | Close current tab | Close the currently active viewer tab |
| `Alt+Left` / `Alt+Right` | Cycle through edits | Navigate between edit locations in the diff viewer |

---

## Edit Mode

Active when the viewer is in edit mode (entered with `e` from the viewer). The
viewer border changes to indicate edit mode is active.

| Key | Action |
|-----|--------|
| Any printable key | Insert text | Characters are inserted at the cursor position |
| `Backspace` / `Delete` | Delete character | Removes the character before/after the cursor |
| Arrow keys | Move cursor | Moves the cursor in the corresponding direction |
| `Home` / `End` | Line start/end | Jumps to the beginning or end of the current line |
| `Cmd+S` / `Ctrl+S` | Save | Writes the file to disk. macOS uses `Cmd+S`; Windows/Linux uses `Ctrl+S` |
| `Cmd+Z` / `Ctrl+Z` | Undo | Reverts the last edit |
| `Cmd+Shift+Z` / `Ctrl+Y` | Redo | Re-applies the last undone edit. macOS uses `Cmd+Shift+Z`; Windows/Linux uses `Ctrl+Y` |
| `Ctrl+S` / `Alt+S` | Speech-to-text | Starts or stops audio transcription into the editor. macOS uses `Ctrl+S`; Windows/Linux uses `Alt+S` |
| `Esc` | Exit edit mode | Returns to read-only viewer mode |

---

## Terminal Command Mode

Active when the terminal pane is visible and focused, but you have **not**
entered type mode. The terminal border turns azure. All global keybindings
continue to work in this mode.

| Key | Action |
|-----|--------|
| `t` | Enter type mode | Begin typing directly into the PTY shell |
| `Esc` | Close terminal | Closes the terminal pane (returns to normal layout) |
| `+` / `-` | Resize height | Increase or decrease the terminal pane height |
| All globals | Work normally | Global keybindings (`G`, `H`, `P`, `[`, `]`, etc.) are active |

---

## Terminal Type Mode

Active when the terminal is in type mode (entered with `t` from terminal command
mode). All keystrokes are forwarded to the PTY except the escape and navigation
overrides listed below.

| Key | Action |
|-----|--------|
| `Esc` | Exit type mode | Returns to terminal command mode |
| `Alt+Left` / `Alt+Right` | Word navigation | Jump one word left/right within the terminal input |
| `Ctrl+Left` / `Ctrl+Right` | Word navigation | Jump one word left/right (alternative binding) |
| All other keys | Forward to PTY | Keystrokes are sent directly to the shell process |

---

## Git Panel

The git panel is a modal overlay opened with `G`. It has three focus areas --
Actions, Files, and Commits -- cycled with `Tab`/`Shift+Tab`. Some keybindings
are context-dependent based on which area is focused and whether the current
worktree is the main branch or a feature branch.

### Actions Pane

| Key | Action | Context |
|-----|--------|---------|
| `l` | Pull | Main branch only |
| `m` | Squash merge | Feature branch only |
| `R` | Rebase | Feature branch only |
| `c` | Commit | Any branch |
| `P` | Push | Any branch |
| `a` | Toggle auto-rebase | Feature branch only |
| `s` | Auto-resolve settings | Feature branch only |

### Files Pane

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate | Move selection through the changed files list |
| `s` | Toggle stage/unstage | Stage or unstage the selected file |
| `S` | Stage/unstage all | Stage or unstage all changed files at once |
| `x` | Discard changes | Discard uncommitted changes for the selected file |
| `Enter` / `d` | View diff | Open the diff for the selected file in the viewer |

### Commits Pane

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate | Move selection through the commit history |
| `Enter` / `d` | View diff | Open the diff for the selected commit |

### Panel-Level

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Cycle pane focus | Rotate focus: Actions, Files, Commits |
| `[` / `]` | Switch worktree | View git state for a different worktree |
| `{` / `}` | Jump page | Page through the commit or file list |
| `r` / `R` | Refresh | Refresh git state for the current view |
| `Cmd+A` / `Ctrl+A` | Select all (viewer) | Select all text in the diff viewer |
| `Cmd+C` / `Ctrl+C` | Copy selection | Copy selected text from the diff viewer |
| `J` / `PageDown` | Page down diff | Scroll the diff viewer down by one page |
| `K` / `PageUp` | Page up diff | Scroll the diff viewer up by one page |
| `Esc` | Close panel | Close the git panel overlay |

---

## Commit Editor

The commit editor appears when `c` is pressed in the git panel actions pane.
It provides a text input area for composing a commit message (optionally
pre-filled with an AI-generated message).

| Key | Action |
|-----|--------|
| `Enter` | Commit | Create the commit with the current message |
| `Cmd+P` | Commit + push | Create the commit and immediately push to the remote |
| `Shift+Enter` | Insert newline | Add a line break in the commit message |
| `Esc` | Cancel | Close the commit editor without committing |

---

## Health Panel

The health panel is a modal overlay opened with `H`. It has two tabs -- God
Files and Documentation -- switched with `Tab`. Some keybindings differ by tab.

### Both Tabs

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate | Move selection through the list |
| `J` / `K` | Page scroll | Jump selection up/down by one page |
| `Alt+Up` / `Alt+Down` | Jump top/bottom | Move selection to the first or last item |
| `Space` | Toggle check | Check or uncheck the selected item |
| `a` | Toggle all | Check or uncheck all items |
| `v` | View checked as tabs | Open all checked items as viewer tabs |

### Tab-Specific

| Key | Action | Tab |
|-----|--------|-----|
| `Enter` / `m` | Modularize | God Files -- spawn an agent to split the selected file |
| `Enter` / `m` | Document | Documentation -- spawn an agent to document the selected item |

### Panel-Level

| Key | Action |
|-----|--------|
| `s` | Scope mode | Toggle scope mode for filtering by directory |
| `Tab` | Switch tab | Alternate between God Files and Documentation tabs |
| `Esc` | Close panel | Close the health panel overlay |

---

## Projects Panel

The projects panel is a modal overlay opened with `P`. When AZUREAL starts
outside of a git repository, this panel appears automatically as a full-screen
view.

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate | Move selection through the project list |
| `Enter` | Switch to project | Load the selected project (snapshots current state, restores target) |
| `a` | Add project | Open dialog to register a new project by path |
| `d` | Delete from list | Remove the selected project from the registered list |
| `n` | Rename display name | Change the display name shown in the project list and tab |
| `i` | Initialize git repo | Initialize a new git repository at the selected path |
| `Esc` | Close | Close the projects panel overlay |
| `Ctrl+Q` | Quit | Quit AZUREAL entirely |

---

## Quick Reference by Key

The table below lists every single-key binding and its meaning across different
contexts. This is useful for answering "what does this key do?" when you are
unsure of your current context.

| Key | Command Mode | Session Pane | File Tree | Viewer | Git Panel | Health Panel | Projects Panel |
|-----|-------------|--------------|-----------|--------|-----------|--------------|----------------|
| `a` | -- | -- | Add file | -- | Auto-rebase / Toggle all | Toggle all | Add project |
| `c` | -- | -- | Copy (clipboard) | -- | Commit | -- | -- |
| `d` | -- | -- | Delete | -- | View diff | -- | Delete project |
| `e` | -- | -- | -- | Edit mode | -- | -- | -- |
| `f` | Toggle file tree | -- | -- | -- | -- | -- | -- |
| `i` | -- | -- | -- | -- | -- | -- | Init git repo |
| `j` | -- | Scroll down | Navigate down | Scroll down | Navigate down | Navigate down | Navigate down |
| `k` | -- | Scroll up | Navigate up | Scroll up | Navigate up | Navigate up | Navigate up |
| `l` | -- | -- | -- | -- | Pull (main) | -- | -- |
| `m` | -- | -- | Move (clipboard) | -- | Squash merge | Modularize/Document | -- |
| `n` | -- | Next match | -- | -- | -- | -- | Rename |
| `p` | Prompt mode | -- | -- | -- | -- | -- | -- |
| `r` | Run command | -- | Rename | -- | Refresh | -- | -- |
| `s` | Session list | Session list | -- | -- | Stage / Auto-resolve / Scope | Scope mode | -- |
| `t` | -- | -- | -- | Save to tab | -- | -- | -- |
| `v` | -- | -- | -- | -- | -- | View as tabs | -- |
| `x` | -- | -- | -- | Close tab | Discard changes | -- | -- |
| `?` | Help overlay | -- | -- | -- | -- | -- | -- |
| `/` | Search/filter | Search | Filter | -- | -- | -- | -- |

---

## Platform Modifier Summary

For quick reference, here is the modifier key mapping between platforms. The
full explanation is at
[Platform Differences](./keybindings/platform-differences.md).

| Action | macOS | Windows / Linux |
|--------|-------|-----------------|
| Copy | `Cmd+C` | `Ctrl+C` |
| Cancel agent | `Ctrl+C` | `Alt+C` |
| Save | `Cmd+S` | `Ctrl+S` |
| Undo | `Cmd+Z` | `Ctrl+Z` |
| Redo | `Cmd+Shift+Z` | `Ctrl+Y` |
| Select all | `Cmd+A` | `Ctrl+A` |
| STT (edit/prompt) | `Ctrl+S` | `Alt+S` |

Non-modifier keys (`p`, `j`, `k`, `T`, `G`, `?`, `/`, etc.) are identical on
all platforms. When in doubt, press `?` to see the correct bindings for your
system.
