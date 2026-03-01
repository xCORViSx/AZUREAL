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

#[cfg(test)]
mod tests {
    use super::*;

    fn hl() -> SyntaxHighlighter { SyntaxHighlighter::new() }

    // ═══════════════════════════════════════════════════════════════════
    // SyntaxHighlighter::new / Default
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn new_creates_highlighter() {
        let _h = SyntaxHighlighter::new();
    }

    #[test]
    fn default_creates_highlighter() {
        let _h = SyntaxHighlighter::default();
    }

    // ═══════════════════════════════════════════════════════════════════
    // highlight_file — line count
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn highlight_empty_content() {
        let result = hl().highlight_file("", "test.rs");
        assert!(result.is_empty());
    }

    #[test]
    fn highlight_single_line() {
        let result = hl().highlight_file("hello", "test.txt");
        assert_eq!(result.len(), 1);
        assert!(!result[0].is_empty());
    }

    #[test]
    fn highlight_multiple_lines() {
        let result = hl().highlight_file("line1\nline2\nline3", "test.txt");
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn highlight_five_lines() {
        let content = "a\nb\nc\nd\ne";
        assert_eq!(hl().highlight_file(content, "test.txt").len(), 5);
    }

    #[test]
    fn highlight_100_lines() {
        let content = (0..100).map(|i| format!("let x{} = {};", i, i)).collect::<Vec<_>>().join("\n");
        assert_eq!(hl().highlight_file(&content, "test.rs").len(), 100);
    }

    // ═══════════════════════════════════════════════════════════════════
    // highlight_file — language detection
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn highlight_rust_code() {
        let result = hl().highlight_file("fn main() {\n    println!(\"hello\");\n}", "test.rs");
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn highlight_python_code() {
        let result = hl().highlight_file("def hello():\n    print('hi')", "test.py");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn highlight_javascript_code() {
        let result = hl().highlight_file("const x = 42;\nconsole.log(x);", "test.js");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn highlight_typescript() {
        let result = hl().highlight_file("const x: number = 42;", "test.ts");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn highlight_json_file() {
        let result = hl().highlight_file("{\"key\": \"value\"}", "test.json");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn highlight_toml_file() {
        let result = hl().highlight_file("[section]\nkey = \"value\"", "test.toml");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn highlight_yaml_file() {
        let result = hl().highlight_file("key: value\nlist:\n  - item", "test.yaml");
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn highlight_markdown_file() {
        let result = hl().highlight_file("# Header\nParagraph", "test.md");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn highlight_html_file() {
        let result = hl().highlight_file("<html><body>hello</body></html>", "test.html");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn highlight_css_file() {
        let result = hl().highlight_file("body { color: red; }", "test.css");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn highlight_shell_script() {
        let result = hl().highlight_file("#!/bin/bash\necho hello", "test.sh");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn highlight_c_code() {
        let result = hl().highlight_file("#include <stdio.h>\nint main() { return 0; }", "test.c");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn highlight_go_code() {
        let result = hl().highlight_file("package main\nfunc main() {}", "test.go");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn highlight_sql_file() {
        let result = hl().highlight_file("SELECT * FROM users;", "test.sql");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn highlight_xml() {
        let result = hl().highlight_file("<root><child/></root>", "test.xml");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn highlight_unknown_extension_fallback() {
        let result = hl().highlight_file("some content", "test.xyz123");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn highlight_no_extension() {
        let result = hl().highlight_file("text", "Makefile");
        assert_eq!(result.len(), 1);
    }

    // ═══════════════════════════════════════════════════════════════════
    // highlight_file — span content
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn highlight_empty_line_produces_span() {
        let result = hl().highlight_file("\n", "test.txt");
        for line_spans in &result {
            assert!(!line_spans.is_empty(), "each line must have at least one span");
        }
    }

    #[test]
    fn highlight_preserves_tabs() {
        let result = hl().highlight_file("\tindented", "test.txt");
        let text: String = result[0].iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains('\t'));
    }

    #[test]
    fn highlight_whitespace_only() {
        let result = hl().highlight_file("   ", "test.txt");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn highlight_special_chars() {
        let result = hl().highlight_file("<>&\"'", "test.txt");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn highlight_unicode_content() {
        let result = hl().highlight_file("let 日本語 = \"テスト\";", "test.rs");
        assert_eq!(result.len(), 1);
    }

    // ═══════════════════════════════════════════════════════════════════
    // scope_to_color — tested via highlight_file
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn rust_keyword_magenta() {
        let result = hl().highlight_file("fn main() {}", "test.rs");
        let has_magenta = result[0].iter().any(|s| s.style.fg == Some(Color::Magenta));
        assert!(has_magenta, "Rust keywords should be Magenta");
    }

    #[test]
    fn rust_string_green() {
        let result = hl().highlight_file("let s = \"hello\";", "test.rs");
        let has_green = result[0].iter().any(|s| s.style.fg == Some(Color::Green));
        assert!(has_green, "String literals should be Green");
    }

    #[test]
    fn rust_comment_gray() {
        let result = hl().highlight_file("// comment", "test.rs");
        let has_gray = result[0].iter().any(|s| matches!(s.style.fg, Some(Color::Rgb(140, 140, 140))));
        assert!(has_gray, "Comments should be gray");
    }

    #[test]
    fn rust_number_yellow() {
        let result = hl().highlight_file("let x = 42;", "test.rs");
        let has_yellow = result[0].iter().any(|s| s.style.fg == Some(Color::Yellow));
        assert!(has_yellow, "Numbers should be Yellow");
    }

    #[test]
    fn plain_text_white() {
        let result = hl().highlight_file("plaintext", "test.txt");
        let has_white = result[0].iter().any(|s| s.style.fg == Some(Color::White));
        assert!(has_white, "Plain text default should be White");
    }

    #[test]
    fn rust_attribute_colored() {
        let result = hl().highlight_file("#[derive(Debug)]", "test.rs");
        assert!(!result[0].is_empty());
    }

    // ═══════════════════════════════════════════════════════════════════
    // highlight_with_bg
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn with_bg_none_no_background() {
        let result = hl().highlight_with_bg("hello", "test.txt", None);
        assert_eq!(result.len(), 1);
        for span in &result[0] {
            assert_eq!(span.style.bg, None);
        }
    }

    #[test]
    fn with_bg_red_sets_background() {
        let result = hl().highlight_with_bg("hello", "test.txt", Some(Color::Red));
        assert_eq!(result.len(), 1);
        for span in &result[0] {
            assert_eq!(span.style.bg, Some(Color::Red));
        }
    }

    #[test]
    fn with_bg_preserves_foreground() {
        let result = hl().highlight_with_bg("fn main() {}", "test.rs", Some(Color::Blue));
        for span in &result[0] {
            assert_eq!(span.style.bg, Some(Color::Blue));
            assert!(span.style.fg.is_some());
        }
    }

    #[test]
    fn with_bg_multiple_lines() {
        let result = hl().highlight_with_bg("line1\nline2", "test.txt", Some(Color::Green));
        assert_eq!(result.len(), 2);
        for line_spans in &result {
            for span in line_spans {
                assert_eq!(span.style.bg, Some(Color::Green));
            }
        }
    }

    #[test]
    fn with_bg_empty_content() {
        let result = hl().highlight_with_bg("", "test.txt", Some(Color::Red));
        assert!(result.is_empty());
    }

    #[test]
    fn with_bg_rgb_color() {
        let bg = Color::Rgb(30, 30, 30);
        let result = hl().highlight_with_bg("text", "test.txt", Some(bg));
        for span in &result[0] {
            assert_eq!(span.style.bg, Some(bg));
        }
    }

    #[test]
    fn highlight_cpp_code() {
        let result = hl().highlight_file("#include <iostream>\nint main() {}", "test.cpp");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn highlight_ruby_code() {
        let result = hl().highlight_file("puts 'hello'", "test.rb");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn highlight_scala_code() {
        let result = hl().highlight_file("object Main extends App {}", "test.scala");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn highlight_lua_code() {
        let result = hl().highlight_file("print('hello')", "test.lua");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn highlight_r_code() {
        let result = hl().highlight_file("x <- c(1, 2, 3)", "test.r");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn highlight_tex_code() {
        let result = hl().highlight_file("\\documentclass{article}", "test.tex");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn highlight_php_code() {
        let result = hl().highlight_file("<?php echo 'hi'; ?>", "test.php");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn highlight_perl_code() {
        let result = hl().highlight_file("print \"hello\\n\";", "test.pl");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn highlight_haskell_code() {
        let result = hl().highlight_file("main = putStrLn \"hello\"", "test.hs");
        assert_eq!(result.len(), 1);
    }
}
