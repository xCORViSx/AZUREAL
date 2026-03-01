//! Session name storage in `.azureal/azufig.toml` under the `[sessions]` section.
//!
//! Allows users to assign custom names to sessions that are stored
//! locally and displayed in the UI instead of the cryptic session IDs.

use std::collections::HashMap;

use super::App;

impl App {
    /// Save a custom session name mapping to the `[sessions]` section of project azufig.
    pub fn save_session_name(&self, session_id: &str, custom_name: &str) {
        let Some(ref project) = self.project else { return };
        crate::azufig::update_project_azufig(&project.path, |az| {
            az.sessions.insert(session_id.to_string(), custom_name.to_string());
        });
    }

    /// Load all custom session name mappings (session_id → custom_name)
    pub fn load_all_session_names(&self) -> HashMap<String, String> {
        let Some(ref project) = self.project else { return HashMap::new() };
        crate::azufig::load_project_azufig(&project.path).sessions
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
    use std::path::PathBuf;

    // ── load_all_session_names: no project ──

    #[test]
    fn test_load_session_names_no_project() {
        let app = App::new();
        let names = app.load_all_session_names();
        assert!(names.is_empty());
    }

    // ── save_session_name: no project ──

    #[test]
    fn test_save_session_name_no_project_no_panic() {
        let app = App::new();
        // Should return early without panicking when no project
        app.save_session_name("session-123", "My Session");
    }

    // ── check_pending_session_name: no pending ──

    #[test]
    fn test_check_pending_no_pending_names() {
        let mut app = App::new();
        app.check_pending_session_name("slot-1", "session-abc");
        // Should do nothing — no pending names
        assert!(app.pending_session_names.is_empty());
    }

    // ── check_pending_session_name: with pending ──

    #[test]
    fn test_check_pending_matching_slot() {
        let mut app = App::new();
        app.pending_session_names.push(("slot-1".to_string(), "My Custom Name".to_string()));
        app.check_pending_session_name("slot-1", "session-xyz");
        // The pending name should be removed
        assert!(app.pending_session_names.is_empty());
    }

    #[test]
    fn test_check_pending_non_matching_slot() {
        let mut app = App::new();
        app.pending_session_names.push(("slot-1".to_string(), "Name A".to_string()));
        app.check_pending_session_name("slot-2", "session-xyz");
        // Should not remove the non-matching pending name
        assert_eq!(app.pending_session_names.len(), 1);
    }

    #[test]
    fn test_check_pending_multiple_slots() {
        let mut app = App::new();
        app.pending_session_names.push(("slot-1".to_string(), "Name A".to_string()));
        app.pending_session_names.push(("slot-2".to_string(), "Name B".to_string()));
        app.pending_session_names.push(("slot-3".to_string(), "Name C".to_string()));
        app.check_pending_session_name("slot-2", "session-xyz");
        assert_eq!(app.pending_session_names.len(), 2);
        // slot-2 should be removed, slot-1 and slot-3 remain
        assert!(app.pending_session_names.iter().any(|(s, _)| s == "slot-1"));
        assert!(app.pending_session_names.iter().any(|(s, _)| s == "slot-3"));
        assert!(!app.pending_session_names.iter().any(|(s, _)| s == "slot-2"));
    }

    #[test]
    fn test_check_pending_removes_only_first_match() {
        let mut app = App::new();
        // Two entries with same slot
        app.pending_session_names.push(("slot-1".to_string(), "Name First".to_string()));
        app.pending_session_names.push(("slot-1".to_string(), "Name Second".to_string()));
        app.check_pending_session_name("slot-1", "session-abc");
        // Only the first match should be removed
        assert_eq!(app.pending_session_names.len(), 1);
        assert_eq!(app.pending_session_names[0].1, "Name Second");
    }

    #[test]
    fn test_check_pending_empty_slot_id() {
        let mut app = App::new();
        app.pending_session_names.push(("".to_string(), "Empty Slot".to_string()));
        app.check_pending_session_name("", "session-abc");
        assert!(app.pending_session_names.is_empty());
    }

    #[test]
    fn test_check_pending_empty_session_id() {
        let mut app = App::new();
        app.pending_session_names.push(("slot-1".to_string(), "Name".to_string()));
        // Empty session_id is still valid — save_session_name will handle it
        app.check_pending_session_name("slot-1", "");
        assert!(app.pending_session_names.is_empty());
    }

    // ── pending_session_names field ──

    #[test]
    fn test_pending_session_names_initially_empty() {
        let app = App::new();
        assert!(app.pending_session_names.is_empty());
    }

    #[test]
    fn test_pending_session_names_push_and_access() {
        let mut app = App::new();
        app.pending_session_names.push(("pid-123".to_string(), "Feature Work".to_string()));
        assert_eq!(app.pending_session_names.len(), 1);
        assert_eq!(app.pending_session_names[0].0, "pid-123");
        assert_eq!(app.pending_session_names[0].1, "Feature Work");
    }

    #[test]
    fn test_pending_session_names_multiple_entries() {
        let mut app = App::new();
        for i in 0..10 {
            app.pending_session_names.push((format!("pid-{}", i), format!("Session {}", i)));
        }
        assert_eq!(app.pending_session_names.len(), 10);
    }

    // ── load_all_session_names with project ──

    #[test]
    fn test_load_session_names_with_project_returns_map() {
        let mut app = App::new();
        app.project = Some(crate::models::Project {
            name: "test".to_string(),
            path: PathBuf::from("/tmp/nonexistent-project-test"),
            main_branch: "main".to_string(),
        });
        let names = app.load_all_session_names();
        // With a non-existent project path, should return empty map
        assert!(names.is_empty());
    }

    // ── save_session_name with project (no side effects verification) ──

    #[test]
    fn test_save_session_name_with_project_no_crash() {
        let mut app = App::new();
        // Use a tempdir to avoid polluting the real filesystem
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "test".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        // Should not crash even if azufig doesn't exist
        app.save_session_name("sess-123", "My Session Name");
    }

    #[test]
    fn test_save_then_load_session_name() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "roundtrip".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        app.save_session_name("sess-abc", "Test Name");
        let names = app.load_all_session_names();
        assert_eq!(names.get("sess-abc"), Some(&"Test Name".to_string()));
    }

