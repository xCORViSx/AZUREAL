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

    // Open key event log for diagnostics (first 200 key events)
    let log_path = dirs::home_dir()
        .map(|h| h.join(".azureal/key_events.log"));
    let log_file = log_path.and_then(|p| {
        use std::fs::OpenOptions;
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(p)
            .ok()
    });
    let log_file = std::sync::Arc::new(std::sync::Mutex::new(log_file));
    let log_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    thread::spawn(move || {
        loop {
            // Block until an event is available (or 50ms timeout so we
            // can detect channel closure without burning CPU)
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                match event::read() {
                    Ok(evt) => {
                        // Log key events for diagnostics (first 200 only)
                        if let Event::Key(ref key) = evt {
                            let count = log_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            if count < 200 {
                                if let Ok(mut guard) = log_file.lock() {
                                    if let Some(ref mut f) = *guard {
                                        use std::io::Write;
                                        let _ = writeln!(f, "#{} code={:?} mods={:?} kind={:?}", count + 1, key.code, key.modifiers, key.kind);
                                        let _ = f.flush();
                                    }
                                }
                            }
                        }

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
