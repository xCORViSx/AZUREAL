# Tool Result Formats

Each tool type has a purpose-built result summary that balances information
density with readability. Results appear beneath their tool call node, indented
with AZURE pipe characters and connector lines (`│` for continuation, `└─` for
the final line).

When a tool produces no output, the result shows a green checkmark (`✓`)
indicating silent success. For Read tools specifically, empty output shows
`(empty file)` instead.

Failed tool results render entirely in red -- both the connector characters and
the result text -- to make errors immediately visible.

---

## Per-Tool Formats

### Read

Displays the file's first line, a line count summary, and the last non-empty
line:

```text
 ┃  │ fn main() {
 ┃  │   (42 lines)
 ┃  └─ }
```

The file path in the tool call header is an underlined orange clickable link
that opens the file in the Viewer. For single-line files, only that line is
shown. For two-line files, first and last are shown without the count.

### Bash

Shows the last 2 non-empty lines of output, since command results are
typically most meaningful at the end:

```text
 ┃  │ Compiling azureal v0.7.0
 ┃  └─ Finished release [optimized] target(s) in 12.34s
```

When the output is empty or all whitespace, a green checkmark (`✓`) is shown.

**Codex compatibility:** The `exec_command` and `write_stdin` tool names are
treated identically to `Bash`. Output wrapped in the Codex execution envelope
(`Chunk ID: ...`, `Output: ...`, `Process exited with code ...`) is
automatically unwrapped to show only the actual output.

### Edit

The file path is a clickable link. Beneath the tool call, an inline diff
preview shows the last 20 edit hunks:

- **Removed lines** are styled with gray text on a dim red background.
- **Added lines** are syntax-highlighted on a dim green background.
- Real file line numbers are shown alongside the diff lines for orientation.

Clicking the file path opens the diff in the Viewer pane with the old and new
strings pre-loaded.

### Write

Displays the file path as a clickable link, followed by a summary line showing
the line count, a checkmark, and the first comment found in the content:

```text
 ┃  └─ ✓ 127 lines  // Session pane rendering
```

The renderer searches for the first line matching a comment pattern (`//`,
`#`, `/*`, `"""`, `///`, `//!`) and shows it as a dim italic hint. If no
comment is found, the first line of content is used instead.

### Grep

Shows the first 3 matching lines, with a count of additional matches:

```text
 ┃  │ src/main.rs:42:    let app = App::new();
 ┃  │ src/lib.rs:10:     pub struct App {
 ┃  │ src/tui/mod.rs:5:  use crate::app::App;
 ┃  └─   (+7 more)
```

When there are 3 or fewer matches, all are shown without the overflow count.

### Glob

Shows the total file count as a single summary line:

```text
 ┃  └─ 23 files
```

### Task (Subagent)

Shows the first 5 lines of the subagent's response:

```text
 ┃  │ I've analyzed the codebase and found three issues:
 ┃  │ 1. Missing error handling in parse_config
 ┃  │ 2. Unused import in lib.rs
 ┃  │ 3. Test coverage gap in session_store
 ┃  │ All three have been fixed.
 ┃  └─   (+12 more lines)
```

When the response exceeds 5 lines, the overflow count is shown.

### WebFetch

Shows the page title and a content preview (default format -- first 3 lines).

### WebSearch

Shows the first 3 search results (default format).

### LSP

Shows the location and surrounding code context (default format).

### Other / Unknown Tools

Any tool not listed above falls back to showing the first 3 lines of output,
with an overflow count if there are more.

---

## System Reminder Stripping

Tool results sometimes include `<system-reminder>` blocks appended by the
backend. These are stripped before rendering -- only content before the first
`<system-reminder>` tag is shown.

---

## Width Constraints

All result text is truncated to fit within the available bubble width. The
maximum text width accounts for the 7-character prefix (` ┃  └─ ` or
` ┃  │ `), so content is truncated at `max_width - 8` characters. Truncated
lines end with an ellipsis.
