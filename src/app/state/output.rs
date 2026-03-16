//! Session output processing and display event handling

use super::App;
use crate::app::util::strip_ansi_escapes;
use crate::events::DisplayEvent;

impl App {
    pub fn process_session_chunk(&mut self, chunk: &str) {
        let cleaned = strip_ansi_escapes(chunk);
        for ch in cleaned.chars() {
            match ch {
                '\n' => {
                    // Move the buffer into the line vec instead of clone+clear —
                    // take() reuses capacity for the next line (zero allocation).
                    self.session_lines
                        .push_back(std::mem::take(&mut self.session_buffer));
                    if self.session_lines.len() > self.max_session_lines {
                        self.session_lines.pop_front();
                    }
                }
                '\r' => self.session_buffer.clear(),
                _ => self.session_buffer.push(ch),
            }
        }
    }

    /// Add a user message to the session pane immediately on prompt submit.
    /// Pushes a real DisplayEvent::UserMessage into display_events so it
    /// renders persistently (no disappearing). Also stores the content as
    /// `pending_user_message` — this is ONLY used as a dedup marker so the
    /// full re-parse on Claude exit can detect and skip the duplicate.
    pub fn add_user_message(&mut self, content: String) {
        // Compaction summaries are internal — show banner, not raw text
        if content.starts_with("This session is being continued from a previous conversation") {
            self.display_events.push(DisplayEvent::Compacting);
            self.invalidate_render_cache();
            self.session_scroll = usize::MAX;
            return;
        }
        // Push a real event so it renders immediately and persists through
        // the entire conversation. stream-json stdout never emits user events,
        // so without this the message would be invisible until Claude exits
        // and the session file is re-parsed.
        self.display_events.push(DisplayEvent::UserMessage {
            _uuid: String::new(),
            content: content.clone(),
        });
        // Dedup marker: full re-parse (on Claude exit) will check this to
        // avoid creating a second UserMessage for the same content.
        self.pending_user_message = Some(content);
        self.invalidate_render_cache();
        self.session_scroll = usize::MAX;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── process_session_chunk: basic character handling ──

    #[test]
    fn test_process_chunk_simple_text() {
        let mut app = App::new();
        app.process_session_chunk("hello");
        assert_eq!(app.session_buffer, "hello");
        assert!(app.session_lines.is_empty());
    }

    #[test]
    fn test_process_chunk_newline_pushes_line() {
        let mut app = App::new();
        app.process_session_chunk("hello\n");
        assert_eq!(app.session_lines.len(), 1);
        assert_eq!(app.session_lines[0], "hello");
        assert!(app.session_buffer.is_empty());
    }

    #[test]
    fn test_process_chunk_multiple_lines() {
        let mut app = App::new();
        app.process_session_chunk("line1\nline2\nline3\n");
        assert_eq!(app.session_lines.len(), 3);
        assert_eq!(app.session_lines[0], "line1");
        assert_eq!(app.session_lines[1], "line2");
        assert_eq!(app.session_lines[2], "line3");
    }

    #[test]
    fn test_process_chunk_carriage_return_clears_buffer() {
        let mut app = App::new();
        app.process_session_chunk("old text\rnew text");
        assert_eq!(app.session_buffer, "new text");
    }

    #[test]
    fn test_process_chunk_cr_then_newline() {
        let mut app = App::new();
        app.process_session_chunk("progress: 50%\rprogress: 100%\n");
        assert_eq!(app.session_lines.len(), 1);
        assert_eq!(app.session_lines[0], "progress: 100%");
    }

    #[test]
    fn test_process_chunk_empty_string() {
        let mut app = App::new();
        app.process_session_chunk("");
        assert!(app.session_buffer.is_empty());
        assert!(app.session_lines.is_empty());
    }

    #[test]
    fn test_process_chunk_only_newlines() {
        let mut app = App::new();
        app.process_session_chunk("\n\n\n");
        assert_eq!(app.session_lines.len(), 3);
        for line in &app.session_lines {
            assert!(line.is_empty());
        }
    }

    #[test]
    fn test_process_chunk_only_carriage_returns() {
        let mut app = App::new();
        app.process_session_chunk("a\rb\rc");
        // Each \r clears buffer, so only "c" remains
        assert_eq!(app.session_buffer, "c");
        assert!(app.session_lines.is_empty());
    }

    #[test]
    fn test_process_chunk_incremental() {
        let mut app = App::new();
        app.process_session_chunk("hel");
        app.process_session_chunk("lo");
        assert_eq!(app.session_buffer, "hello");
    }

    #[test]
    fn test_process_chunk_incremental_with_newline() {
        let mut app = App::new();
        app.process_session_chunk("hel");
        app.process_session_chunk("lo\n");
        assert_eq!(app.session_lines.len(), 1);
        assert_eq!(app.session_lines[0], "hello");
    }

    #[test]
    fn test_process_chunk_unicode() {
        let mut app = App::new();
        app.process_session_chunk("日本語\n");
        assert_eq!(app.session_lines[0], "日本語");
    }

    #[test]
    fn test_process_chunk_emoji() {
        let mut app = App::new();
        app.process_session_chunk("test 🚀\n");
        assert_eq!(app.session_lines[0], "test 🚀");
    }

    // ── process_session_chunk: ANSI escape stripping ──

    #[test]
    fn test_process_chunk_strips_ansi_color() {
        let mut app = App::new();
        app.process_session_chunk("\x1b[32mgreen\x1b[0m\n");
        assert_eq!(app.session_lines[0], "green");
    }

    #[test]
    fn test_process_chunk_strips_ansi_bold() {
        let mut app = App::new();
        app.process_session_chunk("\x1b[1mbold\x1b[0m\n");
        assert_eq!(app.session_lines[0], "bold");
    }

    #[test]
    fn test_process_chunk_strips_complex_ansi() {
        let mut app = App::new();
        app.process_session_chunk("\x1b[38;5;196mred\x1b[0m text\n");
        assert_eq!(app.session_lines[0], "red text");
    }

    // ── process_session_chunk: max_session_lines limit ──

    #[test]
    fn test_process_chunk_respects_max_lines() {
        let mut app = App::new();
        app.max_session_lines = 3;
        app.process_session_chunk("1\n2\n3\n4\n5\n");
        assert_eq!(app.session_lines.len(), 3);
        // Oldest lines should be dropped
        assert_eq!(app.session_lines[0], "3");
        assert_eq!(app.session_lines[1], "4");
        assert_eq!(app.session_lines[2], "5");
    }

    #[test]
    fn test_process_chunk_at_max_lines_drops_oldest() {
        let mut app = App::new();
        app.max_session_lines = 2;
        app.process_session_chunk("a\nb\n");
        assert_eq!(app.session_lines.len(), 2);
        app.process_session_chunk("c\n");
        assert_eq!(app.session_lines.len(), 2);
        assert_eq!(app.session_lines[0], "b");
        assert_eq!(app.session_lines[1], "c");
    }

    // ── process_session_chunk: buffer state after operations ──

    #[test]
    fn test_process_chunk_buffer_empty_after_newline() {
        let mut app = App::new();
        app.process_session_chunk("content\n");
        assert!(app.session_buffer.is_empty());
    }

    #[test]
    fn test_process_chunk_buffer_has_partial_after_no_newline() {
        let mut app = App::new();
        app.process_session_chunk("partial");
        assert_eq!(app.session_buffer, "partial");
    }

    #[test]
    fn test_process_chunk_buffer_cleared_by_cr() {
        let mut app = App::new();
        app.process_session_chunk("old\r");
        assert!(app.session_buffer.is_empty());
    }

    // ── add_user_message: normal messages ──

    #[test]
    fn test_add_user_message_creates_display_event() {
        let mut app = App::new();
        app.add_user_message("Hello Claude".to_string());
        assert_eq!(app.display_events.len(), 1);
        assert!(matches!(
            &app.display_events[0],
            DisplayEvent::UserMessage { .. }
        ));
    }

    #[test]
    fn test_add_user_message_sets_pending() {
        let mut app = App::new();
        app.add_user_message("test prompt".to_string());
        assert_eq!(app.pending_user_message, Some("test prompt".to_string()));
    }

    #[test]
    fn test_add_user_message_sets_scroll_to_max() {
        let mut app = App::new();
        app.session_scroll = 0;
        app.add_user_message("scroll test".to_string());
        assert_eq!(app.session_scroll, usize::MAX);
    }

    #[test]
    fn test_add_user_message_content_in_event() {
        let mut app = App::new();
        app.add_user_message("specific content".to_string());
        if let DisplayEvent::UserMessage { content, .. } = &app.display_events[0] {
            assert_eq!(content, "specific content");
        } else {
            panic!("expected UserMessage event");
        }
    }

    #[test]
    fn test_add_user_message_uuid_is_empty() {
        let mut app = App::new();
        app.add_user_message("test".to_string());
        if let DisplayEvent::UserMessage { _uuid, .. } = &app.display_events[0] {
            assert!(_uuid.is_empty());
        } else {
            panic!("expected UserMessage event");
        }
    }

    #[test]
    fn test_add_user_message_multiple() {
        let mut app = App::new();
        app.add_user_message("first".to_string());
        app.add_user_message("second".to_string());
        assert_eq!(app.display_events.len(), 2);
        // pending_user_message should be the most recent
        assert_eq!(app.pending_user_message, Some("second".to_string()));
    }

    #[test]
    fn test_add_user_message_empty_string() {
        let mut app = App::new();
        app.add_user_message(String::new());
        assert_eq!(app.display_events.len(), 1);
        assert_eq!(app.pending_user_message, Some(String::new()));
    }

    #[test]
    fn test_add_user_message_unicode() {
        let mut app = App::new();
        app.add_user_message("日本語のメッセージ".to_string());
        assert_eq!(
            app.pending_user_message,
            Some("日本語のメッセージ".to_string())
        );
    }

    // ── add_user_message: compaction detection ──

    #[test]
    fn test_add_user_message_compaction_creates_compacting_event() {
        let mut app = App::new();
        app.add_user_message(
            "This session is being continued from a previous conversation that ran out of context."
                .to_string(),
        );
        assert_eq!(app.display_events.len(), 1);
        assert!(matches!(&app.display_events[0], DisplayEvent::Compacting));
    }

    #[test]
    fn test_add_user_message_compaction_does_not_set_pending() {
        let mut app = App::new();
        app.add_user_message(
            "This session is being continued from a previous conversation that ran out of context."
                .to_string(),
        );
        // Compaction messages don't set pending_user_message
        assert!(app.pending_user_message.is_none());
    }

    #[test]
    fn test_add_user_message_compaction_prefix_only() {
        let mut app = App::new();
        // Must start with the exact prefix
        app.add_user_message(
            "This session is being continued from a previous conversation".to_string(),
        );
        assert!(matches!(&app.display_events[0], DisplayEvent::Compacting));
    }

    #[test]
    fn test_add_user_message_not_compaction_similar_text() {
        let mut app = App::new();
        // Doesn't start with the exact prefix
        app.add_user_message("this session is being continued".to_string());
        assert!(matches!(
            &app.display_events[0],
            DisplayEvent::UserMessage { .. }
        ));
    }

    #[test]
    fn test_add_user_message_compaction_sets_scroll_max() {
        let mut app = App::new();
        app.session_scroll = 0;
        app.add_user_message(
            "This session is being continued from a previous conversation that ran out of context."
                .to_string(),
        );
        assert_eq!(app.session_scroll, usize::MAX);
    }

    // ── process_session_chunk: mixed content ──

    #[test]
    fn test_process_chunk_long_line() {
        let mut app = App::new();
        let long = "x".repeat(10000);
        app.process_session_chunk(&format!("{}\n", long));
        assert_eq!(app.session_lines[0].len(), 10000);
    }

    #[test]
    fn test_process_chunk_tabs_and_spaces() {
        let mut app = App::new();
        app.process_session_chunk("\t  hello  \t\n");
        assert_eq!(app.session_lines[0], "\t  hello  \t");
    }

    #[test]
    fn test_process_chunk_windows_crlf() {
        let mut app = App::new();
        app.process_session_chunk("line1\r\nline2\r\n");
        // \r clears buffer, then \n pushes empty, but let's check actual behavior
        // \r clears "line1", then \n pushes empty buffer
        // Then "line2", \r clears, \n pushes empty
        assert_eq!(app.session_lines.len(), 2);
    }

    // ── Additional tests for 50+ threshold ──

    #[test]
    fn test_process_chunk_alternating_cr_newline() {
        let mut app = App::new();
        app.process_session_chunk("aaa\rbbb\nccc\rddd\n");
        // "aaa" then \r clears, "bbb" then \n pushes "bbb"
        // "ccc" then \r clears, "ddd" then \n pushes "ddd"
        assert_eq!(app.session_lines.len(), 2);
    }

    #[test]
    fn test_process_chunk_single_char() {
        let mut app = App::new();
        app.process_session_chunk("x");
        assert_eq!(app.session_buffer, "x");
    }

    #[test]
    fn test_process_chunk_single_newline() {
        let mut app = App::new();
        app.process_session_chunk("\n");
        assert_eq!(app.session_lines.len(), 1);
        assert!(app.session_lines[0].is_empty());
    }

    #[test]
    fn test_process_chunk_single_cr() {
        let mut app = App::new();
        app.session_buffer = "existing".to_string();
        app.process_session_chunk("\r");
        assert!(app.session_buffer.is_empty());
    }

    #[test]
    fn test_process_chunk_many_incremental_calls() {
        let mut app = App::new();
        for c in "hello world".chars() {
            app.process_session_chunk(&c.to_string());
        }
        assert_eq!(app.session_buffer, "hello world");
    }

    #[test]
    fn test_process_chunk_max_lines_one() {
        let mut app = App::new();
        app.max_session_lines = 1;
        app.process_session_chunk("a\nb\nc\n");
        assert_eq!(app.session_lines.len(), 1);
        assert_eq!(app.session_lines[0], "c");
    }

    #[test]
    fn test_add_user_message_long_content() {
        let mut app = App::new();
        let long = "a".repeat(10000);
        app.add_user_message(long.clone());
        assert_eq!(app.pending_user_message, Some(long));
    }

    #[test]
    fn test_add_user_message_newlines_in_content() {
        let mut app = App::new();
        app.add_user_message("line1\nline2\nline3".to_string());
        if let DisplayEvent::UserMessage { content, .. } = &app.display_events[0] {
            assert!(content.contains('\n'));
        }
    }

    #[test]
    fn test_add_user_message_special_chars() {
        let mut app = App::new();
        app.add_user_message(r#"fix the "bug" in <main>"#.to_string());
        if let DisplayEvent::UserMessage { content, .. } = &app.display_events[0] {
            assert!(content.contains('"'));
            assert!(content.contains('<'));
        }
    }

    #[test]
    fn test_display_events_initially_empty() {
        let app = App::new();
        assert!(app.display_events.is_empty());
    }

    #[test]
    fn test_pending_user_message_initially_none() {
        let app = App::new();
        assert!(app.pending_user_message.is_none());
    }

    #[test]
    fn test_session_lines_initially_empty() {
        let app = App::new();
        assert!(app.session_lines.is_empty());
    }

    #[test]
    fn test_session_buffer_initially_empty() {
        let app = App::new();
        assert!(app.session_buffer.is_empty());
    }

    #[test]
    fn test_process_chunk_preserves_leading_whitespace() {
        let mut app = App::new();
        app.process_session_chunk("  indented\n");
        assert_eq!(app.session_lines[0], "  indented");
    }

    #[test]
    fn test_process_chunk_preserves_trailing_whitespace() {
        let mut app = App::new();
        app.process_session_chunk("trailing  \n");
        assert_eq!(app.session_lines[0], "trailing  ");
    }
}
