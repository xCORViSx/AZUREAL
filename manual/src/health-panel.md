# Health Panel

The Health Panel is a diagnostic overlay that scans your codebase for structural
problems -- files that have grown too large, documentation that has fallen
behind. It gives you a quick read on the health of your project and provides
one-click actions to fix what it finds, powered by concurrent agent sessions.

Open the Health Panel with **`Shift+H`** from anywhere in AZUREAL.

---

## Layout

The Health Panel appears as a centered modal overlay, sized at **55% x 70%** of
the terminal (minimum 50 columns by 16 rows). The title bar reads
**"Health: \<worktree\>"**, showing the name of the currently active worktree.

The panel uses a **green accent color** (`Rgb(80, 200, 80)`) for its UI elements
-- borders, highlights, and active indicators.

---

## Tab Bar

The panel contains two tabs, displayed in a horizontal tab bar at the top:

| Tab | Purpose |
|-----|---------|
| **God Files** | Finds source files exceeding 1000 lines of production code |
| **Documentation** | Measures doc-comment coverage across all source files |

Press **`Tab`** to switch between tabs. The panel remembers which tab you were
viewing and reopens on that tab the next time you open it.

---

## Auto-Refresh

While the Health Panel is open, file changes on disk trigger an automatic
debounced rescan with a **500ms** delay. If you or an agent modifies a file, the
panel updates its results without requiring a manual refresh.

---

## Scope Mode

Press **`s`** while the Health Panel is open to enter Scope Mode, which lets you
restrict which directories the panel scans. See
[Scope Mode](./health-panel/scope-mode.md) for details.

---

## Chapter Contents

- **[God Files](./health-panel/god-files.md)** -- The God Files tab: detection
  logic, results list, and modularization actions.
- **[Documentation Health](./health-panel/documentation.md)** -- The
  Documentation tab: coverage scoring, per-file breakdown, and doc-generation
  sessions.
- **[Scope Mode](./health-panel/scope-mode.md)** -- Restricting the scan to
  specific directories.
