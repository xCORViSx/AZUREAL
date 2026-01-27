//! Project command handlers

use anyhow::Result;

use crate::cli::OutputFormat;
use crate::db::Database;

/// Truncate string with ellipsis
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len { s.to_string() }
    else { format!("{}...", &s[..max_len.saturating_sub(3)]) }
}

pub fn handle_project_list(db: &Database, output_format: OutputFormat) -> Result<()> {
    let projects = db.list_projects()?;

    match output_format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&projects)?),
        OutputFormat::Plain => {
            for project in &projects {
                println!("{}\t{}\t{}", project.id, project.name, project.path.display());
            }
        }
        OutputFormat::Table => {
            if projects.is_empty() {
                println!("No projects found.");
            } else {
                println!("{:<6} {:<20} {}", "ID", "NAME", "PATH");
                println!("{}", "-".repeat(70));
                for project in projects {
                    println!("{:<6} {:<20} {}", project.id, truncate(&project.name, 20), project.path.display());
                }
            }
        }
    }
    Ok(())
}

pub fn handle_project_show(db: &Database, project_arg: Option<String>, output_format: OutputFormat) -> Result<()> {
    let project = match project_arg {
        Some(arg) => {
            if let Ok(id) = arg.parse::<i64>() {
                db.get_project(id)?
            } else {
                db.get_project_by_path(&std::path::PathBuf::from(&arg))?
            }
        }
        None => {
            let cwd = std::env::current_dir()?;
            db.get_project_by_path(&cwd)?
        }
    };

    let project = project.ok_or_else(|| anyhow::anyhow!("Project not found"))?;

    match output_format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&project)?),
        OutputFormat::Plain => println!("{}\t{}\t{}", project.id, project.name, project.path.display()),
        OutputFormat::Table => {
            println!("Project: {}", project.name);
            println!("ID: {}", project.id);
            println!("Path: {}", project.path.display());
            println!("Main branch: {}", project.main_branch);
            println!("Created: {}", project.created_at.format("%Y-%m-%d %H:%M:%S UTC"));

            if let Some(prompt) = &project.system_prompt {
                println!("System prompt: {}", truncate(prompt, 60));
            }

            let sessions = db.list_sessions_for_project(project.id)?;
            println!("\nSessions: {}", sessions.len());
            if !sessions.is_empty() {
                for session in sessions.iter().take(5) {
                    println!("  {} {} - {}", session.status.symbol(), truncate(&session.name, 30), session.status.as_str());
                }
                if sessions.len() > 5 { println!("  ... and {} more", sessions.len() - 5); }
            }
        }
    }
    Ok(())
}

pub fn handle_project_remove(db: &Database, project_arg: &str, skip_confirm: bool) -> Result<()> {
    let project = if let Ok(id) = project_arg.parse::<i64>() {
        db.get_project(id)?
    } else {
        db.get_project_by_path(&std::path::PathBuf::from(project_arg))?
    };

    let project = project.ok_or_else(|| anyhow::anyhow!("Project not found"))?;

    if !skip_confirm {
        println!("Remove project '{}' from tracking?", project.name);
        println!("This will NOT delete the project files or worktrees.");
        print!("Type 'yes' to confirm: ");
        use std::io::Write;
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if input.trim() != "yes" {
            println!("Cancelled.");
            return Ok(());
        }
    }

    db.delete_project(project.id)?;
    println!("Removed project: {}", project.name);
    Ok(())
}

pub fn handle_project_config(
    db: &Database,
    project_arg: Option<String>,
    main_branch: Option<String>,
    system_prompt: Option<String>,
) -> Result<()> {
    let path = project_arg
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));

    let project = db.get_project_by_path(&path)?
        .ok_or_else(|| anyhow::anyhow!("Project not found: {}", path.display()))?;

    if main_branch.is_none() && system_prompt.is_none() {
        println!("Project: {}", project.name);
        println!("Main branch: {}", project.main_branch);
        if let Some(prompt) = &project.system_prompt {
            println!("System prompt: {}", prompt);
        } else {
            println!("System prompt: (not set)");
        }
        return Ok(());
    }

    if let Some(branch) = main_branch {
        db.update_project_main_branch(project.id, &branch)?;
        println!("Updated main branch to: {}", branch);
    }

    if let Some(prompt) = system_prompt {
        db.update_project_system_prompt(project.id, Some(&prompt))?;
        println!("Updated system prompt");
    }

    Ok(())
}
