//! Session list overlay helpers
//!
//! Handles opening the session list overlay, computing message counts
//! (two-phase load), and fast JSONL message counting.

use crate::app::App;

/// Open session list overlay — scoped to the currently selected worktree only.
/// Phase 1: show the overlay + loading indicator, refresh file list (fast).
/// Phase 2 (finish_session_list_load) runs on the next event loop iteration
/// so the loading dialog renders before the expensive message count I/O starts.
pub(super) fn open_session_list(app: &mut App) {
    app.show_session_list = true;
    app.session_list_loading = true;
    app.session_list_selected = 0;
    app.session_list_scroll = 0;
    // Refresh file list immediately (cheap directory listing)
    if let Some(session) = app.current_worktree() {
        let branch = session.branch_name.clone();
        if let Some(ref wt_path) = app.worktrees[app.selected_worktree.unwrap()].worktree_path {
            let files = crate::config::list_claude_sessions(wt_path);
            app.session_files.insert(branch, files);
        }
    }
}

/// Phase 2 of session list loading — compute message counts (expensive I/O).
/// Called from event loop after the loading dialog has had a chance to render.
pub fn finish_session_list_load(app: &mut App) {
    if let Some(session) = app.current_worktree() {
        let branch = session.branch_name.clone();
        if let Some(files) = app.session_files.get(&branch) {
            for (session_id, path, _) in files.iter() {
                let file_size = path.metadata().map(|m| m.len()).unwrap_or(0);
                if let Some(&(_, cached_size)) = app.session_msg_counts.get(session_id.as_str()) {
                    if cached_size == file_size { continue; }
                }
                let count = count_messages_in_jsonl(path);
                app.session_msg_counts.insert(session_id.clone(), (count, file_size));
            }
        }
    }
    app.session_list_loading = false;
}

/// Count message bubbles in a JSONL session file for the session list [N msgs] badge.
/// Uses fast string scanning (no JSON parsing) — "type":"user" and "type":"assistant"
/// have zero false positives in Claude Code's compact JSON output.
/// Skips isMeta, tool_result arrays, command hooks, and compaction summaries.
/// ParentUuid dedup skipped for speed (rare rewind case, off by ≤2).
fn count_messages_in_jsonl(path: &std::path::Path) -> usize {
    let Ok(content) = std::fs::read_to_string(path) else { return 0; };
    let mut count = 0usize;
    for line in content.lines() {
        if line.contains("\"type\":\"user\"") {
            // Skip system-generated meta messages
            if line.contains("\"isMeta\":true") { continue; }
            // Skip tool_result lines — only string content creates bubbles
            // Tool result user lines contain {"type":"tool_result",...} blocks
            if line.contains("\"type\":\"tool_result\"") { continue; }
            // Skip non-bubble user events the parser also skips
            if line.contains("<local-command-caveat>") { continue; }
            if line.contains("<local-command-stdout>") { continue; }
            if line.contains("<command-name>") { continue; }
            if line.contains("This session is being continued from a previous conversation") { continue; }
            count += 1;
        } else if line.contains("\"type\":\"assistant\"") {
            // Only count lines with a text content block (those become AssistantText bubbles)
            if line.contains("\"type\":\"text\"") { count += 1; }
        }
    }
    count
}
