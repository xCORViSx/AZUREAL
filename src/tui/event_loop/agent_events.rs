//! Claude process event handling
//!
//! Processes output, lifecycle, and error events from Claude subprocesses.
//! slot_id is the PID string — each Claude process gets a unique slot.
//! Also handles auto-sending staged prompts after Claude exits.

use anyhow::Result;
use crate::app::App;
use crate::backend::AgentProcess;
use crate::claude::AgentEvent;

/// Handle Claude process events for a specific slot (PID string).
/// After an exit event, auto-sends any staged prompt.
pub fn handle_claude_event(slot_id: &str, event: AgentEvent, app: &mut App, claude_process: &AgentProcess) -> Result<()> {
    let is_exit = matches!(event, AgentEvent::Exited { .. });

    match event {
        AgentEvent::Output(output) => app.handle_claude_output(slot_id, output.output_type, output.data),
        AgentEvent::Started { pid } => app.handle_claude_started(slot_id, pid),
        AgentEvent::SessionId(claude_session_id) => app.set_claude_session_id(slot_id, claude_session_id),
        AgentEvent::Exited { code } => app.handle_claude_exited(slot_id, code),
    }

    // Auto-send staged prompt after Claude exits
    if is_exit {
        if app.staged_prompt.is_some() {
            app.check_session_file();
            app.poll_session_file();
        }
        if let Some(prompt) = app.staged_prompt.take() {
            if let Some(wt_path) = app.current_worktree().and_then(|s| s.worktree_path.clone()) {
                let branch = app.current_worktree().map(|s| s.branch_name.clone()).unwrap_or_default();
                app.add_user_message(prompt.clone());
                app.process_session_chunk(&format!("You: {}\n", prompt));
                app.current_todos.clear();
                // Context injection for staged prompts (same as normal prompt flow)
                let (send_prompt, resume_id) = if app.current_session_id.is_some() {
                    let injected = app.current_session_id
                        .and_then(|sid| app.session_store.as_ref().map(|s| (sid, s)))
                        .and_then(|(sid, store)| store.build_context(sid).ok().flatten())
                        .map(|payload| crate::app::context_injection::build_context_prompt(&payload, &prompt))
                        .unwrap_or_else(|| prompt.clone());
                    (injected, None) // No --resume for store sessions
                } else {
                    (prompt.clone(), app.get_claude_session_id(&branch).cloned())
                };
                match claude_process.spawn(&wt_path, &send_prompt, resume_id.as_deref(), app.selected_model.as_deref()) {
                    Ok((rx, pid)) => {
                        if let Some(sid) = app.current_session_id {
                            app.pid_session_target.insert(pid.to_string(), (sid, wt_path.clone()));
                        }
                        app.register_claude(branch, pid, rx);
                        app.set_status("Running...");
                    }
                    Err(e) => app.set_status(format!("Failed to start: {}", e)),
                }
            }
        }
    }

    // Spawn compaction agent if threshold was exceeded during store_append_from_jsonl
    if is_exit {
        if let Some((session_id, wt_path)) = app.compaction_needed.take() {
            spawn_compaction_agent(app, claude_process, session_id, &wt_path);
        }
    }

    Ok(())
}

