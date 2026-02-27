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
use super::render_events::ClickablePath;

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
    /// Existing cache for incremental append (empty = full render)
    pub existing_lines: Vec<Line<'static>>,
    pub existing_anim: Vec<(usize, usize)>,
    pub existing_bubbles: Vec<(usize, bool)>,
    pub existing_clickable: Vec<ClickablePath>,
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
    pub anim_indices: Vec<(usize, usize)>,
    pub bubble_positions: Vec<(usize, bool)>,
    pub clickable_paths: Vec<ClickablePath>,
    pub events_count: usize,
    pub events_start: usize,
    pub width: u16,
    pub seq: u64,
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
                let highlighter = SyntaxHighlighter::new();
                render_loop(req_rx, res_tx, &highlighter);
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
    highlighter: &SyntaxHighlighter,
) {
    while let Ok(mut req) = rx.recv() {
        // Drain queued requests — only render the latest one
        while let Ok(newer) = rx.try_recv() { req = newer; }

        let width = req.width;
        let seq = req.seq;
        let deferred_start = req.deferred_start;

        // Incremental if existing cache was provided (events Vec has only NEW events,
        // pre_scan has state from old events). Full render otherwise.
        let (total_events, lines, anim, bubbles, clickable) = if !req.existing_lines.is_empty() {
            let total = req.total_events;
            let (l, a, b, c) = super::render_events::render_display_events_incremental(
                &req.events, width,
                &req.pending_tools, &req.failed_tools, highlighter,
                req.pending_user_message.as_deref(),
                req.existing_lines, req.existing_anim, req.existing_bubbles,
                req.existing_clickable,
                req.pre_scan,
            );
            (total, l, a, b, c)
        } else {
            // Events are already sliced from deferred_start by submit_render_request —
            // no need to skip here. total_events is the FULL count (for rendered_events_count).
            let total = req.total_events;
            let (l, a, b, c) = super::render_events::render_display_events(
                &req.events, width,
                &req.pending_tools, &req.failed_tools, highlighter,
                req.pending_user_message.as_deref(),
            );
            (total, l, a, b, c)
        };

        let _ = tx.send(RenderResult {
            lines, anim_indices: anim, bubble_positions: bubbles,
            clickable_paths: clickable,
            events_count: total_events, events_start: deferred_start,
            width, seq,
        });
    }
}
