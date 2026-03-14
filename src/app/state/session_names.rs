//! Session name storage via SQLite session store.
//!
//! Allows users to assign custom names to sessions that are stored
//! locally and displayed in the UI instead of the S-number IDs.

use std::collections::HashMap;

use super::App;

impl App {
    /// Save a custom session name. For store sessions (numeric IDs),
    /// saves directly to the SQLite store. Non-numeric IDs are ignored
    /// (legacy UUID sessions no longer support renaming after migration).
    pub fn save_session_name(&self, session_id: &str, custom_name: &str) {
        if let Ok(id) = session_id.parse::<i64>() {
            if let Some(ref store) = self.session_store {
                let _ = store.rename_session(id, custom_name);
            }
        }
    }

    /// Load all session display names (session_id string → display_name).
    pub fn load_all_session_names(&self) -> HashMap<String, String> {
        let mut names = HashMap::new();
        if let Some(ref store) = self.session_store {
            for (id, display) in store.load_all_session_names() {
                names.insert(id.to_string(), display);
            }
        }
        names
    }

    /// Check if there's a pending session name for this slot and save it.
    /// slot_id is the PID string — each concurrent spawn can register its own name.
    pub fn check_pending_session_name(&mut self, slot_id: &str, session_id: &str) {
        if let Some(idx) = self.pending_session_names.iter().position(|(s, _)| s == slot_id) {
            let (_, custom_name) = self.pending_session_names.remove(idx);
            self.save_session_name(session_id, &custom_name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::session_store::SessionStore;

    /// Helper: create an App with a store backed by an in-memory SQLite database.
    fn app_with_store() -> App {
        let mut app = App::new();
        app.session_store = Some(SessionStore::open_memory().unwrap());
        app
    }

    /// Helper: create a store session and return its numeric ID as a string.
    fn create_session(app: &App, worktree: &str) -> String {
        let store = app.session_store.as_ref().unwrap();
        let id = store.create_session(worktree).unwrap();
        id.to_string()
    }

    // ── load_all_session_names: no store ──

    #[test]
    fn load_session_names_no_store() {
        let app = App::new();
        assert!(app.load_all_session_names().is_empty());
    }

    // ── save_session_name: no store / non-numeric ──

    #[test]
    fn save_no_store_no_panic() {
        let app = App::new();
        app.save_session_name("1", "My Session");
    }

    #[test]
    fn save_non_numeric_id_ignored() {
        let app = app_with_store();
        app.save_session_name("uuid-abc", "Should Not Save");
        assert!(app.load_all_session_names().is_empty());
    }

    // ── save + load round-trip ──

    #[test]
    fn save_then_load() {
        let app = app_with_store();
        let id = create_session(&app, "main");
        app.save_session_name(&id, "Test Name");
        let names = app.load_all_session_names();
        assert_eq!(names.get(&id), Some(&"Test Name".to_string()));
    }

    #[test]
    fn save_multiple() {
        let app = app_with_store();
        let id1 = create_session(&app, "main");
        let id2 = create_session(&app, "feat");
        let id3 = create_session(&app, "fix");
        app.save_session_name(&id1, "First");
        app.save_session_name(&id2, "Second");
        app.save_session_name(&id3, "Third");
        let names = app.load_all_session_names();
        assert_eq!(names.len(), 3);
        assert_eq!(names.get(&id1), Some(&"First".to_string()));
        assert_eq!(names.get(&id2), Some(&"Second".to_string()));
        assert_eq!(names.get(&id3), Some(&"Third".to_string()));
    }

    #[test]
    fn save_overwrites() {
        let app = app_with_store();
        let id = create_session(&app, "main");
        app.save_session_name(&id, "Original");
        app.save_session_name(&id, "Updated");
        let names = app.load_all_session_names();
        assert_eq!(names.get(&id), Some(&"Updated".to_string()));
    }

    #[test]
    fn save_empty_name_clears() {
        let app = app_with_store();
        let id = create_session(&app, "main");
        app.save_session_name(&id, "");
        let names = app.load_all_session_names();
        // Empty name → store returns S-number default
        assert_eq!(names.get(&id), Some(&format!("S{id}")));
    }

    #[test]
    fn save_unicode_name() {
        let app = app_with_store();
        let id = create_session(&app, "main");
        app.save_session_name(&id, "功能测试");
        let names = app.load_all_session_names();
        assert_eq!(names.get(&id), Some(&"功能测试".to_string()));
    }

    #[test]
    fn save_name_with_spaces() {
        let app = app_with_store();
        let id = create_session(&app, "main");
        app.save_session_name(&id, "My Feature Work Session");
        let names = app.load_all_session_names();
        assert_eq!(names.get(&id), Some(&"My Feature Work Session".to_string()));
    }

    #[test]
    fn save_emoji_name() {
        let app = app_with_store();
        let id = create_session(&app, "main");
        app.save_session_name(&id, "Bug Fix 🐛");
        let names = app.load_all_session_names();
        assert_eq!(names.get(&id), Some(&"Bug Fix 🐛".to_string()));
    }

    #[test]
    fn save_newlines_in_name() {
        let app = app_with_store();
        let id = create_session(&app, "main");
        app.save_session_name(&id, "Line1\nLine2");
        let names = app.load_all_session_names();
        assert!(names.contains_key(&id));
    }

    #[test]
    fn save_long_name() {
        let app = app_with_store();
        let id = create_session(&app, "main");
        let long_name = "Session ".to_string() + &"X".repeat(500);
        app.save_session_name(&id, &long_name);
        let names = app.load_all_session_names();
        assert_eq!(names.get(&id), Some(&long_name));
    }

    #[test]
    fn save_tabs_in_name() {
        let app = app_with_store();
        let id = create_session(&app, "main");
        app.save_session_name(&id, "Name\twith\ttabs");
        let names = app.load_all_session_names();
        assert!(names.contains_key(&id));
    }

    #[test]
    fn save_special_chars_in_name() {
        let app = app_with_store();
        let id = create_session(&app, "main");
        app.save_session_name(&id, "Name with \"quotes\" & <brackets>");
        let names = app.load_all_session_names();
        assert!(names.contains_key(&id));
    }

    // ── unnamed sessions get S-number default ──

    #[test]
    fn unnamed_session_gets_s_number() {
        let app = app_with_store();
        let id = create_session(&app, "main");
        let names = app.load_all_session_names();
        assert_eq!(names.get(&id), Some(&format!("S{id}")));
    }

    // ── check_pending_session_name ──

    #[test]
    fn check_pending_no_pending() {
        let mut app = App::new();
        app.check_pending_session_name("slot-1", "1");
        assert!(app.pending_session_names.is_empty());
    }

    #[test]
    fn check_pending_matching_slot() {
        let mut app = App::new();
        app.pending_session_names.push(("slot-1".into(), "Custom".into()));
        app.check_pending_session_name("slot-1", "1");
        assert!(app.pending_session_names.is_empty());
    }

    #[test]
    fn check_pending_non_matching_slot() {
        let mut app = App::new();
        app.pending_session_names.push(("slot-1".into(), "Name".into()));
        app.check_pending_session_name("slot-2", "1");
        assert_eq!(app.pending_session_names.len(), 1);
    }

    #[test]
    fn check_pending_multiple_slots() {
        let mut app = App::new();
        app.pending_session_names.push(("slot-1".into(), "A".into()));
        app.pending_session_names.push(("slot-2".into(), "B".into()));
        app.pending_session_names.push(("slot-3".into(), "C".into()));
        app.check_pending_session_name("slot-2", "1");
        assert_eq!(app.pending_session_names.len(), 2);
        assert!(app.pending_session_names.iter().any(|(s, _)| s == "slot-1"));
        assert!(app.pending_session_names.iter().any(|(s, _)| s == "slot-3"));
        assert!(!app.pending_session_names.iter().any(|(s, _)| s == "slot-2"));
    }

    #[test]
    fn check_pending_removes_only_first_match() {
        let mut app = App::new();
        app.pending_session_names.push(("slot-1".into(), "First".into()));
        app.pending_session_names.push(("slot-1".into(), "Second".into()));
        app.check_pending_session_name("slot-1", "1");
        assert_eq!(app.pending_session_names.len(), 1);
        assert_eq!(app.pending_session_names[0].1, "Second");
    }

    #[test]
    fn check_pending_empty_slot_id() {
        let mut app = App::new();
        app.pending_session_names.push(("".into(), "Empty".into()));
        app.check_pending_session_name("", "1");
        assert!(app.pending_session_names.is_empty());
    }

    #[test]
    fn check_pending_saves_to_store() {
        let mut app = app_with_store();
        let id = create_session(&app, "main");
        app.pending_session_names.push(("slot-1".into(), "Saved Name".into()));
        app.check_pending_session_name("slot-1", &id);
        let names = app.load_all_session_names();
        assert_eq!(names.get(&id), Some(&"Saved Name".to_string()));
        assert!(app.pending_session_names.is_empty());
    }

    #[test]
    fn check_pending_called_twice_same_slot() {
        let mut app = App::new();
        app.pending_session_names.push(("slot-1".into(), "Name".into()));
        app.check_pending_session_name("slot-1", "1");
        assert!(app.pending_session_names.is_empty());
        app.check_pending_session_name("slot-1", "2");
        assert!(app.pending_session_names.is_empty());
    }

    #[test]
    fn check_pending_no_match_preserves_all() {
        let mut app = App::new();
        app.pending_session_names.push(("x".into(), "X".into()));
        app.pending_session_names.push(("y".into(), "Y".into()));
        app.check_pending_session_name("z", "1");
        assert_eq!(app.pending_session_names.len(), 2);
    }

    #[test]
    fn check_pending_first_element() {
        let mut app = App::new();
        app.pending_session_names.push(("first".into(), "F".into()));
        app.pending_session_names.push(("second".into(), "S".into()));
        app.check_pending_session_name("first", "1");
        assert_eq!(app.pending_session_names.len(), 1);
        assert_eq!(app.pending_session_names[0].0, "second");
    }

    #[test]
    fn check_pending_last_element() {
        let mut app = App::new();
        app.pending_session_names.push(("first".into(), "F".into()));
        app.pending_session_names.push(("last".into(), "L".into()));
        app.check_pending_session_name("last", "1");
        assert_eq!(app.pending_session_names.len(), 1);
        assert_eq!(app.pending_session_names[0].0, "first");
    }

    #[test]
    fn check_pending_middle_element() {
        let mut app = App::new();
        app.pending_session_names.push(("first".into(), "F".into()));
        app.pending_session_names.push(("middle".into(), "M".into()));
        app.pending_session_names.push(("last".into(), "L".into()));
        app.check_pending_session_name("middle", "1");
        assert_eq!(app.pending_session_names.len(), 2);
        assert_eq!(app.pending_session_names[0].0, "first");
        assert_eq!(app.pending_session_names[1].0, "last");
    }

    #[test]
    fn check_pending_all_elements() {
        let mut app = App::new();
        for i in 0..5 {
            app.pending_session_names.push((format!("slot-{i}"), format!("Name-{i}")));
        }
        for i in 0..5 {
            app.check_pending_session_name(&format!("slot-{i}"), &format!("{i}"));
        }
        assert!(app.pending_session_names.is_empty());
    }

    // ── pending_session_names field ──

    #[test]
    fn pending_initially_empty() {
        let app = App::new();
        assert!(app.pending_session_names.is_empty());
    }

    #[test]
    fn pending_push_and_access() {
        let mut app = App::new();
        app.pending_session_names.push(("pid-123".into(), "Feature Work".into()));
        assert_eq!(app.pending_session_names.len(), 1);
        assert_eq!(app.pending_session_names[0].0, "pid-123");
        assert_eq!(app.pending_session_names[0].1, "Feature Work");
    }

    #[test]
    fn pending_multiple_entries() {
        let mut app = App::new();
        for i in 0..10 {
            app.pending_session_names.push((format!("pid-{i}"), format!("Session {i}")));
        }
        assert_eq!(app.pending_session_names.len(), 10);
    }

    #[test]
    fn pending_order_preserved() {
        let mut app = App::new();
        app.pending_session_names.push(("a".into(), "A".into()));
        app.pending_session_names.push(("b".into(), "B".into()));
        app.pending_session_names.push(("c".into(), "C".into()));
        assert_eq!(app.pending_session_names[0].0, "a");
        assert_eq!(app.pending_session_names[1].0, "b");
        assert_eq!(app.pending_session_names[2].0, "c");
    }

    #[test]
    fn pending_capacity_growth() {
        let mut app = App::new();
        for i in 0..100 {
            app.pending_session_names.push((format!("{i}"), format!("{i}")));
        }
        assert_eq!(app.pending_session_names.len(), 100);
    }

    #[test]
    fn pending_are_vec_of_tuples() {
        let mut app = App::new();
        app.pending_session_names.push(("a".into(), "b".into()));
        let (slot, name) = &app.pending_session_names[0];
        assert_eq!(slot, "a");
        assert_eq!(name, "b");
    }

    // ── load returns hashmap ──

    #[test]
    fn load_returns_hashmap() {
        let app = App::new();
        let names = app.load_all_session_names();
        let _ = names.len();
        let _ = names.is_empty();
    }

    // ── many sessions ──

    #[test]
    fn save_and_load_many() {
        let app = app_with_store();
        let ids: Vec<String> = (0..20).map(|_| create_session(&app, "main")).collect();
        for (i, id) in ids.iter().enumerate() {
            app.save_session_name(id, &format!("Name {i}"));
        }
        let names = app.load_all_session_names();
        assert_eq!(names.len(), 20);
        for (i, id) in ids.iter().enumerate() {
            assert_eq!(names.get(id), Some(&format!("Name {i}")));
        }
    }

    #[test]
    fn check_pending_special_chars_in_slot() {
        let mut app = App::new();
        app.pending_session_names.push(("slot/with/slashes".into(), "Name".into()));
        app.check_pending_session_name("slot/with/slashes", "1");
        assert!(app.pending_session_names.is_empty());
    }

    #[test]
    fn check_pending_with_multiple_different_slots() {
        let mut app = App::new();
        for i in 0..5 {
            app.pending_session_names.push((format!("slot-{i}"), format!("Name {i}")));
        }
        app.check_pending_session_name("slot-3", "3");
        assert_eq!(app.pending_session_names.len(), 4);
        assert!(!app.pending_session_names.iter().any(|(s, _)| s == "slot-3"));
    }
}
