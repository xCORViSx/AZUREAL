//! Session name storage in .azureal/sessions.toml
//!
//! Allows users to assign custom names to sessions that are stored
//! locally and displayed in the UI instead of the cryptic session IDs.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::App;

impl App {
    /// Save a custom session name mapping
    pub fn save_session_name(&self, session_id: &str, custom_name: &str) {
        let Some(ref project) = self.project else { return };
        let azureal_dir = project.path.join(".azureal");
        let _ = fs::create_dir_all(&azureal_dir);

        let sessions_path = azureal_dir.join("sessions.toml");
        let mut names = load_session_names(&sessions_path);
        names.insert(session_id.to_string(), custom_name.to_string());

        if let Ok(content) = toml::to_string_pretty(&names) {
            let _ = fs::write(&sessions_path, content);
        }
    }

    /// Get the custom name for a session ID (if one exists)
    pub fn get_session_name(&self, session_id: &str) -> Option<String> {
        let project = self.project.as_ref()?;
        let sessions_path = project.path.join(".azureal").join("sessions.toml");
        let names = load_session_names(&sessions_path);
        names.get(session_id).cloned()
    }

    /// Check if there's a pending session name to save and save it
    pub fn check_pending_session_name(&mut self, branch_name: &str, session_id: &str) {
        if let Some((pending_branch, custom_name)) = self.pending_session_name.take() {
            if pending_branch == branch_name {
                self.save_session_name(session_id, &custom_name);
            } else {
                // Put it back if it's for a different branch
                self.pending_session_name = Some((pending_branch, custom_name));
            }
        }
    }
}

/// Load session names from TOML file
fn load_session_names(path: &Path) -> HashMap<String, String> {
    if !path.exists() {
        return HashMap::new();
    }

    fs::read_to_string(path)
        .ok()
        .and_then(|content| toml::from_str(&content).ok())
        .unwrap_or_default()
}
