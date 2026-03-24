//! App type definitions — split into focused submodules by responsibility.
//!
//! Submodules:
//! - `core`: Fundamental UI enums (ViewerMode, ViewMode, Focus) and ViewerTab
//! - `file_tree`: FileTreeEntry, FileTreeAction
//! - `branch_dialog`: BranchDialog state + is_git_safe_char helper
//! - `commands`: RunCommand, PresetPrompt, and their picker/dialog types
//! - `projects_panel`: ProjectsPanel state + ProjectsPanelMode
//! - `git_panel`: All git panel types (commits, files, overlays, background ops)
//! - `worktree_dialogs`: Rename/delete dialogs, TablePopup, WorktreeRefreshResult
//! - `health_panel`: Health panel types (god files, documentation, module styles)
//! - `issues_panel`: GitHub Issues panel types

mod branch_dialog;
mod commands;
mod core;
mod file_tree;
mod git_panel;
mod health_panel;
mod issues_panel;
mod projects_panel;
mod worktree_dialogs;

// Re-export everything for backwards compatibility
pub use self::core::*;
pub use branch_dialog::*;
pub use commands::*;
pub use file_tree::*;
pub use git_panel::*;
pub use health_panel::*;
pub use issues_panel::*;
pub use projects_panel::*;
pub use worktree_dialogs::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::config::ProjectEntry;

    // ══════════════════════════════════════════════════════════════════
    // ViewerMode enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn viewer_mode_default_is_empty() {
        assert_eq!(ViewerMode::default(), ViewerMode::Empty);
    }

    #[test]
    fn viewer_mode_clone_and_copy() {
        let m = ViewerMode::File;
        let cloned = m.clone();
        let copied = m;
        assert_eq!(m, cloned);
        assert_eq!(m, copied);
    }

    #[test]
    fn viewer_mode_all_variants_distinct() {
        let variants = [
            ViewerMode::Empty,
            ViewerMode::File,
            ViewerMode::Diff,
            ViewerMode::Image,
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn viewer_mode_debug_format() {
        assert_eq!(format!("{:?}", ViewerMode::Empty), "Empty");
        assert_eq!(format!("{:?}", ViewerMode::File), "File");
        assert_eq!(format!("{:?}", ViewerMode::Diff), "Diff");
        assert_eq!(format!("{:?}", ViewerMode::Image), "Image");
    }

    // ══════════════════════════════════════════════════════════════════
    // ViewMode enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn view_mode_session_eq() {
        assert_eq!(ViewMode::Session, ViewMode::Session);
    }

    #[test]
    fn view_mode_debug() {
        assert_eq!(format!("{:?}", ViewMode::Session), "Session");
    }

    // ══════════════════════════════════════════════════════════════════
    // Focus enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn focus_all_variants_distinct() {
        let variants = [
            Focus::Worktrees,
            Focus::FileTree,
            Focus::Viewer,
            Focus::Session,
            Focus::Input,
            Focus::BranchDialog,
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn focus_debug_format() {
        assert_eq!(format!("{:?}", Focus::Worktrees), "Worktrees");
        assert_eq!(format!("{:?}", Focus::Input), "Input");
        assert_eq!(format!("{:?}", Focus::BranchDialog), "BranchDialog");
    }

    // ══════════════════════════════════════════════════════════════════
    // CommandFieldMode enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn command_field_mode_variants_distinct() {
        assert_ne!(CommandFieldMode::Command, CommandFieldMode::Prompt);
    }

    #[test]
    fn command_field_mode_clone_copy() {
        let m = CommandFieldMode::Prompt;
        let cloned = m.clone();
        let copied = m;
        assert_eq!(m, cloned);
        assert_eq!(m, copied);
    }

    // ══════════════════════════════════════════════════════════════════
    // ProjectsPanelMode enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn projects_panel_mode_all_variants_distinct() {
        let variants = [
            ProjectsPanelMode::Browse,
            ProjectsPanelMode::AddPath,
            ProjectsPanelMode::Rename,
            ProjectsPanelMode::Init,
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // RustModuleStyle enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn rust_module_style_variants_distinct() {
        assert_ne!(RustModuleStyle::FileBased, RustModuleStyle::ModRs);
    }

    #[test]
    fn rust_module_style_clone_copy() {
        let s = RustModuleStyle::FileBased;
        let c = s;
        assert_eq!(s, c);
    }

    // ══════════════════════════════════════════════════════════════════
    // PythonModuleStyle enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn python_module_style_variants_distinct() {
        assert_ne!(PythonModuleStyle::Package, PythonModuleStyle::SingleFile);
    }

    // ══════════════════════════════════════════════════════════════════
    // HealthTab enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn health_tab_variants_distinct() {
        assert_ne!(HealthTab::GodFiles, HealthTab::Documentation);
    }

    #[test]
    fn health_tab_clone_copy() {
        let t = HealthTab::GodFiles;
        let c = t;
        assert_eq!(t, c);
    }

    // ══════════════════════════════════════════════════════════════════
    // FileTreeEntry struct
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn file_tree_entry_clone() {
        let entry = FileTreeEntry {
            path: PathBuf::from("/src/main.rs"),
            name: "main.rs".to_string(),
            is_dir: false,
            depth: 1,
            is_hidden: false,
        };
        let cloned = entry.clone();
        assert_eq!(cloned.path, PathBuf::from("/src/main.rs"));
        assert_eq!(cloned.name, "main.rs");
        assert!(!cloned.is_dir);
        assert_eq!(cloned.depth, 1);
        assert!(!cloned.is_hidden);
    }

    #[test]
    fn file_tree_entry_hidden_dotfile() {
        let entry = FileTreeEntry {
            path: PathBuf::from("/.gitignore"),
            name: ".gitignore".to_string(),
            is_dir: false,
            depth: 0,
            is_hidden: true,
        };
        assert!(entry.is_hidden);
    }

    #[test]
    fn file_tree_entry_directory() {
        let entry = FileTreeEntry {
            path: PathBuf::from("/src"),
            name: "src".to_string(),
            is_dir: true,
            depth: 0,
            is_hidden: false,
        };
        assert!(entry.is_dir);
    }

    #[test]
    fn file_tree_entry_debug_format() {
        let entry = FileTreeEntry {
            path: PathBuf::from("/a"),
            name: "a".to_string(),
            is_dir: false,
            depth: 0,
            is_hidden: false,
        };
        let dbg = format!("{:?}", entry);
        assert!(dbg.contains("FileTreeEntry"));
        assert!(dbg.contains("\"a\""));
    }

    // ══════════════════════════════════════════════════════════════════
    // FileTreeAction enum
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn file_tree_action_add_clone() {
        let action = FileTreeAction::Add("test.rs".to_string());
        let cloned = action.clone();
        if let FileTreeAction::Add(name) = cloned {
            assert_eq!(name, "test.rs");
        } else {
            panic!("Expected Add variant");
        }
    }

    #[test]
    fn file_tree_action_copy_stores_path() {
        let action = FileTreeAction::Copy(PathBuf::from("/foo/bar.txt"));
        if let FileTreeAction::Copy(p) = action {
            assert_eq!(p, PathBuf::from("/foo/bar.txt"));
        } else {
            panic!("Expected Copy variant");
        }
    }

    #[test]
    fn file_tree_action_move_stores_path() {
        let action = FileTreeAction::Move(PathBuf::from("/baz"));
        if let FileTreeAction::Move(p) = action {
            assert_eq!(p, PathBuf::from("/baz"));
        } else {
            panic!("Expected Move variant");
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // BranchDialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn branch_dialog_new_empty() {
        let d = BranchDialog::new(vec![], vec![], vec![]);
        assert!(d.branches.is_empty());
        assert!(d.checked_out.is_empty());
        assert_eq!(d.selected, 0);
        assert!(d.filter.is_empty());
        assert!(d.filtered_indices.is_empty());
    }

    #[test]
    fn branch_dialog_new_populates_filtered_indices() {
        let d = BranchDialog::new(
            vec!["main".into(), "feat/a".into(), "feat/b".into()],
            vec![],
            vec![0, 0, 0],
        );
        assert_eq!(d.filtered_indices, vec![0, 1, 2]);
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn branch_dialog_is_checked_out_exact_match() {
        let d = BranchDialog::new(vec![], vec!["main".into(), "feat/a".into()], vec![]);
        assert!(d.is_checked_out("main"));
        assert!(d.is_checked_out("feat/a"));
        assert!(!d.is_checked_out("feat/b"));
    }

    #[test]
    fn branch_dialog_is_checked_out_remote_prefix_stripped() {
        // "origin/feat" -> local_name = "feat"
        let d = BranchDialog::new(vec![], vec!["feat".into()], vec![]);
        assert!(d.is_checked_out("origin/feat"));
    }

    #[test]
    fn branch_dialog_is_checked_out_multi_slash() {
        // "origin/azureal/health" -> local_name = "azureal/health"
        let d = BranchDialog::new(vec![], vec!["azureal/health".into()], vec![]);
        assert!(d.is_checked_out("origin/azureal/health"));
    }

    #[test]
    fn branch_dialog_is_checked_out_no_slash_no_match() {
        let d = BranchDialog::new(vec![], vec!["other".into()], vec![]);
        assert!(!d.is_checked_out("feat"));
    }

    #[test]
    fn branch_dialog_selected_branch_with_entries() {
        let mut d = BranchDialog::new(vec!["alpha".into(), "beta".into()], vec![], vec![0, 0]);
        // selected==0 is "[+] Create new" row, so move to first branch
        d.select_next();
        assert_eq!(d.selected_branch(), Some(&"alpha".to_string()));
    }

    #[test]
    fn branch_dialog_selected_branch_empty() {
        let d = BranchDialog::new(vec![], vec![], vec![]);
        assert_eq!(d.selected_branch(), None);
    }

    #[test]
    fn branch_dialog_select_next() {
        // display_len = 1 (Create new) + 3 branches = 4, max selected = 3
        let mut d = BranchDialog::new(
            vec!["a".into(), "b".into(), "c".into()],
            vec![],
            vec![0, 0, 0],
        );
        assert_eq!(d.selected, 0);
        d.select_next();
        assert_eq!(d.selected, 1);
        d.select_next();
        assert_eq!(d.selected, 2);
        d.select_next();
        assert_eq!(d.selected, 3);
        // At the end, should not overflow
        d.select_next();
        assert_eq!(d.selected, 3);
    }

    #[test]
    fn branch_dialog_select_prev() {
        let mut d = BranchDialog::new(vec!["a".into(), "b".into()], vec![], vec![0, 0]);
        d.select_next(); // now at 1
        d.select_prev();
        assert_eq!(d.selected, 0);
        // At 0, should not underflow
        d.select_prev();
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn branch_dialog_select_next_on_empty() {
        let mut d = BranchDialog::new(vec![], vec![], vec![]);
        d.select_next();
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn branch_dialog_filter_char_narrows_results() {
        let mut d = BranchDialog::new(
            vec![
                "main".into(),
                "feat/auth".into(),
                "feat/api".into(),
                "fix/bug".into(),
            ],
            vec![],
            vec![0, 0, 0, 0],
        );
        d.filter_char('f');
        assert_eq!(d.filtered_indices, vec![1, 2, 3]); // feat/auth, feat/api, fix/bug
        d.filter_char('e');
        assert_eq!(d.filtered_indices, vec![1, 2]); // feat/auth, feat/api
    }

    #[test]
    fn branch_dialog_filter_case_insensitive() {
        let mut d = BranchDialog::new(vec!["MAIN".into(), "Feature".into()], vec![], vec![0, 0]);
        d.filter_char('m');
        // "MAIN" contains "m" (case insensitive)
        assert!(d.filtered_indices.contains(&0));
    }

    #[test]
    fn branch_dialog_filter_backspace_widens_results() {
        let mut d = BranchDialog::new(vec!["main".into(), "feat/auth".into()], vec![], vec![0, 0]);
        d.filter_char('f');
        d.filter_char('e');
        assert_eq!(d.filtered_indices, vec![1]); // only feat/auth
        d.filter_backspace();
        // Now filter is just "f", both "feat/auth" and nothing else with f
        assert_eq!(d.filter, "f");
        assert_eq!(d.filtered_indices, vec![1]);
        d.filter_backspace();
        // Empty filter, all shown
        assert_eq!(d.filtered_indices, vec![0, 1]);
    }

    #[test]
    fn branch_dialog_filter_backspace_on_empty() {
        let mut d = BranchDialog::new(vec!["a".into()], vec![], vec![0]);
        d.filter_backspace(); // should not panic
        assert!(d.filter.is_empty());
        assert_eq!(d.filtered_indices, vec![0]);
    }

    #[test]
    fn branch_dialog_selected_resets_when_filter_shrinks_results() {
        let mut d = BranchDialog::new(
            vec!["aaa".into(), "bbb".into(), "ccc".into()],
            vec![],
            vec![0, 0, 0],
        );
        d.select_next();
        d.select_next();
        assert_eq!(d.selected, 2);
        // Now filter to only one result
        d.filter_char('a');
        assert_eq!(d.filtered_indices, vec![0]);
        // selected should have been clamped to 0
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn branch_dialog_selected_branch_after_filter() {
        let mut d = BranchDialog::new(
            vec!["main".into(), "feat/auth".into(), "feat/api".into()],
            vec![],
            vec![0, 0, 0],
        );
        d.filter_char('a');
        d.filter_char('p');
        d.filter_char('i');
        // Only "feat/api" matches
        assert_eq!(d.filtered_indices, vec![2]);
        // selected==0 is "[+] Create new", move to first filtered branch
        d.select_next();
        assert_eq!(d.selected_branch(), Some(&"feat/api".to_string()));
    }

    #[test]
    fn branch_dialog_unicode_filter_rejected_by_git_safe() {
        // Emoji chars are rejected by is_git_safe_char, so filter stays empty
        let mut d = BranchDialog::new(
            vec!["feat/unicorn-\u{1F984}".into(), "main".into()],
            vec![],
            vec![0, 0],
        );
        d.filter_char('\u{1F984}');
        // filter is still empty, all branches shown
        assert_eq!(d.filtered_indices, vec![0, 1]);
    }

    // ══════════════════════════════════════════════════════════════════
    // RunCommand
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn run_command_new_basic() {
        let cmd = RunCommand::new("build", "cargo build", false);
        assert_eq!(cmd.name, "build");
        assert_eq!(cmd.command, "cargo build");
        assert!(!cmd.global);
    }

    #[test]
    fn run_command_new_global() {
        let cmd = RunCommand::new("test", "cargo test", true);
        assert!(cmd.global);
    }

    #[test]
    fn run_command_new_from_string_types() {
        let name = String::from("deploy");
        let command = String::from("./deploy.sh");
        let cmd = RunCommand::new(name, command, false);
        assert_eq!(cmd.name, "deploy");
        assert_eq!(cmd.command, "./deploy.sh");
    }

    #[test]
    fn run_command_clone() {
        let cmd = RunCommand::new("x", "y", true);
        let cloned = cmd.clone();
        assert_eq!(cloned.name, "x");
        assert_eq!(cloned.command, "y");
        assert!(cloned.global);
    }

    #[test]
    fn run_command_empty_strings() {
        let cmd = RunCommand::new("", "", false);
        assert!(cmd.name.is_empty());
        assert!(cmd.command.is_empty());
    }

    #[test]
    fn run_command_special_chars() {
        let cmd = RunCommand::new(
            "build & test",
            "cargo build && cargo test 2>&1 | tee log",
            false,
        );
        assert_eq!(cmd.name, "build & test");
        assert_eq!(cmd.command, "cargo build && cargo test 2>&1 | tee log");
    }

    // ══════════════════════════════════════════════════════════════════
    // RunCommandDialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn run_command_dialog_new_defaults() {
        let d = RunCommandDialog::new();
        assert!(d.name.is_empty());
        assert!(d.command.is_empty());
        assert_eq!(d.name_cursor, 0);
        assert_eq!(d.command_cursor, 0);
        assert!(d.editing_name);
        assert!(d.editing_idx.is_none());
        assert_eq!(d.field_mode, CommandFieldMode::Command);
        assert!(!d.global);
    }

    #[test]
    fn run_command_dialog_edit_populates_fields() {
        let cmd = RunCommand::new("test", "cargo test --all", true);
        let d = RunCommandDialog::edit(3, &cmd);
        assert_eq!(d.name, "test");
        assert_eq!(d.command, "cargo test --all");
        assert_eq!(d.name_cursor, 4); // len of "test"
        assert_eq!(d.command_cursor, 16); // len of "cargo test --all"
        assert!(d.editing_name);
        assert_eq!(d.editing_idx, Some(3));
        assert_eq!(d.field_mode, CommandFieldMode::Command);
        assert!(d.global);
    }

    #[test]
    fn run_command_dialog_edit_index_zero() {
        let cmd = RunCommand::new("a", "b", false);
        let d = RunCommandDialog::edit(0, &cmd);
        assert_eq!(d.editing_idx, Some(0));
    }

    // ══════════════════════════════════════════════════════════════════
    // RunCommandPicker
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn run_command_picker_new() {
        let p = RunCommandPicker::new();
        assert_eq!(p.selected, 0);
        assert!(p.confirm_delete.is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    // PresetPrompt
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn preset_prompt_new_basic() {
        let p = PresetPrompt::new("review", "Review this PR for bugs", false);
        assert_eq!(p.name, "review");
        assert_eq!(p.prompt, "Review this PR for bugs");
        assert!(!p.global);
    }

    #[test]
    fn preset_prompt_new_global() {
        let p = PresetPrompt::new("explain", "Explain this code", true);
        assert!(p.global);
    }

    #[test]
    fn preset_prompt_clone() {
        let p = PresetPrompt::new("test", "prompt text", false);
        let cloned = p.clone();
        assert_eq!(cloned.name, "test");
        assert_eq!(cloned.prompt, "prompt text");
    }

    #[test]
    fn preset_prompt_unicode_content() {
        let p = PresetPrompt::new(
            "Japanese",
            "\u{65E5}\u{672C}\u{8A9E}\u{306E}\u{8AAC}\u{660E}",
            false,
        );
        assert_eq!(p.name, "Japanese");
        assert_eq!(p.prompt.chars().count(), 6);
    }

    // ══════════════════════════════════════════════════════════════════
    // PresetPromptPicker
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn preset_prompt_picker_new() {
        let p = PresetPromptPicker::new();
        assert_eq!(p.selected, 0);
        assert!(p.confirm_delete.is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    // PresetPromptDialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn preset_prompt_dialog_new_defaults() {
        let d = PresetPromptDialog::new();
        assert!(d.name.is_empty());
        assert!(d.prompt.is_empty());
        assert_eq!(d.name_cursor, 0);
        assert_eq!(d.prompt_cursor, 0);
        assert!(d.editing_name);
        assert!(d.editing_idx.is_none());
        assert!(!d.global);
    }

    #[test]
    fn preset_prompt_dialog_edit_populates() {
        let preset = PresetPrompt::new("summarize", "Summarize in 3 bullets", true);
        let d = PresetPromptDialog::edit(5, &preset);
        assert_eq!(d.name, "summarize");
        assert_eq!(d.prompt, "Summarize in 3 bullets");
        assert_eq!(d.name_cursor, 9); // char count of "summarize"
        assert_eq!(d.prompt_cursor, 22); // char count of "Summarize in 3 bullets"
        assert!(d.editing_name);
        assert_eq!(d.editing_idx, Some(5));
        assert!(d.global);
    }

    #[test]
    fn preset_prompt_dialog_edit_unicode_cursors() {
        // Unicode chars: cursor should be char count, not byte len
        let preset = PresetPrompt::new("\u{1F600}", "\u{1F4BB}\u{1F680}", false);
        let d = PresetPromptDialog::edit(0, &preset);
        assert_eq!(d.name_cursor, 1); // 1 emoji char
        assert_eq!(d.prompt_cursor, 2); // 2 emoji chars
    }

    // ══════════════════════════════════════════════════════════════════
    // ProjectsPanel
    // ══════════════════════════════════════════════════════════════════

    fn make_entries(count: usize) -> Vec<ProjectEntry> {
        (0..count)
            .map(|i| ProjectEntry {
                path: PathBuf::from(format!("/projects/proj{}", i)),
                display_name: format!("Project {}", i),
            })
            .collect()
    }

    #[test]
    fn projects_panel_new_defaults() {
        let p = ProjectsPanel::new(vec![]);
        assert!(p.entries.is_empty());
        assert_eq!(p.selected, 0);
        assert_eq!(p.mode, ProjectsPanelMode::Browse);
        assert!(p.input.is_empty());
        assert_eq!(p.input_cursor, 0);
        assert!(p.error.is_none());
    }

    #[test]
    fn projects_panel_new_with_entries() {
        let entries = make_entries(3);
        let p = ProjectsPanel::new(entries);
        assert_eq!(p.entries.len(), 3);
        assert_eq!(p.entries[0].display_name, "Project 0");
    }

    #[test]
    fn projects_panel_select_next() {
        let mut p = ProjectsPanel::new(make_entries(3));
        p.error = Some("stale error".into());
        p.select_next();
        assert_eq!(p.selected, 1);
        assert!(p.error.is_none()); // error cleared
        p.select_next();
        assert_eq!(p.selected, 2);
        p.select_next(); // at end, should not move
        assert_eq!(p.selected, 2);
    }

    #[test]
    fn projects_panel_select_prev() {
        let mut p = ProjectsPanel::new(make_entries(3));
        p.selected = 2;
        p.error = Some("err".into());
        p.select_prev();
        assert_eq!(p.selected, 1);
        assert!(p.error.is_none());
        p.select_prev();
        assert_eq!(p.selected, 0);
        p.select_prev(); // at start, should not move
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn projects_panel_select_next_empty() {
        let mut p = ProjectsPanel::new(vec![]);
        p.select_next(); // should not panic
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn projects_panel_start_add() {
        let mut p = ProjectsPanel::new(make_entries(1));
        p.input = "leftover".into();
        p.input_cursor = 5;
        p.error = Some("old error".into());
        p.start_add();
        assert_eq!(p.mode, ProjectsPanelMode::AddPath);
        assert!(p.input.is_empty());
        assert_eq!(p.input_cursor, 0);
        assert!(p.error.is_none());
    }

    #[test]
    fn projects_panel_start_rename() {
        let mut p = ProjectsPanel::new(make_entries(2));
        p.selected = 1;
        p.start_rename();
        assert_eq!(p.mode, ProjectsPanelMode::Rename);
        assert_eq!(p.input, "Project 1");
        assert_eq!(p.input_cursor, "Project 1".len());
        assert!(p.error.is_none());
    }

    #[test]
    fn projects_panel_start_rename_empty_list() {
        let mut p = ProjectsPanel::new(vec![]);
        p.start_rename(); // should not panic, mode stays Browse
        assert_eq!(p.mode, ProjectsPanelMode::Browse);
    }

    #[test]
    fn projects_panel_start_init() {
        let mut p = ProjectsPanel::new(make_entries(1));
        p.input = "stale".into();
        p.error = Some("x".into());
        p.start_init();
        assert_eq!(p.mode, ProjectsPanelMode::Init);
        assert!(p.input.is_empty());
        assert_eq!(p.input_cursor, 0);
        assert!(p.error.is_none());
    }

    #[test]
    fn projects_panel_cancel_input() {
        let mut p = ProjectsPanel::new(make_entries(1));
        p.mode = ProjectsPanelMode::AddPath;
        p.input = "/some/path".into();
        p.input_cursor = 10;
        p.error = Some("bad".into());
        p.cancel_input();
        assert_eq!(p.mode, ProjectsPanelMode::Browse);
        assert!(p.input.is_empty());
        assert_eq!(p.input_cursor, 0);
        assert!(p.error.is_none());
    }

    #[test]
    fn projects_panel_input_char() {
        let mut p = ProjectsPanel::new(vec![]);
        p.mode = ProjectsPanelMode::AddPath;
        p.error = Some("old".into());
        p.input_char('h');
        p.input_char('i');
        assert_eq!(p.input, "hi");
        assert_eq!(p.input_cursor, 2);
        assert!(p.error.is_none());
    }

    #[test]
    fn projects_panel_input_char_inserts_at_cursor() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "ac".into();
        p.input_cursor = 1; // between 'a' and 'c'
        p.input_char('b');
        assert_eq!(p.input, "abc");
        assert_eq!(p.input_cursor, 2);
    }

    #[test]
    fn projects_panel_input_backspace() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "abc".into();
        p.input_cursor = 3;
        p.input_backspace();
        assert_eq!(p.input, "ab");
        assert_eq!(p.input_cursor, 2);
    }

    #[test]
    fn projects_panel_input_backspace_at_start() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "abc".into();
        p.input_cursor = 0;
        p.input_backspace(); // should do nothing
        assert_eq!(p.input, "abc");
        assert_eq!(p.input_cursor, 0);
    }

    #[test]
    fn projects_panel_input_backspace_empty() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input_backspace(); // should not panic
        assert!(p.input.is_empty());
    }

    #[test]
    fn projects_panel_input_delete() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "abc".into();
        p.input_cursor = 1; // at 'b'
        p.input_delete();
        assert_eq!(p.input, "ac");
        assert_eq!(p.input_cursor, 1);
    }

    #[test]
    fn projects_panel_input_delete_at_end() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "abc".into();
        p.input_cursor = 3;
        p.input_delete(); // should do nothing
        assert_eq!(p.input, "abc");
    }

    #[test]
    fn projects_panel_cursor_left() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "abc".into();
        p.input_cursor = 2;
        p.cursor_left();
        assert_eq!(p.input_cursor, 1);
    }

    #[test]
    fn projects_panel_cursor_left_at_zero() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input_cursor = 0;
        p.cursor_left();
        assert_eq!(p.input_cursor, 0);
    }

    #[test]
    fn projects_panel_cursor_right() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "abc".into();
        p.input_cursor = 1;
        p.cursor_right();
        assert_eq!(p.input_cursor, 2);
    }

    #[test]
    fn projects_panel_cursor_right_at_end() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "abc".into();
        p.input_cursor = 3;
        p.cursor_right();
        assert_eq!(p.input_cursor, 3);
    }

    #[test]
    fn projects_panel_cursor_home() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "hello".into();
        p.input_cursor = 3;
        p.cursor_home();
        assert_eq!(p.input_cursor, 0);
    }

    #[test]
    fn projects_panel_cursor_end() {
        let mut p = ProjectsPanel::new(vec![]);
        p.input = "hello".into();
        p.input_cursor = 0;
        p.cursor_end();
        assert_eq!(p.input_cursor, 5);
    }

    // ══════════════════════════════════════════════════════════════════
    // ViewerTab
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn viewer_tab_name_returns_title() {
        let tab = ViewerTab {
            path: Some(PathBuf::from("/foo/bar.rs")),
            content: Some("fn main() {}".into()),
            scroll: 0,
            mode: ViewerMode::File,
            title: "bar.rs".to_string(),
        };
        assert_eq!(tab.name(), "bar.rs");
    }

    #[test]
    fn viewer_tab_name_empty_title() {
        let tab = ViewerTab {
            path: None,
            content: None,
            scroll: 0,
            mode: ViewerMode::Empty,
            title: String::new(),
        };
        assert_eq!(tab.name(), "");
    }

    #[test]
    fn viewer_tab_clone() {
        let tab = ViewerTab {
            path: Some(PathBuf::from("/x")),
            content: Some("content".into()),
            scroll: 42,
            mode: ViewerMode::Diff,
            title: "diff".into(),
        };
        let cloned = tab.clone();
        assert_eq!(cloned.path, Some(PathBuf::from("/x")));
        assert_eq!(cloned.content, Some("content".into()));
        assert_eq!(cloned.scroll, 42);
        assert_eq!(cloned.mode, ViewerMode::Diff);
        assert_eq!(cloned.title, "diff");
    }

    // ══════════════════════════════════════════════════════════════════
    // GitCommit
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn git_commit_clone() {
        let c = GitCommit {
            hash: "abc1234".into(),
            full_hash: "abc1234567890abcdef1234567890abcdef123456".into(),
            subject: "feat: add health panel".into(),
            is_pushed: false,
        };
        let cloned = c.clone();
        assert_eq!(cloned.hash, "abc1234");
        assert_eq!(cloned.subject, "feat: add health panel");
        assert!(!cloned.is_pushed);
    }

    #[test]
    fn git_commit_debug() {
        let c = GitCommit {
            hash: "a".into(),
            full_hash: "a".into(),
            subject: "s".into(),
            is_pushed: true,
        };
        let dbg = format!("{:?}", c);
        assert!(dbg.contains("GitCommit"));
        assert!(dbg.contains("is_pushed: true"));
    }

    // ══════════════════════════════════════════════════════════════════
    // GitChangedFile
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn git_changed_file_clone() {
        let f = GitChangedFile {
            path: "src/main.rs".into(),
            status: 'M',
            additions: 10,
            deletions: 3,
            staged: false,
        };
        let cloned = f.clone();
        assert_eq!(cloned.path, "src/main.rs");
        assert_eq!(cloned.status, 'M');
        assert_eq!(cloned.additions, 10);
        assert_eq!(cloned.deletions, 3);
    }

    #[test]
    fn git_changed_file_added_status() {
        let f = GitChangedFile {
            path: "new_file.rs".into(),
            status: 'A',
            additions: 50,
            deletions: 0,
            staged: false,
        };
        assert_eq!(f.status, 'A');
        assert_eq!(f.deletions, 0);
    }

    #[test]
    fn git_changed_file_deleted_status() {
        let f = GitChangedFile {
            path: "old_file.rs".into(),
            status: 'D',
            additions: 0,
            deletions: 100,
            staged: false,
        };
        assert_eq!(f.status, 'D');
        assert_eq!(f.additions, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    // GodFileEntry
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn god_file_entry_clone() {
        let e = GodFileEntry {
            path: PathBuf::from("/src/big.rs"),
            rel_path: "src/big.rs".into(),
            line_count: 2500,
            checked: true,
        };
        let cloned = e.clone();
        assert_eq!(cloned.path, PathBuf::from("/src/big.rs"));
        assert_eq!(cloned.rel_path, "src/big.rs");
        assert_eq!(cloned.line_count, 2500);
        assert!(cloned.checked);
    }

    // ══════════════════════════════════════════════════════════════════
    // DocEntry
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn doc_entry_clone() {
        let e = DocEntry {
            path: PathBuf::from("/src/lib.rs"),
            rel_path: "src/lib.rs".into(),
            total_items: 20,
            documented_items: 15,
            coverage_pct: 75.0,
            checked: false,
        };
        let cloned = e.clone();
        assert_eq!(cloned.total_items, 20);
        assert_eq!(cloned.documented_items, 15);
        assert!((cloned.coverage_pct - 75.0).abs() < f32::EPSILON);
        assert!(!cloned.checked);
    }

    #[test]
    fn doc_entry_zero_coverage() {
        let e = DocEntry {
            path: PathBuf::from("/a.rs"),
            rel_path: "a.rs".into(),
            total_items: 10,
            documented_items: 0,
            coverage_pct: 0.0,
            checked: false,
        };
        assert_eq!(e.documented_items, 0);
        assert!((e.coverage_pct).abs() < f32::EPSILON);
    }

    #[test]
    fn doc_entry_full_coverage() {
        let e = DocEntry {
            path: PathBuf::from("/b.rs"),
            rel_path: "b.rs".into(),
            total_items: 5,
            documented_items: 5,
            coverage_pct: 100.0,
            checked: true,
        };
        assert_eq!(e.total_items, e.documented_items);
        assert!((e.coverage_pct - 100.0).abs() < f32::EPSILON);
    }

    // ══════════════════════════════════════════════════════════════════
    // GitConflictOverlay
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn git_conflict_overlay_fields() {
        let o = GitConflictOverlay {
            conflicted_files: vec!["src/main.rs".into()],
            auto_merged_files: vec!["Cargo.toml".into()],
            scroll: 0,
            selected: 0,
            continue_with_merge: true,
        };
        assert_eq!(o.conflicted_files.len(), 1);
        assert_eq!(o.auto_merged_files.len(), 1);
        assert!(o.continue_with_merge);
    }

    // ══════════════════════════════════════════════════════════════════
    // PostMergeDialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn post_merge_dialog_fields() {
        let d = PostMergeDialog {
            branch: "azureal/health".into(),
            display_name: "health".into(),
            worktree_path: PathBuf::from("/repo/worktrees/health"),
            selected: 0,
        };
        assert_eq!(d.branch, "azureal/health");
        assert_eq!(d.display_name, "health");
        assert_eq!(d.selected, 0);
    }

    // ══════════════════════════════════════════════════════════════════
    // RcrSession
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn rcr_session_fields() {
        let s = RcrSession {
            branch: "azureal/feat".into(),
            display_name: "feat".into(),
            worktree_path: PathBuf::from("/repo/worktrees/feat"),
            repo_root: PathBuf::from("/repo"),
            slot_id: "12345".into(),
            session_id: None,
            approval_pending: false,
            continue_with_merge: true,
        };
        assert_eq!(s.branch, "azureal/feat");
        assert!(s.session_id.is_none());
        assert!(!s.approval_pending);
        assert!(s.continue_with_merge);
    }

    // ══════════════════════════════════════════════════════════════════
    // AutoResolveOverlay
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn auto_resolve_overlay_fields() {
        let o = AutoResolveOverlay {
            files: vec![("AGENTS.md".into(), true), ("CHANGELOG.md".into(), false)],
            selected: 0,
            adding: false,
            input_buffer: String::new(),
            input_cursor: 0,
        };
        assert_eq!(o.files.len(), 2);
        assert!(o.files[0].1);
        assert!(!o.files[1].1);
        assert!(!o.adding);
    }

    // ══════════════════════════════════════════════════════════════════
    // ModuleStyleDialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn module_style_dialog_fields() {
        let d = ModuleStyleDialog {
            has_rust: true,
            has_python: false,
            rust_style: RustModuleStyle::FileBased,
            python_style: PythonModuleStyle::Package,
            selected: 0,
        };
        assert!(d.has_rust);
        assert!(!d.has_python);
        assert_eq!(d.rust_style, RustModuleStyle::FileBased);
        assert_eq!(d.python_style, PythonModuleStyle::Package);
    }

    // ══════════════════════════════════════════════════════════════════
    // HealthPanel
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn health_panel_fields() {
        let p = HealthPanel {
            worktree_name: "my-feature".into(),
            tab: HealthTab::GodFiles,
            god_files: vec![],
            god_selected: 0,
            god_scroll: 0,
            doc_entries: vec![],
            doc_selected: 0,
            doc_scroll: 0,
            doc_score: 0.0,
            module_style_dialog: None,
        };
        assert_eq!(p.worktree_name, "my-feature");
        assert_eq!(p.tab, HealthTab::GodFiles);
        assert!(p.god_files.is_empty());
        assert!(p.module_style_dialog.is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    // GitActionsPanel — field construction (no methods to test, but
    // verifying all fields initialize correctly)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn git_actions_panel_construction() {
        let p = GitActionsPanel {
            worktree_name: "feat-api".into(),
            worktree_path: PathBuf::from("/repo/worktrees/feat-api"),
            repo_root: PathBuf::from("/repo"),
            main_branch: "main".into(),
            is_on_main: false,
            changed_files: vec![],
            selected_file: 0,
            file_scroll: 0,
            focused_pane: 0,
            selected_action: 0,
            result_message: None,
            commit_overlay: None,
            conflict_overlay: None,
            commits: vec![],
            selected_commit: 0,
            commit_scroll: 0,
            viewer_diff: None,
            viewer_diff_title: None,
            commits_behind_main: 0,
            commits_ahead_main: 0,
            commits_behind_remote: 0,
            commits_ahead_remote: 0,
            auto_resolve_files: vec![],
            auto_resolve_overlay: None,
            squash_merge_receiver: None,
            discard_confirm: None,
            cached_staged_count: 0,
            cached_total_add: 0,
            cached_total_del: 0,
        };
        assert_eq!(p.worktree_name, "feat-api");
        assert!(!p.is_on_main);
        assert_eq!(p.focused_pane, 0);
        assert!(p.result_message.is_none());
        assert!(p.commit_overlay.is_none());
        assert!(p.conflict_overlay.is_none());
        assert!(p.auto_resolve_overlay.is_none());
    }
}
