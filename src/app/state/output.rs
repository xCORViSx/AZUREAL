//! Output processing and display event handling

use crate::app::util::strip_ansi_escapes;
use crate::events::DisplayEvent;
use super::App;

impl App {
    pub fn process_output_chunk(&mut self, chunk: &str) {
        let cleaned = strip_ansi_escapes(chunk);
        for ch in cleaned.chars() {
            match ch {
                '\n' => {
                    // Move the buffer into the line vec instead of clone+clear —
                    // take() reuses capacity for the next line (zero allocation).
                    self.output_lines.push_back(std::mem::take(&mut self.output_buffer));
                    if self.output_lines.len() > self.max_output_lines { self.output_lines.pop_front(); }
                }
                '\r' => self.output_buffer.clear(),
                _ => self.output_buffer.push(ch),
            }
        }
    }

    pub fn add_output(&mut self, chunk: String) {
        let (events, _json) = self.event_parser.parse(&chunk);
        self.display_events.extend(events);
        self.invalidate_render_cache();
        self.process_output_chunk(&chunk);
    }

    /// Add a user message to the convo pane immediately on prompt submit.
    /// Pushes a real DisplayEvent::UserMessage into display_events so it
    /// renders persistently (no disappearing). Also stores the content as
    /// `pending_user_message` — this is ONLY used as a dedup marker so the
    /// full re-parse on Claude exit can detect and skip the duplicate.
    pub fn add_user_message(&mut self, content: String) {
        // Compaction summaries are internal — show banner, not raw text
        if content.starts_with("This session is being continued from a previous conversation") {
            self.display_events.push(DisplayEvent::Compacting);
            self.invalidate_render_cache();
            self.output_scroll = usize::MAX;
            return;
        }
        // Push a real event so it renders immediately and persists through
        // the entire conversation. stream-json stdout never emits user events,
        // so without this the message would be invisible until Claude exits
        // and the session file is re-parsed.
        self.display_events.push(DisplayEvent::UserMessage {
            uuid: String::new(),
            content: content.clone(),
        });
        // Dedup marker: full re-parse (on Claude exit) will check this to
        // avoid creating a second UserMessage for the same content.
        self.pending_user_message = Some(content);
        self.invalidate_render_cache();
        self.output_scroll = usize::MAX;
    }
}
