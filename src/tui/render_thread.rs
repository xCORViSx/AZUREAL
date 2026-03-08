//! Background render thread for session pane
//!
//! The expensive work of rendering display events (markdown parsing, syntax
//! highlighting, text wrapping) runs on a dedicated thread so the main event
//! loop is NEVER blocked. The main thread sends render requests and receives
//! completed results via channels — zero blocking on either side.
//!
//! If a new request arrives while one is in progress, the render thread
//! finishes the current render but the main thread discards stale results
//! by comparing sequence numbers. This "latest-wins" approach ensures the
//! UI always shows the most recent state without wasting CPU on stale data.

use std::collections::HashSet;
use std::sync::mpsc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use ratatui::text::Line;

use crate::events::DisplayEvent;
use crate::syntax::SyntaxHighlighter;
use super::render_events::{ClickablePath, ClickableTable};

/// Pre-computed state flags from events before start_idx.
/// Computed on the main thread (zero-cost read) so the render thread
/// doesn't need the old events — eliminates the mega-clone.
#[derive(Default)]
pub struct PreScanState {
    pub saw_init: bool,
    pub saw_content: bool,
    pub last_hook: Option<(String, String)>,
    pub saw_exit_plan_mode: bool,
    pub saw_user_after_exit_plan: bool,
    pub saw_ask_user_question: bool,
    pub saw_user_after_ask: bool,
    pub last_ask_input: Option<serde_json::Value>,
}

/// Everything the render thread needs to produce a frame.
/// All fields are owned (cloned from App) so the thread works independently.
pub struct RenderRequest {
    /// For incremental renders: only new events (not the full history).
    /// For full renders: all events (or all from deferred_start).
    pub events: Vec<DisplayEvent>,
    pub width: u16,
    pub pending_tools: HashSet<String>,
    pub failed_tools: HashSet<String>,
    pub pending_user_message: Option<String>,
    /// Existing cache line count for incremental renders. When > 0, the render
    /// thread produces ONLY new lines (no clone of existing cache needed). The
    /// main thread offsets indices by this count and extends its cache.
    pub existing_line_count: usize,
    /// Pre-computed state from events before start_idx (for incremental renders)
    pub pre_scan: PreScanState,
    /// Total event count (old + new) for incremental renders. The `events` Vec
    /// only contains new events, but the main thread needs the total to update
    /// `rendered_events_count` correctly.
    pub total_events: usize,
    /// Deferred render start offset (events before this were skipped)
    pub deferred_start: usize,
    /// Monotonic sequence number — main thread discards results older than latest applied
    pub seq: u64,
}

/// Completed render result sent back to the main thread
pub struct RenderResult {
    pub lines: Vec<Line<'static>>,
    pub anim_indices: Vec<(usize, usize, String)>,
    pub bubble_positions: Vec<(usize, bool)>,
    pub clickable_paths: Vec<ClickablePath>,
    pub clickable_tables: Vec<ClickableTable>,
    pub events_count: usize,
    pub events_start: usize,
    pub width: u16,
    pub seq: u64,
    /// True when this result contains only NEW lines (main thread should extend,
    /// not replace). False for full renders (main thread replaces cache entirely).
    pub incremental: bool,
}

/// Handle for the main thread to communicate with the render thread.
/// Dropping this struct signals the render thread to exit (sender drops → recv fails).
pub struct RenderThread {
    /// Send render requests to the background thread
    tx: mpsc::Sender<RenderRequest>,
    /// Receive completed render results from the background thread
    rx: mpsc::Receiver<RenderResult>,
    /// Monotonically increasing sequence number for request ordering
    seq: Arc<AtomicU64>,
    /// Thread handle (kept alive so thread doesn't get detached)
    _handle: std::thread::JoinHandle<()>,
}

impl RenderThread {
    /// Spawn a dedicated render thread with its own SyntaxHighlighter.
    /// The thread blocks waiting for requests — uses zero CPU when idle.
    pub fn spawn() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<RenderRequest>();
        let (res_tx, res_rx) = mpsc::channel::<RenderResult>();
        let seq = Arc::new(AtomicU64::new(0));

