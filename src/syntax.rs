use ratatui::style::{Color, Style};
use crate::tui::util::AZURE;
use ratatui::text::Span;
use syntect::parsing::SyntaxSet;

/// General-purpose syntax highlighter for file viewing
pub struct SyntaxHighlighter {
    syntax_set: SyntaxSet,
}

impl SyntaxHighlighter {
    pub fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        Self { syntax_set }
    }

    /// Highlight lines with optional background color (for edit diffs)
    /// Returns highlighted spans with the specified background applied
    pub fn highlight_with_bg(&self, content: &str, filename: &str, bg: Option<Color>) -> Vec<Vec<Span<'static>>> {
        let base = self.highlight_file(content, filename);
        if let Some(bg_color) = bg {
            base.into_iter().map(|line_spans| {
                line_spans.into_iter().map(|span| {
                    Span::styled(span.content, span.style.bg(bg_color))
                }).collect()
            }).collect()
        } else {
            base
        }
    }

    /// Highlight a file's content using bright hardcoded colors
    pub fn highlight_file(&self, content: &str, filename: &str) -> Vec<Vec<Span<'static>>> {
        use syntect::parsing::{ParseState, ScopeStack, ScopeStackOp};

        let syntax = self.syntax_set
            .find_syntax_for_file(filename)
            .ok()
            .flatten()
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let mut parse_state = ParseState::new(syntax);
        let mut result = Vec::new();

        for line in content.lines() {
            let mut spans = Vec::new();
            let ops = parse_state.parse_line(line, &self.syntax_set);
            let mut scope_stack = ScopeStack::new();
            let mut pos = 0;

            // ops is Vec<(usize, ScopeStackOp)>
            let ops: Vec<(usize, ScopeStackOp)> = ops.into_iter().flatten().collect();

            for (offset, op) in ops {
                // Add text before this operation
                if offset > pos {
                    let text = &line[pos..offset];
                    let color = scope_to_color(&scope_stack);
                    spans.push(Span::styled(text.to_string(), Style::default().fg(color)));
                }
                pos = offset;
                scope_stack.apply(&op).ok();
            }

            // Add remaining text
            if pos < line.len() {
                let text = &line[pos..];
                let color = scope_to_color(&scope_stack);
                spans.push(Span::styled(text.to_string(), Style::default().fg(color)));
            }

            // Empty line
            if spans.is_empty() {
                spans.push(Span::raw(""));
            }

            result.push(spans);
        }

        result
    }
}

/// Map syntax scope to bright terminal color
fn scope_to_color(stack: &syntect::parsing::ScopeStack) -> Color {
    let scopes: Vec<_> = stack.as_slice().iter()
        .map(|s| s.build_string())
        .collect();
    let scope_str = scopes.join(" ");

    // Check scopes from most specific to least specific
    if scope_str.contains("comment") {
        Color::Rgb(140, 140, 140) // Comments: lighter gray than removed lines (100,100,100)
    } else if scope_str.contains("string") || scope_str.contains("char") {
        Color::Green
    } else if scope_str.contains("constant.numeric") || scope_str.contains("number") {
        Color::Yellow
    } else if scope_str.contains("constant") {
        AZURE
    } else if scope_str.contains("keyword") || scope_str.contains("storage") {
        Color::Magenta
    } else if scope_str.contains("entity.name.function") || scope_str.contains("support.function") {
        Color::Rgb(100, 160, 255) // Light blue for functions — ANSI Blue is too dark on dark backgrounds
    } else if scope_str.contains("entity.name.type") || scope_str.contains("support.type") || scope_str.contains("entity.name.class") {
        Color::Yellow
    } else if scope_str.contains("variable.parameter") {
        Color::Rgb(255, 180, 100) // Orange for parameters
    } else if scope_str.contains("variable") {
        Color::White
    } else if scope_str.contains("punctuation") || scope_str.contains("operator") {
        Color::White
    } else if scope_str.contains("entity.name") {
        AZURE
    } else if scope_str.contains("meta.attribute") || scope_str.contains("attribute") {
        Color::Rgb(180, 180, 255) // Light purple for attributes
    } else if scope_str.contains("markup.heading") {
        AZURE
    } else if scope_str.contains("markup.bold") {
        Color::White
    } else if scope_str.contains("markup.italic") {
        Color::White
    } else if scope_str.contains("markup.list") {
        Color::Green
    } else {
        Color::White // Default to bright white
    }
}

impl Default for SyntaxHighlighter {
    fn default() -> Self {
        Self::new()
    }
}
