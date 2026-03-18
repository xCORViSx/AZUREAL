# Help Overlay

Press `?` in command mode to open the help overlay -- a centered panel that lists all keybindings organized by section.

---

## What It Shows

The help overlay displays every keybinding defined in the centralized keybinding system, grouped into logical sections:

- **Mode switching** -- entering and exiting prompt mode, terminal mode, etc.
- **Navigation** -- scrolling, pane focus cycling, worktree tab switching.
- **Panels** -- opening the git panel, health panel, projects panel, main branch browser.
- **Toggles** -- file tree, session list.
- **Worktree operations** -- leader sequences and direct worktree keys.
- **Clipboard & control** -- copy, cancel, model cycling, quit.

Each entry shows the key (or key combination) and a short description of the action it performs. Modifier key symbols are rendered in the platform-native format -- Apple symbols on macOS, text labels on Windows and Linux (see [Platform Differences](./platform-differences.md)).

---

## What It Excludes

Modal panels that display their own keybinding hints in a visible footer are excluded from the help overlay. These panels are **self-documenting** -- the footer already tells you what each key does within that panel. Duplicating those bindings in the help overlay would add noise without adding information.

Examples of self-documenting panels:

- The git panel shows footer hints for staging, committing, pushing, etc.
- The health panel shows footer hints for navigation and actions.
- The projects panel shows footer hints for project switching and management.

If a panel is open and you need to know what keys are available, look at its footer. If you are in the main interface and need to know what keys are available, press `?`.

---

## Interaction

The help overlay is a read-only display. It does not accept text input or support search/filtering.

Press `Esc` or `?` again to close the overlay and return to command mode.

---

## Source of Truth

The help overlay reads from the same centralized keybinding definitions that `lookup_action()` uses at runtime. This means the overlay is always accurate -- it cannot drift out of sync with actual behavior because both the overlay and the input handler read from the same data.

If a keybinding is added, changed, or removed in the code, the help overlay reflects that change automatically. There is no separate help text to maintain.