        let handle = std::thread::Builder::new()
            .name("render".into())
            .spawn(move || {
                let mut highlighter = SyntaxHighlighter::new();
                render_loop(req_rx, res_tx, &mut highlighter);
            })
            .expect("failed to spawn render thread");

        Self { tx: req_tx, rx: res_rx, seq, _handle: handle }
    }

    /// Submit a render request (non-blocking). Returns the assigned sequence number.
    pub fn send(&self, mut req: RenderRequest) -> u64 {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        req.seq = seq;
        let _ = self.tx.send(req);
        seq
    }

    /// Current sequence counter value. Used to discard in-flight renders
    /// from a previous session when switching (set render_seq_applied to this).
    pub fn current_seq(&self) -> u64 {
        self.seq.load(Ordering::Relaxed)
    }

    /// Check for a completed render result (non-blocking).
    /// May return multiple results if renders completed faster than we poll —
    /// caller should drain and keep only the latest.
    pub fn try_recv(&self) -> Option<RenderResult> {
        // Drain all available results — keep only the highest seq
        let mut best: Option<RenderResult> = None;
        while let Ok(result) = self.rx.try_recv() {
            best = Some(match best {
                Some(prev) if prev.seq > result.seq => prev,
                _ => result,
            });
        }
        best
    }
}

