//! Background thread for agent output JSON parsing
//!
//! Moves serde_json::from_str() off the main event loop thread. The main
//! thread sends raw output strings; the processor parses them and returns
//! DisplayEvents + pre-parsed JSON values. This eliminates 10-50ms of
//! JSON parsing per tick that was blocking input during streaming.
//!
//! Backend-aware: resets create the correct parser (Claude or Codex).

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::backend::Backend;
use crate::events::{CodexEventParser, DisplayEvent, EventParser};
use crate::models::OutputType;

/// Maximum number of per-slot streaming parsers retained by the background processor.
const MAX_SLOT_PARSERS: usize = 128;

/// Parsed result from the processor thread.
pub struct ProcessedOutput {
    #[allow(dead_code)] // kept for debug identification
    /// PID-backed slot whose raw output produced this result.
    pub slot_id: String,
    /// Display events decoded from the raw agent output.
    pub events: Vec<DisplayEvent>,
    /// JSON value decoded from a complete stream line when the parser exposes one.
    pub parsed_json: Option<serde_json::Value>,
    /// Stream that produced the raw output.
    pub output_type: OutputType,
    /// Original raw output chunk, preserved for legacy accounting paths.
    pub data: String,
}

/// Commands sent to the processor thread.
enum ProcessorInput {
    /// Raw agent output to parse.
    Parse {
        /// PID-backed slot for the agent process that emitted the output.
        slot_id: String,
        /// Backend format to use for parsing this slot.
        backend: Backend,
        /// Model label used by Codex init events.
        model: String,
        /// Stream that produced the raw output.
        output_type: OutputType,
        /// Raw JSONL fragment from the agent process.
        data: String,
    },
}

/// Trait for streaming event parsers used inside the processor thread.
trait StreamParser: Send {
    /// Parse a raw stream fragment into display events and an optional JSON value.
    fn parse(&mut self, data: &str) -> (Vec<DisplayEvent>, Option<serde_json::Value>);
}

/// Adapts Claude stream parsing to the slot-agnostic processor interface.
impl StreamParser for EventParser {
    /// Parse a Claude stream fragment through the existing Claude parser.
    fn parse(&mut self, data: &str) -> (Vec<DisplayEvent>, Option<serde_json::Value>) {
        EventParser::parse(self, data)
    }
}

/// Adapts Codex stream parsing to the slot-agnostic processor interface.
impl StreamParser for CodexEventParser {
    /// Parse a Codex stream fragment through the existing Codex parser.
    fn parse(&mut self, data: &str) -> (Vec<DisplayEvent>, Option<serde_json::Value>) {
        CodexEventParser::parse(self, data)
    }
}

/// Create the correct parser for a given backend.
fn create_parser(backend: Backend, model: String) -> Box<dyn StreamParser> {
    match backend {
        Backend::Claude => Box::new(EventParser::new()),
        Backend::Codex => Box::new(CodexEventParser::new(model)),
    }
}

/// Parser state retained for one streaming agent slot.
struct SlotParser {
    /// Backend format the parser currently expects.
    backend: Backend,
    /// Model label bound to this parser, used for Codex session metadata.
    model: String,
    /// Backend-specific parser with any partial JSONL buffer for this slot.
    parser: Box<dyn StreamParser>,
}

/// Mark a slot as the most recently used parser entry.
fn touch_slot(parser_order: &mut VecDeque<String>, slot_id: &str) {
    if let Some(pos) = parser_order.iter().position(|existing| existing == slot_id) {
        parser_order.remove(pos);
    }
    parser_order.push_back(slot_id.to_string());
}

/// Remove the least recently used parser when the processor reaches its slot cap.
fn evict_oldest_parser(
    parsers: &mut HashMap<String, SlotParser>,
    parser_order: &mut VecDeque<String>,
) {
    while parsers.len() >= MAX_SLOT_PARSERS {
        let Some(oldest) = parser_order.pop_front() else {
            break;
        };
        if parsers.remove(&oldest).is_some() {
            break;
        }
    }
}

