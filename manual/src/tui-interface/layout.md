# Layout & Panes

AZUREAL's interface is composed of six distinct regions. Each has a fixed
purpose, and most have configurable behavior described in later chapters.

## Pane Map (Normal Mode)

```text
┌─ [★ main] │ [○ feat-a] │ [● feat-b] ┐   ← Worktree Tab Row (1 row)
├──────────┬───────────────┬───────────┤
│          │               │           │
│ FileTree │    Viewer     │  Session  │
│  (15%)   │    (50%)      │   (35%)   │
│          │               │           │
├──────────┴───────────────┤           │
│  Input / Terminal        │           │
├──────────────────────────┴───────────┤
│             Status Bar               │   ← Status Bar (1 row)
└──────────────────────────────────────┘
```

Width percentages are relative to the terminal width. The Session pane extends
the full height from below the tab row down to the status bar; the
Input/Terminal area shares its vertical space with the Viewer and FileTree.

---

## Worktree Tab Row

**Height:** 1 row, pinned to the top of the screen.

**Purpose:** Displays every worktree as a horizontal tab, giving at-a-glance
status for the entire project.

**Behavior:**

- Not focusable -- it sits outside the focus cycle.
- `[` and `]` globally switch between tabs (wrapping at both ends).
- Click any tab to select it.
- The `[★ main]` tab always appears first. It highlights in yellow when
  main-browse mode is active (Shift+M).

**Tab indicators:**

| Symbol | Meaning |
|--------|---------|
| `★` | Main branch |
| `●` (green) | Agent process running |
| `◐` (AZURE) | Unread session output |
| `○` | Normal / idle |
| `◇` (dim) | Archived worktree |

Priority order: Running overrides Unread, which overrides normal status.
Unread clears per-session when that specific session is viewed, and the branch
indicator disappears only when all unread sessions on that branch have been
seen.

**Styling:**

- Active tab: AZURE background, white foreground, bold.
- Inactive tab: gray text with status symbol prefix.
- Archived tab: dim gray text with `◇` prefix.

**Pagination:** Tabs use greedy packing. When they overflow the available
width, an `N/M` page indicator appears.

---

## FileTree (15%)

**Position:** Left column, below the tab row.

**Purpose:** An always-visible directory tree for the currently selected
worktree.

**Key features:**

- **Nerd Font icons** with automatic detection -- approximately 60 file types
  are mapped to language-brand colors (Rust in orange, Python in blue, and so
  on). If Nerd Font glyphs are not detected, the tree falls back to emoji
  icons.
- Expand and collapse directories.
- Double-click to open a file in the Viewer or expand/collapse a directory.
- Border title shows `Filetree (worktree_name)` with an optional
  `[pos/total]` scroll indicator when content overflows.
- File actions: add (`a`), delete (`d`), rename (`r`), copy (`c`), move (`m`)
  via an inline action bar at the bottom of the pane.
- Options overlay (`O`): toggle visibility of dotfiles and common hidden
  entries. Settings persist to the project's `azufig.toml`.

---

## Viewer (50%)

**Position:** Center column, below the tab row.

**Purpose:** A dual-purpose content display -- it renders either file content
or diff detail, depending on context.

**File viewing:**

- Syntax-highlighted source with line numbers (25 languages via tree-sitter).
- Markdown files render with prettified formatting: styled headers, bullets,
  numbered lists, blockquotes, syntax-highlighted code blocks, and box-drawn
  tables. Line numbers are hidden for markdown.
- Image files render via terminal graphics protocol (Kitty, Sixel, or
  halfblock fallback). Images auto-fit the viewport.

**Diff viewing:**

- When a diff is selected in the Session pane, the Viewer switches to show
  diff detail with syntax highlighting.

**Viewer Tabs:**

- Up to 12 tabs across 2 rows (6 per row, fixed-width).
- `t` saves the current file to a tab; `⌥t` opens the tab dialog.
- `[` / `]` navigate between tabs; `x` closes the active tab.
- The tab bar renders inside the Viewer border at rows 1-2, overlaying empty
  padding so content shifts down accordingly.

---

## Session (35%, Full Height)

**Position:** Right column, spanning from below the tab row to above the
status bar.

**Purpose:** Displays the agent conversation output -- prompts, responses, and
tool call results from Claude or Codex sessions.

**Border titles (three positions):**

| Position | Content |
|----------|---------|
| Left | `[x/y]` -- current message position in the session |
| Center | `[session name]` -- custom name or truncated UUID |
| Right | Token usage badge + PID or exit code |

- Token usage is color-coded: green below 60%, yellow at 60-80%, red above
  80%.
- PID shows in green while the agent process is running; switches to exit code
  on completion (green for 0, red for non-zero).

**Session list overlay (`s`):** Replaces the conversation view with a session
file browser, showing status dot, session name, last modified time, and
message count. `j/k` navigate, `Enter` loads a session, `a` starts a new one.
`/` activates name filter; `//` switches to content search.

---

## Input / Terminal

**Position:** Below the FileTree and Viewer, spanning their combined width.

**Purpose:** Accepts prompt input for the agent, or hosts an embedded terminal
emulator. Only one mode is active at a time -- the area toggles between prompt
input and the terminal.

- In prompt mode, text input supports word-wrapped editing with cursor
  positioning via mouse click.
- In terminal mode, a full embedded terminal occupies the same space.

---

## Status Bar

**Height:** 1 row, pinned to the bottom of the screen.

**Layout (three sections):**

| Section | Content |
|---------|---------|
| Left | Worktree status dot + display name + branch (branch hidden when identical to name) |
| Center | Status messages (clickable -- copies message to system clipboard) |
| Right | CPU% + PID badge, rendered in AZURE (`#3399FF`) |

The status bar stores its rect for mouse hit-testing so clicks on the center
section can copy the current message.

---

## Git Mode Layout

Pressing Shift+G replaces the normal layout:

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

- **Left column:** Split into an Actions section (stage, commit, push, etc.)
  and a Changed Files list.
- **Center:** The Viewer, showing diff content for the selected file.
- **Right column:** Commit history for the current branch.
- **Bottom bar:** A minimal git-mode status line with cycling hints.

The tab row persists at the top, identical in behavior to normal mode.
