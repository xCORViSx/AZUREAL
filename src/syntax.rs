use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

/// Syntax highlighter for diff views
pub struct DiffHighlighter {
    syntax_set: SyntaxSet,
    theme: Theme,
}

impl DiffHighlighter {
    pub fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        let theme = theme_set.themes["base16-ocean.dark"].clone();

        Self { syntax_set, theme }
    }

    /// Parse a diff and return syntax-highlighted spans
    pub fn colorize_diff<'a>(&self, diff_text: &'a str) -> Vec<Vec<Span<'a>>> {
        let mut result = Vec::new();
        let mut current_file: Option<String> = None;
        let mut current_syntax: Option<&SyntaxReference> = None;

        for line in diff_text.lines() {
            // Detect file headers
            if line.starts_with("