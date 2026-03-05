//! Background thread for Claude output JSON parsing
//!
//! Moves serde_json::from_str() off the main event loop thread. The main
//! thread sends raw output strings; the processor parses them and returns
//! DisplayEvents + pre-parsed JSON values. This eliminates 10-50ms of
//! JSON parsing per tick that was blocking input during streaming.

use std::sync::mpsc;
use std::thread;

use crate::events::{DisplayEvent, EventParser};
use crate::models::OutputType;

/// Parsed result from the processor thread
pub struct ProcessedOutput {
    #[allow(dead_code)] // kept for debug identification
    pub slot_id: String,
    pub events: Vec<DisplayEvent>,
    pub parsed_json: Option<serde_json::Value>,
    pub output_type: OutputType,
    pub data: String,
}

/// Commands sent to the processor thread
enum ProcessorInput {
    /// Raw Claude output to parse
    Parse {
        slot_id: String,
        output_type: OutputType,
        data: String,
    },
    /// Reset parser state (session changed)
    Reset,
}

/// Background JSON parser for Claude streaming events
pub struct ClaudeProcessor {
    tx: mpsc::Sender<ProcessorInput>,
    rx: mpsc::Receiver<ProcessedOutput>,
}

impl ClaudeProcessor {
    /// Spawn the processor background thread
    pub fn spawn() -> Self {
        let (input_tx, input_rx) = mpsc::channel();
        let (output_tx, output_rx) = mpsc::channel();

        thread::Builder::new()
            .name("claude-parser".into())
            .spawn(move || {
                let mut parser = EventParser::new();
                while let Ok(input) = input_rx.recv() {
                    match input {
                        ProcessorInput::Parse { slot_id, output_type, data } => {
                            let (events, parsed_json) = parser.parse(&data);
                            let _ = output_tx.send(ProcessedOutput {
                                slot_id, events, parsed_json, output_type, data,
                            });
                        }
                        ProcessorInput::Reset => {
                            parser = EventParser::new();
                        }
                    }
                }
            })
            .expect("failed to spawn claude-parser thread");

        ClaudeProcessor { tx: input_tx, rx: output_rx }
    }

    /// Send raw output to the processor for JSON parsing (non-blocking)
    pub fn submit(&self, slot_id: String, output_type: OutputType, data: String) {
        let _ = self.tx.send(ProcessorInput::Parse { slot_id, output_type, data });
    }

    /// Reset parser state (call on session switch)
    pub fn reset(&self) {
        let _ = self.tx.send(ProcessorInput::Reset);
    }

    /// Drain all pending results, discarding them (call after session switch
    /// to prevent stale parsed events from the old session being applied)
    pub fn drain(&self) {
        while self.rx.try_recv().is_ok() {}
    }

    /// Poll for a parsed result (non-blocking)
    pub fn try_recv(&self) -> Option<ProcessedOutput> {
        self.rx.try_recv().ok()
    }
}
