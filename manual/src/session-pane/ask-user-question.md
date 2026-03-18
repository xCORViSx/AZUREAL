# AskUserQuestion

When Claude invokes the `AskUserQuestion` tool, the session pane renders an
interactive question prompt as a centered, bordered box. This tool is used when
Claude needs clarification or wants the user to choose between options before
proceeding.

---

## Visual Layout

The question box renders with magenta borders and a structured layout:

```text
┌────────────────────────────────────────────────────────┐
│ ? Which approach should I take for the refactor?       │
├────────────────────────────────────────────────────────┤
│ 1. Option A                                            │
│    Rename the module and update all imports             │
│ 2. Option B                                            │
│    Create a wrapper module for backward compatibility   │
│ 3. Other (type your answer)                            │
└────────────────────────────────────────────────────────┘
```

**Structure:**

- **Top border** with magenta box-drawing characters.
- **Question header** with a `?` icon, rendered in bold white. Long questions
  wrap within the box width.
- **Separator** (`├──...──┤`) dividing the question from the options.
- **Numbered options** -- each option has a label in AZURE and an optional
  description in dim gray beneath it. Descriptions are indented and wrap
  within the box.
- **Implicit "Other"** -- an "Other (type your answer)" entry always appears
  as the last numbered option, allowing free-form responses.
- **Bottom border** closing the box.

**Multi-select:** When the question supports multiple selections, the header
icon changes from `?` to a checkbox icon to indicate that more than one option
can be chosen.

---

## Box Width

The question box width is capped at 60 characters or the panel width minus 4,
whichever is smaller. This ensures the box fits comfortably within the session
pane without overflowing.

---

## Responding

When the `awaiting_ask_user_question` flag is true, the user's next prompt
response receives a hidden context prefix that lists the questions and options.
The user does not need to restate the question -- they simply type a number to
select an option or type custom text for the "Other" choice.

The hidden context is invisible to the user in the session pane. It exists
solely to give the agent the necessary context to interpret a bare number
(like "2") as a selection from the presented options.

---

## Multiple Questions

Each question in the `questions` array gets its own separate box. If Claude
asks multiple questions in a single `AskUserQuestion` call, they render as
stacked boxes with spacing between them.

---

## Edge Cases

- **No options:** When the `options` field is null or absent, only the "Other"
  entry appears (numbered as 1).
- **Missing labels:** If an option lacks a `label` field, a `?` placeholder
  is shown.
- **Empty descriptions:** Options without descriptions show only the label
  line with no indented description beneath.
- **Narrow terminal:** The box width scales down gracefully. Text wrapping
  ensures content remains readable even at constrained widths.
