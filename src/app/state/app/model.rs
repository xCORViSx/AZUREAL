//! Model selection and token usage badge

use super::App;

impl App {
    /// Recompute the cached token usage badge from current session_tokens + model_context_window.
    /// Call this whenever session_tokens or model_context_window changes — draw path just reads the cache.
    pub fn update_token_badge(&mut self) {
        let mut pct_value = 0.0_f64;
        self.token_badge_cache = self.session_tokens.map(|(ctx_tokens, _)| {
            let base_window = self.model_context_window.unwrap_or(200_000);
            let window = if ctx_tokens > base_window { 1_000_000 } else { base_window };
            // Claude reserves ~33k tokens as auto-compact buffer (compacts at ~83.5% raw).
            // Subtract the buffer so percentage reflects usable context, not total window.
            let usable = window.saturating_sub(33_000);
            let pct = (ctx_tokens as f64 / usable as f64 * 100.0).min(100.0);
            pct_value = pct;
            let color = if pct < 60.0 { ratatui::style::Color::Green }
                else if pct < 90.0 { ratatui::style::Color::Yellow }
                else { ratatui::style::Color::Red };
            (format!(" {:.0}% ", pct), color)
        });
        // Track 95% threshold for compaction inactivity watcher
        let was_high = self.context_pct_high;
        self.context_pct_high = pct_value >= 95.0;
        // Reset banner state when context drops below threshold (e.g. after compaction)
        if was_high && !self.context_pct_high {
            self.compaction_banner_injected = false;
        }
    }

    /// Short display name for the active model. Always returns the selected_model
    /// alias since it's always set (never None).
    pub fn display_model_name(&self) -> &str {
        self.selected_model.as_deref().unwrap_or("opus")
    }

    /// Cycle selected_model through: opus → sonnet → haiku → opus.
    /// Always set — the displayed model is exactly what gets passed as --model to spawn().
    pub fn cycle_model(&mut self) {
        self.selected_model = Some(match self.selected_model.as_deref() {
            Some("opus") => "sonnet",
            Some("sonnet") => "haiku",
            _ => "opus",
        }.to_string());
    }
}
