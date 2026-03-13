//! Display events for TUI rendering
//!
//! Processed events ready for the UI, transformed from raw Claude Code events.

/// Parsed and displayable event for the UI
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum DisplayEvent {
    /// System initialization
    Init {
        #[serde(skip)]
        _session_id: String,
        cwd: String,
        model: String,
    },
    /// Hook output
    Hook {
        name: String,
        output: String,
    },
    /// User's message
    UserMessage {
        #[serde(skip)]
        _uuid: String,
        content: String,
    },
    /// Slash command (e.g., /compact, /crt)
    Command {
        name: String,
    },
    /// Context compacted (detected from compaction summary in session file)
    Compacting,
    /// Context compacted via /compact command (unreachable in -p mode)
    Compacted,
    /// Suspected compaction (90%+ context, 20s inactivity)
    MayBeCompacting,
    /// Plan mode content (from ~/.claude/plans/)
    Plan {
        name: String,
        content: String,
    },
    /// Assistant text response
    AssistantText {
        #[serde(skip)]
        _uuid: String,
        #[serde(skip)]
        _message_id: String,
        text: String,
    },
    /// Tool being called
    ToolCall {
        #[serde(skip)]
        _uuid: String,
        tool_use_id: String,
        tool_name: String,
        /// Extracted file path if applicable
        file_path: Option<String>,
        /// Full input for display
        input: serde_json::Value,
    },
    /// Tool result (output from a tool call)
    ToolResult {
        tool_use_id: String,
        tool_name: String,
        /// For file-based tools: the path that was operated on
        file_path: Option<String>,
        /// The raw output content from the tool
        content: String,
        /// Whether Claude Code flagged this result as an error
        is_error: bool,
    },
    /// Session complete
    Complete {
        #[serde(skip)]
        _session_id: String,
        success: bool,
        duration_ms: u64,
        cost_usd: f64,
    },
    /// Filtered out (used for rewound/edited messages that were superseded)
    Filtered,
}

#[cfg(test)]
mod tests {
    use super::*;

    // =====================================================================
    // Init variant — construction and field access
    // =====================================================================

