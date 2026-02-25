//! GitHub CLI (`gh`) wrapper for the AZUREAL++ panel.
//! All functions shell out to `gh` and parse JSON responses via serde_json.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::app::types::{GitHubIssue, GitHubPR};

/// Detect the upstream repo and fork owner for a project directory.
/// Returns `(upstream_repo, Option<fork_owner>)`.
/// If the repo is a fork, `upstream_repo` is the parent and `fork_owner` is the current owner.
/// If not a fork, `upstream_repo` is the current repo and `fork_owner` is None.
pub fn detect_repo_info(project_path: &Path) -> Result<(String, Option<String>)> {
    let output = Command::new("gh")
        .args(["repo", "view", "--json", "nameWithOwner,parent"])
        .current_dir(project_path)
        .output()
        .context("Failed to run gh repo view")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh repo view failed: {}", stderr.trim());
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .context("Failed to parse gh repo view JSON")?;

    let name_with_owner = json["nameWithOwner"]
        .as_str()
        .unwrap_or("")
        .to_string();

    // If parent exists, this is a fork
    if let Some(parent) = json.get("parent") {
        if let Some(parent_repo) = parent["nameWithOwner"].as_str() {
            let fork_owner = name_with_owner
                .split('/')
                .next()
                .unwrap_or("")
                .to_string();
            return Ok((parent_repo.to_string(), Some(fork_owner)));
        }
    }

    Ok((name_with_owner, None))
}

/// Fetch issues from a GitHub repo via `gh issue list`.
/// This is a blocking call — run in a background thread.
pub fn fetch_issues(repo: &str, include_closed: bool) -> Result<Vec<GitHubIssue>, String> {
    let state = if include_closed { "all" } else { "open" };
    let output = Command::new("gh")
        .args([
            "issue", "list",
            "--repo", repo,
            "--json", "number,title,state,labels,author,createdAt,body",
            "--limit", "50",
            "--state", state,
        ])
        .output()
        .map_err(|e| format!("Failed to run gh: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh issue list failed: {}", stderr.trim()));
    }

    let json: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Failed to parse issues JSON: {}", e))?;

    let issues = json.into_iter().map(|v| {
        let labels: Vec<String> = v["labels"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|l| l["name"].as_str().map(String::from)).collect())
            .unwrap_or_default();
        let author = v["author"]["login"].as_str().unwrap_or("").to_string();
        GitHubIssue {
            number: v["number"].as_u64().unwrap_or(0) as u32,
            title: v["title"].as_str().unwrap_or("").to_string(),
            state: v["state"].as_str().unwrap_or("OPEN").to_string(),
            labels,
            author,
            created_at: v["createdAt"].as_str().unwrap_or("").to_string(),
            body: v["body"].as_str().unwrap_or("").to_string(),
        }
    }).collect();

    Ok(issues)
}

/// Create a new issue on a GitHub repo. Returns the issue number.
pub fn create_issue(repo: &str, title: &str, body: &str) -> Result<u32, String> {
    let output = Command::new("gh")
        .args([
            "issue", "create",
            "--repo", repo,
            "--title", title,
            "--body", body,
        ])
        .output()
        .map_err(|e| format!("Failed to run gh: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh issue create failed: {}", stderr.trim()));
    }

    // gh outputs the issue URL on success — extract number from it
    let stdout = String::from_utf8_lossy(&output.stdout);
    let number = stdout.trim()
        .rsplit('/')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    Ok(number)
}

/// Open an issue in the default browser
pub fn open_issue_in_browser(repo: &str, number: u32) -> Result<(), String> {
    Command::new("gh")
        .args(["issue", "view", &number.to_string(), "--web", "--repo", repo])
        .output()
        .map_err(|e| format!("Failed to open issue: {}", e))?;
    Ok(())
}

/// Fetch PRs from a GitHub repo authored by the current user.
/// Blocking — run in a background thread.
pub fn fetch_prs(repo: &str) -> Result<Vec<GitHubPR>, String> {
    let output = Command::new("gh")
        .args([
            "pr", "list",
            "--repo", repo,
            "--author", "@me",
            "--json", "number,title,state,headRefName",
            "--limit", "30",
            "--state", "all",
        ])
        .output()
        .map_err(|e| format!("Failed to run gh: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh pr list failed: {}", stderr.trim()));
    }

    let json: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Failed to parse PRs JSON: {}", e))?;

    let prs = json.into_iter().map(|v| {
        GitHubPR {
            number: v["number"].as_u64().unwrap_or(0) as u32,
            title: v["title"].as_str().unwrap_or("").to_string(),
            state: v["state"].as_str().unwrap_or("OPEN").to_string(),
            head_branch: v["headRefName"].as_str().unwrap_or("").to_string(),
        }
    }).collect();

    Ok(prs)
}

/// Create a PR from a fork to the upstream repo. Returns the PR URL.
pub fn create_pr(upstream_repo: &str, head: &str, title: &str, body: &str) -> Result<String, String> {
    let output = Command::new("gh")
        .args([
            "pr", "create",
            "--repo", upstream_repo,
            "--head", head,
            "--title", title,
            "--body", body,
        ])
        .output()
        .map_err(|e| format!("Failed to run gh: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh pr create failed: {}", stderr.trim()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Open a PR in the default browser
pub fn open_pr_in_browser(repo: &str, number: u32) -> Result<(), String> {
    Command::new("gh")
        .args(["pr", "view", &number.to_string(), "--web", "--repo", repo])
        .output()
        .map_err(|e| format!("Failed to open PR: {}", e))?;
    Ok(())
}
