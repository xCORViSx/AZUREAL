#!/usr/bin/env bash
# Enforces the centralized keybinding system in keybindings.rs.
# Called as a PreToolUse hook on Edit/Write operations.
#
# Checks 3 categories:
#   1. input_*.rs / event_loop: raw KeyCode/KeyModifiers usage → must use lookup_*_action()
#   2. draw_*.rs: hardcoded key label strings → must use hint generators
#   3. keybindings.rs: new binding array → remind to create lookup + hints
#
# Environment variables (set by Claude Code):
#   CLAUDE_TOOL_ARG_FILE_PATH  — target file path
#   CLAUDE_TOOL_ARG_NEW_STRING — Edit tool's new_string (partial content)
#   CLAUDE_TOOL_ARG_CONTENT    — Write tool's content (full file)

FILE="$CLAUDE_TOOL_ARG_FILE_PATH"
CONTENT="$CLAUDE_TOOL_ARG_NEW_STRING$CLAUDE_TOOL_ARG_CONTENT"

# Skip if no file or no content to check
[ -z "$FILE" ] || [ -z "$CONTENT" ] && exit 0

case "$FILE" in

  # ── 1. Input handlers: detect raw key matching ──
  */src/tui/input_*.rs|*/src/tui/event_loop/actions.rs)
    # Check for raw KeyCode::/KeyModifiers:: usage
    if echo "$CONTENT" | grep -qE '(KeyCode::|KeyModifiers::|match.*key\.(modifiers|code).*,.*key\.(code|modifiers))'; then
      echo 'KEYBINDING VIOLATION: Raw KeyCode/KeyModifiers matching detected in an input handler.
ALL keybindings must go through the centralized system in src/tui/keybindings.rs:
  1) Define binding in a static array (HEALTH_SHARED, GIT_ACTIONS, PROJECTS_BROWSE, PICKER, etc.)
  2) Add Action variant if needed
  3) Use the per-modal lookup function (lookup_health_action, lookup_git_action, etc.)
  4) Match on Action variants, not raw keys
EXCEPTIONS that may use raw KeyCode:
  - Text input chars (Char/Backspace in typing modes)
  - Number quick-select (1-9/0 in pickers)
  - Confirm-delete y/n prompts'
    fi
    ;;

  # ── 2. Draw functions: detect hardcoded key labels ──
  */src/tui/draw_*.rs)
    # Check for hardcoded single-char key labels in Span::styled() calls
    # that should come from keybindings hint generators instead
    if echo "$CONTENT" | grep -qE 'Span::styled\("(Enter|Esc|Tab|Space|[a-zA-Z])[:"]'; then
      # But allow it if keybindings:: is also referenced (using the generators)
      if ! echo "$CONTENT" | grep -q 'keybindings::'; then
        echo 'KEYBINDING VIOLATION: Hardcoded key label detected in draw function.
Footer hints and key labels must come from keybindings.rs hint generators:
  - health_god_files_hints() / health_docs_hints()
  - git_actions_labels() / git_actions_footer()
  - projects_browse_hint_pairs()
  - picker_title() / dialog_footer_hint_pairs()
Never hardcode key strings in Span::styled() — define the binding in a
keybindings.rs array and create/use a hint generator function.'
      fi
    fi
    ;;

  # ── 3. keybindings.rs itself: remind about companion artifacts ──
  */src/tui/keybindings.rs)
    if echo "$CONTENT" | grep -qE 'pub static [A-Z_]+:.*\[Keybinding'; then
      echo 'KEYBINDING CHECKLIST: New binding array detected. Ensure you also create:
  1) A lookup_*_action(mods, code) function that searches this array
  2) Hint generator function(s) for draw files to source labels from
  3) Update help_sections() if the array should appear in the help overlay
  4) Update the Centralized Keybindings section in AGENTS.md'
    fi
    ;;

esac

exit 0
