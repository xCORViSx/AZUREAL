//! Model selection and context usage badge

use super::App;
use crate::backend::Backend;
use ratatui::style::Color;

/// Claude model aliases exposed by Azureal's unified model picker.
const CLAUDE_MODELS: &[&str] = &["opus", "sonnet", "haiku"];

// BEGIN OPENAI_FRONTIER_MODELS
/// OpenAI frontier models in docs order.
/// Sourced from the OpenAI docs "Frontier models" section on 2026-07-11.
/// `azureal models sync-openai-frontier` rewrites this block.
const OPENAI_FRONTIER_MODELS: &[&str] = &[
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
    "gpt-5.5",
    "gpt-5.5-pro",
    "gpt-5.4",
    "gpt-5.4-pro",
    "gpt-5.4-mini",
    "gpt-5.4-nano",
];
// END OPENAI_FRONTIER_MODELS

/// Iterate through Claude aliases followed by the synced OpenAI frontier models.
fn all_models() -> impl Iterator<Item = &'static str> {
    CLAUDE_MODELS
        .iter()
        .chain(OPENAI_FRONTIER_MODELS.iter())
        .copied()
}

/// Materialize the unified model order for cycling and tests.
fn all_models_vec() -> Vec<&'static str> {
    all_models().collect()
}

/// Return the highest-priority Codex model from the synced frontier catalog.
fn first_codex_model() -> Option<&'static str> {
    OPENAI_FRONTIER_MODELS.first().copied()
}

#[cfg(test)]
/// Return the final Codex model so cycling wraparound can be verified.
fn last_codex_model() -> Option<&'static str> {
    OPENAI_FRONTIER_MODELS.last().copied()
}

/// Default model for new/empty sessions.
pub fn default_model() -> &'static str {
    OPENAI_FRONTIER_MODELS[0]
}

/// Map a model string from an Init event back to a unified model-pool alias.
/// Handles exact matches ("gpt-5.4"), Claude API names ("claude-3-5-sonnet-20241022" → "sonnet"),
/// and short aliases passed through `--model`.
pub fn model_alias_from_init(model: &str) -> Option<&'static str> {
    // Exact match first
    if let Some(m) = all_models().find(|&m| m == model) {
        return Some(m);
    }
    // Claude API model names contain the alias as a substring
    for &alias in CLAUDE_MODELS {
        if model.contains(alias) {
            return Some(alias);
        }
    }
    // Codex models start with gpt- but might not be in OPENAI_FRONTIER_MODELS
    if model.starts_with("gpt-") {
        return first_codex_model();
    }
    // Legacy: old sessions stored "codex" as the model string — map to first Codex model
    if model == "codex" {
        return first_codex_model();
    }
    None
}

/// Determine which backend a model belongs to.
/// gpt-* models → Codex, everything else → Claude.
pub fn backend_for_model(model: &str) -> Backend {
    if model.starts_with("gpt-") || model == "codex" {
        Backend::Codex
    } else {
        Backend::Claude
    }
}

/// Accent color for the currently selected model.
pub fn model_color(model: &str) -> Color {
    match model {
        "opus" => Color::Magenta,
        "sonnet" => Color::Cyan,
        "haiku" => Color::Yellow,
        m if m.starts_with("gpt-5.6") => Color::LightCyan,
        m if m.starts_with("gpt-5.5") => Color::Green,
        m if m.starts_with("gpt-5.4") => Color::LightGreen,
        "gpt-5" | "gpt-5-mini" | "gpt-5-nano" => Color::LightBlue,
        "gpt-4.1" => Color::Blue,
        m if m.starts_with("gpt-") => Color::LightBlue,
        _ => Color::DarkGray,
    }
}

