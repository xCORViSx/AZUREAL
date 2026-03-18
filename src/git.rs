//! Git operations for worktree management
//!
//! Split into focused submodules:
//! - `core`: Types (`Git`, `SquashMergeResult`, `WorktreeInfo`) + repo detection, branch info
//! - `branch`: Branch listing and management
//! - `commit`: Commit creation and log queries
//! - `diff`: Diff operations (branch, per-file, single-file, commit)
//! - `merge`: Squash-merge with conflict detection
//! - `rebase`: Rebase and conflict resolution
//! - `remote`: Push, pull, and divergence queries
//! - `staging`: Stage, unstage, discard, and gitignore cleanup
//! - `worktree`: Git worktree operations

mod branch;
mod commit;
mod core;
mod diff;
mod merge;
mod rebase;
mod remote;
mod staging;
mod worktree;

pub use core::{Git, SquashMergeResult, WorktreeInfo};
