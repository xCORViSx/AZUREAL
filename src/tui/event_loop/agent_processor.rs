//! Background thread for agent output JSON parsing
//!
//! Moves serde_json::from_str() off the main event loop thread. The main
//! thread sends raw output strings; the processor parses them and returns
//! DisplayEvents + pre-parsed JSON values. This eliminates 10-50ms of
//! JSON parsing per tick that was blocking input during streaming.
//!
//! Backend-aware: resets create the correct parser (Claude or Codex).

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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
        backend: Backend,
        model: String,
        output_type: OutputType,
        data: String,
    },
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

struct SlotParser {
    backend: Backend,
    model: String,
    parser: Box<dyn StreamParser>,
}

/// Background JSON parser for agent streaming events
pub struct AgentProcessor {
    tx: mpsc::Sender<ProcessorInput>,
    rx: mpsc::Receiver<ProcessedOutput>,
}

impl AgentProcessor {
    /// Spawn the processor background thread with a given backend
    pub fn spawn(_backend: Backend, _model: String) -> Self {
        let (input_tx, input_rx) = mpsc::channel();
        let (output_tx, output_rx) = mpsc::channel();

        thread::Builder::new()
            .name("agent-parser".into())
            .spawn(move || {
                let mut parsers: HashMap<String, SlotParser> = HashMap::new();
                while let Ok(input) = input_rx.recv() {
                    match input {
                        ProcessorInput::Parse {
                            slot_id,
                            backend,
                            model,
                            output_type,
                            data,
                        } => {
                            let slot_parser =
                                parsers
                                    .entry(slot_id.clone())
                                    .or_insert_with(|| SlotParser {
                                        backend,
                                        model: model.clone(),
                                        parser: create_parser(backend, model.clone()),
                                    });
                            if slot_parser.backend != backend || slot_parser.model != model {
                                *slot_parser = SlotParser {
                                    backend,
                                    model: model.clone(),
                                    parser: create_parser(backend, model),
                                };
                            }
                            let (events, parsed_json) = slot_parser.parser.parse(&data);
                            let _ = output_tx.send(ProcessedOutput {
                                slot_id,
                                events,
                                parsed_json,
                                output_type,
                                data,
                            });
                        }
                    }
                }
            })
            .expect("failed to spawn agent-parser thread");

        AgentProcessor {
            tx: input_tx,
            rx: output_rx,
        }
    }

    /// Send raw output to the processor for JSON parsing (non-blocking)
    pub fn submit(
        &self,
        slot_id: String,
        backend: Backend,
        model: String,
        output_type: OutputType,
        data: String,
    ) {
        let _ = self.tx.send(ProcessorInput::Parse {
            slot_id,
            backend,
            model,
            output_type,
            data,
        });
    }

    /// Poll for a parsed result (non-blocking)
    pub fn try_recv(&self) -> Option<ProcessedOutput> {
        self.rx.try_recv().ok()
    }

    /// Wait briefly for a parsed result. Used at process exit to flush output
    /// submitted immediately before the lifecycle event is handled.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<ProcessedOutput> {
        self.rx.recv_timeout(timeout).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::DisplayEvent;
    use std::time::{Duration, Instant};

    fn wait_for_nonempty(processor: &AgentProcessor, expected: usize) -> Vec<ProcessedOutput> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut results = Vec::new();
        while Instant::now() < deadline && results.len() < expected {
            if let Some(result) = processor.try_recv() {
                if !result.events.is_empty() {
                    results.push(result);
                }
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        results
    }

    #[test]
    fn parses_interleaved_slots_with_independent_buffers() {
        let processor = AgentProcessor::spawn(Backend::Codex, "gpt-default".into());
        let a =
            r#"{"type":"session_meta","payload":{"id":"slot-a","cwd":"/a"}}"#.to_string() + "\n";
        let b =
            r#"{"type":"session_meta","payload":{"id":"slot-b","cwd":"/b"}}"#.to_string() + "\n";
        let split_at = a.len() / 2;
        let (a1, a2) = a.split_at(split_at);

        processor.submit(
            "a".into(),
            Backend::Codex,
            "gpt-a".into(),
            OutputType::Stdout,
            a1.into(),
        );
        processor.submit(
            "b".into(),
            Backend::Codex,
            "gpt-b".into(),
            OutputType::Stdout,
            b,
        );
        processor.submit(
            "a".into(),
            Backend::Codex,
            "gpt-a".into(),
            OutputType::Stdout,
            a2.into(),
        );

        let results = wait_for_nonempty(&processor, 2);
        assert_eq!(results.len(), 2);
        let mut seen = results
            .iter()
            .filter_map(|result| match result.events.first() {
                Some(DisplayEvent::Init {
                    _session_id, model, ..
                }) => Some((
                    result.slot_id.as_str(),
                    _session_id.as_str(),
                    model.as_str(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>();
        seen.sort_unstable();
        assert_eq!(
            seen,
            vec![("a", "slot-a", "gpt-a"), ("b", "slot-b", "gpt-b")]
        );
    }
}
