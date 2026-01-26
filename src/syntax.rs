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
            if line.starts_with("diff --git") {
                result.push(vec![Span::styled(
                    line,
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
                    line,
                    Style::default().fg(Color::Yellow),
                )]);
                continue;
            }

            // Detect --- filename (old file)
            if line.starts_with("---") {
                result.push(vec![Span::styled(
                    line,
                    Style::default().fg(Color::Yellow),
                )]);
                continue;
            }

            // Hunk headers
            if line.starts_with("@@") {
                result.push(vec![Span::styled(
                    line,
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                )]);
                continue;
            }

            // Index lines
            if line.starts_with("index ") {
                result.push(vec![Span::styled(
                    line,
                    Style::default().fg(Color::DarkGray),
                )]);
                continue;
            }

            // Added lines
            if line.starts_with('+') && !line.starts_with("+++") {
                let content = &line[1..];
                let mut spans = vec![Span::styled("+", Style::default().fg(Color::Green))];

                if let Some(syntax) = current_syntax {
                    spans.extend(self.highlight_line(content, syntax, Color::Green));
                } else {
                    spans.push(Span::styled(content, Style::default().fg(Color::Green)));
                }

                result.push(spans);
                continue;
            }

            // Removed lines
            if line.starts_with('-') && !line.starts_with("---") {
                let content = &line[1..];
                let mut spans = vec![Span::styled("-", Style::default().fg(Color::Red))];

                if let Some(syntax) = current_syntax {
                    spans.extend(self.highlight_line(content, syntax, Color::Red));
                } else {
                    spans.push(Span::styled(content, Style::default().fg(Color::Red)));
                }

                result.push(spans);
                continue;
            }

            // Context lines
            if line.starts_with(' ') {
                let content = &line[1..];
                let mut spans = vec![Span::raw(" ")];

                if let Some(syntax) = current_syntax {
                    spans.extend(self.highlight_line(content, syntax, Color::Reset));
                } else {
                    spans.push(Span::raw(content));
                }

                result.push(spans);
                continue;
            }

            // Other lines (no prefix)
            result.push(vec![Span::styled(
                line,
                Style::default().fg(Color::DarkGray),
            )]);
        }

        result
    }

    /// Highlight a single line of code with syntax highlighting
    fn highlight_line<'a>(
        &self,
        line: &'a str,
        syntax: &SyntaxReference,
        base_color: Color,
    ) -> Vec<Span<'a>> {
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
                (g as f32 * 0.7 + 255 as f32 * 0.3) as u8,
                (b as f32 * 0.7 + 0 as f32 * 0.3) as u8,
            )
        }
        (Color::Rgb(r, g, b), Color::Red) => {
            // Tint towards red
            Color::Rgb(
                (r as f32 * 0.7 + 255 as f32 * 0.3) as u8,
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
