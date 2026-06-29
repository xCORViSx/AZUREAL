//! Debug output generation with content obfuscation for bug reports

use super::super::App;

/// Build the on-disk debug dump filename from optional user input.
///
/// The suffix is typed in-app, so treat it as untrusted: path separators and
/// shell-special characters are normalized to underscores so the dump always
/// stays inside the project `.azureal` directory.
fn debug_dump_filename(name_suffix: &str) -> String {
    let suffix = name_suffix.trim();
    if suffix.is_empty() {
        return "debug-output".to_string();
    }

    let safe_suffix: String = suffix
        .chars()
        .take(120)
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let safe_suffix = safe_suffix.trim_matches('_');
    if safe_suffix.is_empty() {
        "debug-output".to_string()
    } else {
        format!("debug-output_{}", safe_suffix)
    }
}

/// Debug dump generation methods for application state.
impl App {
    /// Dump debug output to `.azureal/debug-output[_name]` (triggered by ⌃d).
    /// All user/assistant content is obfuscated so the file can be shared in bug reports
    /// without exposing sensitive project details. Tool names, event types, and structural
    /// markers are preserved for diagnostic value. Optional name suffix appended after underscore.
    pub fn dump_debug_output(&mut self, name: &str) {
        let filename = debug_dump_filename(name);
        if let Err(e) = self.dump_debug_output_inner(&filename) {
            self.set_status(format!("Debug dump failed: {}", e));
        } else {
            self.set_status(format!("Debug output saved to .azureal/{}", filename));
        }
    }