/// Spawn a background Claude agent to summarize the conversation for compaction.
/// The agent's receiver goes into `compaction_receivers` (not `agent_receivers`)
/// so its output is captured separately and never displayed in the session pane.
fn spawn_compaction_agent(app: &mut App, claude_process: &AgentProcess, session_id: i64, wt_path: &std::path::Path) {
    let store = match app.session_store.as_ref() {
        Some(s) => s,
        None => return,
    };

    // Find where the last compaction ended
    let last_compaction = store.latest_compaction(session_id).ok().flatten();
    let from_seq = last_compaction.as_ref().map(|c| c.after_seq + 1).unwrap_or(1);

    // Find boundary: compact everything BEFORE the last 3 user messages
    let boundary_seq = match store.compaction_boundary(session_id, from_seq, 3) {
        Ok(Some(b)) => b,
        _ => return, // Not enough user messages to compact
    };

    // Build payload with only the events to be summarized (before boundary)
    let events = match store.load_events_range(session_id, from_seq, boundary_seq) {
        Ok(e) if !e.is_empty() => e,
        _ => return,
    };

    let payload = crate::app::session_store::ContextPayload {
        compaction_summary: last_compaction.map(|c| c.summary),
        events,
    };
    let prompt = crate::app::context_injection::build_compaction_prompt(&payload);

    let compaction_model = match app.backend {
        crate::backend::Backend::Claude => "haiku",
        crate::backend::Backend::Codex => "codex-mini",
    };
    match claude_process.spawn(wt_path, &prompt, None, Some(compaction_model)) {
        Ok((rx, pid)) => {
            let pid_str = pid.to_string();
            app.compaction_receivers.insert(pid_str, (rx, session_id, boundary_seq));
        }
        Err(_) => {} // Compaction is best-effort
    }
}

/// Handle events from compaction agents. Returns true if any events were processed.
pub fn poll_compaction_agents(app: &mut App) -> bool {
    if app.compaction_receivers.is_empty() {
        return false;
    }

    let mut completed: Vec<(String, i64, i64)> = Vec::new();
    let mut had_events = false;

    for (pid, (rx, session_id, _boundary)) in &app.compaction_receivers {
        while let Ok(event) = rx.try_recv() {
            had_events = true;
            match event {
                crate::claude::AgentEvent::Output(output) => {
                    if matches!(output.output_type, crate::models::OutputType::Stdout) {
                        // Parse streaming JSON for assistant text content
                        if let Some(text) = extract_assistant_text(&output.data) {
                            app.compaction_output
                                .entry(pid.clone())
                                .or_default()
                                .push_str(&text);
                        }
                    }
                }
                crate::claude::AgentEvent::Exited { .. } => {
                    completed.push((pid.clone(), *session_id, *_boundary));
                }
                _ => {}
            }
        }
    }

    // Process completed compaction agents
    for (pid, session_id, boundary_seq) in completed {
        app.compaction_receivers.remove(&pid);
        if let Some(summary) = app.compaction_output.remove(&pid) {
            let summary = summary.trim();
            if !summary.is_empty() {
                if let Some(ref store) = app.session_store {
                    // Store compaction at the boundary — events after this seq remain as raw events
                    let _ = store.store_compaction(session_id, boundary_seq, summary);
                }
            }
        }
    }

    had_events
}

