# The TUI Interface

AZUREAL presents a ratatui-based terminal user interface organized around panes, a
worktree tab row, and a status bar. The interface has two primary layouts --
**Normal Mode** for day-to-day development and **Git Mode** (Shift+G) for
repository operations -- plus a collection of overlay panels that appear on
demand.

## Normal Mode

```text
┌─ [★ main] │ [○ feat-a] │ [● feat-b] ┐
├──────────┬───────────────┬───────────┤
│FileTree  │    Viewer     │           │
│  (15%)   │    (50%)      │Session(35%)│
├──────────┴───────────────┤           │
│  Input / Terminal        │           │
├──────────────────────────┴───────────┤
│             Status Bar               │
└──────────────────────────────────────┘
```

The Worktree Tab Row sits at the top, giving a persistent overview of every
active and archived worktree. Below it, three panes divide the workspace:
**FileTree** on the left (15% width), the **Viewer** in the center (50%), and
the **Session** pane on the right (35%, spanning the full height from tab row
to status bar). The **Input/Terminal** area sits beneath FileTree and Viewer,
sharing their combined width. The **Status Bar** occupies the final row.

## Git Mode

```text
╔════════════════════════════════════╗
║ [main] [feat-a] [feat-b] (tab bar) ║
╠══════════╦═══════════════╦═════════╣
║ Actions  ║   Viewer      ║Commits  ║
║──────────║               ║         ║
║ Files    ║               ║         ║
╠══════════╩═══════════════╩═════════╣
║ GIT: wt (Tab/⇧Tab:cycle | Enter)  ║
╚════════════════════════════════════╝
```

Pressing Shift+G replaces the normal layout with a dedicated git panel. The
left column splits into an Actions section and a Changed Files section, the
center remains the Viewer (showing diffs), and the right column displays a
Commit history. A minimal status bar at the bottom replaces the normal one.

## Visual Identity

All accent colors throughout the interface use the **AZURE** constant
(`#3399FF`), tying the color palette to the "AZUREAL" name. Active tabs,
borders, badges, and highlighted elements all share this single brand color.

## OS Terminal Title

The host terminal's title bar updates dynamically to reflect your context:

- No project loaded: `AZUREAL`
- Project active: `AZUREAL @ <project> : <branch>`

The title updates automatically on startup, session switch, and project switch,
and resets to empty on exit.

## Splash Screen

On launch, a 2x-scale block-character "AZUREAL" logo renders in AZURE while
git discovery, session parsing, and file I/O run in the background. A minimum
3-second display ensures the branding registers even on fast machines. The
splash is replaced by the full interface once the event loop starts.

## What This Chapter Covers

- [Layout & Panes](./tui-interface/layout.md) -- detailed breakdown of every
  pane, its dimensions, and its purpose.
- [Focus & Navigation](./tui-interface/focus-navigation.md) -- how keyboard
  focus cycles between panes and how overlays interact with focus.
- [Mouse Support](./tui-interface/mouse-support.md) -- click, scroll, and
  double-click behavior across every pane.
- [Text Selection & Copy](./tui-interface/text-selection.md) -- drag-to-select,
  clipboard copy, and how selection interacts with pane content.
