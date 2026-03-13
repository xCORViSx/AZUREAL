//! Speech-to-text integration (⌃s toggle recording/transcription)

use super::App;

impl App {
    /// Toggle speech-to-text recording. Lazy-initializes the STT background thread on first use.
    /// Press once to start recording (magenta border), press again to stop and transcribe.
    pub fn toggle_stt(&mut self) {
        // Lazy-init: spawn the STT thread only when the user first presses ⌃s
        if self.stt_handle.is_none() {
            self.stt_handle = Some(crate::stt::SttHandle::spawn());
        }
        let handle = self.stt_handle.as_ref().unwrap();
        if self.stt_recording {
            handle.send(crate::stt::SttCommand::StopRecording);
        } else {
            handle.send(crate::stt::SttCommand::StartRecording);
        }
    }

    /// Poll STT events from background thread (non-blocking). Returns true if state changed.
    /// Called every event loop iteration when stt_handle exists.
    /// Collects events first to avoid borrow conflict (try_recv borrows handle, processing borrows &mut self).
    pub fn poll_stt(&mut self) -> bool {
        let events: Vec<_> = self.stt_handle.as_ref()
            .map(|h| std::iter::from_fn(|| h.try_recv()).collect())
            .unwrap_or_default();
        if events.is_empty() { return false; }
        for event in events {
            match event {
                crate::stt::SttEvent::RecordingStarted => {
                    self.stt_recording = true;
                    self.set_status("Recording...");
                }
                crate::stt::SttEvent::RecordingStopped { duration_secs } => {
                    self.stt_recording = false;
                    self.set_status(format!("Transcribing {:.1}s of audio...", duration_secs));
                }
                crate::stt::SttEvent::Transcribed(text) => {
                    self.stt_transcribing = false;
                    self.insert_stt_text(&text);
                    self.clear_status();
                }
                crate::stt::SttEvent::Error(msg) => {
                    self.stt_recording = false;
                    self.stt_transcribing = false;
                    self.set_status(format!("STT: {}", msg));
                }
                crate::stt::SttEvent::ModelLoading => {
                    self.stt_transcribing = true;
                    self.set_status("Loading Whisper model...");
                }
                crate::stt::SttEvent::ModelReady => {}
            }
        }
        true
    }

    /// Insert transcribed text at the current cursor position.
    /// Routes to viewer edit buffer when in edit mode, otherwise to prompt input.
    /// Adds a leading space if the previous char isn't whitespace.
    fn insert_stt_text(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() { return; }

        if self.viewer_edit_mode {
            // Insert into viewer edit buffer at cursor position
            let (line, col) = self.viewer_edit_cursor;
            if let Some(line_str) = self.viewer_edit_content.get(line) {
                // Add space if previous char isn't whitespace
                if col > 0 {
                    if let Some(prev) = line_str.chars().nth(col - 1) {
                        if !prev.is_whitespace() {
                            self.viewer_edit_char(' ');
                        }
                    }
                }
            }
            for c in trimmed.chars() {
                self.viewer_edit_char(c);
            }
            self.viewer_edit_scroll_to_cursor();
        } else {
            // Insert into prompt input at cursor position
            if self.input_cursor > 0 {
                let chars: Vec<char> = self.input.chars().collect();
                if let Some(&prev) = chars.get(self.input_cursor - 1) {
                    if !prev.is_whitespace() {
                        self.input_char(' ');
                    }
                }
            }
            for c in trimmed.chars() {
                self.input_char(c);
            }
        }
    }
}
