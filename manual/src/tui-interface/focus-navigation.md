# Focus & Navigation

AZUREAL uses a keyboard-driven focus model. One pane holds focus at any time,
and the focused pane receives all keyboard input. Visual borders update to
reflect which pane is active.

## Focus Cycle

**Tab** and **Shift+Tab** cycle focus through the four focusable panes:

```text
FileTree  →  Viewer  →  Session  →  Input
    ↑                                  │
    └──────────────────────────────────┘
```

- **Tab** moves forward (FileTree to Viewer to Session to Input, then wraps).
- **Shift+Tab** moves backward (the reverse order).
- The Worktree Tab Row and Status Bar are **not** part of the focus cycle --
  they are controlled by their own dedicated keys.

## Focus and Overlays

Several panes host overlay panels that replace their content temporarily
(session list, filetree options, help). The focus system interacts with
overlays as follows:

- **Tab closes overlays.** Pressing Tab while an overlay is open dismisses
  the overlay and advances focus to the next pane.
- **Shift+Tab preserves some overlays.** When the session list overlay is
  open and Shift+Tab would land on the FileTree, the overlay stays open. This
  allows quickly glancing at the file tree without losing your place in the
  session list.
- **Click outside dismisses overlays.** Clicking on any pane that is not the
  overlay's host pane closes the overlay and focuses the clicked pane.

## Mouse Focus

Clicking any pane immediately sets focus to that pane, bypassing the Tab
cycle. This is the fastest way to jump to a non-adjacent pane. See
[Mouse Support](./mouse-support.md) for the full click behavior reference.

## Worktree Tab Row Navigation

The tab row is outside the focus cycle but has its own navigation keys:

- `[` -- switch to the previous worktree tab (wraps around).
- `]` -- switch to the next worktree tab (wraps around).
- **Shift+M** -- toggle main-branch browse mode (highlights the `[★ main]`
  tab in yellow).
- **Click** any tab to select it directly.

Switching worktrees via the tab row does not change which pane has focus.

## Git Mode Navigation

In Git Mode (Shift+G), the focus cycle changes to match the git panel layout:

- **Tab** / **Shift+Tab** cycles through the three git-specific panes
  (Actions, Files, Commits). The Viewer always shows the diff for the
  selected file or commit but is not a separate focus target.
- The bottom bar shows `Tab/⇧Tab:cycle | Enter` as a reminder.
- **Shift+G** or **Esc** exits Git Mode and returns to the normal focus
  cycle.

## Pane-Specific Navigation

Each pane accepts its own set of keys when focused. A brief summary:

| Pane | Navigation |
|------|------------|
| FileTree | `j/k` to move, `Enter` to open, `h/l` to collapse/expand |
| Viewer | `j/k` to scroll, `Alt+Up`/`Alt+Down` for top/bottom |
| Session | `j/k` to scroll, `s` for session list, `Alt+Up`/`Alt+Down` for top/bottom |
| Input | Standard text editing, `Enter` to send prompt |

Detailed keybindings for each pane are covered in the
[Keybindings & Input Modes](../keybindings.md) chapter.