/// Background JSON parser for agent streaming events.
pub struct AgentProcessor {
    /// Sender used by the event loop to submit raw agent output.
    tx: mpsc::Sender<ProcessorInput>,
    /// Receiver used by the event loop to collect parsed agent output.
    rx: mpsc::Receiver<ProcessedOutput>,
}

/// Lifecycle and I/O methods for the background agent parser.
impl AgentProcessor {
    /// Spawn the processor background thread with a given backend.
    pub fn spawn(_backend: Backend, _model: String) -> Self {
        let (input_tx, input_rx) = mpsc::channel();
        let (output_tx, output_rx) = mpsc::channel();

        thread::Builder::new()
            .name("agent-parser".into())
            .spawn(move || {
                let mut parsers: HashMap<String, SlotParser> = HashMap::new();
                let mut parser_order: VecDeque<String> = VecDeque::new();
                while let Ok(input) = input_rx.recv() {
                    match input {
                        ProcessorInput::Parse {
                            slot_id,
                            backend,
                            model,
                            output_type,
                            data,
                        } => {
                            if !parsers.contains_key(&slot_id) {
                                evict_oldest_parser(&mut parsers, &mut parser_order);
                                parsers.insert(
                                    slot_id.clone(),
                                    SlotParser {
                                        backend,
                                        model: model.clone(),
                                        parser: create_parser(backend, model.clone()),
                                    },
                                );
                            }
                            touch_slot(&mut parser_order, &slot_id);

                            let Some(slot_parser) = parsers.get_mut(&slot_id) else {
                                continue;
                            };
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

    /// Send raw output to the processor for JSON parsing without blocking the event loop.
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

    /// Poll for a parsed result without blocking.
    pub fn try_recv(&self) -> Option<ProcessedOutput> {
        self.rx.try_recv().ok()
    }

    /// Wait briefly for a parsed result. Used at process exit to flush output
    /// submitted immediately before the lifecycle event is handled.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<ProcessedOutput> {
        self.rx.recv_timeout(timeout).ok()
    }
}

/// Tests for slot-scoped parsing and parser retention limits.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::DisplayEvent;
    use std::time::{Duration, Instant};

    /// Wait for a fixed number of parsed outputs, including empty event batches.
    fn wait_for_results(processor: &AgentProcessor, expected: usize) -> Vec<ProcessedOutput> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut results = Vec::new();
        while Instant::now() < deadline && results.len() < expected {
            if let Some(result) = processor.try_recv() {
                results.push(result);
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        results
    }

    /// Wait for parsed outputs that contain at least one display event.
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

    /// Interleaved streaming chunks keep independent partial JSONL buffers per slot.
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

    /// The processor evicts stale parser buffers once many completed slots have accumulated.
    #[test]
    fn evicts_oldest_slot_parser_when_capacity_is_exceeded() {
        let processor = AgentProcessor::spawn(Backend::Codex, "gpt-default".into());
        let line = r#"{"type":"session_meta","payload":{"id":"slot-zero","cwd":"/zero"}}"#
            .to_string()
            + "\n";
        let split_at = line.len() / 2;
        let (first_half, second_half) = line.split_at(split_at);

        processor.submit(
            "slot-zero".into(),
            Backend::Codex,
            "gpt-zero".into(),
            OutputType::Stdout,
            first_half.into(),
        );

        for idx in 1..=MAX_SLOT_PARSERS {
            processor.submit(
                format!("slot-{}", idx),
                Backend::Codex,
                format!("gpt-{}", idx),
                OutputType::Stdout,
                String::new(),
            );
        }

        processor.submit(
            "slot-zero".into(),
            Backend::Codex,
            "gpt-zero".into(),
            OutputType::Stdout,
            second_half.into(),
        );

        let results = wait_for_results(&processor, MAX_SLOT_PARSERS + 2);
        assert_eq!(results.len(), MAX_SLOT_PARSERS + 2);
        assert!(results.iter().all(|result| {
            !result.events.iter().any(|event| {
                matches!(
                    event,
                    DisplayEvent::Init {
                        _session_id,
                        ..
                    } if _session_id == "slot-zero"
                )
            })
        }));
    }
}
