//! Model maintenance command handlers

use anyhow::{bail, Context, Result};
use chrono::Local;
use std::path::{Path, PathBuf};

use crate::openai_models;

const MODEL_FILE_RELATIVE_PATH: &str = "src/app/state/app/model.rs";

fn find_repo_root(start: &Path) -> Result<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join("Cargo.toml").exists() && dir.join(MODEL_FILE_RELATIVE_PATH).exists() {
            return Ok(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    bail!("Could not find repo root from {}", start.to_string_lossy());
}

pub fn handle_models_sync_openai_frontier(check: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to read current directory")?;
    let repo_root = find_repo_root(&cwd)?;
    let model_file = repo_root.join(MODEL_FILE_RELATIVE_PATH);
    let models = openai_models::fetch_frontier_model_ids()?;
    let synced_on = Local::now().format("%Y-%m-%d").to_string();

    if check {
        let source = std::fs::read_to_string(&model_file)
            .with_context(|| format!("Failed to read {}", model_file.display()))?;
        let current_models = openai_models::extract_frontier_model_ids_from_model_source(&source)?;
        if current_models == models {
            println!("OpenAI frontier models are in sync.");
            for model in &models {
                println!("{}", model);
            }
            return Ok(());
        }
        bail!("OpenAI frontier models are out of sync. Run `azureal models sync-openai-frontier`.");
    }

    let changed = openai_models::sync_model_file(&model_file, &models, &synced_on)?;
    if changed {
        println!("Updated {}.", model_file.display());
    } else {
        println!("Already up to date: {}.", model_file.display());
    }
    for model in &models {
        println!("{}", model);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_repo_root_accepts_repo_root() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(find_repo_root(&root).unwrap(), root);
    }

    #[test]
    fn test_find_repo_root_walks_up_from_nested_dir() {
        let nested = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/app/state/app");
        assert_eq!(
            find_repo_root(&nested).unwrap(),
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        );
    }

    #[test]
    fn test_find_repo_root_errors_outside_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let err = find_repo_root(tmp.path()).unwrap_err().to_string();
        assert!(err.contains("Could not find repo root"));
    }
}
