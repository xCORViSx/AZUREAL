use crate::tui::util::AZURE;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A project represents a git repository (derived from current working directory)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    pub main_branch: String,
    /// Git branch prefix derived from the repo folder name (lowercase, sanitized).
    /// Branches are created as `{branch_prefix}/feature-name`.
    #[serde(default = "default_branch_prefix")]
    pub branch_prefix: String,
}

fn default_branch_prefix() -> String {
    "azureal".to_string()
}

impl Project {
    /// Create a project from a git repo path.
    /// Uses display_name if provided, otherwise falls back to folder name.
    pub fn from_path(path: PathBuf, main_branch: String) -> Self {
        let name = crate::config::project_display_name(&path).unwrap_or_else(|| {
            path.file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "unnamed".to_string())
        });
        let branch_prefix = branch_prefix_for_path(&path);
        Self {
            name,
            path,
            main_branch,
            branch_prefix,
        }
    }

    pub fn worktrees_dir(&self) -> PathBuf {
        self.path.join("worktrees")
    }
}

/// Derive a git-safe branch prefix from the repo's remote origin name,
/// falling back to the folder name if no remote exists.
/// Lowercases and sanitizes for git branch naming.
/// Examples: "git@github.com:user/MyProject.git" → "myproject", folder "mp" ignored
pub fn branch_prefix_for_path(path: &Path) -> String {
    let raw = crate::config::repo_name_from_origin(path)
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| {
            path.file_name()
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_else(|| "project".to_string())
        });

    let mut result = String::new();
    let mut last_dash = false;
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            result.push(c);
            last_dash = false;
        } else if !last_dash && !result.is_empty() {
            result.push('-');
            last_dash = true;
        }
    }
    while result.ends_with('-') {
        result.pop();
    }

    if result.is_empty() {
        "project".to_string()
    } else {
        result
    }
}

/// Session status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorktreeStatus {
    Pending,
    Running,
    Waiting,
    Stopped,
    Completed,
    Failed,
}

impl WorktreeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorktreeStatus::Pending => "pending",
            WorktreeStatus::Running => "running",
            WorktreeStatus::Waiting => "waiting",
            WorktreeStatus::Stopped => "stopped",
            WorktreeStatus::Completed => "completed",
            WorktreeStatus::Failed => "failed",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            WorktreeStatus::Pending => "○",
            WorktreeStatus::Running => "●",
            WorktreeStatus::Waiting => "○",
            WorktreeStatus::Stopped => "◌",
            WorktreeStatus::Completed => "✓",
            WorktreeStatus::Failed => "✗",
        }
    }

    pub fn color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            WorktreeStatus::Pending => Color::Gray,
            WorktreeStatus::Running => Color::Green,
            WorktreeStatus::Waiting => Color::Yellow,
            WorktreeStatus::Stopped => Color::Gray,
            WorktreeStatus::Completed => AZURE,
            WorktreeStatus::Failed => Color::Red,
        }
    }
}

/// Strip the branch prefix from a full branch name for display.
/// Strips everything up to and including the first `/`.
/// Returns the original string unchanged if it has no `/`.
pub fn strip_branch_prefix(branch: &str) -> &str {
    match branch.find('/') {
        Some(idx) => &branch[idx + 1..],
        None => branch,
    }
}

/// A worktree represents a git worktree paired with an optional Claude session.
/// Derived from git worktrees + Claude session files (stateless).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    /// Full branch name (e.g., "{prefix}/feature-name")
    pub branch_name: String,
    /// Worktree path (None if archived - branch exists but no worktree)
    pub worktree_path: Option<PathBuf>,
    /// Claude CLI session ID for --resume (read from Claude's session file)
    pub claude_session_id: Option<String>,
    /// Whether this is an archived worktree (branch exists, no worktree dir)
    pub archived: bool,
}

impl Worktree {
    /// Display name (branch name without the prefix)
    pub fn name(&self) -> &str {
        strip_branch_prefix(&self.branch_name)
    }

