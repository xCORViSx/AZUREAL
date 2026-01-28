//! Terminal User Interface module
//!
//! Split into focused submodules:
//! - `run`: TUI entry point and main layout
//! - `event_loop`: Core event loop and event handling
//! - `input_*`: Input handlers for different UI modes
//! - `draw_*`: Rendering functions for different UI components
//! - `colorize`: Output colorization
//! - `markdown`: Markdown parsing
//! - `render_events`: Display event rendering
//! - `render_tools`: Tool result rendering
//! - `util`: Small utility functions

mod draw_dialogs;
mod draw_input;
mod draw_output;
mod draw_sidebar;
mod draw_status;
mod draw_terminal;
mod draw_wizard;
mod event_loop;
mod input_dialogs;
mod input_output;
mod input_rebase;
mod input_sessions;
mod input_terminal;
mod input_wizard;
mod run;

pub mod colorize;
pub mod markdown;
pub mod render_events;
pub mod render_tools;
pub mod util;

pub use run::run;