/// Model-selection and context-meter behavior attached to application state.
impl App {
    /// Return the subset of the unified model pool whose backend is detected as installed.
    /// Falls back to the full pool if neither backend is found (the app can't
    /// function without at least one, so don't hide everything).
    fn available_models(&self) -> Vec<&'static str> {
        let filtered: Vec<&str> = all_models()
            .filter(|m| match backend_for_model(m) {
                Backend::Claude => self.claude_available,
                Backend::Codex => self.codex_available,
            })
            .collect();
        if filtered.is_empty() {
            all_models_vec()
        } else {
            filtered
        }
    }

    /// First available model (respects backend availability).
    pub fn first_available_model(&self) -> &'static str {
        let pool = self.available_models();
        if pool.contains(&default_model()) {
            return default_model();
        }
        pool.first().copied().unwrap_or(default_model())
    }

    /// Extract the model from the loaded session's event stream.
    /// Scans backward for `ModelSwitch` tags first (user explicitly changed the
    /// model), then falls back to `Init` events (model from session start).
    /// Returns `None` if the session is empty or the model string is unrecognized.
    pub fn last_session_model(&self) -> Option<&'static str> {
        // ModelSwitch tags take priority — they represent explicit user choice
        for e in self.display_events.iter().rev() {
            if let crate::events::DisplayEvent::ModelSwitch { model } = e {
                if let Some(alias) = model_alias_from_init(model) {
                    return Some(alias);
                }
            }
        }
        // Fall back to the last Init event (model the session was started with)
        self.display_events.iter().rev().find_map(|e| match e {
            crate::events::DisplayEvent::Init { model, .. } => model_alias_from_init(model),
            _ => None,
        })
    }

    /// Restore `selected_model` and `backend` from the current session's events.
    /// Called after `load_session_output()` so that worktree/project switches
    /// pick up the model from the newly loaded session instead of keeping the
    /// previous session's model.
    /// If the restored model's backend is not installed, falls back to the
    /// first available model.
    pub fn restore_model_from_session(&mut self) {
        let mut restored = self.last_session_model().unwrap_or(default_model());
        // If the restored model's backend is not installed, fall back
        let pool = self.available_models();
        if !pool.contains(&restored) {
            restored = pool.first().copied().unwrap_or(default_model());
        }
        self.selected_model = Some(restored.to_string());
        let new_backend = backend_for_model(restored);
        if new_backend != self.backend {
            self.backend = new_backend;
            self.agent_processor_needs_reset = true;
        }
    }

    /// Recompute the cached context usage badge from Azureal's custom
    /// compaction counter: chars since last compaction / 400k threshold.
    /// Call after store append or compaction — the draw path just reads the cache.
    /// For live updates during streaming, use `update_token_badge_live()` instead.
    pub fn update_token_badge(&mut self) {
        let store_chars = match (&self.session_store, self.current_session_id) {
            (Some(store), Some(sid)) => store.total_chars_since_compaction(sid).unwrap_or(0),
            _ => 0,
        };
        self.store_chars_cached = store_chars;
        // Sync the live char counter from the store (authoritative at rest)
        self.chars_since_compaction = store_chars;
        self.apply_token_badge(store_chars);

        // Startup/load-time compaction trigger: if the store already has chars
        // above threshold (e.g. after app restart with uncompacted data), set
        // compaction_needed so the event loop spawns the agent on next tick.
        if store_chars >= crate::app::session_store::COMPACTION_THRESHOLD
            && self.compaction_needed.is_none()
            && self.compaction_receivers.is_empty()
        {
            if let Some(sid) = self.current_session_id {
                if let Some(wt_path) = self
                    .current_worktree()
                    .and_then(|s| s.worktree_path.clone())
                {
                    self.compaction_needed = Some((sid, wt_path));
                }
            }
        }
    }

    /// Lightweight badge update during streaming — uses the authoritative live
    /// char counter (synced from store on load, then incremented as prompts and
    /// parsed output arrive). No store I/O.
    pub fn update_token_badge_live(&mut self) {
        self.apply_token_badge(self.chars_since_compaction);
    }

    /// Update cached context-usage text, color, and compaction threshold state.
    fn apply_token_badge(&mut self, total_chars: usize) {
        let pct_value = self.char_usage_pct(total_chars);
        let pct_value = if let Some(pct) = pct_value {
            let color = if pct < 60.0 {
                ratatui::style::Color::Green
            } else if pct < 90.0 {
                ratatui::style::Color::Yellow
            } else {
                ratatui::style::Color::Red
            };
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

    /// Convert stored character usage into a percentage capped at one hundred.
    fn char_usage_pct(&self, total_chars: usize) -> Option<f64> {
        if total_chars > 0 || self.current_session_id.is_some() {
            let threshold = crate::app::session_store::COMPACTION_THRESHOLD as f64;
            Some((total_chars as f64 / threshold * 100.0).min(100.0))
        } else {
            None
        }
    }

    /// Short display name for the active model. Always returns the selected_model
    /// alias since it's always set (never None).
    pub fn display_model_name(&self) -> &str {
        self.selected_model.as_deref().unwrap_or(default_model())
    }

    /// Cycle selected_model through available models only.
    /// Skips model families whose backend CLI is not installed.
    /// Also updates self.backend to match the new model and injects a
    /// `ModelSwitch` tag into the session store for persistence.
    pub fn cycle_model(&mut self) {
        let pool = self.available_models();
        if pool.is_empty() {
            return;
        }
        let current = self.selected_model.as_deref().unwrap_or(default_model());
        let idx = pool
            .iter()
            .position(|&m| m == current)
            .or_else(|| pool.iter().position(|&m| m == default_model()))
            .unwrap_or(0);
        let next = pool[(idx + 1) % pool.len()];
        self.selected_model = Some(next.to_string());
        let new_backend = backend_for_model(next);
        if new_backend != self.backend {
            self.backend = new_backend;
            // Reset the background parser so it uses the new backend's format
            self.agent_processor_needs_reset = true;
        }
        // Inject ModelSwitch tag into the event stream + persist to session store
        let tag = crate::events::DisplayEvent::ModelSwitch {
            model: next.to_string(),
        };
        self.display_events.push(tag.clone());
        if let (Some(store), Some(sid)) = (&self.session_store, self.current_session_id) {
            let _ = store.append_events(sid, &[tag]);
        }
    }
}

#[cfg(test)]
/// Regression tests for model selection, persistence, colors, and context usage.
mod tests {
    use super::*;

    /// Build an application whose selected model matches the production default.
    fn app_default() -> App {
        let mut app = App::new();
        app.selected_model = Some(default_model().to_string());
        app
    }

    // ── default_model ──

    /// Verifies that default model is GPT-5.6 sol.
    #[test]
    fn test_default_model_is_gpt_5_6_sol() {
        assert_eq!(default_model(), "gpt-5.6-sol");
    }

    // ── backend_for_model ──

    /// Verifies that backend for Claude models.
    #[test]
    fn test_backend_for_claude_models() {
        for &model in CLAUDE_MODELS {
            assert_eq!(backend_for_model(model), Backend::Claude);
        }
    }

    /// Verifies that backend for Codex models.
    #[test]
    fn test_backend_for_codex_models() {
        for &model in OPENAI_FRONTIER_MODELS {
            assert_eq!(backend_for_model(model), Backend::Codex);
        }
        assert_eq!(backend_for_model("codex"), Backend::Codex);
    }

    /// Verifies that backend for unknown defaults Claude.
    #[test]
    fn test_backend_for_unknown_defaults_claude() {
        assert_eq!(backend_for_model("unknown"), Backend::Claude);
    }

    // ── model_alias_from_init ──

    /// Verifies that alias exact match.
    #[test]
    fn test_alias_exact_match() {
        for model in all_models_vec() {
            assert_eq!(model_alias_from_init(model), Some(model));
        }
    }

    /// Verifies that alias Claude api name.
    #[test]
    fn test_alias_claude_api_name() {
        assert_eq!(
            model_alias_from_init("claude-3-5-sonnet-20241022"),
            Some("sonnet")
        );
        assert_eq!(model_alias_from_init("claude-opus-4-6"), Some("opus"));
        assert_eq!(
            model_alias_from_init("claude-3-haiku-20240307"),
            Some("haiku")
        );
    }

    /// Verifies that alias unknown returns none.
    #[test]
    fn test_alias_unknown_returns_none() {
        assert_eq!(model_alias_from_init("unknown"), None);
        assert_eq!(model_alias_from_init(""), None);
    }

    /// Verifies that alias unknown gpt falls back to first Codex.
    #[test]
    fn test_alias_unknown_gpt_falls_back_to_first_codex() {
        // An unlisted gpt model still maps to a Codex entry
        assert!(model_alias_from_init("gpt-99").unwrap().starts_with("gpt-"));
    }

    /// Verifies that alias legacy Codex string.
    #[test]
    fn test_alias_legacy_codex_string() {
        // Old sessions stored "codex" as the model — should map to first Codex model
        let result = model_alias_from_init("codex");
        assert!(result.is_some());
        assert!(result.unwrap().starts_with("gpt-"));
    }

    // ── last_session_model ──

    /// Verifies that last session model empty events.
    #[test]
    fn test_last_session_model_empty_events() {
        let app = App::new();
        assert_eq!(app.last_session_model(), None);
    }

    /// Verifies that last session model from init.
    #[test]
    fn test_last_session_model_from_init() {
        use crate::events::DisplayEvent;
        let mut app = App::new();
        app.display_events = vec![
            DisplayEvent::Init {
                _session_id: String::new(),
                cwd: String::new(),
                model: "gpt-5.4".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text: "hi".into(),
            },
        ];
        assert_eq!(app.last_session_model(), Some("gpt-5.4"));
    }

    /// Verifies that last session model picks last init.
    #[test]
    fn test_last_session_model_picks_last_init() {
        use crate::events::DisplayEvent;
        let mut app = App::new();
        app.display_events = vec![
            DisplayEvent::Init {
                _session_id: String::new(),
                cwd: String::new(),
                model: "opus".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text: "first".into(),
            },
            DisplayEvent::Init {
                _session_id: String::new(),
                cwd: String::new(),
                model: "sonnet".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text: "second".into(),
            },
        ];
        assert_eq!(app.last_session_model(), Some("sonnet"));
    }

    /// Verifies that last session model model switch overrides init.
    #[test]
    fn test_last_session_model_model_switch_overrides_init() {
        use crate::events::DisplayEvent;
        let mut app = App::new();
        app.display_events = vec![
            DisplayEvent::Init {
                _session_id: String::new(),
                cwd: String::new(),
                model: "opus".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text: "hi".into(),
            },
            DisplayEvent::ModelSwitch {
                model: "gpt-5.4".into(),
            },
        ];
        // ModelSwitch should take priority over Init
        assert_eq!(app.last_session_model(), Some("gpt-5.4"));
    }

    /// Verifies that last session model picks last model switch.
    #[test]
    fn test_last_session_model_picks_last_model_switch() {
        use crate::events::DisplayEvent;
        let mut app = App::new();
        app.display_events = vec![
            DisplayEvent::Init {
                _session_id: String::new(),
                cwd: String::new(),
                model: "opus".into(),
            },
            DisplayEvent::ModelSwitch {
                model: "sonnet".into(),
            },
            DisplayEvent::ModelSwitch {
                model: "gpt-5.4".into(),
            },
            DisplayEvent::ModelSwitch {
                model: "haiku".into(),
            },
        ];
        assert_eq!(app.last_session_model(), Some("haiku"));
    }

    /// Verifies that last session model no switch falls back to init.
    #[test]
    fn test_last_session_model_no_switch_falls_back_to_init() {
        use crate::events::DisplayEvent;
        let mut app = App::new();
        app.display_events = vec![
            DisplayEvent::Init {
                _session_id: String::new(),
                cwd: String::new(),
                model: "sonnet".into(),
            },
            DisplayEvent::UserMessage {
                _uuid: String::new(),
                content: "hello".into(),
            },
            DisplayEvent::AssistantText {
                _uuid: String::new(),
                _message_id: String::new(),
                text: "hi".into(),
            },
        ];
        assert_eq!(app.last_session_model(), Some("sonnet"));
    }

    // ── Unified model cycling ──

    /// Verifies that cycle opus to sonnet.
    #[test]
    fn test_cycle_opus_to_sonnet() {
        let mut app = app_default();
        app.selected_model = Some("opus".to_string());
        app.backend = Backend::Claude;
        assert_eq!(app.display_model_name(), "opus");
        app.cycle_model();
        assert_eq!(app.display_model_name(), "sonnet");
        assert_eq!(app.backend, Backend::Claude);
    }

    /// Verifies that cycle haiku to first Codex model.
    #[test]
    fn test_cycle_haiku_to_first_codex_model() {
        let mut app = app_default();
        app.selected_model = Some("haiku".to_string());
        app.cycle_model();
        assert_eq!(app.display_model_name(), first_codex_model().unwrap());
        assert_eq!(app.backend, Backend::Codex);
    }

    /// Verifies that cycle last Codex wraps to opus.
    #[test]
    fn test_cycle_last_codex_wraps_to_opus() {
        let mut app = app_default();
        app.selected_model = Some(last_codex_model().unwrap().to_string());
        app.backend = Backend::Codex;
        app.cycle_model();
        assert_eq!(app.display_model_name(), "opus");
        assert_eq!(app.backend, Backend::Claude);
    }

    /// Verifies that full cycle all models.
    #[test]
    fn test_full_cycle_all_models() {
        let mut app = app_default();
        let models = all_models_vec();
        let default_idx = models
            .iter()
            .position(|&model| model == default_model())
            .unwrap();
        let expected: Vec<&str> = models
            .iter()
            .cycle()
            .skip(default_idx + 1)
            .take(models.len())
            .copied()
            .collect();
        for name in expected {
            app.cycle_model();
            assert_eq!(app.display_model_name(), name);
            assert_eq!(app.backend, backend_for_model(name));
        }
    }

    /// Verifies that cycle unknown model defaults to model after default.
    #[test]
    fn test_cycle_unknown_model_defaults_to_model_after_default() {
        let mut app = app_default();
        app.selected_model = Some("unknown".to_string());
        app.cycle_model();
        assert_eq!(app.display_model_name(), "gpt-5.6-terra");
    }

    /// Verifies that cycle injects model switch event.
    #[test]
    fn test_cycle_injects_model_switch_event() {
        use crate::events::DisplayEvent;
        let mut app = app_default();
        assert!(app.display_events.is_empty());
        app.cycle_model();
        assert_eq!(app.display_events.len(), 1);
        match &app.display_events[0] {
            DisplayEvent::ModelSwitch { model } => assert_eq!(model, "gpt-5.6-terra"),
            other => panic!("expected ModelSwitch, got {:?}", other),
        }
    }

    /// Verifies that cycle model switch persists to store.
    #[test]
    fn test_cycle_model_switch_persists_to_store() {
        use crate::app::session_store::SessionStore;
        let mut app = app_default();
        let store = SessionStore::open_memory().unwrap();
        let sid = store.create_session("test").unwrap();
        app.session_store = Some(store);
        app.current_session_id = Some(sid);
        app.cycle_model(); // gpt-5.6-sol → gpt-5.6-terra
                           // Verify the ModelSwitch event was persisted to the store
        let events = app
            .session_store
            .as_ref()
            .unwrap()
            .load_events(sid)
            .unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            crate::events::DisplayEvent::ModelSwitch { model } => {
                assert_eq!(model, "gpt-5.6-terra")
            }
            other => panic!("expected ModelSwitch, got {:?}", other),
        }
    }

    // ── Backend availability gating ──

    /// Verifies that available models both available.
    #[test]
    fn test_available_models_both_available() {
        let app = app_default();
        assert_eq!(app.available_models().len(), all_models_vec().len());
    }

    /// Verifies that available models Codex unavailable.
    #[test]
    fn test_available_models_codex_unavailable() {
        let mut app = app_default();
        app.codex_available = false;
        let models = app.available_models();
        assert_eq!(models, CLAUDE_MODELS.to_vec());
    }

    /// Verifies that available models Claude unavailable.
    #[test]
    fn test_available_models_claude_unavailable() {
        let mut app = app_default();
        app.claude_available = false;
        let models = app.available_models();
        assert_eq!(models, OPENAI_FRONTIER_MODELS.to_vec());
    }

    /// Verifies that available models neither falls back to all.
    #[test]
    fn test_available_models_neither_falls_back_to_all() {
        let mut app = app_default();
        app.claude_available = false;
        app.codex_available = false;
        assert_eq!(app.available_models().len(), all_models_vec().len());
    }

    /// Verifies that cycle skips Codex when unavailable.
    #[test]
    fn test_cycle_skips_codex_when_unavailable() {
        let mut app = app_default();
        app.codex_available = false;
        // Default gpt-5.6-sol is unavailable, so cycling uses the Claude-only pool.
        app.cycle_model();
        assert_eq!(app.display_model_name(), "sonnet");
        app.cycle_model();
        assert_eq!(app.display_model_name(), "haiku");
        app.cycle_model();
        assert_eq!(app.display_model_name(), "opus");
        assert_eq!(app.backend, Backend::Claude);
    }

    /// Verifies that cycle skips Claude when unavailable.
    #[test]
    fn test_cycle_skips_claude_when_unavailable() {
        let mut app = app_default();
        app.claude_available = false;
        app.selected_model = Some(first_codex_model().unwrap().to_string());
        app.backend = Backend::Codex;
        // Should cycle only through Codex models and never land on Claude
        for _ in 0..OPENAI_FRONTIER_MODELS.len() {
            app.cycle_model();
            assert!(app.display_model_name().starts_with("gpt-"));
            assert_eq!(app.backend, Backend::Codex);
        }
    }

    /// Verifies that first available model defaults GPT-5.6 sol.
    #[test]
    fn test_first_available_model_defaults_gpt_5_6_sol() {
        let app = app_default();
        assert_eq!(app.first_available_model(), "gpt-5.6-sol");
    }

    /// Verifies that first available model Claude unavailable.
    #[test]
    fn test_first_available_model_claude_unavailable() {
        let mut app = app_default();
        app.claude_available = false;
        assert_eq!(app.first_available_model(), first_codex_model().unwrap());
    }

    // ── display_model_name ──

    /// Verifies that display model none defaults GPT-5.6 sol.
    #[test]
    fn test_display_model_none_defaults_gpt_5_6_sol() {
        let mut app = App::new();
        app.selected_model = None;
        assert_eq!(app.display_model_name(), "gpt-5.6-sol");
    }

    /// Verifies that display model set value.
    #[test]
    fn test_display_model_set_value() {
        let mut app = App::new();
        app.selected_model = Some("gpt-5.4".to_string());
        assert_eq!(app.display_model_name(), "gpt-5.4");
    }

    // ── Unified model pool ──

    /// Verifies that all models count matches segments.
    #[test]
    fn test_all_models_count_matches_segments() {
        assert_eq!(
            all_models_vec().len(),
            CLAUDE_MODELS.len() + OPENAI_FRONTIER_MODELS.len()
        );
    }

    /// Verifies that all models contains default.
    #[test]
    fn test_all_models_contains_default() {
        assert!(all_models_vec().contains(&default_model()));
    }

    /// Verifies that all models Claude then Codex.
    #[test]
    fn test_all_models_claude_then_codex() {
        for &m in CLAUDE_MODELS {
            assert_eq!(backend_for_model(m), Backend::Claude);
        }
        for &m in OPENAI_FRONTIER_MODELS {
            assert_eq!(backend_for_model(m), Backend::Codex);
        }
    }

    /// Verifies that model color Claude.
    #[test]
    fn test_model_color_claude() {
        assert_eq!(model_color("sonnet"), Color::Cyan);
    }

    /// Verifies that model color frontier family.
    #[test]
    fn test_model_color_frontier_family() {
        assert_eq!(model_color("gpt-5.6-sol"), Color::LightCyan);
        assert_eq!(model_color("gpt-5.5"), Color::Green);
        assert_eq!(model_color("gpt-5.4-mini"), Color::LightGreen);
        assert_eq!(model_color("gpt-5"), Color::LightBlue);
        assert_eq!(model_color("gpt-4.1"), Color::Blue);
    }

    /// Verifies that model color unknown.
    #[test]
    fn test_model_color_unknown() {
        assert_eq!(model_color("x"), Color::DarkGray);
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

    /// Verifies that token badge none without store.
    #[test]
    fn test_token_badge_none_without_store() {
        let mut app = App::new();
        app.update_token_badge();
        assert!(app.token_badge_cache.is_none());
    }

    /// Verifies that token badge none without session id.
    #[test]
    fn test_token_badge_none_without_session_id() {
        use crate::app::session_store::SessionStore;
        let mut app = App::new();
        app.session_store = Some(SessionStore::open_memory().unwrap());
        app.current_session_id = None;
        app.update_token_badge();
        assert!(app.token_badge_cache.is_none());
    }

    /// Verifies that token badge green low usage.
    #[test]
    fn test_token_badge_green_low_usage() {
        // 100k chars out of 400k = 25%
        let mut app = app_with_store_chars(100_000);
        app.update_token_badge();
        let (text, color) = app.token_badge_cache.unwrap();
        assert_eq!(color, ratatui::style::Color::Green);
        assert!(text.contains("25"));
    }

    /// Verifies that token badge yellow medium usage.
    #[test]
    fn test_token_badge_yellow_medium_usage() {
        // 280k chars out of 400k = 70%
        let mut app = app_with_store_chars(280_000);
        app.update_token_badge();
        let (_, color) = app.token_badge_cache.unwrap();
        assert_eq!(color, ratatui::style::Color::Yellow);
    }

    /// Verifies that token badge red high usage.
    #[test]
    fn test_token_badge_red_high_usage() {
        // 380k chars out of 400k = 95%
        let mut app = app_with_store_chars(380_000);
        app.update_token_badge();
        let (_, color) = app.token_badge_cache.unwrap();
        assert_eq!(color, ratatui::style::Color::Red);
        assert!(app.context_pct_high);
    }

    /// Verifies that token badge capped at 100.
    #[test]
    fn test_token_badge_capped_at_100() {
        // 500k chars out of 400k — should cap at 100%
        let mut app = app_with_store_chars(500_000);
        app.update_token_badge();
        let (text, color) = app.token_badge_cache.unwrap();
        assert_eq!(color, ratatui::style::Color::Red);
        assert!(text.contains("100"));
    }

    /// Verifies that token badge zero chars.
    #[test]
    fn test_token_badge_zero_chars() {
        let mut app = app_with_store_chars(0);
        app.update_token_badge();
        let (text, _) = app.token_badge_cache.unwrap();
        assert!(text.contains("0"));
    }

    /// Verifies that token badge ignores session token metadata.
    #[test]
    fn test_token_badge_ignores_session_token_metadata() {
        let mut app = app_with_store_chars(500_000);

        app.update_token_badge();

        let (text, color) = app.token_badge_cache.unwrap();
        assert_eq!(color, ratatui::style::Color::Red);
        assert!(text.contains("100"));
    }

    /// Verifies that token badge compaction resets pct.
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
        app.session_store
            .as_ref()
            .unwrap()
            .store_compaction(sid, max_seq, "summary")
            .unwrap();
        app.update_token_badge();
        assert!(!app.context_pct_high);
        let (text, color) = app.token_badge_cache.unwrap();
        assert!(text.contains("0"));
        assert_eq!(color, ratatui::style::Color::Green);
    }

    /// Verifies that token badge live does not double count loaded display events.
    #[test]
    fn test_token_badge_live_does_not_double_count_loaded_display_events() {
        use crate::events::DisplayEvent;

        let mut app = app_with_store_chars(200_000);
        app.display_events = vec![DisplayEvent::UserMessage {
            _uuid: String::new(),
            content: "x".repeat(200_000),
        }];

        app.update_token_badge();
        app.update_token_badge_live();

        let (text, color) = app.token_badge_cache.unwrap();
        assert!(text.contains("50"));
        assert_eq!(color, ratatui::style::Color::Green);
    }
}