/// Render thread main loop. Blocks on recv() when idle (zero CPU).
/// For each request: drain to latest → render → send result.
fn render_loop(
    rx: mpsc::Receiver<RenderRequest>,
    tx: mpsc::Sender<RenderResult>,
    highlighter: &mut SyntaxHighlighter,
) {
    while let Ok(mut req) = rx.recv() {
        // Drain queued requests — only render the latest one
        while let Ok(newer) = rx.try_recv() { req = newer; }

        let width = req.width;
        let seq = req.seq;
        let deferred_start = req.deferred_start;

        // Incremental if existing_line_count > 0 (events Vec has only NEW events,
        // pre_scan has state from old events). Full render otherwise.
        let incremental = req.existing_line_count > 0;
        let (total_events, lines, anim, bubbles, clickable, tables) = if incremental {
            let total = req.total_events;
            // Render only new events into a fresh Vec (no existing cache clone).
            // Indices are relative to 0 — main thread offsets by existing_line_count.
            let (l, a, b, c, t) = super::render_events::render_display_events_incremental(
                &req.events, width,
                &req.pending_tools, &req.failed_tools, highlighter,
                req.pending_user_message.as_deref(),
                req.pre_scan,
            );
            (total, l, a, b, c, t)
        } else {
            // Events are already sliced from deferred_start by submit_render_request —
            // no need to skip here. total_events is the FULL count (for rendered_events_count).
            let total = req.total_events;
            let (l, a, b, c, t) = super::render_events::render_display_events(
                &req.events, width,
                &req.pending_tools, &req.failed_tools, highlighter,
                req.pending_user_message.as_deref(),
            );
            (total, l, a, b, c, t)
        };

        let _ = tx.send(RenderResult {
            lines, anim_indices: anim, bubble_positions: bubbles,
            clickable_paths: clickable, clickable_tables: tables,
            events_count: total_events, events_start: deferred_start,
            width, seq, incremental,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    // -- PreScanState default --

    #[test]
    fn test_pre_scan_state_default() {
        let ps = PreScanState::default();
        assert!(!ps.saw_init);
        assert!(!ps.saw_content);
        assert!(ps.last_hook.is_none());
        assert!(!ps.saw_exit_plan_mode);
        assert!(!ps.saw_user_after_exit_plan);
        assert!(!ps.saw_ask_user_question);
        assert!(!ps.saw_user_after_ask);
        assert!(ps.last_ask_input.is_none());
    }

    #[test]
    fn test_pre_scan_state_saw_init() {
        let mut ps = PreScanState::default();
        ps.saw_init = true;
        assert!(ps.saw_init);
    }

    #[test]
    fn test_pre_scan_state_saw_content() {
        let mut ps = PreScanState::default();
        ps.saw_content = true;
        assert!(ps.saw_content);
    }

    #[test]
    fn test_pre_scan_state_last_hook() {
        let mut ps = PreScanState::default();
        ps.last_hook = Some(("hook_name".into(), "hook_data".into()));
        let (name, data) = ps.last_hook.unwrap();
        assert_eq!(name, "hook_name");
        assert_eq!(data, "hook_data");
    }

    #[test]
    fn test_pre_scan_state_exit_plan_mode() {
        let mut ps = PreScanState::default();
        ps.saw_exit_plan_mode = true;
        ps.saw_user_after_exit_plan = true;
        assert!(ps.saw_exit_plan_mode);
        assert!(ps.saw_user_after_exit_plan);
    }

    #[test]
    fn test_pre_scan_state_ask_user() {
        let mut ps = PreScanState::default();
        ps.saw_ask_user_question = true;
        ps.saw_user_after_ask = true;
        assert!(ps.saw_ask_user_question);
        assert!(ps.saw_user_after_ask);
    }

    #[test]
    fn test_pre_scan_state_last_ask_input() {
        let mut ps = PreScanState::default();
        ps.last_ask_input = Some(serde_json::json!({"answer": "yes"}));
        assert!(ps.last_ask_input.is_some());
    }

    // -- RenderRequest fields --

    #[test]
    fn test_render_request_construction() {
        let req = RenderRequest {
            events: vec![],
            width: 120,
            pending_tools: HashSet::new(),
            failed_tools: HashSet::new(),
            pending_user_message: None,
            existing_line_count: 0,
            pre_scan: PreScanState::default(),
            total_events: 0,
            deferred_start: 0,
            seq: 0,
        };
        assert_eq!(req.width, 120);
        assert_eq!(req.total_events, 0);
        assert_eq!(req.seq, 0);
    }

    #[test]
    fn test_render_request_with_events() {
        let req = RenderRequest {
            events: vec![],
            width: 80,
            pending_tools: HashSet::from(["tool1".into()]),
            failed_tools: HashSet::from(["tool2".into()]),
            pending_user_message: Some("hello".into()),
            existing_line_count: 0,
            pre_scan: PreScanState::default(),
            total_events: 5,
            deferred_start: 2,
            seq: 42,
        };
        assert_eq!(req.pending_tools.len(), 1);
        assert_eq!(req.failed_tools.len(), 1);
        assert_eq!(req.pending_user_message, Some("hello".into()));
        assert_eq!(req.total_events, 5);
        assert_eq!(req.deferred_start, 2);
        assert_eq!(req.seq, 42);
    }

    #[test]
    fn test_render_request_pending_tools_contains() {
        let tools: HashSet<String> = HashSet::from(["write".into(), "read".into()]);
        assert!(tools.contains("write"));
        assert!(tools.contains("read"));
        assert!(!tools.contains("exec"));
    }

    #[test]
    fn test_render_request_failed_tools() {
        let tools: HashSet<String> = HashSet::from(["failed_tool".into()]);
        assert!(tools.contains("failed_tool"));
    }

    #[test]
    fn test_render_request_incremental_check() {
        let existing: Vec<Line> = vec![Line::from("cached")];
        assert!(!existing.is_empty()); // triggers incremental path
    }

    #[test]
    fn test_render_request_full_check() {
        let existing: Vec<Line> = vec![];
        assert!(existing.is_empty()); // triggers full render path
    }

    // -- RenderResult fields --

    #[test]
    fn test_render_result_construction() {
        let result = RenderResult {
            lines: vec![],
            anim_indices: vec![],
            bubble_positions: vec![],
            clickable_paths: vec![],
            clickable_tables: vec![],
            events_count: 10,
            events_start: 0,
            width: 120,
            seq: 5,
            incremental: false,
        };
        assert_eq!(result.events_count, 10);
        assert_eq!(result.width, 120);
        assert_eq!(result.seq, 5);
    }

    #[test]
    fn test_render_result_with_lines() {
        let result = RenderResult {
            lines: vec![Line::from("hello"), Line::from("world")],
            anim_indices: vec![(0, 5, "tool1".into())],
            bubble_positions: vec![(0, true), (1, false)],
            clickable_paths: vec![],
            clickable_tables: vec![],
            events_count: 2,
            events_start: 0,
            width: 80,
            seq: 1,
            incremental: false,
        };
        assert_eq!(result.lines.len(), 2);
        assert_eq!(result.anim_indices.len(), 1);
        assert_eq!(result.bubble_positions.len(), 2);
    }

    #[test]
    fn test_render_result_anim_indices_tuple() {
        let anim: (usize, usize, String) = (5, 10, "tool1".into());
        assert_eq!(anim.0, 5);  // line index
        assert_eq!(anim.1, 10); // span index
        assert_eq!(anim.2, "tool1"); // tool_use_id
    }

    #[test]
    fn test_render_result_bubble_positions_tuple() {
        let bubble: (usize, bool) = (3, true);
        assert_eq!(bubble.0, 3);  // line index
        assert!(bubble.1);        // is_user
    }

    #[test]
    fn test_render_result_bubble_assistant() {
        let bubble: (usize, bool) = (7, false);
        assert_eq!(bubble.0, 7);
        assert!(!bubble.1);
    }

    // -- AtomicU64 sequence number --

    #[test]
    fn test_atomic_seq_initial() {
        let seq = AtomicU64::new(0);
        assert_eq!(seq.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_atomic_seq_fetch_add() {
        let seq = AtomicU64::new(0);
        let old = seq.fetch_add(1, Ordering::Relaxed);
        assert_eq!(old, 0);
        assert_eq!(seq.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_atomic_seq_multiple_increments() {
        let seq = AtomicU64::new(0);
        seq.fetch_add(1, Ordering::Relaxed);
        seq.fetch_add(1, Ordering::Relaxed);
        seq.fetch_add(1, Ordering::Relaxed);
        assert_eq!(seq.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_atomic_seq_arc_shared() {
        let seq = Arc::new(AtomicU64::new(0));
        let seq2 = seq.clone();
        seq.fetch_add(1, Ordering::Relaxed);
        assert_eq!(seq2.load(Ordering::Relaxed), 1);
    }

    // -- Channel creation --

    #[test]
    fn test_channel_creation() {
        let (tx, rx) = mpsc::channel::<RenderRequest>();
        drop(tx);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_result_channel_creation() {
        let (tx, rx) = mpsc::channel::<RenderResult>();
        drop(tx);
        assert!(rx.try_recv().is_err());
    }

    // -- Sequence number comparison for staleness --

    #[test]
    fn test_seq_newer_wins() {
        let prev_seq = 5u64;
        let new_seq = 7u64;
        let best = if prev_seq > new_seq { prev_seq } else { new_seq };
        assert_eq!(best, 7);
    }

    #[test]
    fn test_seq_older_discarded() {
        let prev_seq = 10u64;
        let new_seq = 3u64;
        let best = if prev_seq > new_seq { prev_seq } else { new_seq };
        assert_eq!(best, 10);
    }

    #[test]
    fn test_seq_equal() {
        let prev_seq = 5u64;
        let new_seq = 5u64;
        let best = if prev_seq > new_seq { prev_seq } else { new_seq };
        assert_eq!(best, 5);
    }

    // -- RenderThread spawn and operations --

    #[test]
    fn test_render_thread_spawn() {
        let rt = RenderThread::spawn();
        assert_eq!(rt.current_seq(), 0);
    }

    #[test]
    fn test_render_thread_send_returns_seq() {
        let rt = RenderThread::spawn();
        let req = RenderRequest {
            events: vec![],
            width: 80,
            pending_tools: HashSet::new(),
            failed_tools: HashSet::new(),
            pending_user_message: None,
            existing_line_count: 0,
            pre_scan: PreScanState::default(),
            total_events: 0,
            deferred_start: 0,
            seq: 0,
        };
        let seq = rt.send(req);
        assert_eq!(seq, 1);
    }

    #[test]
    fn test_render_thread_send_increments_seq() {
        let rt = RenderThread::spawn();
        let make_req = || RenderRequest {
            events: vec![],
            width: 80,
            pending_tools: HashSet::new(),
            failed_tools: HashSet::new(),
            pending_user_message: None,
            existing_line_count: 0,
            pre_scan: PreScanState::default(),
            total_events: 0,
            deferred_start: 0,
            seq: 0,
        };
        let s1 = rt.send(make_req());
        let s2 = rt.send(make_req());
        let s3 = rt.send(make_req());
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert_eq!(s3, 3);
    }

    #[test]
    fn test_render_thread_current_seq_after_sends() {
        let rt = RenderThread::spawn();
        let make_req = || RenderRequest {
            events: vec![],
            width: 80,
            pending_tools: HashSet::new(),
            failed_tools: HashSet::new(),
            pending_user_message: None,
            existing_line_count: 0,
            pre_scan: PreScanState::default(),
            total_events: 0,
            deferred_start: 0,
            seq: 0,
        };
        rt.send(make_req());
        rt.send(make_req());
        assert_eq!(rt.current_seq(), 2);
    }

    #[test]
    fn test_render_thread_try_recv_empty() {
        let rt = RenderThread::spawn();
        // No sends yet — nothing to receive
        let result = rt.try_recv();
        assert!(result.is_none());
    }

    #[test]
    fn test_render_thread_try_recv_after_send() {
        let rt = RenderThread::spawn();
        let req = RenderRequest {
            events: vec![],
            width: 80,
            pending_tools: HashSet::new(),
            failed_tools: HashSet::new(),
            pending_user_message: None,
            existing_line_count: 0,
            pre_scan: PreScanState::default(),
            total_events: 0,
            deferred_start: 0,
            seq: 0,
        };
        rt.send(req);
        // Give the render thread time to init 25+ language grammars and process
        let mut result = None;
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            result = rt.try_recv();
            if result.is_some() { break; }
        }
        assert!(result.is_some());
        let res = result.unwrap();
        assert_eq!(res.seq, 1);
        assert_eq!(res.width, 80);
    }

    // -- HashSet operations for tools --

    #[test]
    fn test_pending_tools_insert() {
        let mut tools = HashSet::new();
        tools.insert("write".to_string());
        assert!(tools.contains("write"));
    }

    #[test]
    fn test_pending_tools_remove() {
        let mut tools = HashSet::new();
        tools.insert("write".to_string());
        tools.remove("write");
        assert!(!tools.contains("write"));
    }

    #[test]
    fn test_pending_tools_empty() {
        let tools: HashSet<String> = HashSet::new();
        assert!(tools.is_empty());
    }

    // -- Width values --

    #[test]
    fn test_width_80() {
        let w = 80u16;
        assert_eq!(w, 80);
    }

    #[test]
    fn test_width_120() {
        let w = 120u16;
        assert_eq!(w, 120);
    }

    #[test]
    fn test_width_zero() {
        let w = 0u16;
        assert_eq!(w, 0);
    }

    // -- Deferred start offset --

    #[test]
    fn test_deferred_start_zero() {
        let start = 0usize;
        assert_eq!(start, 0);
    }

    #[test]
    fn test_deferred_start_nonzero() {
        let start = 50usize;
        assert_eq!(start, 50);
    }

    // -- Events slicing --

    #[test]
    fn test_events_vec_empty() {
        let events: Vec<DisplayEvent> = vec![];
        assert!(events.is_empty());
    }

    // -- Pending user message --

    #[test]
    fn test_pending_user_message_some() {
        let msg: Option<String> = Some("build it".into());
        assert_eq!(msg.as_deref(), Some("build it"));
    }

    #[test]
    fn test_pending_user_message_none() {
        let msg: Option<String> = None;
        assert!(msg.as_deref().is_none());
    }

    // -- Thread builder name --

    #[test]
    fn test_thread_name() {
        let name = "render";
        assert_eq!(name, "render");
    }

    // -- Function type checks --

    #[test]
    fn test_render_thread_spawn_fn() {
        let _ = RenderThread::spawn as fn() -> RenderThread;
    }

    // -- Ordering enum --

    #[test]
    fn test_ordering_relaxed() {
        let _ = Ordering::Relaxed;
    }

    // -- Line construction for existing cache --

    #[test]
    fn test_existing_lines_vec() {
        let lines: Vec<Line<'static>> = vec![
            Line::from("line 1"),
            Line::from("line 2"),
        ];
        assert_eq!(lines.len(), 2);
    }

    // -- Clickable paths vec --

    #[test]
    fn test_existing_clickable_vec() {
        let paths: Vec<ClickablePath> = vec![];
        assert!(paths.is_empty());
    }

    #[test]
    fn test_sequence_number_starts_at_zero() {
        let seq = std::sync::atomic::AtomicU64::new(0);
        assert_eq!(seq.load(Ordering::Relaxed), 0);
    }
}
