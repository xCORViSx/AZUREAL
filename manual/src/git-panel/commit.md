# Commit with AI Messages

Pressing `c` in the git panel triggers a commit workflow that uses your selected
AI model to generate a conventional commit message from the staged diff. The
message appears in an editable text area in the viewer pane, where you can
review it, modify it, and confirm the commit -- all without leaving the git
panel.

---

## The Commit Flow

When you press `c`, the following sequence runs:

1. **Stage** -- All files marked as staged in the UI are staged in the git
   index.
2. **Diff** -- AZUREAL captures the staged diff (`git diff --staged`).
3. **Generate** -- A background thread spawns to send the diff to the selected
   AI model with instructions to produce a conventional commit message.
4. **Display** -- The generated message fills an editable text area in the
   viewer pane.

Steps 1-3 happen automatically. You interact starting at step 4.

---

## AI Backend Selection

The backend used for message generation depends on your currently selected
model:

| Model Pattern | Backend | Command |
|---------------|---------|---------|
| `gpt-*` | Codex | `codex exec --ephemeral` |
| All others | Claude | `claude -p` |

The `--no-session-persistence` flag is passed to Claude to prevent the creation
of `.jsonl` session files for these ephemeral generation requests.

### Cross-Backend Fallback

If the primary backend fails (network error, timeout, API issue), AZUREAL
automatically retries with the alternate backend. A Claude failure retries with
Codex; a Codex failure retries with Claude. This fallback is transparent -- you
see the generated message regardless of which backend produced it.

---

## The Commit Editor

Once the AI generates a message, the viewer pane transforms into a commit
editor:

```text
╔═══════════════════════════════════════╗
║  feat: add retry logic for API calls  ║
║                                       ║
║  Implement exponential backoff with   ║
║  jitter for transient API failures.   ║
║  Maximum of 3 retries before          ║
║  propagating the error to the caller. ║
╚═══════════════════════════════════════╝
```

The editor supports full text editing with word-wrap. You can rewrite the
message entirely, tweak a word, or accept it as-is.

### Commit Editor Keybindings

| Key | Action |
|-----|--------|
| `Enter` | Commit with the current message |
| `Cmd+P` / `Ctrl+P` | Commit and push in one action |
| `Shift+Enter` | Insert a newline (for multi-line messages) |
| `Esc` | Cancel the commit |

Both `Enter` and `Cmd+P` use a **deferred pattern**: a loading popup renders
immediately while the git operation runs in the background. This prevents the
UI from appearing frozen during the commit or push.

---

## Message Cleanup

The AI-generated message goes through post-processing before it appears in the
editor:

- **Markdown code fences** are stripped. Some models wrap their output in
  triple-backtick fences; these are removed so the message is plain text
  suitable for a git commit.
- **Leading/trailing whitespace** is trimmed.

The result is a clean conventional commit message ready for review.

---

## Conventional Commit Format

The AI is instructed to produce messages following the conventional commit
specification:

```text
<type>(<optional scope>): <description>

<optional body>
```

Common types include `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, and
`ci`. The model infers the appropriate type from the diff content. You can
always override the type or any other part of the message in the editor before
confirming.
