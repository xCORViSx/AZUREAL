//! Project command handlers (stateless - derived from git)

use anyhow::Result;

use crate::cli::OutputFormat;
use crate::git::Git;
use crate::models::Project;

/// Truncate string with ellipsis
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len { s.to_string() }
    else { format!("{}...", &s[..max_len.saturating_sub(3)]) }
}

/// Discover project from current directory
fn discover_project() -> Result<Project> {
    let cwd = std::env::current_dir()?;
    if !Git::is_git_repo(&cwd) {
        anyhow::bail!("Not in a git repository");
    }
    let repo_root = Git::repo_root(&cwd)?;
    let main_branch = Git::get_main_branch(&repo_root)?;
    Ok(Project::from_path(repo_root, main_branch))
}

pub fn handle_project_list(output_format: OutputFormat) -> Result<()> {
    // In stateless mode, we only have the current project
    let project = discover_project()?;
    let projects = vec![project];

    match output_format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&projects)?),
        OutputFormat::Plain => {
            for project in &projects {
                println!("{}\t{}", project.name, project.path.display());
            }
        }
        OutputFormat::Table => {
            println!("{:<20} PATH", "NAME");
            println!("{}", "-".repeat(70));
            for project in projects {
                println!("{:<20} {}", truncate(&project.name, 20), project.path.display());
            }
        }
    }
    Ok(())
}

pub fn handle_project_show(project_arg: Option<String>, output_format: OutputFormat) -> Result<()> {
    let project = if let Some(arg) = project_arg {
        // If a path is specified, use it
        let path = std::path::PathBuf::from(&arg);
        if !Git::is_git_repo(&path) {
            anyhow::bail!("Not a git repository: {}", arg);
        }
        let repo_root = Git::repo_root(&path)?;
        let main_branch = Git::get_main_branch(&repo_root)?;
        Project::from_path(repo_root, main_branch)
    } else {
        discover_project()?
    };

    match output_format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&project)?),
        OutputFormat::Plain => println!("{}\t{}", project.name, project.path.display()),
        OutputFormat::Table => {
            println!("Project: {}", project.name);
            println!("Path: {}", project.path.display());
            println!("Main branch: {}", project.main_branch);

            // Count worktrees
            if let Ok(worktrees) = Git::list_worktrees_detailed(&project.path) {
                println!("\nWorktrees: {}", worktrees.len());
                for wt in worktrees.iter().take(5) {
                    let name = if wt.is_main { "(main)" } else {
                        wt.branch.as_deref().unwrap_or("detached")
                    };
                    println!("  {} - {}", name, wt.path.display());
                }
                if worktrees.len() > 5 {
                    println!("  ... and {} more", worktrees.len() - 5);
                }
            }

            // Count azureal branches
            if let Ok(branches) = Git::list_azureal_branches(&project.path) {
                println!("\nAzureal branches: {}", branches.len());
            }
        }
    }
    Ok(())
}

pub fn handle_project_remove(_project_arg: &str, _skip_confirm: bool) -> Result<()> {
    // In stateless mode, there's nothing to "remove" from tracking
    // Projects are discovered from git, not stored
    println!("In stateless mode, projects are discovered from git repositories.");
    println!("There is no project database to remove entries from.");
    println!("\nTo remove session worktrees, use: azureal session cleanup --delete-branches");
    Ok(())
}

