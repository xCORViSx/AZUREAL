# Scope Mode

Scope Mode lets you restrict which directories the Health Panel scans. By
default, the panel scans the entire source tree. If your project has directories
you want to exclude from health analysis (vendored code, generated files,
third-party dependencies), Scope Mode lets you explicitly choose which
directories to include.

---

## Entering Scope Mode

Press **`s`** while the Health Panel is open. This is a panel-level keybinding
that works from either the God Files or Documentation tab.

---

## The Scope Interface

Scope Mode opens the **File Tree** in a special selection mode. The interface
looks and works like the normal file tree, but with green visual treatment to
distinguish it:

- **Green double-line border** surrounds the file tree.
- The title bar reads **"Health Scope (N dirs)"**, where N is the number of
  currently selected directories.
- **Green highlights** mark directories that are included in the scan scope.

### Directory Inheritance

When you include a directory, all of its **subdirectories are automatically
included**. You do not need to select each subdirectory individually. Selecting
`src/` implicitly includes `src/models/`, `src/views/`, and everything else
nested beneath it.

---

## Keybindings

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate the file tree |
| `Enter` | Toggle the highlighted directory in or out of scope |
| `Esc` | Save the current scope and return to the Health Panel |

---

## Persistence

When you press **`Esc`** to leave Scope Mode, two things happen:

1. The selected scope is **saved to `azufig.toml`** in the project's
   `.azureal/` directory. The scope persists across Health Panel opens and
   AZUREAL restarts.
2. The Health Panel **immediately rescans** using the updated scope, refreshing
   both the God Files and Documentation tabs with results filtered to the
   selected directories.

---

## Quick Reference

```text
s             Enter Scope Mode (from Health Panel)
Enter         Toggle directory in/out of scope
Esc           Save scope to azufig.toml and rescan
j/k           Navigate file tree
```
