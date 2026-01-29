# Bug: `-p --resume` fails with "tool_use ids must be unique" when tools are invoked

## Description

When using `-p` (print mode) with `--resume` to continue a conversation, any turn that triggers tool usage fails with an API error about duplicate `tool_use` IDs. Interactive mode (without `-p`) works correctly with the same prompts.

## Environment

- Claude Code version: 2.1.19
- OS: macOS (Darwin 24.6.0)
- Shell: zsh

## Steps to Reproduce

```bash
# Step 1: Start a new session with a simple prompt
claude -p "hello" --verbose --output-format stream-json
# Note the session_id from the init event output

# Step 2: Resume with a prompt that triggers tools
claude -p "read README.md" --resume <session_id> --verbose --output-format stream-json
```

## Expected Behavior

The resumed session should execute the tool call and return results, maintaining conversation context.

## Actual Behavior

```
API Error: 400 {"type":"error","error":{"type":"invalid_request_error","message":"messages.3.content.1: `tool_use` ids must be unique"},"request_id":"req_..."}
```

## Additional Testing

| First Prompt | Resume Prompt | Result |
|--------------|---------------|--------|
| No tools (e.g., "hello") | No tools (e.g., "thanks") | ✅ Works |
| No tools | With tools (e.g., "read file.txt") | ❌ **FAILS** |
| With tools | No tools | ✅ Works |
| With tools | With tools | ❌ **FAILS** |

**Pattern: Any `-p --resume` that invokes tools fails.**

## Comparison with Interactive Mode

The same conversation flow works perfectly in interactive mode:

```bash
claude
# > hello
# > read README.md  ← Works fine
```

Interactive mode maintains session state in memory. The bug appears to be specific to how `-p --resume` reconstructs conversation history for the API call.

## Workarounds Attempted (None Worked)

- Using `--fork-session` with `--resume`
- Using `--continue` instead of `--resume <id>`
- Using `--session-id` with custom UUIDs
- Running in fresh directories with no prior Claude sessions
- Spawning via PTY (pseudo-terminal) instead of piped stdin/stdout
- Different permission modes

## Impact

This bug prevents programmatic wrappers from maintaining conversation context when tool usage is involved. Wrappers must either:
1. Lose conversation context (don't use `--resume`)
2. Avoid tool-triggering prompts on resumed turns (impractical)

## System Information

```
Claude Code v2.1.19
macOS 14.x
zsh shell
Opus 4.5 model
```

## Session Files

The stored session files (`~/.claude/projects/.../sessionid.jsonl`) do NOT contain duplicate `tool_use` IDs. The duplication appears to occur during the resume process when constructing the API request.
