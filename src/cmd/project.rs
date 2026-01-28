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
            println!("{:<20} {}", "NAME", "PATH");
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

            // Count azural branches
            if let Ok(branches) = Git::list_azural_branches(&project.path) {
                println!("\nAzural branches: {}", branches.len());
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
    println!("\nTo remove session worktrees, use: azural session cleanup --delete-branches");
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