    /// Worktree status (derived from runtime state, not stored).
    /// `is_running` = whether any Claude process is active on this branch.
    pub fn status(&self, is_running: bool) -> WorktreeStatus {
        if self.archived {
            WorktreeStatus::Stopped
        } else if is_running {
            WorktreeStatus::Running
        } else if self.claude_session_id.is_some() {
            WorktreeStatus::Waiting
        } else {
            WorktreeStatus::Pending
        }
    }
}

/// Output type for Claude events
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputType {
    Stdout,
    Stderr,
    System,
    Json,
    Error,
    Hook,
}

/// Git diff information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffInfo {
    pub session_id: String,
    pub diff_text: String,
    pub files_changed: Vec<String>,
    pub additions: i32,
    pub deletions: i32,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Result of a rebase operation
#[derive(Debug, Clone)]
pub enum RebaseResult {
    /// Rebase was aborted
    Aborted,
    /// Rebase failed with an error (message for future display)
    Failed(#[allow(dead_code)] String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── strip_branch_prefix ──

    #[test]
    fn test_strip_prefix_with_prefix() {
        assert_eq!(strip_branch_prefix("azureal/feature-name"), "feature-name");
    }

    #[test]
    fn test_strip_prefix_without_prefix() {
        assert_eq!(strip_branch_prefix("main"), "main");
    }

    #[test]
    fn test_strip_prefix_just_prefix() {
        // "azureal" alone has no slash, so no stripping
        assert_eq!(strip_branch_prefix("azureal"), "azureal");
    }

    #[test]
    fn test_strip_prefix_empty_string() {
        assert_eq!(strip_branch_prefix(""), "");
    }

    #[test]
    fn test_strip_prefix_nested_slashes() {
        assert_eq!(strip_branch_prefix("azureal/feature/sub"), "feature/sub");
    }

    #[test]
    fn test_strip_prefix_different_prefix() {
        // Any prefix is stripped (strips at first /)
        assert_eq!(strip_branch_prefix("other/feature"), "feature");
    }

    // ── WorktreeStatus::as_str ──

    #[test]
    fn test_status_as_str() {
        assert_eq!(WorktreeStatus::Pending.as_str(), "pending");
        assert_eq!(WorktreeStatus::Running.as_str(), "running");
        assert_eq!(WorktreeStatus::Waiting.as_str(), "waiting");
        assert_eq!(WorktreeStatus::Stopped.as_str(), "stopped");
        assert_eq!(WorktreeStatus::Completed.as_str(), "completed");
        assert_eq!(WorktreeStatus::Failed.as_str(), "failed");
    }

    // ── WorktreeStatus::symbol ──

    #[test]
    fn test_status_symbol() {
        assert_eq!(WorktreeStatus::Pending.symbol(), "○");
        assert_eq!(WorktreeStatus::Running.symbol(), "●");
        assert_eq!(WorktreeStatus::Waiting.symbol(), "○");
        assert_eq!(WorktreeStatus::Stopped.symbol(), "◌");
        assert_eq!(WorktreeStatus::Completed.symbol(), "✓");
        assert_eq!(WorktreeStatus::Failed.symbol(), "✗");
    }

    // ── WorktreeStatus::color ──

    #[test]
    fn test_status_color() {
        use ratatui::style::Color;
        assert_eq!(WorktreeStatus::Pending.color(), Color::Gray);
        assert_eq!(WorktreeStatus::Running.color(), Color::Green);
        assert_eq!(WorktreeStatus::Waiting.color(), Color::Yellow);
        assert_eq!(WorktreeStatus::Stopped.color(), Color::Gray);
        assert_eq!(WorktreeStatus::Completed.color(), AZURE);
        assert_eq!(WorktreeStatus::Failed.color(), Color::Red);
    }

    // ── Worktree::name ──

    #[test]
    fn test_worktree_name_strips_prefix() {
        let wt = Worktree {
            branch_name: "azureal/my-feature".to_string(),
            worktree_path: None,
            claude_session_id: None,
            archived: false,
        };
        assert_eq!(wt.name(), "my-feature");
    }

    #[test]
    fn test_worktree_name_no_prefix() {
        let wt = Worktree {
            branch_name: "main".to_string(),
            worktree_path: None,
            claude_session_id: None,
            archived: false,
        };
        assert_eq!(wt.name(), "main");
    }

    // ── Worktree::status ──

    #[test]
    fn test_worktree_status_archived() {
        let wt = Worktree {
            branch_name: "azureal/old".to_string(),
            worktree_path: None,
            claude_session_id: Some("abc".to_string()),
            archived: true,
        };
        // archived takes precedence regardless of other fields
        assert_eq!(wt.status(true), WorktreeStatus::Stopped);
        assert_eq!(wt.status(false), WorktreeStatus::Stopped);
    }

    #[test]
    fn test_worktree_status_running() {
        let wt = Worktree {
            branch_name: "azureal/feat".to_string(),
            worktree_path: Some(PathBuf::from("/tmp/wt")),
            claude_session_id: Some("abc".to_string()),
            archived: false,
        };
        assert_eq!(wt.status(true), WorktreeStatus::Running);
    }

    #[test]
    fn test_worktree_status_waiting() {
        let wt = Worktree {
            branch_name: "azureal/feat".to_string(),
            worktree_path: Some(PathBuf::from("/tmp/wt")),
            claude_session_id: Some("abc".to_string()),
            archived: false,
        };
        assert_eq!(wt.status(false), WorktreeStatus::Waiting);
    }

    #[test]
    fn test_worktree_status_pending() {
        let wt = Worktree {
            branch_name: "azureal/new".to_string(),
            worktree_path: Some(PathBuf::from("/tmp/wt")),
            claude_session_id: None,
            archived: false,
        };
        assert_eq!(wt.status(false), WorktreeStatus::Pending);
    }

    // ── branch_prefix_for_path ──

    #[test]
    fn test_branch_prefix_from_path() {
        assert_eq!(
            branch_prefix_for_path(Path::new("/Users/me/AZUREAL")),
            "azureal"
        );
        assert_eq!(
            branch_prefix_for_path(Path::new("/home/user/My Project")),
            "my-project"
        );
        assert_eq!(
            branch_prefix_for_path(Path::new("/tmp/MyProject")),
            "myproject"
        );
        assert_eq!(branch_prefix_for_path(Path::new("/")), "project"); // fallback
        assert_eq!(
            branch_prefix_for_path(Path::new("/tmp/test-repo")),
            "test-repo"
        );
        assert_eq!(branch_prefix_for_path(Path::new("/tmp/123")), "123");
    }

    // ── strip_branch_prefix: more edge cases ──

    #[test]
    fn test_strip_prefix_just_prefix_with_slash() {
        // "azureal/" with nothing after → empty string
        assert_eq!(strip_branch_prefix("azureal/"), "");
    }

    #[test]
    fn test_strip_prefix_double_prefix() {
        // "azureal/azureal/x" → strips first prefix only
        assert_eq!(strip_branch_prefix("azureal/azureal/x"), "azureal/x");
    }

    #[test]
    fn test_strip_prefix_unicode_suffix() {
        assert_eq!(strip_branch_prefix("azureal/功能"), "功能");
    }

    #[test]
    fn test_strip_prefix_emoji_suffix() {
        assert_eq!(strip_branch_prefix("azureal/🚀launch"), "🚀launch");
    }

    #[test]
    fn test_strip_prefix_any_prefix() {
        // Any prefix before first / is stripped
        assert_eq!(strip_branch_prefix("azureal-extra/feat"), "feat");
        assert_eq!(strip_branch_prefix("myproject/clips"), "clips");
        assert_eq!(strip_branch_prefix("my-project/feature"), "feature");
    }

    #[test]
    fn test_strip_prefix_case_insensitive() {
        // All prefixes stripped regardless of case
        assert_eq!(strip_branch_prefix("Azureal/feat"), "feat");
        assert_eq!(strip_branch_prefix("AZUREAL/feat"), "feat");
    }

    #[test]
    fn test_strip_prefix_with_dots() {
        assert_eq!(strip_branch_prefix("azureal/v1.2.3"), "v1.2.3");
    }

    #[test]
    fn test_strip_prefix_with_dashes() {
        assert_eq!(
            strip_branch_prefix("azureal/my-cool-feature"),
            "my-cool-feature"
        );
    }

    #[test]
    fn test_strip_prefix_only_slash() {
        // "/" → empty string after the slash
        assert_eq!(strip_branch_prefix("/"), "");
    }

    #[test]
    fn test_strip_prefix_whitespace() {
        assert_eq!(strip_branch_prefix("azureal/ spaces"), " spaces");
    }

    // ── WorktreeStatus: trait verification ──

    #[test]
    fn test_worktree_status_partial_eq() {
        assert_eq!(WorktreeStatus::Pending, WorktreeStatus::Pending);
        assert_ne!(WorktreeStatus::Pending, WorktreeStatus::Running);
    }

    #[test]
    fn test_worktree_status_eq_all_variants() {
        let variants = [
            WorktreeStatus::Pending,
            WorktreeStatus::Running,
            WorktreeStatus::Waiting,
            WorktreeStatus::Stopped,
            WorktreeStatus::Completed,
            WorktreeStatus::Failed,
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn test_worktree_status_copy() {
        let status = WorktreeStatus::Running;
        let copy = status; // Copy trait
        assert_eq!(status, copy);
    }

    #[test]
    fn test_worktree_status_clone() {
        let status = WorktreeStatus::Failed;
        let cloned = status.clone();
        assert_eq!(status, cloned);
    }

    #[test]
    fn test_worktree_status_debug() {
        assert_eq!(format!("{:?}", WorktreeStatus::Pending), "Pending");
        assert_eq!(format!("{:?}", WorktreeStatus::Running), "Running");
        assert_eq!(format!("{:?}", WorktreeStatus::Waiting), "Waiting");
        assert_eq!(format!("{:?}", WorktreeStatus::Stopped), "Stopped");
        assert_eq!(format!("{:?}", WorktreeStatus::Completed), "Completed");
        assert_eq!(format!("{:?}", WorktreeStatus::Failed), "Failed");
    }

    // ── WorktreeStatus: serde ──

    #[test]
    fn test_worktree_status_serialize_all() {
        for status in [
            WorktreeStatus::Pending,
            WorktreeStatus::Running,
            WorktreeStatus::Waiting,
            WorktreeStatus::Stopped,
            WorktreeStatus::Completed,
            WorktreeStatus::Failed,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: WorktreeStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    // ── Worktree construction variations ──

    #[test]
    fn test_worktree_all_fields_populated() {
        let wt = Worktree {
            branch_name: "azureal/feature-x".to_string(),
            worktree_path: Some(PathBuf::from("/tmp/wt/feature-x")),
            claude_session_id: Some("sess-abc-123".to_string()),
            archived: false,
        };
        assert_eq!(wt.name(), "feature-x");
        assert_eq!(
            wt.worktree_path.as_deref(),
            Some(Path::new("/tmp/wt/feature-x"))
        );
        assert_eq!(wt.claude_session_id.as_deref(), Some("sess-abc-123"));
        assert!(!wt.archived);
    }

    #[test]
    fn test_worktree_no_path_no_session() {
        let wt = Worktree {
            branch_name: "azureal/orphan".to_string(),
            worktree_path: None,
            claude_session_id: None,
            archived: false,
        };
        assert!(wt.worktree_path.is_none());
        assert!(wt.claude_session_id.is_none());
    }

    #[test]
    fn test_worktree_archived_with_session() {
        let wt = Worktree {
            branch_name: "azureal/old-feature".to_string(),
            worktree_path: None,
            claude_session_id: Some("old-sess".to_string()),
            archived: true,
        };
        assert!(wt.archived);
        assert!(wt.claude_session_id.is_some());
    }

    #[test]
    fn test_worktree_clone() {
        let wt = Worktree {
            branch_name: "azureal/test".to_string(),
            worktree_path: Some(PathBuf::from("/wt")),
            claude_session_id: Some("id".to_string()),
            archived: false,
        };
        let cloned = wt.clone();
        assert_eq!(wt.branch_name, cloned.branch_name);
        assert_eq!(wt.worktree_path, cloned.worktree_path);
        assert_eq!(wt.claude_session_id, cloned.claude_session_id);
        assert_eq!(wt.archived, cloned.archived);
    }

    #[test]
    fn test_worktree_debug() {
        let wt = Worktree {
            branch_name: "azureal/dbg".to_string(),
            worktree_path: None,
            claude_session_id: None,
            archived: false,
        };
        let dbg = format!("{:?}", wt);
        assert!(dbg.contains("Worktree"));
        assert!(dbg.contains("azureal/dbg"));
    }

    // ── Worktree::status full truth table ──
    // 8 combinations of (archived, is_running, has_session_id)

    #[test]
    fn test_status_archived_false_running_false_session_none() {
        let wt = Worktree {
            branch_name: "b".to_string(),
            worktree_path: Some(PathBuf::from("/w")),
            claude_session_id: None,
            archived: false,
        };
        assert_eq!(wt.status(false), WorktreeStatus::Pending);
    }

    #[test]
    fn test_status_archived_false_running_false_session_some() {
        let wt = Worktree {
            branch_name: "b".to_string(),
            worktree_path: Some(PathBuf::from("/w")),
            claude_session_id: Some("s".to_string()),
            archived: false,
        };
        assert_eq!(wt.status(false), WorktreeStatus::Waiting);
    }

    #[test]
    fn test_status_archived_false_running_true_session_none() {
        let wt = Worktree {
            branch_name: "b".to_string(),
            worktree_path: Some(PathBuf::from("/w")),
            claude_session_id: None,
            archived: false,
        };
        assert_eq!(wt.status(true), WorktreeStatus::Running);
    }

    #[test]
    fn test_status_archived_false_running_true_session_some() {
        let wt = Worktree {
            branch_name: "b".to_string(),
            worktree_path: Some(PathBuf::from("/w")),
            claude_session_id: Some("s".to_string()),
            archived: false,
        };
        assert_eq!(wt.status(true), WorktreeStatus::Running);
    }

    #[test]
    fn test_status_archived_true_running_false_session_none() {
        let wt = Worktree {
            branch_name: "b".to_string(),
            worktree_path: None,
            claude_session_id: None,
            archived: true,
        };
        assert_eq!(wt.status(false), WorktreeStatus::Stopped);
    }

    #[test]
    fn test_status_archived_true_running_false_session_some() {
        let wt = Worktree {
            branch_name: "b".to_string(),
            worktree_path: None,
            claude_session_id: Some("s".to_string()),
            archived: true,
        };
        assert_eq!(wt.status(false), WorktreeStatus::Stopped);
    }

    #[test]
    fn test_status_archived_true_running_true_session_none() {
        let wt = Worktree {
            branch_name: "b".to_string(),
            worktree_path: None,
            claude_session_id: None,
            archived: true,
        };
        assert_eq!(wt.status(true), WorktreeStatus::Stopped);
    }

    #[test]
    fn test_status_archived_true_running_true_session_some() {
        let wt = Worktree {
            branch_name: "b".to_string(),
            worktree_path: None,
            claude_session_id: Some("s".to_string()),
            archived: true,
        };
        assert_eq!(wt.status(true), WorktreeStatus::Stopped);
    }

    // ── Worktree serde ──

    #[test]
    fn test_worktree_serialize_roundtrip() {
        let wt = Worktree {
            branch_name: "azureal/serde-test".to_string(),
            worktree_path: Some(PathBuf::from("/tmp/serde")),
            claude_session_id: Some("uuid-test".to_string()),
            archived: false,
        };
        let json = serde_json::to_string(&wt).unwrap();
        let parsed: Worktree = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.branch_name, wt.branch_name);
        assert_eq!(parsed.worktree_path, wt.worktree_path);
        assert_eq!(parsed.claude_session_id, wt.claude_session_id);
        assert_eq!(parsed.archived, wt.archived);
    }

    #[test]
    fn test_worktree_serialize_with_none_fields() {
        let wt = Worktree {
            branch_name: "main".to_string(),
            worktree_path: None,
            claude_session_id: None,
            archived: true,
        };
        let json = serde_json::to_string(&wt).unwrap();
        assert!(json.contains("null") || json.contains("\"worktree_path\":null"));
        let parsed: Worktree = serde_json::from_str(&json).unwrap();
        assert!(parsed.worktree_path.is_none());
        assert!(parsed.claude_session_id.is_none());
    }

    // ── DiffInfo ──

    #[test]
    fn test_diff_info_construction() {
        let diff = DiffInfo {
            session_id: "sess-1".to_string(),
            diff_text: "+added line\n-removed line".to_string(),
            files_changed: vec!["src/main.rs".to_string(), "Cargo.toml".to_string()],
            additions: 10,
            deletions: 3,
            timestamp: chrono::Utc::now(),
        };
        assert_eq!(diff.session_id, "sess-1");
        assert_eq!(diff.files_changed.len(), 2);
        assert_eq!(diff.additions, 10);
        assert_eq!(diff.deletions, 3);
    }

    #[test]
    fn test_diff_info_empty() {
        let diff = DiffInfo {
            session_id: String::new(),
            diff_text: String::new(),
            files_changed: vec![],
            additions: 0,
            deletions: 0,
            timestamp: chrono::Utc::now(),
        };
        assert!(diff.diff_text.is_empty());
        assert!(diff.files_changed.is_empty());
    }

    #[test]
    fn test_diff_info_clone() {
        let diff = DiffInfo {
            session_id: "id".to_string(),
            diff_text: "text".to_string(),
            files_changed: vec!["a.rs".to_string()],
            additions: 1,
            deletions: 1,
            timestamp: chrono::Utc::now(),
        };
        let cloned = diff.clone();
        assert_eq!(diff.session_id, cloned.session_id);
        assert_eq!(diff.additions, cloned.additions);
    }

    #[test]
    fn test_diff_info_serialize_roundtrip() {
        let diff = DiffInfo {
            session_id: "roundtrip".to_string(),
            diff_text: "+line".to_string(),
            files_changed: vec!["file.rs".to_string()],
            additions: 5,
            deletions: 2,
            timestamp: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&diff).unwrap();
        let parsed: DiffInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.session_id, "roundtrip");
        assert_eq!(parsed.additions, 5);
        assert_eq!(parsed.deletions, 2);
        assert_eq!(parsed.files_changed, vec!["file.rs"]);
    }

    // ── OutputType ──

    #[test]
    fn test_output_type_all_variants_exist() {
        let _stdout = OutputType::Stdout;
        let _stderr = OutputType::Stderr;
        let _system = OutputType::System;
        let _json = OutputType::Json;
        let _error = OutputType::Error;
        let _hook = OutputType::Hook;
    }

    #[test]
    fn test_output_type_partial_eq() {
        assert_eq!(OutputType::Stdout, OutputType::Stdout);
        assert_ne!(OutputType::Stdout, OutputType::Stderr);
        assert_eq!(OutputType::Error, OutputType::Error);
    }

    #[test]
    fn test_output_type_eq_exhaustive() {
        let variants = [
            OutputType::Stdout,
            OutputType::Stderr,
            OutputType::System,
            OutputType::Json,
            OutputType::Error,
            OutputType::Hook,
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn test_output_type_copy() {
        let ot = OutputType::Json;
        let ot2 = ot;
        assert_eq!(ot, ot2);
    }

    #[test]
    fn test_output_type_clone() {
        let ot = OutputType::Hook;
        let cloned = ot.clone();
        assert_eq!(ot, cloned);
    }

    #[test]
    fn test_output_type_serialize_roundtrip() {
        for variant in [
            OutputType::Stdout,
            OutputType::Stderr,
            OutputType::System,
            OutputType::Json,
            OutputType::Error,
            OutputType::Hook,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: OutputType = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn test_output_type_debug() {
        assert_eq!(format!("{:?}", OutputType::Stdout), "Stdout");
        assert_eq!(format!("{:?}", OutputType::Stderr), "Stderr");
        assert_eq!(format!("{:?}", OutputType::System), "System");
        assert_eq!(format!("{:?}", OutputType::Json), "Json");
        assert_eq!(format!("{:?}", OutputType::Error), "Error");
        assert_eq!(format!("{:?}", OutputType::Hook), "Hook");
    }

    // ── RebaseResult ──

    #[test]
    fn test_rebase_result_aborted() {
        let result = RebaseResult::Aborted;
        assert!(matches!(result, RebaseResult::Aborted));
    }

    #[test]
    fn test_rebase_result_failed() {
        let result = RebaseResult::Failed("conflict in main.rs".to_string());
        assert!(matches!(result, RebaseResult::Failed(_)));
    }

    #[test]
    fn test_rebase_result_failed_message() {
        let result = RebaseResult::Failed("merge conflict".to_string());
        if let RebaseResult::Failed(msg) = result {
            assert_eq!(msg, "merge conflict");
        } else {
            panic!("expected Failed variant");
        }
    }

    #[test]
    fn test_rebase_result_failed_empty_message() {
        let result = RebaseResult::Failed(String::new());
        if let RebaseResult::Failed(msg) = result {
            assert!(msg.is_empty());
        } else {
            panic!("expected Failed variant");
        }
    }

    #[test]
    fn test_rebase_result_clone() {
        let result = RebaseResult::Failed("err".to_string());
        let cloned = result.clone();
        if let RebaseResult::Failed(msg) = cloned {
            assert_eq!(msg, "err");
        } else {
            panic!("expected Failed variant");
        }
    }

    #[test]
    fn test_rebase_result_debug() {
        let aborted = format!("{:?}", RebaseResult::Aborted);
        assert_eq!(aborted, "Aborted");

        let failed = format!("{:?}", RebaseResult::Failed("x".to_string()));
        assert!(failed.contains("Failed"));
        assert!(failed.contains("x"));
    }

    // ── Worktree::name edge cases ──

    #[test]
    fn test_worktree_name_empty_branch() {
        let wt = Worktree {
            branch_name: String::new(),
            worktree_path: None,
            claude_session_id: None,
            archived: false,
        };
        assert_eq!(wt.name(), "");
    }

    #[test]
    fn test_worktree_name_just_slash() {
        let wt = Worktree {
            branch_name: "/".to_string(),
            worktree_path: None,
            claude_session_id: None,
            archived: false,
        };
        assert_eq!(wt.name(), "");
    }

    // ── WorktreeStatus: as_str / symbol consistency ──

    #[test]
    fn test_status_as_str_is_lowercase() {
        for status in [
            WorktreeStatus::Pending,
            WorktreeStatus::Running,
            WorktreeStatus::Waiting,
            WorktreeStatus::Stopped,
            WorktreeStatus::Completed,
            WorktreeStatus::Failed,
        ] {
            let s = status.as_str();
            assert_eq!(
                s,
                s.to_lowercase(),
                "as_str for {:?} should be lowercase",
                status
            );
        }
    }

    #[test]
    fn test_status_symbol_is_non_empty() {
        for status in [
            WorktreeStatus::Pending,
            WorktreeStatus::Running,
            WorktreeStatus::Waiting,
            WorktreeStatus::Stopped,
            WorktreeStatus::Completed,
            WorktreeStatus::Failed,
        ] {
            assert!(
                !status.symbol().is_empty(),
                "symbol for {:?} should not be empty",
                status
            );
        }
    }
}
