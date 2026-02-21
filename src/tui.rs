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
mod file_icons;
mod draw_file_tree;
mod draw_git_actions;
mod draw_health;
mod draw_input;
mod draw_output;
mod draw_projects;
mod draw_sidebar;
mod draw_status;
mod draw_terminal;
mod draw_viewer;
mod draw_wizard;
mod event_loop;
pub mod render_thread;
mod input_dialogs;
mod input_file_tree;
mod input_git_actions;
mod input_health;
mod input_output;
mod input_projects;
mod input_worktrees;
mod input_terminal;
mod input_viewer;
mod input_wizard;
pub mod keybindings;
mod run;

pub mod colorize;
pub mod markdown;
pub mod render_events;
pub mod render_markdown;
pub mod render_tools;
pub mod render_wrap;
pub mod util;

pub use run::run;
