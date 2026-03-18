//! Git diff operations
//!
//! Diff queries: branch diff, per-file stats, single-file diff, commit diff.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::models::DiffInfo;

use super::Git;

impl Git {
    /// Get the diff between worktree and main branch
    pub fn get_diff(worktree_path: &Path, main_branch: &str) -> Result<DiffInfo> {
        let diff_output = Command::new("git")
            .args(["diff", &format!("{}...HEAD", main_branch)])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get git diff")?;

        let diff_text = String::from_utf8_lossy(&diff_output.stdout).to_string();

        let stats_output = Command::new("git")
            .args(["diff", "--stat", &format!("{}...HEAD", main_branch)])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get git diff stats")?;

        let stats_text = String::from_utf8_lossy(&stats_output.stdout);

        let mut files_changed = Vec::new();
        let mut additions = 0;
        let mut deletions = 0;

        for line in stats_text.lines() {
            if line.contains('|') {
                if let Some(file) = line.split('|').next() {
                    files_changed.push(file.trim().to_string());
                }
            } else if line.contains("insertion") || line.contains("deletion") {
                for part in line.split(',') {
                    let part = part.trim();
                    if part.contains("insertion") {
                        if let Some(num) = part.split_whitespace().next() {
                            additions = num.parse().unwrap_or(0);
                        }
                    } else if part.contains("deletion") {
                        if let Some(num) = part.split_whitespace().next() {
                            deletions = num.parse().unwrap_or(0);
                        }
                    }
                }
            }
        }

        Ok(DiffInfo {
            session_id: String::new(),
            diff_text,
            files_changed,
            additions,
            deletions,
            timestamp: chrono::Utc::now(),
        })
    }

    /// Get per-file diff stats against main branch.
    /// Returns Vec<(path, status_char, additions, deletions, staged)> by combining
    /// `git diff --name-status` (M/A/D/R) with `git diff --numstat` (+/-).
    /// The `staged` bool is true if the file has staged changes (in the index).
    pub fn get_diff_files(
        worktree_path: &Path,
        _main_branch: &str,
    ) -> Result<Vec<(String, char, usize, usize, bool)>> {
        // Show working tree changes (staged + unstaged) — this is what the user
        // is actively working on. Uses `git diff HEAD` to compare working tree
        // against last commit, capturing both staged and unstaged modifications.
        // Untracked files added separately via `git ls-files --others --exclude-standard`.

        // M\tpath — status of each changed file vs HEAD
        let status_out = Command::new("git")
            .args(["diff", "HEAD", "--name-status"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get diff name-status")?;
        let status_text = String::from_utf8_lossy(&status_out.stdout);

        // add\tdel\tpath — line-level stats for each changed file
        let numstat_out = Command::new("git")
            .args(["diff", "HEAD", "--numstat"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get diff numstat")?;
        let numstat_text = String::from_utf8_lossy(&numstat_out.stdout);

        // Build path → (additions, deletions) lookup from numstat
        let mut stats: std::collections::HashMap<String, (usize, usize)> =
            std::collections::HashMap::new();
        for line in numstat_text.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                let add = parts[0].parse().unwrap_or(0);
                let del = parts[1].parse().unwrap_or(0);
                stats.insert(parts[2].to_string(), (add, del));
            }
        }

        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for line in status_text.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                let status = parts[0].chars().next().unwrap_or('M');
                let path = parts.last().unwrap().to_string();
                let (add, del) = stats.get(&path).copied().unwrap_or((0, 0));
                seen.insert(path.clone());
                // Default all files to staged=true; user unstages explicitly via UI
                result.push((path, status, add, del, true));
            }
        }

        // Also pick up untracked files (shown as '?' status, 0/0 stats, never staged)
        let untracked_out = Command::new("git")
            .args(["ls-files", "--others", "--exclude-standard"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to list untracked files")?;
        for line in String::from_utf8_lossy(&untracked_out.stdout).lines() {
            let path = line.trim().to_string();
            if !path.is_empty() && !seen.contains(&path) {
                result.push((path, '?', 0, 0, true));
            }
        }

        // Filter out gitignored files — tracked files in .gitignore still appear
        // in `git diff HEAD` but are noise the user doesn't want to see
        if !result.is_empty() {
            let paths: Vec<&str> = result.iter().map(|(p, ..)| p.as_str()).collect();
            let mut child = Command::new("git")
                .args(["check-ignore", "--no-index", "--stdin"])
                .current_dir(worktree_path)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .context("Failed to spawn git check-ignore")?;
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = stdin.write_all(paths.join("\n").as_bytes());
            }
            let ignore_out = child
                .wait_with_output()
                .context("git check-ignore failed")?;
            let ignored: std::collections::HashSet<&str> = std::str::from_utf8(&ignore_out.stdout)
                .unwrap_or("")
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .collect();
            result.retain(|(path, ..)| !ignored.contains(path.as_str()));
        }

        Ok(result)
    }

    /// Get the diff for a single file (working tree vs HEAD, for viewer display)
    pub fn get_file_diff(
        worktree_path: &Path,
        _main_branch: &str,
        file_path: &str,
    ) -> Result<String> {
        let output = Command::new("git")
            .args(["diff", "HEAD", "--", file_path])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get file diff")?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get full diff for a single commit (for the viewer pane in Git panel)
    pub fn get_commit_diff(worktree_path: &Path, hash: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["show", hash, "--stat", "--patch"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to get commit diff")?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}
