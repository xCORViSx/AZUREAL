# God Files

The God Files tab identifies source files that have grown beyond a maintainable
size. Any source file exceeding **1000 lines of production code** appears in this
list. You can select one or more files and spawn concurrent agent sessions to
break them into well-structured modules.

---

## Detection Logic

### Line Counting

The scanner counts production lines of code, not total file length. For **Rust
files**, any `#[cfg(test)]` block and its contents are excluded from the count.
This means a 1200-line Rust file with a 300-line test module registers as 900
production lines and does not appear in the results.

For all other languages, the entire file length is counted.

### Source Root Detection

The scanner looks for well-known source directories at the project root:

```text
src/  lib/  crates/  packages/  app/  cmd/  pkg/  internal/  ...
```

Approximately 16 directory names are recognized. If none are found, the scanner
falls back to scanning the entire project root.

### Excluded Directories

Approximately 75 directories are automatically skipped during scanning. These
include build output directories (`target/`, `node_modules/`, `dist/`, `.git/`,
etc.) and other non-source paths that would produce false positives.

### Recognized Extensions

The scanner recognizes approximately 75 source file extensions (`.rs`, `.py`,
`.ts`, `.js`, `.go`, `.java`, `.c`, `.cpp`, and many more). Files with
unrecognized extensions are ignored.

---

## Results List

Detected god files are displayed in a sorted list, ordered by **line count
descending** (largest files first). Each entry shows the file path relative to
the project root and its production line count.

---

## Keybindings

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate up / down one entry |
| `J` / `K` | Page down / page up |
| `Alt+Up` / `Alt+Down` | Jump to top / bottom |
| `Space` | Toggle checkbox on the highlighted file |
| `a` | Toggle all checkboxes (select all / deselect all) |
| `v` | View all checked files as Viewer tabs (up to 12) |
| `Enter` or `m` | Modularize all checked files |

---

## Modularization

When you press **`Enter`** or **`m`** with one or more files checked, AZUREAL
spawns agent sessions to break those files into smaller modules.

### Module Style Selection

If any of the checked files are **Rust** or **Python**, a dialog appears asking
you to choose a module style:

| Language | Option A | Option B |
|----------|----------|----------|
| Rust | File-based module root (`foo.rs` + `foo/bar.rs`) | Directory module (`foo/mod.rs` + `foo/bar.rs`) |
| Python | Package (`foo/__init__.py` + submodules) | Single-file split |

For other languages, the dialog is skipped and modularization proceeds
immediately.

### Parallel Execution

Each checked file spawns its own agent session tagged with `[GFM]` (God File
Modularization). All sessions run concurrently. The agent receives the file
contents and instructions to decompose it into well-structured, appropriately
sized modules while preserving all existing behavior.

You can monitor progress in the session pane -- each `[GFM]` session appears as
a separate conversation.

---

## Quick Reference

```text
Shift+H       Open Health Panel
Tab           Switch to Documentation tab
j/k           Navigate results
J/K           Page down / page up
Alt+Up/Down   Jump to top / bottom
Space         Toggle checkbox
a             Toggle all checkboxes
v             View checked files as tabs
Enter / m     Modularize checked files
s             Enter Scope Mode
```
