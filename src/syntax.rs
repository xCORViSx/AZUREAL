use std::collections::HashMap;

use ratatui::style::{Color, Style};
use ratatui::text::Span;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use crate::tui::util::AZURE;

/// Capture names recognized by all language configs.
/// Index = Highlight.0 → capture_color() maps to terminal color.
const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",          // 0
    "comment",            // 1
    "constant",           // 2
    "constant.builtin",   // 3
    "constructor",        // 4
    "embedded",           // 5
    "escape",             // 6
    "function",           // 7
    "function.builtin",   // 8
    "function.method",    // 9
    "keyword",            // 10
    "label",              // 11
    "number",             // 12
    "operator",           // 13
    "property",           // 14
    "punctuation",        // 15
    "punctuation.bracket", // 16
    "punctuation.delimiter", // 17
    "string",             // 18
    "string.special",     // 19
    "tag",                // 20
    "type",               // 21
    "type.builtin",       // 22
    "variable",           // 23
    "variable.builtin",   // 24
    "variable.parameter", // 25
];

/// Map capture index to terminal color (preserves original scope_to_color scheme)
fn capture_color(index: usize) -> Color {
    match index {
        0 => Color::Rgb(180, 180, 255),   // attribute
        1 => Color::Rgb(140, 140, 140),   // comment
        2 => AZURE,                        // constant
        3 => Color::Yellow,                // constant.builtin (numbers, true/false)
        4 => AZURE,                        // constructor
        5 => Color::White,                 // embedded
        6 => Color::Magenta,               // escape
        7 => Color::Rgb(100, 160, 255),    // function
        8 => Color::Rgb(100, 160, 255),    // function.builtin
        9 => Color::Rgb(100, 160, 255),    // function.method
        10 => Color::Magenta,              // keyword
        11 => AZURE,                       // label
        12 => Color::Yellow,               // number
        13 => Color::White,                // operator
        14 => Color::White,                // property
        15 => Color::White,                // punctuation
        16 => Color::White,                // punctuation.bracket
        17 => Color::White,                // punctuation.delimiter
        18 => Color::Green,                // string
        19 => Color::Green,                // string.special
        20 => AZURE,                       // tag
        21 => Color::Yellow,               // type
        22 => Color::Yellow,               // type.builtin
        23 => Color::White,                // variable
        24 => Color::Magenta,              // variable.builtin (self, this)
        25 => Color::Rgb(255, 180, 100),   // variable.parameter
        _ => Color::White,
    }
}

/// General-purpose syntax highlighter backed by tree-sitter
pub struct SyntaxHighlighter {
    highlighter: Highlighter,
    /// Language name → pre-configured HighlightConfiguration
    configs: HashMap<&'static str, HighlightConfiguration>,
    /// File extension → language name
    ext_to_lang: HashMap<&'static str, &'static str>,
    /// Code fence token → language name
    token_to_lang: HashMap<&'static str, &'static str>,
}

