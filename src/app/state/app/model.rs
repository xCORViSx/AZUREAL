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

/// Map a model string from an Init event back to an ALL_MODELS alias.
/// Handles exact matches ("gpt-5.4"), Claude API names ("claude-3-5-sonnet-20241022" → "sonnet"),
/// and short aliases passed through `--model`.
pub fn model_alias_from_init(model: &str) -> Option<&'static str> {
    // Exact match first
    if let Some(&m) = ALL_MODELS.iter().find(|&&m| m == model) {
        return Some(m);
    }
    // Claude API model names contain the alias as a substring
    for &alias in &["opus", "sonnet", "haiku"] {
        if model.contains(alias) {
            return Some(alias);
        }
    }
    // Codex models start with gpt- but might not be in ALL_MODELS
    if model.starts_with("gpt-") {
        return ALL_MODELS.iter().find(|&&m| m.starts_with("gpt-")).copied();
    }
    // Legacy: old sessions stored "codex" as the model string — map to first Codex model
    if model == "codex" {
        return ALL_MODELS.iter().find(|&&m| m.starts_with("gpt-")).copied();
    }
    None
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
    /// Extract the model from the loaded session's event stream.
    /// Scans backward for `ModelSwitch` tags first (user explicitly changed the
    /// model), then falls back to `Init` events (model from session start).
    /// Returns `None` if the session is empty or the model string is unrecognized.
    pub fn last_session_model(&self) -> Option<&'static str> {
        // ModelSwitch tags take priority — they represent explicit user choice
        for e in self.display_events.iter().rev() {
            match e {
                crate::events::DisplayEvent::ModelSwitch { model } => {
                    if let Some(alias) = model_alias_from_init(model) {
                        return Some(alias);
                    }
                }
                _ => {}
            }
        }
        // Fall back to the last Init event (model the session was started with)
        self.display_events.iter().rev()
            .find_map(|e| match e {
                crate::events::DisplayEvent::Init { model, .. } => model_alias_from_init(model),
                _ => None,
            })
    }

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
    /// Also updates self.backend to match the new model and injects a
    /// `ModelSwitch` tag into the session store for persistence.
    pub fn cycle_model(&mut self) {
        let current = self.selected_model.as_deref().unwrap_or(ALL_MODELS[0]);
        let idx = ALL_MODELS.iter().position(|&m| m == current).unwrap_or(0);
        let next = ALL_MODELS[(idx + 1) % ALL_MODELS.len()];
        self.selected_model = Some(next.to_string());
        let new_backend = backend_for_model(next);
        if new_backend != self.backend {
            self.backend = new_backend;
            // Reset the background parser so it uses the new backend's format
            self.agent_processor_needs_reset = true;
        }
        // Inject ModelSwitch tag into the event stream + persist to session store
        let tag = crate::events::DisplayEvent::ModelSwitch { model: next.to_string() };
        self.display_events.push(tag.clone());
        if let (Some(store), Some(sid)) = (&self.session_store, self.current_session_id) {
            let _ = store.append_events(sid, &[tag]);
        }
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

    // ── model_alias_from_init ──

    #[test]
    fn test_alias_exact_match() {
        assert_eq!(model_alias_from_init("opus"), Some("opus"));
        assert_eq!(model_alias_from_init("gpt-5.4"), Some("gpt-5.4"));
        assert_eq!(model_alias_from_init("gpt-5.1-codex-mini"), Some("gpt-5.1-codex-mini"));
    }

    #[test]
    fn test_alias_claude_api_name() {
        assert_eq!(model_alias_from_init("claude-3-5-sonnet-20241022"), Some("sonnet"));
        assert_eq!(model_alias_from_init("claude-opus-4-6"), Some("opus"));
        assert_eq!(model_alias_from_init("claude-3-haiku-20240307"), Some("haiku"));
    }

    #[test]
    fn test_alias_unknown_returns_none() {
        assert_eq!(model_alias_from_init("unknown"), None);
        assert_eq!(model_alias_from_init(""), None);
    }

    #[test]
    fn test_alias_unknown_gpt_falls_back_to_first_codex() {
        // An unlisted gpt model still maps to a Codex entry
        assert!(model_alias_from_init("gpt-99").unwrap().starts_with("gpt-"));
    }

    #[test]
    fn test_alias_legacy_codex_string() {
        // Old sessions stored "codex" as the model — should map to first Codex model
        let result = model_alias_from_init("codex");
        assert!(result.is_some());
        assert!(result.unwrap().starts_with("gpt-"));
    }

    // ── last_session_model ──

    #[test]
    fn test_last_session_model_empty_events() {
        let app = App::new();
        assert_eq!(app.last_session_model(), None);
    }

    #[test]
    fn test_last_session_model_from_init() {
        use crate::events::DisplayEvent;
        let mut app = App::new();
        app.display_events = vec![
            DisplayEvent::Init { _session_id: String::new(), cwd: String::new(), model: "gpt-5.4".into() },
            DisplayEvent::AssistantText { _uuid: String::new(), _message_id: String::new(), text: "hi".into() },
        ];
        assert_eq!(app.last_session_model(), Some("gpt-5.4"));
    }

    #[test]
    fn test_last_session_model_picks_last_init() {
        use crate::events::DisplayEvent;
        let mut app = App::new();
        app.display_events = vec![
            DisplayEvent::Init { _session_id: String::new(), cwd: String::new(), model: "opus".into() },
            DisplayEvent::AssistantText { _uuid: String::new(), _message_id: String::new(), text: "first".into() },
            DisplayEvent::Init { _session_id: String::new(), cwd: String::new(), model: "sonnet".into() },
            DisplayEvent::AssistantText { _uuid: String::new(), _message_id: String::new(), text: "second".into() },
        ];
        assert_eq!(app.last_session_model(), Some("sonnet"));
    }

    #[test]
    fn test_last_session_model_model_switch_overrides_init() {
        use crate::events::DisplayEvent;
        let mut app = App::new();
        app.display_events = vec![
            DisplayEvent::Init { _session_id: String::new(), cwd: String::new(), model: "opus".into() },
            DisplayEvent::AssistantText { _uuid: String::new(), _message_id: String::new(), text: "hi".into() },
            DisplayEvent::ModelSwitch { model: "gpt-5.4".into() },
        ];
        // ModelSwitch should take priority over Init
        assert_eq!(app.last_session_model(), Some("gpt-5.4"));
    }

    #[test]
    fn test_last_session_model_picks_last_model_switch() {
        use crate::events::DisplayEvent;
        let mut app = App::new();
        app.display_events = vec![
            DisplayEvent::Init { _session_id: String::new(), cwd: String::new(), model: "opus".into() },
            DisplayEvent::ModelSwitch { model: "sonnet".into() },
            DisplayEvent::ModelSwitch { model: "gpt-5.4".into() },
            DisplayEvent::ModelSwitch { model: "haiku".into() },
        ];
        assert_eq!(app.last_session_model(), Some("haiku"));
    }

    #[test]
    fn test_last_session_model_no_switch_falls_back_to_init() {
        use crate::events::DisplayEvent;
        let mut app = App::new();
        app.display_events = vec![
            DisplayEvent::Init { _session_id: String::new(), cwd: String::new(), model: "sonnet".into() },
            DisplayEvent::UserMessage { _uuid: String::new(), content: "hello".into() },
            DisplayEvent::AssistantText { _uuid: String::new(), _message_id: String::new(), text: "hi".into() },
        ];
        assert_eq!(app.last_session_model(), Some("sonnet"));
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

    #[test]
    fn test_cycle_injects_model_switch_event() {
        use crate::events::DisplayEvent;
        let mut app = app_default();
        assert!(app.display_events.is_empty());
        app.cycle_model();
        assert_eq!(app.display_events.len(), 1);
        match &app.display_events[0] {
            DisplayEvent::ModelSwitch { model } => assert_eq!(model, "sonnet"),
            other => panic!("expected ModelSwitch, got {:?}", other),
        }
    }

    #[test]
    fn test_cycle_model_switch_persists_to_store() {
        use crate::app::session_store::SessionStore;
        let mut app = app_default();
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("test").unwrap();
        app.session_store = Some(store);
        app.current_session_id = Some(sid);
        app.cycle_model(); // opus → sonnet
        // Verify the ModelSwitch event was persisted to the store
        let events = app.session_store.as_ref().unwrap().load_events(sid).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            crate::events::DisplayEvent::ModelSwitch { model } => assert_eq!(model, "sonnet"),
            other => panic!("expected ModelSwitch, got {:?}", other),
        }
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
