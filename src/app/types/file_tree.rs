//! File tree entry and action types

use std::path::PathBuf;

/// Entry in the file tree (file or directory)
#[derive(Debug, Clone)]
pub struct FileTreeEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
    pub is_hidden: bool,
}

/// Active file tree action requiring text input or confirmation
#[derive(Debug, Clone)]
pub enum FileTreeAction {
    /// Creating a new file (input = filename). Trailing '/' means create directory.
    Add(String),
    /// Renaming the selected entry (input = new name)
    Rename(String),
    /// Clipboard copy — source path stored, navigate to target dir and press Enter
    Copy(PathBuf),
    /// Clipboard move — source path stored, navigate to target dir and press Enter (dashed border)
    Move(PathBuf),
    /// Deleting the selected entry (awaiting 'y' confirmation)
    Delete,
}