impl SyntaxHighlighter {
    pub fn new() -> Self {
        let highlighter = Highlighter::new();
        let mut configs = HashMap::new();
        let mut ext_to_lang = HashMap::new();
        let mut token_to_lang = HashMap::new();

        // Helper: register a language with its grammar, queries, extensions, and tokens
        let mut reg = |name: &'static str,
                       language: tree_sitter::Language,
                       highlights: &str,
                       injections: &str,
                       locals: &str,
                       exts: &[&'static str],
                       tokens: &[&'static str]| {
            if let Ok(mut config) = HighlightConfiguration::new(language, name, highlights, injections, locals) {
                config.configure(HIGHLIGHT_NAMES);
                configs.insert(name, config);
                for ext in exts { ext_to_lang.insert(*ext, name); }
                for tok in tokens { token_to_lang.insert(*tok, name); }
            }
        };

        // -- Core languages --

        reg("rust", tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY, "",
            &["rs"], &["rust", "rs"]);

        reg("python", tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::HIGHLIGHTS_QUERY, "", "",
            &["py", "pyw", "pyi"], &["python", "py"]);

        reg("javascript", tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::INJECTIONS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
            &["js", "mjs", "cjs", "jsx"], &["javascript", "js", "jsx"]);

        reg("typescript", tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tree_sitter_typescript::HIGHLIGHTS_QUERY, "", "",
            &["ts", "mts", "cts"], &["typescript", "ts"]);

        reg("tsx", tree_sitter_typescript::LANGUAGE_TSX.into(),
            tree_sitter_typescript::HIGHLIGHTS_QUERY, "", "",
            &["tsx"], &["tsx"]);

        reg("json", tree_sitter_json::LANGUAGE.into(),
            tree_sitter_json::HIGHLIGHTS_QUERY, "", "",
            &["json", "jsonc"], &["json", "jsonc"]);

        reg("toml", tree_sitter_toml_ng::LANGUAGE.into(),
            tree_sitter_toml_ng::HIGHLIGHTS_QUERY, "", "",
            &["toml"], &["toml"]);

        reg("bash", tree_sitter_bash::LANGUAGE.into(),
            tree_sitter_bash::HIGHLIGHT_QUERY, "", "",
            &["sh", "bash", "zsh"], &["bash", "sh", "shell", "zsh"]);

        reg("c", tree_sitter_c::LANGUAGE.into(),
            tree_sitter_c::HIGHLIGHT_QUERY, "", "",
            &["c", "h"], &["c"]);

        reg("cpp", tree_sitter_cpp::LANGUAGE.into(),
            tree_sitter_cpp::HIGHLIGHT_QUERY, "", "",
            &["cpp", "cc", "cxx", "hpp", "hxx", "hh"], &["cpp", "c++", "cxx", "cc"]);

        reg("go", tree_sitter_go::LANGUAGE.into(),
            tree_sitter_go::HIGHLIGHTS_QUERY, "", "",
            &["go"], &["go", "golang"]);

        reg("html", tree_sitter_html::LANGUAGE.into(),
            tree_sitter_html::HIGHLIGHTS_QUERY,
            tree_sitter_html::INJECTIONS_QUERY, "",
            &["html", "htm"], &["html", "htm"]);

        reg("css", tree_sitter_css::LANGUAGE.into(),
            tree_sitter_css::HIGHLIGHTS_QUERY, "", "",
            &["css"], &["css"]);

        reg("java", tree_sitter_java::LANGUAGE.into(),
            tree_sitter_java::HIGHLIGHTS_QUERY, "", "",
            &["java"], &["java"]);

        reg("ruby", tree_sitter_ruby::LANGUAGE.into(),
            tree_sitter_ruby::HIGHLIGHTS_QUERY,
            tree_sitter_ruby::LOCALS_QUERY, "",
            &["rb"], &["ruby", "rb"]);

        reg("lua", tree_sitter_lua::LANGUAGE.into(),
            tree_sitter_lua::HIGHLIGHTS_QUERY,
            tree_sitter_lua::INJECTIONS_QUERY, "",
            &["lua"], &["lua"]);

        reg("yaml", tree_sitter_yaml::LANGUAGE.into(),
            tree_sitter_yaml::HIGHLIGHTS_QUERY, "", "",
            &["yaml", "yml"], &["yaml", "yml"]);

        reg("markdown", tree_sitter_md::LANGUAGE.into(),
            tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
            tree_sitter_md::INJECTION_QUERY_BLOCK, "",
            &["md", "markdown"], &["markdown", "md"]);

        reg("scala", tree_sitter_scala::LANGUAGE.into(),
            tree_sitter_scala::HIGHLIGHTS_QUERY, "",
            tree_sitter_scala::LOCALS_QUERY,
            &["scala", "sc"], &["scala"]);

        reg("r", tree_sitter_r::LANGUAGE.into(),
            tree_sitter_r::HIGHLIGHTS_QUERY, "", "",
            &["r", "R"], &["r", "R"]);

        reg("haskell", tree_sitter_haskell::LANGUAGE.into(),
            tree_sitter_haskell::HIGHLIGHTS_QUERY,
            tree_sitter_haskell::INJECTIONS_QUERY,
            tree_sitter_haskell::LOCALS_QUERY,
            &["hs"], &["haskell", "hs"]);

        reg("php", tree_sitter_php::LANGUAGE_PHP.into(),
            tree_sitter_php::HIGHLIGHTS_QUERY,
            tree_sitter_php::INJECTIONS_QUERY, "",
            &["php"], &["php"]);

        reg("sql", tree_sitter_sequel::LANGUAGE.into(),
            tree_sitter_sequel::HIGHLIGHTS_QUERY, "", "",
            &["sql"], &["sql"]);

        // Perl + LaTeX: grammars exist but no HIGHLIGHTS_QUERY in crate — empty queries = plain text
        reg("perl", tree_sitter_perl::LANGUAGE.into(),
            "", "", "",
            &["pl", "pm"], &["perl", "pl"]);

        // LaTeX: tree-sitter-latex crate has broken external scanner linking,
        // so .tex files fall back to plain text via ext_to_lang miss.

        Self { highlighter, configs, ext_to_lang, token_to_lang }
    }

    /// Highlight a file's content. Language detected from filename extension.
    pub fn highlight_file(&mut self, content: &str, filename: &str) -> Vec<Vec<Span<'static>>> {
        let lang = filename
            .rsplit('.')
            .next()
            .and_then(|ext| self.ext_to_lang.get(ext).copied());
        match lang {
            Some(name) => self.highlight_impl(content, name),
            None => plain_text_lines(content),
        }
    }