    /// Write the debug dump to an already-sanitized filename.
    fn dump_debug_output_inner(&mut self, filename: &str) -> anyhow::Result<()> {
        use crate::events::DisplayEvent;
        use std::collections::HashMap;
        use std::io::Write;

        /// Deterministic content obfuscator for shareable debug dumps.
        ///
        /// It keeps punctuation and structural markers while mapping repeated
        /// words to stable fake words, preserving enough shape for diagnostics.
        // Deterministic word obfuscator: maps each unique word to a consistent fake word
        // so structural patterns are preserved (same word → same replacement every time).
        // Keeps punctuation, whitespace, numbers, file extensions, and structural tokens.
        struct Obfuscator {
            /// Stable fake-word mapping keyed by lowercase source token.
            map: HashMap<String, String>,
            /// Monotonic counter used to generate deterministic replacements.
            counter: usize,
        }
        /// Content obfuscation operations used while writing the debug dump.
        impl Obfuscator {
            /// Create an empty obfuscator with no prior token mappings.
            fn new() -> Self {
                Self {
                    map: HashMap::new(),
                    counter: 0,
                }
            }

            /// Generate a deterministic fake word with roughly the requested length.
            // Generate a fake word from a counter (aaa, aab, aac, ... aba, abb, ...)
            fn fake_word(&mut self, len: usize) -> String {
                let id = self.counter;
                self.counter += 1;
                // 3-letter base from counter, then pad/truncate to roughly match original length
                let base: String = (0..3)
                    .rev()
                    .map(|i| (b'a' + ((id / 26_usize.pow(i as u32)) % 26) as u8) as char)
                    .collect();
                if len <= 3 {
                    base[..len.min(3)].to_string()
                } else {
                    format!("{}{}", base, "x".repeat(len.saturating_sub(3)))
                }
            }

            /// Obfuscate one word while preserving simple case patterns.
            // Obfuscate a word, preserving case pattern. Skips structural tokens.
            fn word(&mut self, w: &str) -> String {
                if w.is_empty() {
                    return String::new();
                }
                // Preserve: numbers, punctuation-only tokens, very short (1-2 char) structural tokens,
                // file extensions (.rs, .md, .toml, .json, .txt, .jsonl),
                // and common programming keywords that don't leak project info
                if w.chars()
                    .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
                {
                    return w.to_string();
                }
                if w.len() <= 2 {
                    return w.to_string();
                }
                let key = w.to_lowercase();
                if let Some(existing) = self.map.get(&key) {
                    return existing.clone();
                }
                let fake = self.fake_word(w.len());
                // Match case pattern of original: ALL_CAPS, Capitalized, lowercase
                let result = if w.chars().all(|c| c.is_uppercase() || !c.is_alphabetic()) {
                    fake.to_uppercase()
                } else if w.starts_with(|c: char| c.is_uppercase()) {
                    let mut chars = fake.chars();
                    match chars.next() {
                        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                        None => fake,
                    }
                } else {
                    fake.clone()
                };
                self.map.insert(key, result.clone());
                result
            }

            /// Obfuscate prose while preserving punctuation and whitespace shape.
            // Obfuscate a full text string, preserving whitespace and punctuation structure
            fn text(&mut self, s: &str) -> String {
                let mut result = String::with_capacity(s.len());
                let mut word = String::new();
                for ch in s.chars() {
                    if ch.is_alphanumeric() || ch == '_' {
                        word.push(ch);
                    } else {
                        if !word.is_empty() {
                            result.push_str(&self.word(&word));
                            word.clear();
                        }
                        result.push(ch);
                    }
                }
                if !word.is_empty() {
                    result.push_str(&self.word(&word));
                }
                result
            }

            /// Obfuscate a path while keeping separators and extensions recognizable.
            // Obfuscate a file path, keeping / separators and file extensions
            fn path(&mut self, p: &str) -> String {
                p.split('/')
                    .map(|seg| {
                        if seg.is_empty() {
                            return String::new();
                        }
                        // Split filename from extension
                        if let Some(dot_pos) = seg.rfind('.') {
                            let (name, ext) = seg.split_at(dot_pos);
                            format!("{}{}", self.word(name), ext) // keep extension as-is
                        } else {
                            self.word(seg)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("/")
            }
        }

        let mut ob = Obfuscator::new();

        let debug_dir = crate::config::ensure_project_data_dir()?
            .ok_or_else(|| anyhow::anyhow!("Not in a git repository"))?;
        let debug_path = debug_dir.join(filename);
        let mut file = std::fs::File::create(&debug_path)?;

        // Diagnostic header — safe metadata (no content leaked)
        writeln!(file, "=== AZUREAL DEBUG DUMP ===")?;
        writeln!(file, "Dump time: {:?}", std::time::SystemTime::now())?;
        writeln!(
            file,
            "Session file: {:?}",
            self.session_file_path
                .as_ref()
                .map(|p| ob.path(&p.display().to_string()))
        )?;

        // Session file health check — only structural info, no content
        if let Some(ref path) = self.session_file_path {
            if let Ok(content) = std::fs::read_to_string(path) {
                let file_size = content.len();
                let ends_with_newline = content.ends_with('\n');
                writeln!(
                    file,
                    "File size: {} bytes, ends with newline: {}",
                    file_size, ends_with_newline
                )?;
                writeln!(file, "Last 50 chars: [redacted]")?;
                if let Some(last_line) = content.lines().last() {
                    let is_valid_json =
                        serde_json::from_str::<serde_json::Value>(last_line).is_ok();
                    writeln!(file, "Last line valid JSON: {}", is_valid_json)?;
                    if !is_valid_json {
                        writeln!(
                            file,
                            "Last line length: {} chars (invalid JSON)",
                            last_line.len()
                        )?;
                    }
                }
            }
        }
        writeln!(file)?;
        writeln!(
            file,
            "JSONL lines: {} (parse errors: {})",
            self.parse_total_lines, self.parse_errors
        )?;
        writeln!(file)?;
        writeln!(file, "=== ASSISTANT PARSING STATS ===")?;
        writeln!(
            file,
            "  Total 'assistant' events in JSONL: {}",
            self.assistant_total
        )?;
        writeln!(
            file,
            "  - No 'message' field: {}",
            self.assistant_no_message
        )?;
        writeln!(
            file,
            "  - No 'content' array: {}",
            self.assistant_no_content_arr
        )?;
        writeln!(
            file,
            "  - Text blocks created: {}",
            self.assistant_text_blocks
        )?;
        writeln!(file)?;
        writeln!(file, "Total display_events: {}", self.display_events.len())?;

        // Event type counts — no content leaked
        let mut user_msgs = 0;
        let mut assistant_texts = 0;
        let mut tool_calls = 0;
        let mut tool_results = 0;
        let mut hooks = 0;
        let mut other = 0;
        for event in &self.display_events {
            match event {
                DisplayEvent::UserMessage { .. } => user_msgs += 1,
                DisplayEvent::AssistantText { .. } => assistant_texts += 1,
                DisplayEvent::ToolCall { .. } => tool_calls += 1,
                DisplayEvent::ToolResult { .. } => tool_results += 1,
                DisplayEvent::Hook { .. } => hooks += 1,
                _ => other += 1,
            }
        }
        writeln!(file, "Event breakdown:")?;
        writeln!(file, "  UserMessage: {}", user_msgs)?;
        writeln!(file, "  AssistantText: {}", assistant_texts)?;
        writeln!(file, "  ToolCall: {}", tool_calls)?;
        writeln!(file, "  ToolResult: {}", tool_results)?;
        writeln!(file, "  Hook: {}", hooks)?;
        writeln!(file, "  Other: {}", other)?;
        writeln!(file)?;

        // Last 5 events — content obfuscated, tool names preserved for diagnostics
        writeln!(file, "=== LAST 5 EVENTS ===")?;
        let start = self.display_events.len().saturating_sub(5);
        for (i, event) in self.display_events.iter().skip(start).enumerate() {
            let preview = match event {
                DisplayEvent::UserMessage { content, .. } => {
                    let ob_text = ob.text(&content.chars().take(80).collect::<String>());
                    format!("UserMessage: {}...", ob_text)
                }
                DisplayEvent::AssistantText { text, .. } => {
                    let ob_text = ob.text(&text.chars().take(80).collect::<String>());
                    format!("AssistantText: {}...", ob_text)
                }
                DisplayEvent::ToolCall {
                    tool_name,
                    file_path,
                    ..
                } => {
                    let ob_path = file_path.as_ref().map(|p| ob.path(p)).unwrap_or_default();
                    format!("ToolCall: {} {}", tool_name, ob_path)
                }
                DisplayEvent::ToolResult {
                    tool_name,
                    file_path,
                    content,
                    ..
                } => {
                    let ob_path = file_path.as_ref().map(|p| ob.path(p)).unwrap_or_default();
                    format!("ToolResult: {} {} ({}B)", tool_name, ob_path, content.len())
                }
                DisplayEvent::Hook { name, output } => {
                    format!("Hook: {} ({}B)", name, output.len())
                }
                DisplayEvent::Complete {
                    duration_ms,
                    cost_usd,
                    ..
                } => {
                    format!("Complete: {}ms, ${:.4}", duration_ms, cost_usd)
                }
                DisplayEvent::Init { model, .. } => format!("Init: model={}", model),
                DisplayEvent::Command { name } => format!("Command: {}", name),
                DisplayEvent::Compacting => "Compacting".to_string(),
                DisplayEvent::Compacted => "Compacted".to_string(),
                DisplayEvent::MayBeCompacting => "MayBeCompacting".to_string(),
                DisplayEvent::Plan { name, .. } => format!("Plan: {}", ob.text(name)),
                DisplayEvent::ModelSwitch { model } => format!("ModelSwitch: {}", model),
                DisplayEvent::Filtered => "Filtered".to_string(),
            };
            writeln!(file, "  [{}] {}", start + i, preview)?;
        }
        writeln!(file)?;

        // Full rendered output — every line obfuscated
        writeln!(file, "=== RENDERED OUTPUT ===")?;
        let (rendered_lines, _, _, _, _) = crate::tui::util::render_display_events(
            &self.display_events,
            120,
            &self.pending_tool_calls,
            &self.failed_tool_calls,
            &mut self.syntax_highlighter,
            None,
            self.viewing_historic_session,
        );
        writeln!(file, "Total rendered lines: {}", rendered_lines.len())?;
        writeln!(file)?;

        for line in rendered_lines.iter() {
            let text: String = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect();
            writeln!(file, "{}", ob.text(&text))?;
        }

        Ok(())
    }
}

#[cfg(test)]
/// Tests for debug dump filename safety and normalization.
mod tests {
    use super::debug_dump_filename;

    /// Empty debug dump names use the default filename.
    #[test]
    fn debug_dump_filename_default_for_empty_suffix() {
        assert_eq!(debug_dump_filename("   "), "debug-output");
    }

    /// Debug dump names normalize path traversal into a plain filename.
    #[test]
    fn debug_dump_filename_strips_path_separators() {
        let filename = debug_dump_filename("../nested\\secret name");

        assert_eq!(filename, "debug-output_.._nested_secret_name");
        assert!(!filename.contains('/'));
        assert!(!filename.contains('\\'));
    }

    /// Debug dump names cap untrusted suffixes to avoid filesystem limits.
    #[test]
    fn debug_dump_filename_caps_long_suffixes() {
        let filename = debug_dump_filename(&"a".repeat(200));

        assert_eq!(filename.len(), "debug-output_".len() + 120);
    }
}