/// Extract assistant text content from a streaming JSON line.
/// Claude CLI outputs `{"type":"assistant","message":{"content":[{"type":"text","text":"..."}]}}`.
fn extract_assistant_text(data: &str) -> Option<String> {
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }
        let content = v.get("message")?.get("content")?.as_array()?;
        let mut text = String::new();
        for block in content {
            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                    text.push_str(t);
                }
            }
        }
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::{AgentEvent, AgentOutput};
    use crate::models::OutputType;

    // ── Helper: build a minimal App with a worktree so handle_claude_event can work ──

    fn app_with_worktree(branch: &str) -> App {
        let mut app = App::new();
        app.worktrees.push(crate::models::Worktree {
            branch_name: branch.to_string(),
            worktree_path: Some(std::path::PathBuf::from("/tmp/test-wt")),
            claude_session_id: None,
            archived: false,
        });
        app.selected_worktree = Some(0);
        app
    }

    // ── 1. Output event propagates data to app ──

    #[test]
    fn test_output_event_stdout() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let event = AgentEvent::Output(AgentOutput {
            output_type: OutputType::Stdout,
            data: "hello\n".into(),
        });
        let result = handle_claude_event("123", event, &mut app, &cp);
        assert!(result.is_ok());
    }

    #[test]
    fn test_output_event_stderr() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let event = AgentEvent::Output(AgentOutput {
            output_type: OutputType::Stderr,
            data: "warning\n".into(),
        });
        assert!(handle_claude_event("456", event, &mut app, &cp).is_ok());
    }

    #[test]
    fn test_output_event_system_type() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let event = AgentEvent::Output(AgentOutput {
            output_type: OutputType::System,
            data: "system msg\n".into(),
        });
        assert!(handle_claude_event("789", event, &mut app, &cp).is_ok());
    }

    // ── 2. Started event sets running state ──

    #[test]
    fn test_started_event_inserts_running_session() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let event = AgentEvent::Started { pid: 42 };
        handle_claude_event("42", event, &mut app, &cp).unwrap();
        assert!(app.running_sessions.contains("42"));
    }

    #[test]
    fn test_started_event_clears_exit_code() {
        let mut app = App::new();
        app.agent_exit_codes.insert("42".into(), 1);
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("42", AgentEvent::Started { pid: 42 }, &mut app, &cp).unwrap();
        assert!(!app.agent_exit_codes.contains_key("42"));
    }

    #[test]
    fn test_started_event_sets_status() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("42", AgentEvent::Started { pid: 42 }, &mut app, &cp).unwrap();
        assert!(app.status_message.is_some());
        assert!(app.status_message.as_ref().unwrap().contains("started"));
    }

    // ── 3. SessionId event stores session UUID ──

    #[test]
    fn test_session_id_event() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let event = AgentEvent::SessionId("uuid-abc-123".into());
        handle_claude_event("42", event, &mut app, &cp).unwrap();
        assert_eq!(app.agent_session_ids.get("42"), Some(&"uuid-abc-123".to_string()));
    }

    #[test]
    fn test_session_id_overwrites_previous() {
        let mut app = App::new();
        app.agent_session_ids.insert("42".into(), "old-uuid".into());
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("42", AgentEvent::SessionId("new-uuid".into()), &mut app, &cp).unwrap();
        assert_eq!(app.agent_session_ids.get("42").unwrap(), "new-uuid");
    }

    // ── 4. Exited event cleans up running state ──

    #[test]
    fn test_exited_event_removes_running_session() {
        let mut app = App::new();
        app.running_sessions.insert("42".into());
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("42", AgentEvent::Exited { code: Some(0) }, &mut app, &cp).unwrap();
        assert!(!app.running_sessions.contains("42"));
    }

    #[test]
    fn test_exited_event_stores_exit_code() {
        let mut app = App::new();
        app.running_sessions.insert("42".into());
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("42", AgentEvent::Exited { code: Some(1) }, &mut app, &cp).unwrap();
        assert_eq!(app.agent_exit_codes.get("42"), Some(&1));
    }

    #[test]
    fn test_exited_event_code_none() {
        let mut app = App::new();
        app.running_sessions.insert("42".into());
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("42", AgentEvent::Exited { code: None }, &mut app, &cp).unwrap();
        // No code stored when None
        assert!(!app.agent_exit_codes.contains_key("42"));
    }

    // ── 5. Auto-send staged prompt after exit ──

    #[test]
    fn test_exit_without_staged_prompt_is_noop() {
        let mut app = app_with_worktree("feature");
        app.staged_prompt = None;
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("42", AgentEvent::Exited { code: Some(0) }, &mut app, &cp).unwrap();
        assert!(app.staged_prompt.is_none());
    }

    #[test]
    fn test_exit_with_staged_prompt_takes_it() {
        let mut app = app_with_worktree("feature");
        app.staged_prompt = Some("build it".into());
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("42", AgentEvent::Exited { code: Some(0) }, &mut app, &cp).unwrap();
        // Staged prompt consumed even if spawn fails (no wt_path match or spawn error)
        assert!(app.staged_prompt.is_none());
    }

    #[test]
    fn test_exit_staged_prompt_clears_todos() {
        let mut app = app_with_worktree("feature");
        app.current_todos.push(crate::app::TodoItem {
            content: "old".into(),
            status: crate::app::TodoStatus::Pending,
            active_form: "".into(),
        });
        app.staged_prompt = Some("next".into());
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("42", AgentEvent::Exited { code: Some(0) }, &mut app, &cp).unwrap();
        assert!(app.current_todos.is_empty());
    }

    // ── 6. Non-exit events don't trigger auto-send ──

    #[test]
    fn test_started_does_not_consume_staged_prompt() {
        let mut app = app_with_worktree("feature");
        app.staged_prompt = Some("later".into());
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("42", AgentEvent::Started { pid: 42 }, &mut app, &cp).unwrap();
        assert_eq!(app.staged_prompt.as_deref(), Some("later"));
    }

    #[test]
    fn test_session_id_does_not_consume_staged_prompt() {
        let mut app = app_with_worktree("feature");
        app.staged_prompt = Some("later".into());
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("42", AgentEvent::SessionId("sid".into()), &mut app, &cp).unwrap();
        assert_eq!(app.staged_prompt.as_deref(), Some("later"));
    }

    #[test]
    fn test_output_does_not_consume_staged_prompt() {
        let mut app = app_with_worktree("feature");
        app.staged_prompt = Some("later".into());
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let event = AgentEvent::Output(AgentOutput {
            output_type: OutputType::Stdout,
            data: "data".into(),
        });
        handle_claude_event("42", event, &mut app, &cp).unwrap();
        assert_eq!(app.staged_prompt.as_deref(), Some("later"));
    }

    // ── 7. Return value is always Ok ──

    #[test]
    fn test_return_ok_on_output() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let event = AgentEvent::Output(AgentOutput {
            output_type: OutputType::Stdout,
            data: "x".into(),
        });
        assert!(handle_claude_event("1", event, &mut app, &cp).is_ok());
    }

    #[test]
    fn test_return_ok_on_started() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        assert!(handle_claude_event("1", AgentEvent::Started { pid: 1 }, &mut app, &cp).is_ok());
    }

    #[test]
    fn test_return_ok_on_exited() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        assert!(handle_claude_event("1", AgentEvent::Exited { code: Some(0) }, &mut app, &cp).is_ok());
    }

    #[test]
    fn test_return_ok_on_session_id() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        assert!(handle_claude_event("1", AgentEvent::SessionId("s".into()), &mut app, &cp).is_ok());
    }

    // ── 8. Multiple events in sequence ──

    #[test]
    fn test_start_then_exit_lifecycle() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("99", AgentEvent::Started { pid: 99 }, &mut app, &cp).unwrap();
        assert!(app.running_sessions.contains("99"));
        handle_claude_event("99", AgentEvent::Exited { code: Some(0) }, &mut app, &cp).unwrap();
        assert!(!app.running_sessions.contains("99"));
    }

    #[test]
    fn test_full_lifecycle_start_session_output_exit() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("10", AgentEvent::Started { pid: 10 }, &mut app, &cp).unwrap();
        handle_claude_event("10", AgentEvent::SessionId("sid-1".into()), &mut app, &cp).unwrap();
        let out = AgentEvent::Output(AgentOutput {
            output_type: OutputType::Stdout,
            data: "result\n".into(),
        });
        handle_claude_event("10", out, &mut app, &cp).unwrap();
        handle_claude_event("10", AgentEvent::Exited { code: Some(0) }, &mut app, &cp).unwrap();
        assert!(!app.running_sessions.contains("10"));
        assert_eq!(app.agent_session_ids.get("10").unwrap(), "sid-1");
    }

    // ── 9. Different slot_ids are independent ──

    #[test]
    fn test_independent_slot_ids() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("1", AgentEvent::Started { pid: 1 }, &mut app, &cp).unwrap();
        handle_claude_event("2", AgentEvent::Started { pid: 2 }, &mut app, &cp).unwrap();
        assert!(app.running_sessions.contains("1"));
        assert!(app.running_sessions.contains("2"));
        handle_claude_event("1", AgentEvent::Exited { code: Some(0) }, &mut app, &cp).unwrap();
        assert!(!app.running_sessions.contains("1"));
        assert!(app.running_sessions.contains("2"));
    }

    #[test]
    fn test_session_ids_independent_per_slot() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("a", AgentEvent::SessionId("sid-a".into()), &mut app, &cp).unwrap();
        handle_claude_event("b", AgentEvent::SessionId("sid-b".into()), &mut app, &cp).unwrap();
        assert_eq!(app.agent_session_ids.get("a").unwrap(), "sid-a");
        assert_eq!(app.agent_session_ids.get("b").unwrap(), "sid-b");
    }

    // ── 10. Edge cases ──

    #[test]
    fn test_empty_slot_id() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let r = handle_claude_event("", AgentEvent::Started { pid: 0 }, &mut app, &cp);
        assert!(r.is_ok());
        assert!(app.running_sessions.contains(""));
    }

    #[test]
    fn test_exit_code_zero() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("x", AgentEvent::Exited { code: Some(0) }, &mut app, &cp).unwrap();
        assert_eq!(app.agent_exit_codes.get("x"), Some(&0));
    }

    #[test]
    fn test_exit_code_negative() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("x", AgentEvent::Exited { code: Some(-1) }, &mut app, &cp).unwrap();
        assert_eq!(app.agent_exit_codes.get("x"), Some(&-1));
    }

    #[test]
    fn test_exit_code_large() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("x", AgentEvent::Exited { code: Some(255) }, &mut app, &cp).unwrap();
        assert_eq!(app.agent_exit_codes.get("x"), Some(&255));
    }

    #[test]
    fn test_output_event_json_type() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let event = AgentEvent::Output(AgentOutput {
            output_type: OutputType::Json,
            data: r#"{"key":"val"}"#.into(),
        });
        assert!(handle_claude_event("42", event, &mut app, &cp).is_ok());
    }

    #[test]
    fn test_output_event_error_type() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let event = AgentEvent::Output(AgentOutput {
            output_type: OutputType::Error,
            data: "error msg".into(),
        });
        assert!(handle_claude_event("42", event, &mut app, &cp).is_ok());
    }

    #[test]
    fn test_output_event_hook_type() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let event = AgentEvent::Output(AgentOutput {
            output_type: OutputType::Hook,
            data: "hook output".into(),
        });
        assert!(handle_claude_event("42", event, &mut app, &cp).is_ok());
    }

    #[test]
    fn test_multiple_exits_same_slot() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("42", AgentEvent::Exited { code: Some(0) }, &mut app, &cp).unwrap();
        handle_claude_event("42", AgentEvent::Exited { code: Some(1) }, &mut app, &cp).unwrap();
        // Last exit code wins
        assert_eq!(app.agent_exit_codes.get("42"), Some(&1));
    }

    #[test]
    fn test_exit_removes_receiver_entry() {
        let mut app = App::new();
        let (tx, rx) = std::sync::mpsc::channel::<AgentEvent>();
        app.agent_receivers.insert("42".into(), rx);
        drop(tx);
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("42", AgentEvent::Exited { code: Some(0) }, &mut app, &cp).unwrap();
        assert!(!app.agent_receivers.contains_key("42"));
    }

    #[test]
    fn test_staged_prompt_not_consumed_on_non_exit() {
        let mut app = app_with_worktree("feat");
        app.staged_prompt = Some("pending prompt".into());
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let event = AgentEvent::Output(AgentOutput {
            output_type: OutputType::Stdout,
            data: "streaming...\n".into(),
        });
        handle_claude_event("42", event, &mut app, &cp).unwrap();
        assert_eq!(app.staged_prompt.as_deref(), Some("pending prompt"));
    }

    #[test]
    fn test_session_id_long_string() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let long_id = "a".repeat(256);
        handle_claude_event("42", AgentEvent::SessionId(long_id.clone()), &mut app, &cp).unwrap();
        assert_eq!(app.agent_session_ids.get("42").unwrap(), &long_id);
    }

    #[test]
    fn test_exit_with_staged_prompt_adds_user_message() {
        let mut app = app_with_worktree("feat");
        app.staged_prompt = Some("do stuff".into());
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("42", AgentEvent::Exited { code: Some(0) }, &mut app, &cp).unwrap();
        // The staged prompt was consumed and added as a user message
        assert!(app.staged_prompt.is_none());
    }

    // ── 11. Multiple slot_ids get independent session ids ──

    #[test]
    fn test_multiple_session_ids_stored_per_slot() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("s1", AgentEvent::SessionId("id-s1".into()), &mut app, &cp).unwrap();
        handle_claude_event("s2", AgentEvent::SessionId("id-s2".into()), &mut app, &cp).unwrap();
        handle_claude_event("s3", AgentEvent::SessionId("id-s3".into()), &mut app, &cp).unwrap();
        assert_eq!(app.agent_session_ids.get("s1").map(|s| s.as_str()), Some("id-s1"));
        assert_eq!(app.agent_session_ids.get("s2").map(|s| s.as_str()), Some("id-s2"));
        assert_eq!(app.agent_session_ids.get("s3").map(|s| s.as_str()), Some("id-s3"));
    }

    // ── 12. Started event uses numeric pid string for slot ──

    #[test]
    fn test_started_large_pid() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let pid = 99999u32;
        handle_claude_event(&pid.to_string(), AgentEvent::Started { pid }, &mut app, &cp).unwrap();
        assert!(app.running_sessions.contains(&pid.to_string()));
    }

    // ── 13. Exit clears running session for the correct slot only ──

    #[test]
    fn test_exit_one_slot_leaves_other_running() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("10", AgentEvent::Started { pid: 10 }, &mut app, &cp).unwrap();
        handle_claude_event("20", AgentEvent::Started { pid: 20 }, &mut app, &cp).unwrap();
        handle_claude_event("30", AgentEvent::Started { pid: 30 }, &mut app, &cp).unwrap();
        handle_claude_event("20", AgentEvent::Exited { code: Some(0) }, &mut app, &cp).unwrap();
        assert!(app.running_sessions.contains("10"));
        assert!(!app.running_sessions.contains("20"));
        assert!(app.running_sessions.contains("30"));
    }

    // ── 14. Exited code 128 stored correctly ──

    #[test]
    fn test_exit_code_128() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("pid128", AgentEvent::Exited { code: Some(128) }, &mut app, &cp).unwrap();
        assert_eq!(app.agent_exit_codes.get("pid128"), Some(&128));
    }

    // ── 15. Output event with empty data is handled ──

    #[test]
    fn test_output_event_empty_data() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let event = AgentEvent::Output(AgentOutput {
            output_type: OutputType::Stdout,
            data: "".into(),
        });
        assert!(handle_claude_event("x", event, &mut app, &cp).is_ok());
    }

    // ── 16. SessionId with unicode string ──

    #[test]
    fn test_session_id_unicode() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let id = "αβγδ-session-id".to_string();
        handle_claude_event("u1", AgentEvent::SessionId(id.clone()), &mut app, &cp).unwrap();
        assert_eq!(app.agent_session_ids.get("u1").unwrap(), &id);
    }

    // ── 17. Staged prompt is None after consuming it on exit ──

    #[test]
    fn test_staged_prompt_none_after_exit_with_no_worktree() {
        // App with no worktrees: staged prompt is consumed but spawn fails gracefully
        let mut app = App::new();
        app.staged_prompt = Some("some work".into());
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("99", AgentEvent::Exited { code: Some(0) }, &mut app, &cp).unwrap();
        assert!(app.staged_prompt.is_none());
    }

    // ── 18. Exit does not insert new running session ──

    #[test]
    fn test_exit_does_not_add_to_running() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("fresh", AgentEvent::Exited { code: Some(0) }, &mut app, &cp).unwrap();
        // "fresh" was never in running_sessions; exit should not add it
        assert!(!app.running_sessions.contains("fresh"));
    }

    // ── 19. Output does not add to running sessions ──

    #[test]
    fn test_output_does_not_add_to_running() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let event = AgentEvent::Output(AgentOutput {
            output_type: OutputType::Stdout,
            data: "line\n".into(),
        });
        handle_claude_event("q", event, &mut app, &cp).unwrap();
        assert!(!app.running_sessions.contains("q"));
    }

    // ── 20. SessionId event does not modify running_sessions ──

    #[test]
    fn test_session_id_event_does_not_change_running() {
        let mut app = App::new();
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("r", AgentEvent::SessionId("sid-r".into()), &mut app, &cp).unwrap();
        assert!(!app.running_sessions.contains("r"));
    }

    #[test]
    fn test_exit_clears_staged_prompt_even_without_worktree() {
        let mut app = App::new();
        app.staged_prompt = Some("queued prompt".into());
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("z", AgentEvent::Exited { code: Some(0) }, &mut app, &cp).unwrap();
        assert!(app.staged_prompt.is_none());
    }

    #[test]
    fn test_output_event_does_not_consume_staged_prompt() {
        let mut app = App::new();
        app.staged_prompt = Some("waiting".into());
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        let event = AgentEvent::Output(AgentOutput {
            output_type: OutputType::Stdout,
            data: "data".into(),
        });
        handle_claude_event("s", event, &mut app, &cp).unwrap();
        assert_eq!(app.staged_prompt.as_deref(), Some("waiting"));
    }

    #[test]
    fn test_started_event_does_not_consume_staged_prompt() {
        let mut app = App::new();
        app.staged_prompt = Some("pending".into());
        let cp = AgentProcess::new(crate::config::Config::default(), crate::backend::Backend::Claude);
        handle_claude_event("s", AgentEvent::Started { pid: 999 }, &mut app, &cp).unwrap();
        assert_eq!(app.staged_prompt.as_deref(), Some("pending"));
    }

    // ── extract_assistant_text ──

    #[test]
    fn test_extract_assistant_text_valid() {
        let data = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello world"}]}}"#;
        assert_eq!(extract_assistant_text(data), Some("hello world".into()));
    }

    #[test]
    fn test_extract_assistant_text_multiple_blocks() {
        let data = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"part1"},{"type":"text","text":" part2"}]}}"#;
        assert_eq!(extract_assistant_text(data), Some("part1 part2".into()));
    }

    #[test]
    fn test_extract_assistant_text_non_assistant() {
        let data = r#"{"type":"user","message":{"content":"hello"}}"#;
        assert!(extract_assistant_text(data).is_none());
    }

    #[test]
    fn test_extract_assistant_text_empty() {
        assert!(extract_assistant_text("").is_none());
    }

    #[test]
    fn test_extract_assistant_text_invalid_json() {
        assert!(extract_assistant_text("not json").is_none());
    }

    #[test]
    fn test_extract_assistant_text_tool_use_block_skipped() {
        let data = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"x","name":"Read","input":{}}]}}"#;
        assert!(extract_assistant_text(data).is_none());
    }

    #[test]
    fn test_extract_assistant_text_multiline_picks_assistant() {
        let data = r#"{"type":"system","message":{}}
{"type":"assistant","message":{"content":[{"type":"text","text":"found it"}]}}"#;
        assert_eq!(extract_assistant_text(data), Some("found it".into()));
    }

    #[test]
    fn test_extract_assistant_text_no_content() {
        let data = r#"{"type":"assistant","message":{}}"#;
        assert!(extract_assistant_text(data).is_none());
    }

    #[test]
    fn test_extract_assistant_text_empty_content() {
        let data = r#"{"type":"assistant","message":{"content":[]}}"#;
        assert!(extract_assistant_text(data).is_none());
    }

    #[test]
    fn test_extract_assistant_text_empty_text() {
        let data = r#"{"type":"assistant","message":{"content":[{"type":"text","text":""}]}}"#;
        assert!(extract_assistant_text(data).is_none());
    }

    // ── poll_compaction_agents ──

    #[test]
    fn test_poll_compaction_no_receivers() {
        let mut app = App::new();
        assert!(!poll_compaction_agents(&mut app));
    }

    #[test]
    fn test_poll_compaction_exit_stores_summary() {
        let mut app = App::new();
        let store = crate::app::session_store::SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();
        // Append some events so store_compaction has a valid max_seq
        store.append_events(sid, &[
            crate::events::DisplayEvent::UserMessage { _uuid: String::new(), content: "test".into() },
        ]).unwrap();
        app.session_store = Some(store);

        // Simulate compaction output already accumulated
        app.compaction_output.insert("99".into(), "The conversation covered X, Y, Z.".into());

        // Create a channel and send exit event
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(AgentEvent::Exited { code: Some(0) }).unwrap();
        drop(tx);
        app.compaction_receivers.insert("99".into(), (rx, sid, 0));

        assert!(poll_compaction_agents(&mut app));
        assert!(app.compaction_receivers.is_empty());
        assert!(app.compaction_output.is_empty());

        // Verify compaction was stored
        let compaction = app.session_store.as_ref().unwrap().latest_compaction(sid).unwrap();
        assert!(compaction.is_some());
        assert_eq!(compaction.unwrap().summary, "The conversation covered X, Y, Z.");
    }

    #[test]
    fn test_poll_compaction_empty_summary_not_stored() {
        let mut app = App::new();
        let store = crate::app::session_store::SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();
        app.session_store = Some(store);

        // Empty compaction output
        app.compaction_output.insert("88".into(), "   ".into());

        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(AgentEvent::Exited { code: Some(0) }).unwrap();
        drop(tx);
        app.compaction_receivers.insert("88".into(), (rx, sid, 0));

        poll_compaction_agents(&mut app);

        // Should NOT store an empty/whitespace-only summary
        let compaction = app.session_store.as_ref().unwrap().latest_compaction(sid).unwrap();
        assert!(compaction.is_none());
    }

    #[test]
    fn test_poll_compaction_no_output_no_store() {
        let mut app = App::new();
        let store = crate::app::session_store::SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();
        app.session_store = Some(store);

        // No compaction_output entry at all
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(AgentEvent::Exited { code: Some(0) }).unwrap();
        drop(tx);
        app.compaction_receivers.insert("77".into(), (rx, sid, 0));

        poll_compaction_agents(&mut app);

        let compaction = app.session_store.as_ref().unwrap().latest_compaction(sid).unwrap();
        assert!(compaction.is_none());
    }

    #[test]
    fn test_poll_compaction_accumulates_output() {
        let mut app = App::new();
        let store = crate::app::session_store::SessionStore::open_memory().unwrap();
        let sid = store.create_session("main").unwrap();
        store.append_events(sid, &[
            crate::events::DisplayEvent::UserMessage { _uuid: String::new(), content: "x".into() },
        ]).unwrap();
        app.session_store = Some(store);

        let (tx, rx) = std::sync::mpsc::channel();
        // Send two output events then exit
        let assistant_json_1 = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Part 1. "}]}}"#;
        let assistant_json_2 = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Part 2."}]}}"#;
        tx.send(AgentEvent::Output(AgentOutput {
            output_type: OutputType::Stdout,
            data: assistant_json_1.into(),
        })).unwrap();
        tx.send(AgentEvent::Output(AgentOutput {
            output_type: OutputType::Stdout,
            data: assistant_json_2.into(),
        })).unwrap();
        tx.send(AgentEvent::Exited { code: Some(0) }).unwrap();
        drop(tx);
        app.compaction_receivers.insert("66".into(), (rx, sid, 0));

        // First poll accumulates output
        poll_compaction_agents(&mut app);

        // Should have processed exit and stored compaction
        let compaction = app.session_store.as_ref().unwrap().latest_compaction(sid).unwrap();
        assert!(compaction.is_some());
        assert_eq!(compaction.unwrap().summary, "Part 1. Part 2.");
    }
}