    /// Highlight a code block by language token (e.g. "rust", "python", "js").
    /// Falls back to plain text for unknown / empty tokens.
    pub fn highlight_code_block(&mut self, content: &str, lang: &str) -> Vec<Vec<Span<'static>>> {
        if lang.is_empty() {
            return plain_text_lines(content);
        }
        let name = self.token_to_lang.get(lang).copied();
        match name {
            Some(n) => self.highlight_impl(content, n),
            // Try as file extension fallback (e.g. "rs" → rust)
            None => {
                let ext_name = self.ext_to_lang.get(lang).copied();
                match ext_name {
                    Some(n) => self.highlight_impl(content, n),
                    None => plain_text_lines(content),
                }
            }
        }
    }

    /// Highlight with optional background color (for edit diffs)
    pub fn highlight_with_bg(&mut self, content: &str, filename: &str, bg: Option<Color>) -> Vec<Vec<Span<'static>>> {
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

    /// Core highlighting: run tree-sitter-highlight and convert events to ratatui spans
    fn highlight_impl(&mut self, content: &str, lang_name: &str) -> Vec<Vec<Span<'static>>> {
        if content.is_empty() {
            return Vec::new();
        }

        // Split borrows: configs (immutable) vs highlighter (mutable)
        let configs = &self.configs;
        let config = match configs.get(lang_name) {
            Some(c) => c,
            None => return plain_text_lines(content),
        };

        let source = content.as_bytes();
        let events = match self.highlighter.highlight(
            config, source, None,
            |injection_lang| configs.get(injection_lang),
        ) {
            Ok(e) => e,
            Err(_) => return plain_text_lines(content),
        };

        let mut result: Vec<Vec<Span<'static>>> = Vec::new();
        let mut current_spans: Vec<Span<'static>> = Vec::new();
        let mut color_stack: Vec<Color> = vec![Color::White];

        for event in events {
            match event {
                Ok(HighlightEvent::Source { start, end }) => {
                    let text = &content[start..end];
                    let color = *color_stack.last().unwrap_or(&Color::White);
                    let style = Style::default().fg(color);

                    // Split by newlines — each \n starts a new line in output
                    let mut first = true;
                    for chunk in text.split('\n') {
                        if !first {
                            // Finalize previous line
                            if current_spans.is_empty() {
                                current_spans.push(Span::raw(""));
                            }
                            result.push(std::mem::take(&mut current_spans));
                        }
                        first = false;
                        let chunk = chunk.strip_suffix('\r').unwrap_or(chunk);
                        if !chunk.is_empty() {
                            current_spans.push(Span::styled(chunk.to_string(), style));
                        }
                    }
                }
                Ok(HighlightEvent::HighlightStart(h)) => {
                    color_stack.push(capture_color(h.0));
                }
                Ok(HighlightEvent::HighlightEnd) => {
                    color_stack.pop();
                }
                Err(_) => break,
            }
        }

        // Push final line
        if !current_spans.is_empty() {
            result.push(current_spans);
        }

        result
    }
}

