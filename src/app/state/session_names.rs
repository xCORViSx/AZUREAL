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
