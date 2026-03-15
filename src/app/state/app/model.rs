//! Model selection and token usage badge

use crate::backend::Backend;
use super::App;

/// Unified model pool — Claude models first, then Codex models.
/// Ctrl+M cycles through the entire pool regardless of backend.
/// opus → sonnet → haiku → gpt-5.4 → gpt-5.3-codex → gpt-5.2-codex → gpt-5.2 → gpt-5.1-codex-max → gpt-5.1-codex-mini → wrap
const ALL_MODELS: &[&str] = &[
    "opus",
    "sonnet",
    "haiku",
    "gpt-5.4",
    "gpt-5.3-codex",
    "gpt-5.2-codex",
    "gpt-5.2",
    "gpt-5.1-codex-max",
    "gpt-5.1-codex-mini",
];

/// Default model (first in the unified pool)
pub fn default_model() -> &'static str {
    ALL_MODELS[0]
}

/// Determine which backend a model belongs to.
/// gpt-* models → Codex, everything else → Claude.
pub fn backend_for_model(model: &str) -> Backend {
    if model.starts_with("gpt-") {
        Backend::Codex
    } else {
        Backend::Claude
    }
}

impl App {
    /// Recompute the cached context usage badge from session store character count.
    /// Percentage = chars_since_compaction / COMPACTION_THRESHOLD (400k).
    /// Call after store append or compaction — the draw path just reads the cache.
    /// For live updates during streaming, use `update_token_badge_live()` instead.
    pub fn update_token_badge(&mut self) {
        let store_chars = match (&self.session_store, self.current_session_id) {
            (Some(store), Some(sid)) => {
                store.total_chars_since_compaction(sid).unwrap_or(0)
            }
            _ => 0,
        };
        self.store_chars_cached = store_chars;
        self.apply_token_badge(store_chars);
    }

    /// Lightweight badge update during streaming — uses cached store chars plus
    /// live display_events char count. No store I/O.
    pub fn update_token_badge_live(&mut self) {
        let live_chars: usize = self.display_events.iter()
            .map(crate::app::session_store::event_char_len)
            .sum();
        self.apply_token_badge(self.store_chars_cached + live_chars);
    }