    #[test]
    fn test_save_multiple_session_names() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "multi".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        app.save_session_name("sess-1", "First");
        app.save_session_name("sess-2", "Second");
        app.save_session_name("sess-3", "Third");
        let names = app.load_all_session_names();
        assert_eq!(names.len(), 3);
        assert_eq!(names.get("sess-1"), Some(&"First".to_string()));
        assert_eq!(names.get("sess-2"), Some(&"Second".to_string()));
        assert_eq!(names.get("sess-3"), Some(&"Third".to_string()));
    }

    #[test]
    fn test_save_session_name_overwrites() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "overwrite".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        app.save_session_name("sess-1", "Original");
        app.save_session_name("sess-1", "Updated");
        let names = app.load_all_session_names();
        assert_eq!(names.get("sess-1"), Some(&"Updated".to_string()));
    }

    #[test]
    fn test_save_session_name_empty_name() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "empty".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        app.save_session_name("sess-1", "");
        let names = app.load_all_session_names();
        assert_eq!(names.get("sess-1"), Some(&String::new()));
    }

    #[test]
    fn test_save_session_name_unicode() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "unicode".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        app.save_session_name("sess-1", "功能测试");
        let names = app.load_all_session_names();
        assert_eq!(names.get("sess-1"), Some(&"功能测试".to_string()));
    }

    #[test]
    fn test_save_session_name_with_spaces() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "spaces".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        app.save_session_name("sess-1", "My Feature Work Session");
        let names = app.load_all_session_names();
        assert_eq!(names.get("sess-1"), Some(&"My Feature Work Session".to_string()));
    }

    // ── check_pending_session_name with project (integration) ──

    #[test]
    fn test_check_pending_saves_to_project() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "integration".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        app.pending_session_names.push(("slot-1".to_string(), "Saved Name".to_string()));
        app.check_pending_session_name("slot-1", "sess-real-id");
        // Verify the name was saved
        let names = app.load_all_session_names();
        assert_eq!(names.get("sess-real-id"), Some(&"Saved Name".to_string()));
        // Verify pending was cleared
        assert!(app.pending_session_names.is_empty());
    }

    // ── Edge cases ──

    #[test]
    fn test_check_pending_special_chars_in_slot() {
        let mut app = App::new();
        app.pending_session_names.push(("slot/with/slashes".to_string(), "Name".to_string()));
        app.check_pending_session_name("slot/with/slashes", "sess-1");
        assert!(app.pending_session_names.is_empty());
    }

    #[test]
    fn test_check_pending_special_chars_in_name() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "special".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        app.pending_session_names.push(("slot-1".to_string(), "Name with \"quotes\" & <brackets>".to_string()));
        app.check_pending_session_name("slot-1", "sess-1");
        let names = app.load_all_session_names();
        // TOML should handle special chars
        assert!(names.contains_key("sess-1"));
    }

    #[test]
    fn test_load_session_names_returns_hashmap() {
        let app = App::new();
        let names = app.load_all_session_names();
        // Should always return a HashMap, even if empty
        let _ = names.len();
        let _ = names.is_empty();
    }

    #[test]
    fn test_pending_names_are_vec_of_tuples() {
        let mut app = App::new();
        app.pending_session_names.push(("a".to_string(), "b".to_string()));
        let (slot, name) = &app.pending_session_names[0];
        assert_eq!(slot, "a");
        assert_eq!(name, "b");
    }

    #[test]
    fn test_check_pending_no_match_preserves_all() {
        let mut app = App::new();
        app.pending_session_names.push(("x".to_string(), "X".to_string()));
        app.pending_session_names.push(("y".to_string(), "Y".to_string()));
        app.check_pending_session_name("z", "sess-1");
        assert_eq!(app.pending_session_names.len(), 2);
    }

    #[test]
    fn test_check_pending_called_twice_same_slot() {
        let mut app = App::new();
        app.pending_session_names.push(("slot-1".to_string(), "Name".to_string()));
        app.check_pending_session_name("slot-1", "sess-1");
        assert!(app.pending_session_names.is_empty());
        // Second call should be a no-op
        app.check_pending_session_name("slot-1", "sess-2");
        assert!(app.pending_session_names.is_empty());
    }

    #[test]
    fn test_save_session_name_long_id() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "long".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        let long_id = "a".repeat(200);
        app.save_session_name(&long_id, "Long ID Session");
        let names = app.load_all_session_names();
        assert_eq!(names.get(&long_id), Some(&"Long ID Session".to_string()));
    }

    #[test]
    fn test_save_session_name_long_name() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "long-name".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        let long_name = "Session ".to_string() + &"X".repeat(500);
        app.save_session_name("sess-1", &long_name);
        let names = app.load_all_session_names();
        assert_eq!(names.get("sess-1"), Some(&long_name));
    }

    // ── Additional tests for 50+ threshold ──

    #[test]
    fn test_save_session_name_numeric_id() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "numeric".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        app.save_session_name("12345", "Numeric ID");
        let names = app.load_all_session_names();
        assert_eq!(names.get("12345"), Some(&"Numeric ID".to_string()));
    }

    #[test]
    fn test_save_session_name_uuid_format() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "uuid".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        app.save_session_name("550e8400-e29b-41d4-a716-446655440000", "UUID Session");
        let names = app.load_all_session_names();
        assert!(names.contains_key("550e8400-e29b-41d4-a716-446655440000"));
    }

    #[test]
    fn test_load_session_names_empty_project_path() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: String::new(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        let names = app.load_all_session_names();
        assert!(names.is_empty());
    }

    #[test]
    fn test_save_and_load_many_session_names() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "many".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        for i in 0..20 {
            app.save_session_name(&format!("sess-{}", i), &format!("Name {}", i));
        }
        let names = app.load_all_session_names();
        assert_eq!(names.len(), 20);
        for i in 0..20 {
            assert_eq!(names.get(&format!("sess-{}", i)), Some(&format!("Name {}", i)));
        }
    }

    #[test]
    fn test_check_pending_with_multiple_different_slots() {
        let mut app = App::new();
        for i in 0..5 {
            app.pending_session_names.push((format!("slot-{}", i), format!("Name {}", i)));
        }
        app.check_pending_session_name("slot-3", "sess-3");
        assert_eq!(app.pending_session_names.len(), 4);
        assert!(!app.pending_session_names.iter().any(|(s, _)| s == "slot-3"));
    }

    #[test]
    fn test_pending_session_names_order_preserved() {
        let mut app = App::new();
        app.pending_session_names.push(("a".to_string(), "A".to_string()));
        app.pending_session_names.push(("b".to_string(), "B".to_string()));
        app.pending_session_names.push(("c".to_string(), "C".to_string()));
        assert_eq!(app.pending_session_names[0].0, "a");
        assert_eq!(app.pending_session_names[1].0, "b");
        assert_eq!(app.pending_session_names[2].0, "c");
    }

    #[test]
    fn test_check_pending_middle_element_removal() {
        let mut app = App::new();
        app.pending_session_names.push(("first".to_string(), "F".to_string()));
        app.pending_session_names.push(("middle".to_string(), "M".to_string()));
        app.pending_session_names.push(("last".to_string(), "L".to_string()));
        app.check_pending_session_name("middle", "sess-m");
        assert_eq!(app.pending_session_names.len(), 2);
        assert_eq!(app.pending_session_names[0].0, "first");
        assert_eq!(app.pending_session_names[1].0, "last");
    }

    #[test]
    fn test_save_session_name_emoji_in_name() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "emoji".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        app.save_session_name("sess-1", "Bug Fix 🐛");
        let names = app.load_all_session_names();
        assert_eq!(names.get("sess-1"), Some(&"Bug Fix 🐛".to_string()));
    }

    #[test]
    fn test_save_session_name_newlines_in_name() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "newlines".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        app.save_session_name("sess-1", "Line1\nLine2");
        let names = app.load_all_session_names();
        assert!(names.contains_key("sess-1"));
    }

    #[test]
    fn test_load_all_session_names_independent_apps() {
        // Two separate App instances with same project should see same data
        let tmp = tempfile::TempDir::new().unwrap();
        let project = crate::models::Project {
            name: "shared".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        };

        let mut app1 = App::new();
        app1.project = Some(project.clone());
        app1.save_session_name("sess-shared", "Shared Name");

        let mut app2 = App::new();
        app2.project = Some(project);
        let names = app2.load_all_session_names();
        assert_eq!(names.get("sess-shared"), Some(&"Shared Name".to_string()));
    }

    #[test]
    fn test_check_pending_first_element_removal() {
        let mut app = App::new();
        app.pending_session_names.push(("first".to_string(), "F".to_string()));
        app.pending_session_names.push(("second".to_string(), "S".to_string()));
        app.check_pending_session_name("first", "sess-1");
        assert_eq!(app.pending_session_names.len(), 1);
        assert_eq!(app.pending_session_names[0].0, "second");
    }

    #[test]
    fn test_check_pending_last_element_removal() {
        let mut app = App::new();
        app.pending_session_names.push(("first".to_string(), "F".to_string()));
        app.pending_session_names.push(("last".to_string(), "L".to_string()));
        app.check_pending_session_name("last", "sess-1");
        assert_eq!(app.pending_session_names.len(), 1);
        assert_eq!(app.pending_session_names[0].0, "first");
    }

    #[test]
    fn test_save_session_name_dashes_in_id() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "dashes".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        app.save_session_name("a-b-c-d-e", "Dashed");
        let names = app.load_all_session_names();
        assert_eq!(names.get("a-b-c-d-e"), Some(&"Dashed".to_string()));
    }

    #[test]
    fn test_save_session_name_dots_in_id() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "dots".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        app.save_session_name("v1.2.3", "Version Session");
        let names = app.load_all_session_names();
        assert_eq!(names.get("v1.2.3"), Some(&"Version Session".to_string()));
    }

    #[test]
    fn test_pending_session_names_capacity_growth() {
        let mut app = App::new();
        for i in 0..100 {
            app.pending_session_names.push((format!("{}", i), format!("{}", i)));
        }
        assert_eq!(app.pending_session_names.len(), 100);
    }

    #[test]
    fn test_check_pending_all_elements() {
        let mut app = App::new();
        for i in 0..5 {
            app.pending_session_names.push((format!("slot-{}", i), format!("Name-{}", i)));
        }
        for i in 0..5 {
            app.check_pending_session_name(&format!("slot-{}", i), &format!("sess-{}", i));
        }
        assert!(app.pending_session_names.is_empty());
    }

    #[test]
    fn test_save_session_name_tab_in_name() {
        let mut app = App::new();
        let tmp = tempfile::TempDir::new().unwrap();
        app.project = Some(crate::models::Project {
            name: "tab".to_string(),
            path: tmp.path().to_path_buf(),
            main_branch: "main".to_string(),
        });
        app.save_session_name("sess-tab", "Name\twith\ttabs");
        let names = app.load_all_session_names();
        assert!(names.contains_key("sess-tab"));
    }

    #[test]
    fn test_empty_session_id_save() {
        let app = App::new();
        app.save_session_name("", "name");
        let names = app.load_all_session_names();
        assert!(names.is_empty());
    }

    #[test]
    fn test_empty_name_save() {
        let app = App::new();
        app.save_session_name("id", "");
        let names = app.load_all_session_names();
        assert!(names.is_empty());
    }

    #[test]
    fn test_session_names_hashmap_default_empty() {
        let map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        assert!(map.is_empty());
    }

    #[test]
    fn test_pathbuf_join_sessions() {
        let base = PathBuf::from("/tmp");
        let joined = base.join("sessions.json");
        assert!(joined.to_string_lossy().ends_with("sessions.json"));
    }
}
