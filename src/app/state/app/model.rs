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
    /// Recompute the cached context usage badge from session store character count.
    /// Percentage = chars_since_compaction / COMPACTION_THRESHOLD (400k).
    /// Call after store append or compaction — draw path just reads the cache.
    pub fn update_token_badge(&mut self) {
        let pct_value = match (&self.session_store, self.current_session_id) {
            (Some(store), Some(sid)) => {
                match store.total_chars_since_compaction(sid) {
                    Ok(chars) => {
                        let threshold = crate::app::session_store::COMPACTION_THRESHOLD as f64;
                        let pct = (chars as f64 / threshold * 100.0).min(100.0);
                        let color = if pct < 60.0 { ratatui::style::Color::Green }
                            else if pct < 90.0 { ratatui::style::Color::Yellow }
                            else { ratatui::style::Color::Red };
                        self.token_badge_cache = Some((format!(" {:.0}% ", pct), color));
                        pct
                    }
                    Err(_) => {
                        self.token_badge_cache = None;
                        0.0
                    }
                }
            }
            _ => {
                self.token_badge_cache = None;
                0.0
            }
        };
        // Track 90% threshold for compaction inactivity watcher
        let was_high = self.context_pct_high;
        self.context_pct_high = pct_value >= 90.0;
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

    // ── update_token_badge (sourced from session store chars / 400k threshold) ──

    /// Helper: create an App with an in-memory session store and a session with
    /// the given total character count (via a single UserMessage event).
    fn app_with_store_chars(chars: usize) -> App {
        use crate::app::session_store::SessionStore;
        use crate::events::DisplayEvent;

        let mut app = App::new();
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("test").unwrap();
        if chars > 0 {
            let content = "x".repeat(chars);
            let events = vec![DisplayEvent::UserMessage {
                _uuid: String::new(),
                content,
            }];
            store.append_events(sid, &events).unwrap();
        }
        app.session_store = Some(store);
        app.current_session_id = Some(sid);
        app
    }

    #[test]
    fn test_token_badge_none_without_store() {
        let mut app = App::new();
        app.update_token_badge();
        assert!(app.token_badge_cache.is_none());
    }

    #[test]
    fn test_token_badge_none_without_session_id() {
        use crate::app::session_store::SessionStore;
        let mut app = App::new();
        app.session_store = Some(SessionStore::open_memory().unwrap());
        app.current_session_id = None;
        app.update_token_badge();
        assert!(app.token_badge_cache.is_none());
    }

    #[test]
    fn test_token_badge_green_low_usage() {
        // 100k chars out of 400k = 25%
        let mut app = app_with_store_chars(100_000);
        app.update_token_badge();
        let (text, color) = app.token_badge_cache.unwrap();
        assert_eq!(color, ratatui::style::Color::Green);
        assert!(text.contains("25"));
    }

    #[test]
    fn test_token_badge_yellow_medium_usage() {
        // 280k chars out of 400k = 70%
        let mut app = app_with_store_chars(280_000);
        app.update_token_badge();
        let (_, color) = app.token_badge_cache.unwrap();
        assert_eq!(color, ratatui::style::Color::Yellow);
    }

    #[test]
    fn test_token_badge_red_high_usage() {
        // 380k chars out of 400k = 95%
        let mut app = app_with_store_chars(380_000);
        app.update_token_badge();
        let (_, color) = app.token_badge_cache.unwrap();
        assert_eq!(color, ratatui::style::Color::Red);
        assert!(app.context_pct_high);
    }

    #[test]
    fn test_token_badge_capped_at_100() {
        // 500k chars out of 400k — should cap at 100%
        let mut app = app_with_store_chars(500_000);
        app.update_token_badge();
        let (text, color) = app.token_badge_cache.unwrap();
        assert_eq!(color, ratatui::style::Color::Red);
        assert!(text.contains("100"));
    }

    #[test]
    fn test_token_badge_zero_chars() {
        let mut app = app_with_store_chars(0);
        app.update_token_badge();
        let (text, _) = app.token_badge_cache.unwrap();
        assert!(text.contains("0"));
    }

    #[test]
    fn test_token_badge_compaction_resets_pct() {
        use crate::app::session_store::SessionStore;
        use crate::events::DisplayEvent;

        let mut app = App::new();
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("test").unwrap();
        // Add 380k chars (95%)
        let events = vec![DisplayEvent::UserMessage {
            _uuid: String::new(),
            content: "x".repeat(380_000),
        }];
        store.append_events(sid, &events).unwrap();
        app.session_store = Some(store);
        app.current_session_id = Some(sid);
        app.update_token_badge();
        assert!(app.context_pct_high);

        // Store compaction — chars_since_compaction drops to 0
        let max_seq = app.session_store.as_ref().unwrap().max_seq(sid).unwrap();
        app.session_store.as_ref().unwrap().store_compaction(sid, max_seq, "summary").unwrap();
        app.update_token_badge();
        assert!(!app.context_pct_high);
        let (text, color) = app.token_badge_cache.unwrap();
        assert!(text.contains("0"));
        assert_eq!(color, ratatui::style::Color::Green);
    }
}
