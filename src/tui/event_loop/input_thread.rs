//! Dedicated input reader thread
//!
//! Reads crossterm events on a background thread so stdin is always being
//! drained — even during terminal.draw() (~18ms) or other blocking work on
//! the main thread. Without this, keystrokes that arrive during a draw sit
//! in the kernel tty buffer and can be dropped by some terminal emulators
//! under heavy output load.

use crossterm::event::{self, Event, KeyEventKind};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Spawns a background thread that continuously reads crossterm events
/// and sends them to the returned receiver. The thread exits when the
/// receiver is dropped (send fails).
pub fn spawn_input_thread() -> mpsc::Receiver<Event> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        loop {
            // Block until an event is available (or 50ms timeout so we
            // can detect channel closure without burning CPU)
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                match event::read() {
                    Ok(evt) => {
                        // Filter Release events early — main loop never uses them
                        if let Event::Key(ref key) = evt {
                            if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                                continue;
                            }
                        }
                        if tx.send(evt).is_err() {
                            break; // Receiver dropped, shut down
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    });
    rx
}
