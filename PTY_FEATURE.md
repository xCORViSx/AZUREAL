# PTY Process Spawning Feature

## Overview

This feature implements bidirectional PTY (pseudo-terminal) communication for managing Claude Code sessions. It enables interactive control of Claude processes, allowing users to send input, receive output, and manage running sessions dynamically.

## Key Components

### 1. ClaudeProcess Management (src/claude.rs)

**Enhanced Features:**
- **PTY Master Storage**: Maintains a HashMap of active PTY connections keyed by session ID
- **Bidirectional Communication**: Supports both reading output from and writing input to Claude processes
- **Session Lifecycle Management**: Tracks active sessions and automatically cleans up PTYs when processes exit

**Key Methods:**
- `spawn(session_id, working_dir, prompt, resume_session_id)` - Spawns a new Claude process with PTY
- `send_input(session_id, input)` - Sends interactive input to a running session
- `is_session_running(session_id)` - Checks if a session has an active PTY
- `stop_session(session_id)` - Terminates a running session by closing its PTY

### 2. TUI Integration (src/tui.rs)

**New Keybindings:**
- `i` - Send input to running session (when a session is active)
- `s` - Stop the currently running session
- `Enter` - Start a pending session or submit input when in input mode

**Dynamic Status Bar:**
- Shows different commands based on whether a session is running
- Provides contextual help for the current focus and session state

### 3. Application State (src/app.rs)

**New State:**
- `running_session_id: Option<String>` - Tracks which session is currently running

## Technical Implementation

### PTY Architecture

```rust
// PTY storage structure
active_ptys: Arc<Mutex<HashMap<String, Box<dyn MasterPty + Send>>>>
```

The implementation uses:
- **portable-pty**: Cross-platform PTY library
- **Thread-safe PTY management**: Arc<Mutex<>> for concurrent access
- **Automatic cleanup**: PTYs are removed from the map when processes exit

### Communication Flow

1. **Process Start**:
   - Create PTY pair (master/slave)
   - Spawn Claude process with slave PTY
   - Store master PTY in HashMap
   - Start reader thread for output
   - Start waiter thread for exit handling

2. **Input Handling**:
   - Retrieve PTY master from HashMap by session ID
   - Get writer handle from PTY
   - Write input with newline
   - Flush to ensure immediate delivery

3. **Output Handling**:
   - Clone PTY reader in separate thread
   - Read lines continuously
   - Parse and emit ClaudeEvent::Output
   - Save to database and display in TUI

4. **Process Cleanup**:
   - Wait for process exit in dedicated thread
   - Remove PTY from HashMap
   - Emit ClaudeEvent::Exited
   - Clear receiver and running session ID

## Usage

### Starting a Session

1. Navigate to a session in the TUI (use `j`/`k`)
2. Press `Enter` to start the session
3. Claude Code will spawn in a PTY

### Sending Input to Running Session

1. With a session running, press `i` to enter input mode
2. Type your input (e.g., answering a prompt, providing approval)
3. Press `Enter` to send
4. Input is written to the PTY and visible in the output

### Stopping a Session

1. With a session running, press `s` to stop
2. PTY is closed, process terminates
3. Session status is updated

## Benefits

1. **Interactive Control**: Full bidirectional communication with Claude processes
2. **Permission Handling**: Can approve/deny permissions dynamically
3. **Follow-up Prompts**: Send additional instructions without restarting
4. **Clean Architecture**: Centralized PTY management with automatic cleanup
5. **Thread Safety**: Concurrent access to PTY map without race conditions

## Future Enhancements

Possible improvements:
- PTY size configuration (rows/cols)
- Signal handling (SIGTERM, SIGINT)
- Input history and command recall
- Multiple simultaneous running sessions
- PTY output buffering and replay
- ANSI color/escape sequence handling
