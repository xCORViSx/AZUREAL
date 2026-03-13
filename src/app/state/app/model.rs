//! Model selection and token usage badge

use crate::backend::Backend;
use super::App;

/// Claude model cycle: opus → sonnet → haiku → opus
const CLAUDE_MODELS: &[&str] = &["opus", "sonnet", "haiku"];
/// Codex model cycle: o3 → o4-mini → codex-mini → o3
const CODEX_MODELS: &[&str] = &["o3", "o4-mini", "codex-mini"];

/// Default model for each backend
pub fn default_model(backend: Backend) -> &'static str {
    match backend {
        Backend::Claude => "opus",
        Backend::Codex => "o3",
    }
}

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
        self.selected_model.as_deref().unwrap_or(default_model(self.backend))
    }

    /// Cycle selected_model through the backend's model list.
    /// Claude: opus → sonnet → haiku → opus
    /// Codex: o3 → o4-mini → codex-mini → o3
    pub fn cycle_model(&mut self) {
        let models = match self.backend {
            Backend::Claude => CLAUDE_MODELS,
            Backend::Codex => CODEX_MODELS,
        };
        let current = self.selected_model.as_deref().unwrap_or(models[0]);
        let idx = models.iter().position(|&m| m == current).unwrap_or(0);
        let next = models[(idx + 1) % models.len()];
        self.selected_model = Some(next.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_backend(backend: Backend) -> App {
        let mut app = App::new();
        app.backend = backend;
        app.selected_model = Some(default_model(backend).to_string());
        app
    }

    // ── default_model ──

    #[test]
    fn test_default_model_claude() {
        assert_eq!(default_model(Backend::Claude), "opus");
    }

    #[test]
    fn test_default_model_codex() {
        assert_eq!(default_model(Backend::Codex), "o3");
    }

    // ── Claude model cycling ──

    #[test]
    fn test_claude_cycle_opus_to_sonnet() {
        let mut app = app_with_backend(Backend::Claude);
        assert_eq!(app.display_model_name(), "opus");
        app.cycle_model();
        assert_eq!(app.display_model_name(), "sonnet");
    }

    #[test]
    fn test_claude_cycle_sonnet_to_haiku() {
        let mut app = app_with_backend(Backend::Claude);
        app.selected_model = Some("sonnet".to_string());
        app.cycle_model();
        assert_eq!(app.display_model_name(), "haiku");
    }

    #[test]
    fn test_claude_cycle_haiku_to_opus() {
        let mut app = app_with_backend(Backend::Claude);
        app.selected_model = Some("haiku".to_string());
        app.cycle_model();
        assert_eq!(app.display_model_name(), "opus");
    }

    #[test]
    fn test_claude_full_cycle_wraps() {
        let mut app = app_with_backend(Backend::Claude);
        app.cycle_model(); // opus → sonnet
        app.cycle_model(); // sonnet → haiku
        app.cycle_model(); // haiku → opus
        assert_eq!(app.display_model_name(), "opus");
    }

    #[test]
    fn test_claude_unknown_model_defaults_to_sonnet() {
        let mut app = app_with_backend(Backend::Claude);
        app.selected_model = Some("unknown".to_string());
        app.cycle_model();
        // Unknown position defaults to index 0 (opus), cycles to index 1 (sonnet)
        assert_eq!(app.display_model_name(), "sonnet");
    }

    // ── Codex model cycling ──

    #[test]
    fn test_codex_cycle_o3_to_o4mini() {
        let mut app = app_with_backend(Backend::Codex);
        assert_eq!(app.display_model_name(), "o3");
        app.cycle_model();
        assert_eq!(app.display_model_name(), "o4-mini");
    }

    #[test]
    fn test_codex_cycle_o4mini_to_codexmini() {
        let mut app = app_with_backend(Backend::Codex);
        app.selected_model = Some("o4-mini".to_string());
        app.cycle_model();
        assert_eq!(app.display_model_name(), "codex-mini");
    }

    #[test]
    fn test_codex_cycle_codexmini_to_o3() {
        let mut app = app_with_backend(Backend::Codex);
        app.selected_model = Some("codex-mini".to_string());
        app.cycle_model();
        assert_eq!(app.display_model_name(), "o3");
    }

    #[test]
    fn test_codex_full_cycle_wraps() {
        let mut app = app_with_backend(Backend::Codex);
        app.cycle_model(); // o3 → o4-mini
        app.cycle_model(); // o4-mini → codex-mini
        app.cycle_model(); // codex-mini → o3
        assert_eq!(app.display_model_name(), "o3");
    }

    #[test]
    fn test_codex_unknown_model_defaults_to_o4mini() {
        let mut app = app_with_backend(Backend::Codex);
        app.selected_model = Some("gpt-5".to_string());
        app.cycle_model();
        assert_eq!(app.display_model_name(), "o4-mini");
    }

    // ── display_model_name ──

    #[test]
    fn test_display_model_none_claude() {
        let mut app = App::new();
        app.backend = Backend::Claude;
        app.selected_model = None;
        assert_eq!(app.display_model_name(), "opus");
    }

    #[test]
    fn test_display_model_none_codex() {
        let mut app = App::new();
        app.backend = Backend::Codex;
        app.selected_model = None;
        assert_eq!(app.display_model_name(), "o3");
    }

    #[test]
    fn test_display_model_set_value() {
        let mut app = App::new();
        app.selected_model = Some("sonnet".to_string());
        assert_eq!(app.display_model_name(), "sonnet");
    }

    // ── CLAUDE_MODELS / CODEX_MODELS constants ──

    #[test]
    fn test_claude_models_has_three() {
        assert_eq!(CLAUDE_MODELS.len(), 3);
    }

    #[test]
    fn test_codex_models_has_three() {
        assert_eq!(CODEX_MODELS.len(), 3);
    }

    #[test]
    fn test_claude_models_first_is_default() {
        assert_eq!(CLAUDE_MODELS[0], default_model(Backend::Claude));
    }

    #[test]
    fn test_codex_models_first_is_default() {
        assert_eq!(CODEX_MODELS[0], default_model(Backend::Codex));
    }

    // ── update_token_badge ──

    #[test]
    fn test_token_badge_none_without_tokens() {
        let mut app = App::new();
        app.session_tokens = None;
        app.update_token_badge();
        assert!(app.token_badge_cache.is_none());
    }

    #[test]
    fn test_token_badge_green_low_usage() {
        let mut app = App::new();
        app.session_tokens = Some((50_000, 1_000));
        app.model_context_window = Some(200_000);
        app.update_token_badge();
        let (_, color) = app.token_badge_cache.unwrap();
        assert_eq!(color, ratatui::style::Color::Green);
    }

    #[test]
    fn test_token_badge_yellow_medium_usage() {
        let mut app = App::new();
        app.session_tokens = Some((120_000, 1_000));
        app.model_context_window = Some(200_000);
        app.update_token_badge();
        let (_, color) = app.token_badge_cache.unwrap();
        assert_eq!(color, ratatui::style::Color::Yellow);
    }

    #[test]
    fn test_token_badge_red_high_usage() {
        let mut app = App::new();
        app.session_tokens = Some((165_000, 1_000));
        app.model_context_window = Some(200_000);
        app.update_token_badge();
        let (_, color) = app.token_badge_cache.unwrap();
        assert_eq!(color, ratatui::style::Color::Red);
    }
}
