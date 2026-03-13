//! Deferred actions for two-phase draw pattern

use std::path::PathBuf;

/// Action to run after a loading indicator popup renders on-screen.
/// Part of the two-phase deferred draw pattern: set the loading message +
/// deferred action → draw renders the popup → event loop executes the action
/// after draw completes → triggers another draw to show the result.
pub enum DeferredAction {
    /// Load a session file from the session list overlay
    LoadSession { branch: String, idx: usize },
    /// Load a file into the viewer pane (from FileTree Enter/click)
    LoadFile { path: PathBuf },
    /// Open the Worktree Health panel (scans god files + documentation)
    OpenHealthPanel,
    /// Switch to a different project (kills processes, reloads everything)
    SwitchProject { path: PathBuf },
    /// Rescan all health features after scope mode changes
    RescanHealthScope { dirs: Vec<String> },
    /// Git commit from the commit overlay (message already captured)
    GitCommit { worktree: PathBuf, message: String },
    /// Git commit + push from the commit overlay
    GitCommitAndPush { worktree: PathBuf, message: String },
}
