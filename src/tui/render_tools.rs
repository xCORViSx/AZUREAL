//! Tool rendering utilities for TUI
//!
//! Handles extraction of tool parameters and rendering tool results.
//! Delegates to submodules for specific functionality:
//! - `tool_params`: display name mapping, parameter extraction, line truncation
//! - `tool_result`: tool result and write preview rendering
//! - `diff_parse`: diff/patch parsing into structured line types
//! - `diff_render`: diff rendering with syntax highlighting

/// Parses edit payloads into preview strings and structured diff lines.
mod diff_parse;
/// Renders parsed edit diffs with terminal styles and syntax highlighting.
mod diff_render;
/// Extracts compact tool labels and terminal-width-safe parameter previews.
mod tool_params;
/// Renders tool-result summaries and write-preview snippets.
mod tool_result;

pub use diff_parse::extract_edit_preview_strings;
pub use diff_render::render_edit_diff;
pub use tool_params::{extract_tool_param, tool_display_name};

#[allow(unused_imports)] // used by tests via `use super::*`
pub use tool_params::truncate_line;
pub use tool_result::{render_tool_result, render_write_preview};
