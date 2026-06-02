//! Prompt context assembly
//!
//! Builds the hidden context wrapper for continuing prompts. SQLite remains the
//! source of truth, with the currently visible live tail appended when it has not
//! landed in the store yet.

use crate::app::session_store::{overlap_prefix_len, ContextPayload};
use crate::app::state::App;
use crate::events::DisplayEvent;

impl App {
    pub(crate) fn build_context_prompt_for_current_session(&self, prompt: &str) -> String {
        match self.context_payload_for_current_session() {
            Ok(Some(payload)) => {
                crate::app::context_injection::build_context_prompt(&payload, prompt)
            }
            Ok(None) | Err(_) => prompt.to_string(),
        }
    }

    fn context_payload_for_current_session(&self) -> anyhow::Result<Option<ContextPayload>> {
        let Some(session_id) = self.current_session_id else {
            return Ok(None);
        };
        let Some(store) = self.session_store.as_ref() else {
            return Ok(None);
        };

        let mut payload = match store.build_context(session_id)? {
            Some(payload) => payload,
            None => ContextPayload {
                compaction_summary: None,
                events: Vec::new(),
            },
        };

        if let Some(live_suffix) = self.live_context_suffix(session_id, store)? {
            payload.events.extend(live_suffix);
        }

        if payload.compaction_summary.is_none() && payload.events.is_empty() {
            return Ok(None);
        }
        Ok(Some(payload))
    }

    fn live_context_suffix(
        &self,
        session_id: i64,
        store: &crate::app::session_store::SessionStore,
    ) -> anyhow::Result<Option<Vec<DisplayEvent>>> {
        if self.display_events.is_empty() {
            return Ok(None);
        }

        let live_events = crate::app::context_injection::strip_injected_context_from_events(
            self.display_events.clone(),
        );
        if live_events.is_empty() {
            return Ok(None);
        }

        let stored_events = store.load_events(session_id)?;
        if stored_events.is_empty() {
            return Ok(Some(live_events));
        }

        let overlap = overlap_prefix_len(&stored_events, &live_events);
        if overlap == 0 || overlap >= live_events.len() {
            return Ok(None);
        }

        Ok(Some(live_events.into_iter().skip(overlap).collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::session_store::SessionStore;
    use tempfile::TempDir;

    fn user(content: &str) -> DisplayEvent {
        DisplayEvent::UserMessage {
            _uuid: String::new(),
            content: content.to_string(),
        }
    }

    fn assistant(text: &str) -> DisplayEvent {
        DisplayEvent::AssistantText {
            _uuid: String::new(),
            _message_id: String::new(),
            text: text.to_string(),
        }
    }

    #[test]
    fn context_prompt_appends_visible_tail_missing_from_store() {
        let temp = TempDir::new().unwrap();
        let store = SessionStore::open(temp.path()).unwrap();
        let sid = store.create_session("main").unwrap();
        store.append_events(sid, &[user("first")]).unwrap();

        let mut app = App::new();
        app.session_store = Some(store);
        app.session_store_path = Some(temp.path().to_path_buf());
        app.current_session_id = Some(sid);
        app.display_events = vec![user("first"), assistant("not yet stored")];

        let prompt = app.build_context_prompt_for_current_session("continue");

        assert!(prompt.contains("first"));
        assert!(prompt.contains("not yet stored"));
        assert!(prompt.contains("continue"));
    }

    #[test]
    fn context_prompt_does_not_duplicate_fully_stored_display_events() {
        let temp = TempDir::new().unwrap();
        let store = SessionStore::open(temp.path()).unwrap();
        let sid = store.create_session("main").unwrap();
        store
            .append_events(sid, &[user("first"), assistant("stored")])
            .unwrap();

        let mut app = App::new();
        app.session_store = Some(store);
        app.session_store_path = Some(temp.path().to_path_buf());
        app.current_session_id = Some(sid);
        app.display_events = vec![user("first"), assistant("stored")];

        let prompt = app.build_context_prompt_for_current_session("continue");

        assert_eq!(prompt.matches("stored").count(), 1);
    }
}
