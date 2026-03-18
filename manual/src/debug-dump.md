# Debug Dump

Debug Dump exports a snapshot of AZUREAL's internal state to a text file for
troubleshooting. The output contains parsing statistics, event breakdowns,
rendered output samples, and recent event history -- everything needed to
diagnose rendering or parsing issues. Sensitive data is automatically obfuscated
before writing.

---

## Creating a Dump

Press **`Ctrl+D`** to start a debug dump. A two-phase process follows:

1. **Naming dialog** -- A text input appears asking for a name. Type a short
   identifier (e.g., "broken-render", "missing-tool-result") and press
   **`Enter`**.
2. **Dump execution** -- The dump runs on the **next frame** after the dialog
   closes. This ensures the dialog itself does not appear in the captured state.

The output file is saved to:

```text
.azureal/debug-output-{name}.txt
```

For example, entering "broken-render" produces
`.azureal/debug-output-broken-render.txt` in the project root.

---

## Contents

The dump file includes the following sections:

| Section | Description |
|---------|-------------|
| **Parsing stats** | Counts of parsed events by type, error rates, timing data |
| **Event breakdown** | Summary of event types seen during the session |
| **Last 5 events** | The five most recent events in full detail |
| **Full rendered output** | The complete rendered session pane content as it appears on screen |

This gives you both the raw data (events, stats) and the visual result (rendered
output) in a single file, making it straightforward to identify where parsing
diverges from rendering.

---

## Obfuscation

Debug dumps are designed to be safe to share. All sensitive content is replaced
using **deterministic word substitution** -- each unique token in the output is
mapped to a replacement word, and the same token always maps to the same
replacement. This means:

- File paths, variable names, and code content are replaced with neutral words.
- The **structure** of the output is fully preserved -- you can still see event
  boundaries, nesting, formatting, and layout.
- **Tool names**, **event types**, **parsing statistics**, and structural
  metadata are preserved verbatim, since these are needed for diagnosis.

The deterministic mapping means that if the same variable name appears in
multiple places in the dump, it will have the same replacement word everywhere,
so patterns and relationships remain visible even in obfuscated output.

---

## Quick Reference

```text
Ctrl+D        Open debug dump naming dialog
Enter         Confirm name and write dump
Esc           Cancel
```

| Detail | Value |
|--------|-------|
| Output path | `.azureal/debug-output-{name}.txt` |
| Execution timing | Next frame after dialog close |
| Obfuscation | Deterministic word replacement |
| Preserved verbatim | Tool names, event types, parsing stats, structure |
| Replaced | File paths, code content, variable names, user text |