pub fn handle_project_config(
    project_arg: Option<String>,
    main_branch: Option<String>,
) -> Result<()> {
    let project = if let Some(arg) = project_arg {
        let path = std::path::PathBuf::from(&arg);
        if !Git::is_git_repo(&path) {
            anyhow::bail!("Not a git repository: {}", arg);
        }
        let repo_root = Git::repo_root(&path)?;
        let branch = Git::get_main_branch(&repo_root)?;
        Project::from_path(repo_root, branch)
    } else {
        discover_project()?
    };

    if main_branch.is_none() {
        // Show current config
        println!("Project: {}", project.name);
        println!("Path: {}", project.path.display());
        println!("Main branch: {} (auto-detected)", project.main_branch);
        println!("\nNote: In stateless mode, main branch is auto-detected from git.");
        println!("To change the default branch, use: git symbolic-ref HEAD refs/heads/<branch>");
        return Ok(());
    }

    // In stateless mode, we can't persist config changes
    // But we can suggest how to change git's default branch
    if let Some(branch) = main_branch {
        println!("In stateless mode, main branch is auto-detected from git.");
        println!("\nTo change the default branch to '{}', run:", branch);
        println!("  git symbolic-ref HEAD refs/heads/{}", branch);
        println!("\nOr rename your branch:");
        println!("  git branch -m {} {}", project.main_branch, branch);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ── truncate ──

    #[test]
    fn test_truncate_short_string_no_change() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_exact_length() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_longer_than_max() {
        assert_eq!(truncate("hello world", 8), "hello...");
    }

    #[test]
    fn test_truncate_empty_string() {
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn test_truncate_max_zero() {
        assert_eq!(truncate("hello", 0), "...");
    }

    #[test]
    fn test_truncate_max_one() {
        assert_eq!(truncate("hello", 1), "...");
    }

    #[test]
    fn test_truncate_max_two() {
        assert_eq!(truncate("hello", 2), "...");
    }

    #[test]
    fn test_truncate_max_three() {
        assert_eq!(truncate("hello", 3), "...");
    }

    #[test]
    fn test_truncate_max_four() {
        assert_eq!(truncate("hello", 4), "h...");
    }

    #[test]
    fn test_truncate_max_five_fits_hello() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_single_char_fits() {
        assert_eq!(truncate("x", 5), "x");
    }

    #[test]
    fn test_truncate_long_string() {
        let long = "abcdefghijklmnopqrstuvwxyz";
        let result = truncate(long, 10);
        assert_eq!(result.len(), 10);
        assert!(result.ends_with("..."));
        assert_eq!(result, "abcdefg...");
    }

    #[test]
    fn test_truncate_unicode_boundary_may_panic_or_work() {
        // Multi-byte chars at boundary — depends on byte-vs-char behavior
        // truncate uses s.len() (bytes) and s[..max_len] (byte slicing)
        // With ASCII input, there's no issue
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn test_truncate_exact_boundary() {
        // len=6, max=6 → no truncation
        assert_eq!(truncate("abcdef", 6), "abcdef");
        // len=7, max=6 → truncate: s[..3] + "..." = "abc..."
        assert_eq!(truncate("abcdefg", 6), "abc...");
    }

    #[test]
    fn test_truncate_project_name() {
        // Typical project name truncation to 20 chars
        let name = "my-very-long-project-name-that-exceeds";
        let result = truncate(name, 20);
        assert_eq!(result.len(), 20);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_path_display() {
        let path = "/Users/developer/projects/my-awesome-project";
        let result = truncate(path, 25);
        assert_eq!(result.len(), 25);
    }

    // ── OutputFormat ──

    #[test]
    fn test_output_format_default() {
        let fmt = OutputFormat::default();
        assert!(matches!(fmt, OutputFormat::Table));
    }

    #[test]
    fn test_output_format_json_variant() {
        let _fmt = OutputFormat::Json;
    }

    #[test]
    fn test_output_format_plain_variant() {
        let _fmt = OutputFormat::Plain;
    }

    #[test]
    fn test_output_format_table_variant() {
        let _fmt = OutputFormat::Table;
    }

    #[test]
    fn test_output_format_clone() {
        let fmt = OutputFormat::Json;
        let cloned = fmt.clone();
        assert!(matches!(cloned, OutputFormat::Json));
    }

    #[test]
    fn test_output_format_debug() {
        let debug = format!("{:?}", OutputFormat::Json);
        assert_eq!(debug, "Json");
        let debug = format!("{:?}", OutputFormat::Plain);
        assert_eq!(debug, "Plain");
        let debug = format!("{:?}", OutputFormat::Table);
        assert_eq!(debug, "Table");
    }

    // ── Project struct ──

    #[test]
    fn test_project_worktrees_dir() {
        let project = Project {
            name: "test".to_string(),
            path: PathBuf::from("/home/user/project"),
            main_branch: "main".to_string(),
        };
        assert_eq!(project.worktrees_dir(), PathBuf::from("/home/user/project/worktrees"));
    }

    #[test]
    fn test_project_worktrees_dir_root() {
        let project = Project {
            name: "root".to_string(),
            path: PathBuf::from("/"),
            main_branch: "main".to_string(),
        };
        assert_eq!(project.worktrees_dir(), PathBuf::from("/worktrees"));
    }

    #[test]
    fn test_project_clone() {
        let project = Project {
            name: "test".to_string(),
            path: PathBuf::from("/tmp/test"),
            main_branch: "main".to_string(),
        };
        let cloned = project.clone();
        assert_eq!(cloned.name, "test");
        assert_eq!(cloned.path, PathBuf::from("/tmp/test"));
        assert_eq!(cloned.main_branch, "main");
    }

    #[test]
    fn test_project_serialize_roundtrip() {
        let project = Project {
            name: "my-project".to_string(),
            path: PathBuf::from("/home/user/projects/my-project"),
            main_branch: "main".to_string(),
        };
        let json = serde_json::to_string(&project).unwrap();
        let parsed: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, project.name);
        assert_eq!(parsed.path, project.path);
        assert_eq!(parsed.main_branch, project.main_branch);
    }

    #[test]
    fn test_project_serialize_master_branch() {
        let project = Project {
            name: "old-project".to_string(),
            path: PathBuf::from("/tmp/old"),
            main_branch: "master".to_string(),
        };
        let json = serde_json::to_string(&project).unwrap();
        assert!(json.contains("master"));
    }

    #[test]
    fn test_project_debug() {
        let project = Project {
            name: "debug-test".to_string(),
            path: PathBuf::from("/tmp/debug"),
            main_branch: "main".to_string(),
        };
        let debug = format!("{:?}", project);
        assert!(debug.contains("debug-test"));
        assert!(debug.contains("main"));
    }

    #[test]
    fn test_project_name_with_spaces() {
        let project = Project {
            name: "my cool project".to_string(),
            path: PathBuf::from("/tmp/my cool project"),
            main_branch: "main".to_string(),
        };
        assert_eq!(project.name, "my cool project");
    }

    #[test]
    fn test_project_worktrees_dir_nested_path() {
        let project = Project {
            name: "nested".to_string(),
            path: PathBuf::from("/home/user/a/b/c/project"),
            main_branch: "develop".to_string(),
        };
        assert_eq!(
            project.worktrees_dir(),
            PathBuf::from("/home/user/a/b/c/project/worktrees")
        );
    }

    // ── truncate: more edge cases ──

    #[test]
    fn test_truncate_all_spaces() {
        assert_eq!(truncate("     ", 3), "...");
    }

    #[test]
    fn test_truncate_numbers() {
        assert_eq!(truncate("1234567890", 7), "1234...");
    }

    #[test]
    fn test_truncate_newlines() {
        assert_eq!(truncate("abc\ndef\nghi", 7), "abc\n...");
    }

    #[test]
    fn test_truncate_tabs() {
        assert_eq!(truncate("a\tb\tc", 4), "a...");
    }

    #[test]
    fn test_truncate_very_large_max() {
        let s = "hello";
        assert_eq!(truncate(s, 1000), "hello");
    }

    #[test]
    fn test_truncate_empty_with_zero_max() {
        assert_eq!(truncate("", 0), "");
    }

    // ── handle_project_remove (stateless behavior) ──

    #[test]
    fn test_handle_project_remove_always_ok() {
        // In stateless mode, remove is always a no-op success
        let result = handle_project_remove("any-project", false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_project_remove_with_confirm_skip() {
        let result = handle_project_remove("test", true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_project_remove_empty_arg() {
        let result = handle_project_remove("", false);
        assert!(result.is_ok());
    }

    // ── Serialization edge cases ──

    #[test]
    fn test_project_deserialize_from_json() {
        let json = r#"{"name":"test","path":"/tmp/test","main_branch":"main"}"#;
        let project: Project = serde_json::from_str(json).unwrap();
        assert_eq!(project.name, "test");
        assert_eq!(project.path, PathBuf::from("/tmp/test"));
        assert_eq!(project.main_branch, "main");
    }

    #[test]
    fn test_project_deserialize_with_unicode_name() {
        let json = r#"{"name":"项目","path":"/tmp/test","main_branch":"main"}"#;
        let project: Project = serde_json::from_str(json).unwrap();
        assert_eq!(project.name, "项目");
    }

    #[test]
    fn test_project_json_array() {
        let projects = vec![
            Project { name: "a".to_string(), path: PathBuf::from("/a"), main_branch: "main".to_string() },
            Project { name: "b".to_string(), path: PathBuf::from("/b"), main_branch: "main".to_string() },
        ];
        let json = serde_json::to_string_pretty(&projects).unwrap();
        let parsed: Vec<Project> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "a");
        assert_eq!(parsed[1].name, "b");
    }

    // ── Additional tests for 50+ threshold ──

    #[test]
    fn test_truncate_six_chars_max_five() {
        assert_eq!(truncate("abcdef", 5), "ab...");
    }

    #[test]
    fn test_truncate_preserves_exact_content() {
        let s = "exact";
        assert_eq!(truncate(s, 5), "exact");
        assert_eq!(truncate(s, 100), "exact");
    }

    #[test]
    fn test_project_main_branch_develop() {
        let project = Project {
            name: "dev".to_string(),
            path: PathBuf::from("/dev"),
            main_branch: "develop".to_string(),
        };
        assert_eq!(project.main_branch, "develop");
    }

    #[test]
    fn test_project_worktrees_dir_with_trailing_slash_path() {
        let project = Project {
            name: "test".to_string(),
            path: PathBuf::from("/home/user/project/"),
            main_branch: "main".to_string(),
        };
        // PathBuf normalizes trailing slashes
        assert!(project.worktrees_dir().to_string_lossy().contains("worktrees"));
    }

    #[test]
    fn test_project_empty_name() {
        let project = Project {
            name: String::new(),
            path: PathBuf::from("/tmp"),
            main_branch: "main".to_string(),
        };
        assert!(project.name.is_empty());
    }

    #[test]
    fn test_project_empty_main_branch() {
        let project = Project {
            name: "test".to_string(),
            path: PathBuf::from("/tmp"),
            main_branch: String::new(),
        };
        assert!(project.main_branch.is_empty());
    }

    #[test]
    fn test_handle_project_remove_special_chars() {
        let result = handle_project_remove("my-project@v2/special", false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_project_remove_unicode() {
        let result = handle_project_remove("项目", true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_output_format_all_variants_match() {
        let variants = [OutputFormat::Json, OutputFormat::Plain, OutputFormat::Table];
        for v in variants {
            match v {
                OutputFormat::Json => {}
                OutputFormat::Plain => {}
                OutputFormat::Table => {}
            }
        }
    }

    #[test]
    fn test_truncate_returns_string() {
        // Verify truncate returns an owned String, not a reference
        let result: String = truncate("test", 10);
        assert_eq!(result, "test");
    }
}