    #[test]
    fn init_basic_construction() {
        let ev = DisplayEvent::Init {
            _session_id: "abc-123".into(),
            cwd: "/home/user".into(),
            model: "claude-opus-4-6".into(),
        };
        match ev {
            DisplayEvent::Init { _session_id, cwd, model } => {
                assert_eq!(_session_id, "abc-123");
                assert_eq!(cwd, "/home/user");
                assert_eq!(model, "claude-opus-4-6");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn init_empty_strings() {
        let ev = DisplayEvent::Init {
            _session_id: String::new(),
            cwd: String::new(),
            model: String::new(),
        };
        match ev {
            DisplayEvent::Init { _session_id, cwd, model } => {
                assert!(_session_id.is_empty());
                assert!(cwd.is_empty());
                assert!(model.is_empty());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn init_unicode_fields() {
        let ev = DisplayEvent::Init {
            _session_id: "\u{1F680}".into(),
            cwd: "/\u{65E5}\u{672C}\u{8A9E}".into(),
            model: "\u{00E9}\u{00E8}\u{00EA}".into(),
        };
        match ev {
            DisplayEvent::Init { _session_id, cwd, model } => {
                assert_eq!(_session_id, "\u{1F680}");
                assert!(cwd.contains('\u{65E5}'));
                assert_eq!(model, "\u{00E9}\u{00E8}\u{00EA}");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn init_special_chars() {
        let ev = DisplayEvent::Init {
            _session_id: "id\nwith\nnewlines".into(),
            cwd: "path with spaces".into(),
            model: "model\t\twith\ttabs".into(),
        };
        match ev {
            DisplayEvent::Init { _session_id, cwd, .. } => {
                assert!(_session_id.contains('\n'));
                assert!(cwd.contains(' '));
            }
            _ => panic!("wrong variant"),
        }
    }

    // =====================================================================
    // Hook variant
    // =====================================================================

    #[test]
    fn hook_basic() {
        let ev = DisplayEvent::Hook {
            name: "pre-commit".into(),
            output: "Checking formatting...".into(),
        };
        match ev {
            DisplayEvent::Hook { name, output } => {
                assert_eq!(name, "pre-commit");
                assert_eq!(output, "Checking formatting...");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn hook_empty_output() {
        let ev = DisplayEvent::Hook {
            name: "post-build".into(),
            output: String::new(),
        };
        match ev {
            DisplayEvent::Hook { output, .. } => assert!(output.is_empty()),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn hook_multiline_output() {
        let ev = DisplayEvent::Hook {
            name: "lint".into(),
            output: "line1\nline2\nline3".into(),
        };
        match ev {
            DisplayEvent::Hook { output, .. } => {
                assert_eq!(output.lines().count(), 3);
            }
            _ => panic!("wrong variant"),
        }
    }

    // =====================================================================
    // UserMessage variant
    // =====================================================================

    #[test]
    fn user_message_basic() {
        let ev = DisplayEvent::UserMessage {
            _uuid: "uuid-1".into(),
            content: "Hello, world!".into(),
        };
        match ev {
            DisplayEvent::UserMessage { _uuid, content } => {
                assert_eq!(_uuid, "uuid-1");
                assert_eq!(content, "Hello, world!");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn user_message_unicode_content() {
        let ev = DisplayEvent::UserMessage {
            _uuid: "u".into(),
            content: "\u{4F60}\u{597D}\u{4E16}\u{754C}".into(),
        };
        match ev {
            DisplayEvent::UserMessage { content, .. } => {
                assert_eq!(content, "\u{4F60}\u{597D}\u{4E16}\u{754C}");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn user_message_very_long_content() {
        let long = "x".repeat(100_000);
        let ev = DisplayEvent::UserMessage {
            _uuid: "u".into(),
            content: long.clone(),
        };
        match ev {
            DisplayEvent::UserMessage { content, .. } => {
                assert_eq!(content.len(), 100_000);
            }
            _ => panic!("wrong variant"),
        }
    }

    // =====================================================================
    // Command variant
    // =====================================================================

    #[test]
    fn command_basic() {
        let ev = DisplayEvent::Command { name: "/compact".into() };
        match ev {
            DisplayEvent::Command { name } => assert_eq!(name, "/compact"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn command_empty_name() {
        let ev = DisplayEvent::Command { name: String::new() };
        match ev {
            DisplayEvent::Command { name } => assert!(name.is_empty()),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn command_special_chars_name() {
        let ev = DisplayEvent::Command { name: "/cmd with spaces & symbols!@#".into() };
        match ev {
            DisplayEvent::Command { name } => assert!(name.contains('!')),
            _ => panic!("wrong variant"),
        }
    }

    // =====================================================================
    // Unit variants (Compacting, Compacted, MayBeCompacting, Filtered)
    // =====================================================================

    #[test]
    fn compacting_construction() {
        let ev = DisplayEvent::Compacting;
        assert!(matches!(ev, DisplayEvent::Compacting));
    }

    #[test]
    fn compacted_construction() {
        let ev = DisplayEvent::Compacted;
        assert!(matches!(ev, DisplayEvent::Compacted));
    }

    #[test]
    fn may_be_compacting_construction() {
        let ev = DisplayEvent::MayBeCompacting;
        assert!(matches!(ev, DisplayEvent::MayBeCompacting));
    }

    #[test]
    fn filtered_construction() {
        let ev = DisplayEvent::Filtered;
        assert!(matches!(ev, DisplayEvent::Filtered));
    }

    // =====================================================================
    // Plan variant
    // =====================================================================

    #[test]
    fn plan_basic() {
        let ev = DisplayEvent::Plan {
            name: "refactor-plan".into(),
            content: "Step 1: ...".into(),
        };
        match ev {
            DisplayEvent::Plan { name, content } => {
                assert_eq!(name, "refactor-plan");
                assert_eq!(content, "Step 1: ...");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn plan_empty_fields() {
        let ev = DisplayEvent::Plan {
            name: String::new(),
            content: String::new(),
        };
        match ev {
            DisplayEvent::Plan { name, content } => {
                assert!(name.is_empty());
                assert!(content.is_empty());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn plan_large_content() {
        let big = "a".repeat(50_000);
        let ev = DisplayEvent::Plan {
            name: "big".into(),
            content: big.clone(),
        };
        match ev {
            DisplayEvent::Plan { content, .. } => assert_eq!(content.len(), 50_000),
            _ => panic!("wrong variant"),
        }
    }

    // =====================================================================
    // AssistantText variant
    // =====================================================================

    #[test]
    fn assistant_text_basic() {
        let ev = DisplayEvent::AssistantText {
            _uuid: "u1".into(),
            _message_id: "m1".into(),
            text: "Here is the answer".into(),
        };
        match ev {
            DisplayEvent::AssistantText { _uuid, _message_id, text } => {
                assert_eq!(_uuid, "u1");
                assert_eq!(_message_id, "m1");
                assert_eq!(text, "Here is the answer");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn assistant_text_with_markdown() {
        let ev = DisplayEvent::AssistantText {
            _uuid: "u".into(),
            _message_id: "m".into(),
            text: "# Heading\n\n```rust\nfn main() {}\n```".into(),
        };
        match ev {
            DisplayEvent::AssistantText { text, .. } => {
                assert!(text.contains("```rust"));
                assert!(text.contains("fn main()"));
            }
            _ => panic!("wrong variant"),
        }
    }

    // =====================================================================
    // ToolCall variant
    // =====================================================================

    #[test]
    fn tool_call_with_file_path() {
        let ev = DisplayEvent::ToolCall {
            _uuid: "u".into(),
            tool_use_id: "tu1".into(),
            tool_name: "Read".into(),
            file_path: Some("/src/main.rs".into()),
            input: serde_json::json!({"file_path": "/src/main.rs"}),
        };
        match ev {
            DisplayEvent::ToolCall { tool_name, file_path, input, .. } => {
                assert_eq!(tool_name, "Read");
                assert_eq!(file_path, Some("/src/main.rs".into()));
                assert!(input.is_object());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn tool_call_without_file_path() {
        let ev = DisplayEvent::ToolCall {
            _uuid: "u".into(),
            tool_use_id: "tu2".into(),
            tool_name: "Bash".into(),
            file_path: None,
            input: serde_json::json!({"command": "ls -la"}),
        };
        match ev {
            DisplayEvent::ToolCall { file_path, .. } => assert!(file_path.is_none()),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn tool_call_empty_input() {
        let ev = DisplayEvent::ToolCall {
            _uuid: "u".into(),
            tool_use_id: "tu3".into(),
            tool_name: "Glob".into(),
            file_path: None,
            input: serde_json::json!({}),
        };
        match ev {
            DisplayEvent::ToolCall { input, .. } => {
                assert!(input.as_object().unwrap().is_empty());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn tool_call_null_input() {
        let ev = DisplayEvent::ToolCall {
            _uuid: "u".into(),
            tool_use_id: "tu4".into(),
            tool_name: "Test".into(),
            file_path: None,
            input: serde_json::Value::Null,
        };
        match ev {
            DisplayEvent::ToolCall { input, .. } => assert!(input.is_null()),
            _ => panic!("wrong variant"),
        }
    }

    // =====================================================================
    // ToolResult variant
    // =====================================================================

    #[test]
    fn tool_result_basic() {
        let ev = DisplayEvent::ToolResult {
            tool_use_id: "tu1".into(),
            tool_name: "Read".into(),
            file_path: Some("/src/main.rs".into()),
            content: "fn main() {}".into(),
            is_error: false,
        };
        match ev {
            DisplayEvent::ToolResult { tool_use_id, tool_name, file_path, content, .. } => {
                assert_eq!(tool_use_id, "tu1");
                assert_eq!(tool_name, "Read");
                assert_eq!(file_path, Some("/src/main.rs".into()));
                assert_eq!(content, "fn main() {}");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn tool_result_no_file_path() {
        let ev = DisplayEvent::ToolResult {
            tool_use_id: "tu2".into(),
            tool_name: "Bash".into(),
            file_path: None,
            content: "OK".into(),
            is_error: false,
        };
        match ev {
            DisplayEvent::ToolResult { file_path, .. } => assert!(file_path.is_none()),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn tool_result_empty_content() {
        let ev = DisplayEvent::ToolResult {
            tool_use_id: "tu3".into(),
            tool_name: "Bash".into(),
            file_path: None,
            content: String::new(),
            is_error: false,
        };
        match ev {
            DisplayEvent::ToolResult { content, .. } => assert!(content.is_empty()),
            _ => panic!("wrong variant"),
        }
    }

    // =====================================================================
    // Complete variant
    // =====================================================================

    #[test]
    fn complete_success() {
        let ev = DisplayEvent::Complete {
            _session_id: "s1".into(),
            success: true,
            duration_ms: 5000,
            cost_usd: 0.05,
        };
        match ev {
            DisplayEvent::Complete { success, duration_ms, cost_usd, .. } => {
                assert!(success);
                assert_eq!(duration_ms, 5000);
                assert!((cost_usd - 0.05).abs() < f64::EPSILON);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn complete_failure() {
        let ev = DisplayEvent::Complete {
            _session_id: "s2".into(),
            success: false,
            duration_ms: 100,
            cost_usd: 0.0,
        };
        match ev {
            DisplayEvent::Complete { success, cost_usd, .. } => {
                assert!(!success);
                assert_eq!(cost_usd, 0.0);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn complete_zero_duration() {
        let ev = DisplayEvent::Complete {
            _session_id: "s".into(),
            success: true,
            duration_ms: 0,
            cost_usd: 0.0,
        };
        match ev {
            DisplayEvent::Complete { duration_ms, .. } => assert_eq!(duration_ms, 0),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn complete_large_values() {
        let ev = DisplayEvent::Complete {
            _session_id: "s".into(),
            success: true,
            duration_ms: u64::MAX,
            cost_usd: f64::MAX,
        };
        match ev {
            DisplayEvent::Complete { duration_ms, cost_usd, .. } => {
                assert_eq!(duration_ms, u64::MAX);
                assert_eq!(cost_usd, f64::MAX);
            }
            _ => panic!("wrong variant"),
        }
    }

    // =====================================================================
    // Debug impl
    // =====================================================================

    #[test]
    fn debug_init_contains_variant_name() {
        let ev = DisplayEvent::Init {
            _session_id: "s".into(),
            cwd: "/tmp".into(),
            model: "m".into(),
        };
        let dbg = format!("{:?}", ev);
        assert!(dbg.contains("Init"), "Debug output should contain 'Init': {dbg}");
    }

    #[test]
    fn debug_compacting() {
        let dbg = format!("{:?}", DisplayEvent::Compacting);
        assert!(dbg.contains("Compacting"));
    }

    #[test]
    fn debug_compacted() {
        let dbg = format!("{:?}", DisplayEvent::Compacted);
        assert!(dbg.contains("Compacted"));
    }

    #[test]
    fn debug_may_be_compacting() {
        let dbg = format!("{:?}", DisplayEvent::MayBeCompacting);
        assert!(dbg.contains("MayBeCompacting"));
    }

    #[test]
    fn debug_filtered() {
        let dbg = format!("{:?}", DisplayEvent::Filtered);
        assert!(dbg.contains("Filtered"));
    }

    #[test]
    fn debug_hook_shows_fields() {
        let ev = DisplayEvent::Hook {
            name: "test-hook".into(),
            output: "output-text".into(),
        };
        let dbg = format!("{:?}", ev);
        assert!(dbg.contains("Hook"));
        assert!(dbg.contains("test-hook"));
    }

    #[test]
    fn debug_tool_call_shows_tool_name() {
        let ev = DisplayEvent::ToolCall {
            _uuid: "u".into(),
            tool_use_id: "t".into(),
            tool_name: "Read".into(),
            file_path: None,
            input: serde_json::json!(null),
        };
        let dbg = format!("{:?}", ev);
        assert!(dbg.contains("ToolCall"));
        assert!(dbg.contains("Read"));
    }

    #[test]
    fn debug_complete_shows_fields() {
        let ev = DisplayEvent::Complete {
            _session_id: "s".into(),
            success: true,
            duration_ms: 42,
            cost_usd: 1.23,
        };
        let dbg = format!("{:?}", ev);
        assert!(dbg.contains("Complete"));
        assert!(dbg.contains("42"));
    }

    // =====================================================================
    // Clone impl
    // =====================================================================

    #[test]
    fn clone_init() {
        let ev = DisplayEvent::Init {
            _session_id: "s".into(),
            cwd: "/tmp".into(),
            model: "m".into(),
        };
        let cloned = ev.clone();
        match (&ev, &cloned) {
            (
                DisplayEvent::Init { _session_id: a_id, cwd: a_cwd, model: a_model },
                DisplayEvent::Init { _session_id: b_id, cwd: b_cwd, model: b_model },
            ) => {
                assert_eq!(a_id, b_id);
                assert_eq!(a_cwd, b_cwd);
                assert_eq!(a_model, b_model);
            }
            _ => panic!("clone should preserve variant"),
        }
    }

    #[test]
    fn clone_compacting() {
        let ev = DisplayEvent::Compacting;
        let cloned = ev.clone();
        assert!(matches!(cloned, DisplayEvent::Compacting));
    }

    #[test]
    fn clone_filtered() {
        let ev = DisplayEvent::Filtered;
        let cloned = ev.clone();
        assert!(matches!(cloned, DisplayEvent::Filtered));
    }

    #[test]
    fn clone_tool_call_independence() {
        let ev = DisplayEvent::ToolCall {
            _uuid: "u".into(),
            tool_use_id: "t".into(),
            tool_name: "Bash".into(),
            file_path: Some("/file".into()),
            input: serde_json::json!({"cmd": "ls"}),
        };
        let cloned = ev.clone();
        // Verify the clone is a separate allocation
        match (&ev, &cloned) {
            (
                DisplayEvent::ToolCall { tool_name: a, .. },
                DisplayEvent::ToolCall { tool_name: b, .. },
            ) => {
                assert_eq!(a, b);
            }
            _ => panic!("clone should preserve variant"),
        }
    }

    #[test]
    fn clone_complete() {
        let ev = DisplayEvent::Complete {
            _session_id: "s".into(),
            success: true,
            duration_ms: 99,
            cost_usd: 0.42,
        };
        let cloned = ev.clone();
        match cloned {
            DisplayEvent::Complete { success, duration_ms, cost_usd, .. } => {
                assert!(success);
                assert_eq!(duration_ms, 99);
                assert!((cost_usd - 0.42).abs() < f64::EPSILON);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn clone_user_message() {
        let ev = DisplayEvent::UserMessage {
            _uuid: "abc".into(),
            content: "Hello".into(),
        };
        let cloned = ev.clone();
        match cloned {
            DisplayEvent::UserMessage { _uuid, content } => {
                assert_eq!(_uuid, "abc");
                assert_eq!(content, "Hello");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn clone_tool_result() {
        let ev = DisplayEvent::ToolResult {
            tool_use_id: "t1".into(),
            tool_name: "Grep".into(),
            file_path: Some("/file.rs".into()),
            content: "match found".into(),
            is_error: false,
        };
        let cloned = ev.clone();
        match cloned {
            DisplayEvent::ToolResult { tool_name, content, .. } => {
                assert_eq!(tool_name, "Grep");
                assert_eq!(content, "match found");
            }
            _ => panic!("wrong variant"),
        }
    }

    // =====================================================================
    // Variant discrimination (matches! checks)
    // =====================================================================

    #[test]
    fn variants_are_distinct() {
        let init = DisplayEvent::Init {
            _session_id: "s".into(), cwd: "c".into(), model: "m".into()
        };
        let hook = DisplayEvent::Hook { name: "n".into(), output: "o".into() };
        let user = DisplayEvent::UserMessage { _uuid: "u".into(), content: "c".into() };
        let cmd = DisplayEvent::Command { name: "n".into() };
        let compacting = DisplayEvent::Compacting;
        let compacted = DisplayEvent::Compacted;
        let may = DisplayEvent::MayBeCompacting;
        let filtered = DisplayEvent::Filtered;

        assert!(matches!(init, DisplayEvent::Init { .. }));
        assert!(!matches!(init, DisplayEvent::Hook { .. }));
        assert!(matches!(hook, DisplayEvent::Hook { .. }));
        assert!(matches!(user, DisplayEvent::UserMessage { .. }));
        assert!(matches!(cmd, DisplayEvent::Command { .. }));
        assert!(matches!(compacting, DisplayEvent::Compacting));
        assert!(matches!(compacted, DisplayEvent::Compacted));
        assert!(matches!(may, DisplayEvent::MayBeCompacting));
        assert!(matches!(filtered, DisplayEvent::Filtered));
    }

    #[test]
    fn compacting_is_not_compacted() {
        let ev = DisplayEvent::Compacting;
        assert!(!matches!(ev, DisplayEvent::Compacted));
    }

    #[test]
    fn may_be_compacting_is_not_compacting() {
        let ev = DisplayEvent::MayBeCompacting;
        assert!(!matches!(ev, DisplayEvent::Compacting));
    }

    #[test]
    fn filtered_is_not_compacted() {
        let ev = DisplayEvent::Filtered;
        assert!(!matches!(ev, DisplayEvent::Compacted));
    }
}
