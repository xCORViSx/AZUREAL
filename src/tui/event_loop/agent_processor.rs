//! Background thread for agent output JSON parsing
//!
//! Moves serde_json::from_str() off the main event loop thread. The main
//! thread sends raw output strings; the processor parses them and returns
//! DisplayEvents + pre-parsed JSON values. This eliminates 10-50ms of
//! JSON parsing per tick that was blocking input during streaming.
//!
//! Backend-aware: resets create the correct parser (Claude or Codex).

use std::sync::mpsc;
use std::thread;

use crate::backend::Backend;
use crate::events::{CodexEventParser, DisplayEvent, EventParser};
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
    /// Raw agent output to parse
    Parse {
        slot_id: String,
        output_type: OutputType,
        data: String,
    },
    /// Reset parser state (session changed or backend switched)
    Reset { backend: Backend, model: String },
}

/// Trait for streaming event parsers (used only inside processor thread)
trait StreamParser: Send {
    fn parse(&mut self, data: &str) -> (Vec<DisplayEvent>, Option<serde_json::Value>);
}

impl StreamParser for EventParser {
    fn parse(&mut self, data: &str) -> (Vec<DisplayEvent>, Option<serde_json::Value>) {
        EventParser::parse(self, data)
    }
}

impl StreamParser for CodexEventParser {
    fn parse(&mut self, data: &str) -> (Vec<DisplayEvent>, Option<serde_json::Value>) {
        CodexEventParser::parse(self, data)
    }
}

/// Create the correct parser for a given backend
fn create_parser(backend: Backend, model: String) -> Box<dyn StreamParser> {
    match backend {
        Backend::Claude => Box::new(EventParser::new()),
        Backend::Codex => Box::new(CodexEventParser::new(model)),
    }
}

/// Background JSON parser for agent streaming events
pub struct AgentProcessor {
    tx: mpsc::Sender<ProcessorInput>,
    rx: mpsc::Receiver<ProcessedOutput>,
}

impl AgentProcessor {
    /// Spawn the processor background thread with a given backend
    pub fn spawn(backend: Backend, model: String) -> Self {
        let (input_tx, input_rx) = mpsc::channel();
        let (output_tx, output_rx) = mpsc::channel();

        thread::Builder::new()
            .name("agent-parser".into())
            .spawn(move || {
                let mut parser = create_parser(backend, model);
                while let Ok(input) = input_rx.recv() {
                    match input {
                        ProcessorInput::Parse { slot_id, output_type, data } => {
                            let (events, parsed_json) = parser.parse(&data);
                            let _ = output_tx.send(ProcessedOutput {
                                slot_id, events, parsed_json, output_type, data,
                            });
                        }
                        ProcessorInput::Reset { backend, model } => {
                            parser = create_parser(backend, model);
                        }
                    }
                }
            })
            .expect("failed to spawn agent-parser thread");

        AgentProcessor { tx: input_tx, rx: output_rx }
    }

    /// Send raw output to the processor for JSON parsing (non-blocking)
    pub fn submit(&self, slot_id: String, output_type: OutputType, data: String) {
        let _ = self.tx.send(ProcessorInput::Parse { slot_id, output_type, data });
    }

    /// Reset parser state with a specific backend (call on session switch)
    pub fn reset(&self, backend: Backend, model: String) {
        let _ = self.tx.send(ProcessorInput::Reset { backend, model });
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