    fn apply_token_badge(&mut self, total_chars: usize) {
        let threshold = crate::app::session_store::COMPACTION_THRESHOLD as f64;
        let pct_value = if total_chars > 0 || self.current_session_id.is_some() {
            let pct = (total_chars as f64 / threshold * 100.0).min(100.0);
            let color = if pct < 60.0 { ratatui::style::Color::Green }
                else if pct < 90.0 { ratatui::style::Color::Yellow }
                else { ratatui::style::Color::Red };
            self.token_badge_cache = Some((format!(" {:.0}% ", pct), color));
            pct
        } else {
            self.token_badge_cache = None;
            0.0
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
        self.selected_model.as_deref().unwrap_or(default_model())
    }

    /// Cycle selected_model through the unified model pool.
    /// opus → sonnet → haiku → gpt-5.4 → … → gpt-5.1-codex-mini → wrap
    /// Also updates self.backend to match the new model.
    pub fn cycle_model(&mut self) {
        let current = self.selected_model.as_deref().unwrap_or(ALL_MODELS[0]);
        let idx = ALL_MODELS.iter().position(|&m| m == current).unwrap_or(0);
        let next = ALL_MODELS[(idx + 1) % ALL_MODELS.len()];
        self.selected_model = Some(next.to_string());
        self.backend = backend_for_model(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_default() -> App {
        let mut app = App::new();
        app.selected_model = Some(default_model().to_string());
        app
    }

    // ── default_model ──

    #[test]
    fn test_default_model_is_opus() {
        assert_eq!(default_model(), "opus");
    }

    // ── backend_for_model ──

    #[test]
    fn test_backend_for_claude_models() {
        assert_eq!(backend_for_model("opus"), Backend::Claude);
        assert_eq!(backend_for_model("sonnet"), Backend::Claude);
        assert_eq!(backend_for_model("haiku"), Backend::Claude);
    }

    #[test]
    fn test_backend_for_codex_models() {
        assert_eq!(backend_for_model("gpt-5.4"), Backend::Codex);
        assert_eq!(backend_for_model("gpt-5.3-codex"), Backend::Codex);
        assert_eq!(backend_for_model("gpt-5.2-codex"), Backend::Codex);
        assert_eq!(backend_for_model("gpt-5.2"), Backend::Codex);
        assert_eq!(backend_for_model("gpt-5.1-codex-max"), Backend::Codex);
        assert_eq!(backend_for_model("gpt-5.1-codex-mini"), Backend::Codex);
    }

    #[test]
    fn test_backend_for_unknown_defaults_claude() {
        assert_eq!(backend_for_model("unknown"), Backend::Claude);
    }

    // ── Unified model cycling ──

    #[test]
    fn test_cycle_opus_to_sonnet() {
        let mut app = app_default();
        assert_eq!(app.display_model_name(), "opus");
        app.cycle_model();
        assert_eq!(app.display_model_name(), "sonnet");
        assert_eq!(app.backend, Backend::Claude);
    }

    #[test]
    fn test_cycle_haiku_to_gpt54() {
        let mut app = app_default();
        app.selected_model = Some("haiku".to_string());
        app.cycle_model();
        assert_eq!(app.display_model_name(), "gpt-5.4");
        assert_eq!(app.backend, Backend::Codex);
    }

    #[test]
    fn test_cycle_last_codex_wraps_to_opus() {
        let mut app = app_default();
        app.selected_model = Some("gpt-5.1-codex-mini".to_string());
        app.backend = Backend::Codex;
        app.cycle_model();
        assert_eq!(app.display_model_name(), "opus");
        assert_eq!(app.backend, Backend::Claude);
    }

    #[test]
    fn test_full_cycle_all_nine() {
        let mut app = app_default();
        let expected = [
            ("sonnet", Backend::Claude),
            ("haiku", Backend::Claude),
            ("gpt-5.4", Backend::Codex),
            ("gpt-5.3-codex", Backend::Codex),
            ("gpt-5.2-codex", Backend::Codex),
            ("gpt-5.2", Backend::Codex),
            ("gpt-5.1-codex-max", Backend::Codex),
            ("gpt-5.1-codex-mini", Backend::Codex),
            ("opus", Backend::Claude),
        ];
        for &(name, backend) in &expected {
            app.cycle_model();
            assert_eq!(app.display_model_name(), name);
            assert_eq!(app.backend, backend);
        }
    }

    #[test]
    fn test_cycle_unknown_model_defaults_to_sonnet() {
        let mut app = app_default();
        app.selected_model = Some("unknown".to_string());
        app.cycle_model();
        // Unknown defaults to index 0 (opus), cycles to index 1 (sonnet)
        assert_eq!(app.display_model_name(), "sonnet");
    }

    // ── display_model_name ──

    #[test]
    fn test_display_model_none_defaults_opus() {
        let mut app = App::new();
        app.selected_model = None;
        assert_eq!(app.display_model_name(), "opus");
    }

    #[test]
    fn test_display_model_set_value() {
        let mut app = App::new();
        app.selected_model = Some("gpt-5.4".to_string());
        assert_eq!(app.display_model_name(), "gpt-5.4");
    }

    // ── ALL_MODELS constant ──

    #[test]
    fn test_all_models_has_nine() {
        assert_eq!(ALL_MODELS.len(), 9);
    }

    #[test]
    fn test_all_models_first_is_default() {
        assert_eq!(ALL_MODELS[0], default_model());
    }

    #[test]
    fn test_all_models_claude_then_codex() {
        // First 3 are Claude, rest are Codex
        for &m in &ALL_MODELS[..3] {
            assert_eq!(backend_for_model(m), Backend::Claude);
        }
        for &m in &ALL_MODELS[3..] {
            assert_eq!(backend_for_model(m), Backend::Codex);
        }
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
