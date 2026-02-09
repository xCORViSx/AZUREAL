use ratatui::style::{Color, Modifier, Style};
use crate::tui::util::AZURE;
use ratatui::text::Span;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

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

    /// Parse a diff and return syntax-highlighted spans (owned for caching)
    pub fn colorize_diff(&self, diff_text: &str) -> Vec<Vec<Span<'static>>> {
        let mut result = Vec::new();
        let mut current_file: Option<String> = None;
        let mut current_syntax: Option<&SyntaxReference> = None;

        for line in diff_text.lines() {
            // Detect file headers
            if line.starts_with("diff --git") {
                result.push(vec![Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )]);
                continue;
            }

            // Detect +++ filename (new file)
            if line.starts_with("+++") {
                if let Some(filename) = line.strip_prefix("+++ b/") {
                    current_file = Some(filename.to_string());
                    current_syntax = self.syntax_set.find_syntax_for_file(filename).ok().flatten();
                }
                result.push(vec![Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Yellow),
                )]);
                continue;
            }

            // Detect --- filename (old file)
            if line.starts_with("---") {
                result.push(vec![Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Yellow),
                )]);
                continue;
            }

            // Hunk headers
            if line.starts_with("@@") {
                result.push(vec![Span::styled(
                    line.to_string(),
                    Style::default().fg(AZURE).add_modifier(Modifier::BOLD),
                )]);
                continue;
            }

            // Index lines
            if line.starts_with("index ") {
                result.push(vec![Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::DarkGray),
                )]);
                continue;
            }

            // Added lines
            if line.starts_with('+') && !line.starts_with("+++") {
                let content = &line[1..];
                let mut spans = vec![Span::styled("+".to_string(), Style::default().fg(Color::Green))];

                if let Some(syntax) = current_syntax {
                    spans.extend(self.highlight_line(content, syntax, Color::Green));
                } else {
                    spans.push(Span::styled(content.to_string(), Style::default().fg(Color::Green)));
                }

                result.push(spans);
                continue;
            }

            // Removed lines - plain gray, no syntax highlighting
            if line.starts_with('-') && !line.starts_with("---") {
                let content = &line[1..];
                let gray = Color::Rgb(100, 100, 100);
                result.push(vec![
                    Span::styled("-".to_string(), Style::default().fg(gray)),
                    Span::styled(content.to_string(), Style::default().fg(gray)),
                ]);
                continue;
            }

            // Context lines
            if let Some(content) = line.strip_prefix(' ') {
                let mut spans = vec![Span::raw(" ")];

                if let Some(syntax) = current_syntax {
                    spans.extend(self.highlight_line(content, syntax, Color::Reset));
                } else {
                    spans.push(Span::raw(content.to_string()));
                }

                result.push(spans);
                continue;
            }

            // Other lines (no prefix)
            result.push(vec![Span::styled(
                line.to_string(),
                Style::default().fg(Color::DarkGray),
            )]);
        }

        result
    }

    /// Highlight a single line of code with syntax highlighting (owned for caching)
    fn highlight_line(
        &self,
        line: &str,
        syntax: &SyntaxReference,
        base_color: Color,
    ) -> Vec<Span<'static>> {
        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        let mut spans = Vec::new();

        // Highlight the line
        if let Ok(ranges) = highlighter.highlight_line(line, &self.syntax_set) {
            for (style, text) in ranges {
                // Convert syntect color to ratatui color
                let fg_color = if base_color != Color::Reset {
                    // For added/removed lines, tint the syntax colors
                    blend_color(syntect_to_ratatui_color(style.foreground), base_color)
                } else {
                    syntect_to_ratatui_color(style.foreground)
                };

                let mut ratatui_style = Style::default().fg(fg_color);

                // Apply text styles
                if style.font_style.contains(syntect::highlighting::FontStyle::BOLD) {
                    ratatui_style = ratatui_style.add_modifier(Modifier::BOLD);
                }
                if style.font_style.contains(syntect::highlighting::FontStyle::ITALIC) {
                    ratatui_style = ratatui_style.add_modifier(Modifier::ITALIC);
                }
                if style
                    .font_style
                    .contains(syntect::highlighting::FontStyle::UNDERLINE)
                {
                    ratatui_style = ratatui_style.add_modifier(Modifier::UNDERLINED);
                }

                spans.push(Span::styled(text.to_string(), ratatui_style));
            }
        } else {
            // Fallback if highlighting fails
            spans.push(Span::styled(line.to_string(), Style::default().fg(base_color)));
        }

        spans
    }
}

/// Convert syntect color to ratatui color
fn syntect_to_ratatui_color(color: syntect::highlighting::Color) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

/// Blend a syntax color with a base diff color (for added/removed lines)
fn blend_color(syntax_color: Color, base_color: Color) -> Color {
    match (syntax_color, base_color) {
        (Color::Rgb(r, g, b), Color::Green) => {
            // Tint towards green
            Color::Rgb(
                (r as f32 * 0.7 + 0 as f32 * 0.3) as u8,
                (g as f32 * 0.7 + 255_f32 * 0.3) as u8,
                (b as f32 * 0.7 + 0 as f32 * 0.3) as u8,
            )
        }
        (Color::Rgb(r, g, b), Color::Red) => {
            // Tint towards red
            Color::Rgb(
                (r as f32 * 0.7 + 255_f32 * 0.3) as u8,
                (g as f32 * 0.7 + 0 as f32 * 0.3) as u8,
                (b as f32 * 0.7 + 0 as f32 * 0.3) as u8,
            )
        }
        _ => syntax_color,
    }
}

impl Default for DiffHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

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
