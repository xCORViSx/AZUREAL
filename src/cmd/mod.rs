//! Command handlers for CLI subcommands

mod project;
mod session;

pub use project::*;
pub use session::*;

use anyhow::Result;
use std::io::{BufRead, BufReader};

/// Handle the hooks command - view/clear the hooks log
pub fn handle_hooks(lines: usize, json: bool, name_filter: Option<String>, clear: bool) -> Result<()> {
    let hooks_path = crate::config::config_dir().join("hooks.jsonl");

    if clear {
        if hooks_path.exists() {
            std::fs::remove_file(&hooks_path)?;
            println!("Hooks log cleared.");
        } else {
            println!("No hooks log to clear.");
        }
        return Ok(());
    }

    if !hooks_path.exists() {
        println!("No hooks recorded yet. Hooks will be logged to: {}", hooks_path.display());
        return Ok(());
    }

    // Read all lines, then take last N
    let file = std::fs::File::open(&hooks_path)?;
    let reader = BufReader::new(file);
    let all_lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();

    // Filter by hook name if specified
    let filtered: Vec<&String> = if let Some(ref filter) = name_filter {
        all_lines.iter().filter(|line| {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                v.get("hook_name")
                    .and_then(|n| n.as_str())
                    .map(|n| n.contains(filter))
                    .unwrap_or(false)
            } else { false }
        }).collect()
    } else {
        all_lines.iter().collect()
    };

    // Take last N
    let start = filtered.len().saturating_sub(lines);
    let recent = &filtered[start..];

    if recent.is_empty() {
        println!("No hooks found{}", name_filter.map(|n| format!(" matching '{}'", n)).unwrap_or_default());
        return Ok(());
    }

    if json {
        // Output raw JSON lines
        for line in recent { println!("{}", line); }
    } else {
        // Formatted output
        println!("╭─ Hooks Log ({} entries) ─╮", recent.len());
        println!("│");
        for line in recent {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                let ts = v.get("timestamp").and_then(|t| t.as_str()).unwrap_or("?");
                let name = v.get("hook_name").and_then(|n| n.as_str()).unwrap_or("?");
                let output = v.get("output").and_then(|o| o.as_str()).unwrap_or("");
                let session = v.get("session_id").and_then(|s| s.as_str()).unwrap_or("");

                // Parse timestamp for display
                let time_display = ts.split('T').nth(1).and_then(|t| t.split('.').next()).unwrap_or(ts);

                println!("│ {} │ \x1b[36m{}\x1b[0m", time_display, name);
                if !session.is_empty() {
                    println!("│   session: {}", &session[..session.len().min(8)]);
                }
                if !output.is_empty() {
                    for (i, line) in output.lines().take(3).enumerate() {
                        if i == 0 { println!("│   output: {}", line); }
                        else { println!("│           {}", line); }
                    }
                    if output.lines().count() > 3 {
                        println!("│           ... ({} more lines)", output.lines().count() - 3);
                    }
                }
                println!("│");
            }
        }
        println!("╰────────────────────────────────────╯");
        println!("\nLog file: {}", hooks_path.display());
    }

    Ok(())
}