/// Plain text fallback: one white span per line
fn plain_text_lines(content: &str) -> Vec<Vec<Span<'static>>> {
    content.lines().map(|line| {
        vec![Span::styled(line.to_string(), Style::default().fg(Color::White))]
    }).collect()
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
    // Color verification — tested via highlight_file
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

    // ═══════════════════════════════════════════════════════════════════
    // Additional language detection
    // ═══════════════════════════════════════════════════════════════════

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

    #[test]
    fn highlight_java_code() {
        let result = hl().highlight_file("public class Main {}", "test.java");
        assert_eq!(result.len(), 1);
    }

    // ═══════════════════════════════════════════════════════════════════
    // highlight_code_block — language token matching
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn code_block_rust_by_token() {
        let result = hl().highlight_code_block("fn main() {}", "rust");
        assert_eq!(result.len(), 1);
        let has_magenta = result[0].iter().any(|s| s.style.fg == Some(Color::Magenta));
        assert!(has_magenta, "Rust keyword should be Magenta via token lookup");
    }

    #[test]
    fn code_block_python_by_token() {
        let result = hl().highlight_code_block("def hello():\n    pass", "python");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn code_block_js_by_token() {
        let result = hl().highlight_code_block("const x = 42;", "js");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn code_block_javascript_by_token() {
        let result = hl().highlight_code_block("const x = 42;", "javascript");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn code_block_empty_lang_plain_text() {
        let result = hl().highlight_code_block("some text", "");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn code_block_unknown_lang_fallback() {
        let result = hl().highlight_code_block("content", "unknownlang123");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn code_block_toml_by_token() {
        let result = hl().highlight_code_block("[section]\nkey = \"value\"", "toml");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn code_block_json_by_token() {
        let result = hl().highlight_code_block("{\"key\": 1}", "json");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn code_block_bash_by_token() {
        let result = hl().highlight_code_block("echo hello", "bash");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn code_block_empty_content() {
        let result = hl().highlight_code_block("", "rust");
        assert!(result.is_empty());
    }

    #[test]
    fn code_block_multiline() {
        let result = hl().highlight_code_block("fn a() {}\nfn b() {}\nfn c() {}", "rust");
        assert_eq!(result.len(), 3);
    }

    // ═══════════════════════════════════════════════════════════════════
    // Language coverage — every registered language parses without panic
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn all_languages_parse_without_panic() {
        let mut h = hl();
        let samples: &[(&str, &str)] = &[
            ("test.rs", "fn main() {}"),
            ("test.py", "def f(): pass"),
            ("test.js", "const x = 1;"),
            ("test.ts", "let x: number = 1;"),
            ("test.tsx", "const App = () => <div/>;"),
            ("test.json", "{\"a\": 1}"),
            ("test.toml", "key = \"val\""),
            ("test.sh", "echo hi"),
            ("test.c", "int main() { return 0; }"),
            ("test.cpp", "int main() {}"),
            ("test.go", "package main"),
            ("test.html", "<p>hi</p>"),
            ("test.css", "body {}"),
            ("test.java", "class A {}"),
            ("test.rb", "puts 'hi'"),
            ("test.lua", "print('hi')"),
            ("test.yaml", "key: val"),
            ("test.md", "# hi"),
            ("test.scala", "object A"),
            ("test.r", "x <- 1"),
            ("test.hs", "main = return ()"),
            ("test.php", "<?php echo 1; ?>"),
            ("test.sql", "SELECT 1;"),
            ("test.pl", "print 1;"),
            ("test.tex", "\\section{hi}"),
        ];
        for (file, code) in samples {
            let result = h.highlight_file(code, file);
            assert!(!result.is_empty(), "Language {} should produce output", file);
        }
    }
}
