//! Git operations for worktree management
//!
//! Split into focused submodules:
//! - `core`: Basic git operations (repo detection, branch info, diffs)
//! - `branch`: Branch listing and management
//! - `worktree`: Git worktree operations
//! - `rebase`: Rebase and conflict resolution

mod branch;
mod core;
mod rebase;
mod worktree;

pub use core::{Git, SquashMergeResult, WorktreeInfo};
