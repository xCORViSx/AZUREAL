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
                    let line = self.output_buffer.clone();
                    self.output_lines.push_back(line);
                    self.output_buffer.clear();
                    if self.output_lines.len() > self.max_output_lines { self.output_lines.pop_front(); }
                }
                '\r' => self.output_buffer.clear(),
                _ => self.output_buffer.push(ch),
            }
        }
    }

    pub fn add_output(&mut self, chunk: String) {
        let events = self.event_parser.parse(&chunk);
        self.display_events.extend(events);
        self.invalidate_render_cache();
        self.process_output_chunk(&chunk);
    }

    pub fn add_user_message(&mut self, content: String) {
        // Store as pending - will be shown until session file contains it
        self.pending_user_message = Some(content);
        self.invalidate_render_cache();
        self.output_scroll = usize::MAX;
    }
}
