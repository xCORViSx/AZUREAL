//! Dialog overlays (help, context menu, branch dialog, session creation)
//!
//! Thin orchestrator — each dialog lives in its own submodule file.

mod help_overlay;
mod preset_prompt;
mod run_command;
mod table_popup;
mod welcome_modal;
mod worktree_dialogs;

pub use help_overlay::draw_help_overlay;
pub use preset_prompt::{draw_preset_prompt_dialog, draw_preset_prompt_picker};
pub use run_command::{draw_run_command_dialog, draw_run_command_picker};
pub use table_popup::draw_table_popup;
pub use welcome_modal::draw_welcome_modal;
pub use worktree_dialogs::{
    draw_branch_dialog, draw_delete_worktree_dialog, draw_rename_worktree_dialog,
};

#[cfg(test)]
mod tests {
    use crate::app::types::{
        CommandFieldMode, PresetPrompt, PresetPromptDialog, PresetPromptPicker, RunCommand,
        RunCommandDialog, RunCommandPicker,
    };
    use crate::app::BranchDialog;
    use crate::tui::util::{truncate, AZURE};
    use ratatui::{
        layout::{Constraint, Layout, Rect},
        style::{Color, Modifier, Style},
        text::{Line, Span},
    };

    // ══════════════════════════════════════════════════════════════════
    // BranchDialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_branch_dialog_new_populates_filtered_indices() {
        let d = BranchDialog::new(vec!["main".into(), "dev".into()], vec![], vec![0, 0]);
        assert_eq!(d.filtered_indices, vec![0, 1]);
    }

    #[test]
    fn test_branch_dialog_new_selected_starts_zero() {
        let d = BranchDialog::new(vec!["a".into()], vec![], vec![0]);
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn test_branch_dialog_new_filter_empty() {
        let d = BranchDialog::new(vec!["x".into()], vec![], vec![0]);
        assert!(d.filter.is_empty());
    }

    #[test]
    fn test_branch_dialog_is_checked_out_exact() {
        let d = BranchDialog::new(vec![], vec!["main".into()], vec![]);
        assert!(d.is_checked_out("main"));
    }

    #[test]
    fn test_branch_dialog_is_checked_out_remote_prefix() {
        let d = BranchDialog::new(vec![], vec!["feature".into()], vec![]);
        assert!(d.is_checked_out("origin/feature"));
    }

    #[test]
    fn test_branch_dialog_is_checked_out_false() {
        let d = BranchDialog::new(vec![], vec!["main".into()], vec![]);
        assert!(!d.is_checked_out("dev"));
    }

    #[test]
    fn test_branch_dialog_apply_filter_narrows() {
        let mut d = BranchDialog::new(
            vec!["main".into(), "dev".into(), "feature".into()],
            vec![],
            vec![0, 0, 0],
        );
        d.filter = "dev".into();
        d.apply_filter();
        assert_eq!(d.filtered_indices, vec![1]);
    }

    #[test]
    fn test_branch_dialog_apply_filter_case_insensitive() {
        let mut d = BranchDialog::new(vec!["Main".into(), "DEV".into()], vec![], vec![0, 0]);
        d.filter = "dev".into();
        d.apply_filter();
        assert_eq!(d.filtered_indices, vec![1]);
    }

    #[test]
    fn test_branch_dialog_apply_filter_empty_shows_all() {
        let mut d = BranchDialog::new(vec!["a".into(), "b".into()], vec![], vec![0, 0]);
        d.filter.clear();
        d.apply_filter();
        assert_eq!(d.filtered_indices, vec![0, 1]);
    }

    #[test]
    fn test_branch_dialog_apply_filter_resets_selected() {
        let mut d = BranchDialog::new(
            vec!["a".into(), "b".into(), "c".into()],
            vec![],
            vec![0, 0, 0],
        );
        d.selected = 2;
        d.filter = "z".into();
        d.apply_filter();
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn test_branch_dialog_selected_branch() {
        let mut d = BranchDialog::new(vec!["a".into(), "b".into()], vec![], vec![0, 0]);
        // selected==0 is "[+] Create new", move to first branch
        d.select_next();
        assert_eq!(d.selected_branch().unwrap(), "a");
    }

    #[test]
    fn test_branch_dialog_selected_branch_after_filter() {
        let mut d = BranchDialog::new(vec!["alpha".into(), "beta".into()], vec![], vec![0, 0]);
        d.filter = "bet".into();
        d.apply_filter();
        // selected==0 is "[+] Create new", move to first filtered branch
        d.select_next();
        assert_eq!(d.selected_branch().unwrap(), "beta");
    }

    #[test]
    fn test_branch_dialog_selected_branch_empty() {
        let mut d = BranchDialog::new(vec!["a".into()], vec![], vec![0]);
        d.filter = "zzz".into();
        d.apply_filter();
        assert!(d.selected_branch().is_none());
    }

    #[test]
    fn test_branch_dialog_select_next() {
        let mut d = BranchDialog::new(
            vec!["a".into(), "b".into(), "c".into()],
            vec![],
            vec![0, 0, 0],
        );
        d.select_next();
        assert_eq!(d.selected, 1);
        d.select_next();
        assert_eq!(d.selected, 2);
    }

    #[test]
    fn test_branch_dialog_select_next_at_end() {
        // display_len = 1 (Create new) + 2 branches = 3, max selected = 2
        let mut d = BranchDialog::new(vec!["a".into(), "b".into()], vec![], vec![0, 0]);
        d.selected = 2;
        d.select_next();
        assert_eq!(d.selected, 2); // no change — already at end
    }

    #[test]
    fn test_branch_dialog_select_prev() {
        let mut d = BranchDialog::new(vec!["a".into(), "b".into()], vec![], vec![0, 0]);
        d.selected = 1;
        d.select_prev();
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn test_branch_dialog_select_prev_at_zero() {
        let mut d = BranchDialog::new(vec!["a".into()], vec![], vec![0]);
        d.select_prev();
        assert_eq!(d.selected, 0);
    }

    #[test]
    fn test_branch_dialog_filter_char() {
        let mut d = BranchDialog::new(vec!["abc".into(), "def".into()], vec![], vec![0, 0]);
        d.filter_char('a');
        assert_eq!(d.filter, "a");
        assert_eq!(d.filtered_indices, vec![0]);
    }

    #[test]
    fn test_branch_dialog_filter_backspace() {
        let mut d = BranchDialog::new(vec!["abc".into(), "def".into()], vec![], vec![0, 0]);
        d.filter = "ab".into();
        d.cursor_pos = 2; // cursor at end
        d.filter_backspace();
        assert_eq!(d.filter, "a");
    }

    #[test]
    fn test_branch_dialog_filter_backspace_empty() {
        let mut d = BranchDialog::new(vec!["abc".into()], vec![], vec![0]);
        d.filter_backspace();
        assert!(d.filter.is_empty());
    }

    // ══════════════════════════════════════════════════════════════════
    // Layout centering math
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_dialog_centering_60x20_on_100x50() {
        let area_w = 100u16;
        let area_h = 50u16;
        let dw = 60u16.min(area_w.saturating_sub(4));
        let dh = 20u16.min(area_h.saturating_sub(4));
        let dx = (area_w.saturating_sub(dw)) / 2;
        let dy = (area_h.saturating_sub(dh)) / 2;
        assert_eq!(dw, 60);
        assert_eq!(dh, 20);
        assert_eq!(dx, 20);
        assert_eq!(dy, 15);
    }

    #[test]
    fn test_dialog_centering_small_terminal() {
        let area_w = 30u16;
        let area_h = 10u16;
        let dw = 60u16.min(area_w.saturating_sub(4));
        let dh = 20u16.min(area_h.saturating_sub(4));
        assert_eq!(dw, 26);
        assert_eq!(dh, 6);
    }

    #[test]
    fn test_dialog_centering_very_small() {
        let area_w = 5u16;
        let dw = 60u16.min(area_w.saturating_sub(4));
        assert_eq!(dw, 1);
    }

    #[test]
    fn test_modal_50_centering() {
        let area_w = 80u16;
        let mw = 50u16.min(area_w.saturating_sub(4));
        let mx = (area_w.saturating_sub(mw)) / 2;
        assert_eq!(mw, 50);
        assert_eq!(mx, 15);
    }

    // ══════════════════════════════════════════════════════════════════
    // Rect construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_rect_new() {
        let r = Rect::new(10, 20, 30, 40);
        assert_eq!(r.x, 10);
        assert_eq!(r.y, 20);
        assert_eq!(r.width, 30);
        assert_eq!(r.height, 40);
    }

    #[test]
    fn test_rect_area() {
        let r = Rect::new(0, 0, 10, 10);
        assert_eq!(r.area(), 100);
    }

    // ══════════════════════════════════════════════════════════════════
    // Style construction
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_azure_color_value() {
        assert_eq!(AZURE, Color::Rgb(51, 153, 255));
    }

    #[test]
    fn test_style_fg_azure() {
        let s = Style::default().fg(AZURE);
        assert_eq!(s.fg, Some(AZURE));
    }

    #[test]
    fn test_style_bold_modifier() {
        let s = Style::default().add_modifier(Modifier::BOLD);
        assert!(s.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_style_bg_blue_fg_white() {
        let s = Style::default().bg(Color::Blue).fg(Color::White);
        assert_eq!(s.bg, Some(Color::Blue));
        assert_eq!(s.fg, Some(Color::White));
    }

    // ══════════════════════════════════════════════════════════════════
    // Span and Line
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_span_raw_content() {
        let s = Span::raw("hello");
        assert_eq!(s.content.as_ref(), "hello");
    }

    #[test]
    fn test_span_styled_content() {
        let s = Span::styled("text", Style::default().fg(Color::Red));
        assert_eq!(s.content.as_ref(), "text");
        assert_eq!(s.style.fg, Some(Color::Red));
    }

    #[test]
    fn test_line_from_spans() {
        let line = Line::from(vec![Span::raw("a"), Span::raw("b")]);
        assert_eq!(line.spans.len(), 2);
    }

    #[test]
    fn test_line_from_string() {
        let line = Line::from("hello");
        assert_eq!(line.spans.len(), 1);
    }

    // ══════════════════════════════════════════════════════════════════
    // truncate
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn test_truncate_exact() {
        assert_eq!(truncate("abcde", 5), "abcde");
    }

    #[test]
    fn test_truncate_over() {
        let r = truncate("abcdef", 4);
        assert_eq!(r.chars().count(), 4);
        assert!(r.ends_with('\u{2026}'));
    }

    // ══════════════════════════════════════════════════════════════════
    // CommandFieldMode
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_command_field_mode_command() {
        let m = CommandFieldMode::Command;
        assert_eq!(m, CommandFieldMode::Command);
    }

    #[test]
    fn test_command_field_mode_prompt() {
        let m = CommandFieldMode::Prompt;
        assert_eq!(m, CommandFieldMode::Prompt);
    }

    #[test]
    fn test_command_field_mode_ne() {
        assert_ne!(CommandFieldMode::Command, CommandFieldMode::Prompt);
    }

    #[test]
    fn test_field_title_command() {
        let (field_title, mode_hint) = match CommandFieldMode::Command {
            CommandFieldMode::Command => (" Command ", " Tab:Prompt "),
            CommandFieldMode::Prompt => (" Prompt ", " Tab:Command "),
        };
        assert_eq!(field_title, " Command ");
        assert_eq!(mode_hint, " Tab:Prompt ");
    }

    #[test]
    fn test_field_title_prompt() {
        let (field_title, mode_hint) = match CommandFieldMode::Prompt {
            CommandFieldMode::Command => (" Command ", " Tab:Prompt "),
            CommandFieldMode::Prompt => (" Prompt ", " Tab:Command "),
        };
        assert_eq!(field_title, " Prompt ");
        assert_eq!(mode_hint, " Tab:Command ");
    }

    // ══════════════════════════════════════════════════════════════════
    // RunCommandDialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_run_command_dialog_new_defaults() {
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
    fn test_run_command_dialog_edit() {
        let cmd = RunCommand::new("build", "cargo build", true);
        let d = RunCommandDialog::edit(3, &cmd);
        assert_eq!(d.name, "build");
        assert_eq!(d.command, "cargo build");
        assert_eq!(d.editing_idx, Some(3));
        assert!(d.global);
    }

    // ══════════════════════════════════════════════════════════════════
    // RunCommandPicker
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_run_command_picker_new() {
        let p = RunCommandPicker::new();
        assert_eq!(p.selected, 0);
        assert!(p.confirm_delete.is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    // PresetPrompt
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_preset_prompt_new() {
        let p = PresetPrompt::new("test", "run tests", false);
        assert_eq!(p.name, "test");
        assert_eq!(p.prompt, "run tests");
        assert!(!p.global);
    }

    #[test]
    fn test_preset_prompt_global() {
        let p = PresetPrompt::new("g", "p", true);
        assert!(p.global);
    }

    // ══════════════════════════════════════════════════════════════════
    // PresetPromptPicker
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_preset_prompt_picker_new() {
        let p = PresetPromptPicker::new();
        assert_eq!(p.selected, 0);
        assert!(p.confirm_delete.is_none());
    }

    // ══════════════════════════════════════════════════════════════════
    // PresetPromptDialog
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_preset_prompt_dialog_new() {
        let d = PresetPromptDialog::new();
        assert!(d.name.is_empty());
        assert!(d.prompt.is_empty());
        assert!(d.editing_name);
        assert!(d.editing_idx.is_none());
    }

    #[test]
    fn test_preset_prompt_dialog_edit() {
        let p = PresetPrompt::new("fix", "fix the bug", false);
        let d = PresetPromptDialog::edit(2, &p);
        assert_eq!(d.name, "fix");
        assert_eq!(d.prompt, "fix the bug");
        assert_eq!(d.editing_idx, Some(2));
    }

    // ══════════════════════════════════════════════════════════════════
    // Number hint formatting (from draw_preset_prompt_picker logic)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_num_hint_first_nine() {
        for idx in 0..9usize {
            let hint = format!(" [{}] ", idx + 1);
            assert!(hint.contains(&(idx + 1).to_string()));
        }
    }

    #[test]
    fn test_num_hint_tenth() {
        let idx = 9usize;
        let hint = if idx == 9 {
            " [0] ".to_string()
        } else {
            "     ".to_string()
        };
        assert_eq!(hint, " [0] ");
    }

    #[test]
    fn test_num_hint_eleventh_plus() {
        let idx = 10usize;
        let hint = if idx < 9 {
            format!(" [{}] ", idx + 1)
        } else if idx == 9 {
            " [0] ".to_string()
        } else {
            "     ".to_string()
        };
        assert_eq!(hint, "     ");
    }

    // ══════════════════════════════════════════════════════════════════
    // Scope badge logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_scope_badge_global() {
        let global = true;
        let badge = if global { " G " } else { " P " };
        assert_eq!(badge, " G ");
    }

    #[test]
    fn test_scope_badge_project() {
        let global = false;
        let badge = if global { " G " } else { " P " };
        assert_eq!(badge, " P ");
    }

    // ══════════════════════════════════════════════════════════════════
    // Filter title logic
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_filter_title_empty() {
        let filter = "";
        let title = if filter.is_empty() {
            " Filter (type to search) "
        } else {
            " Filter "
        };
        assert_eq!(title, " Filter (type to search) ");
    }

    #[test]
    fn test_filter_title_non_empty() {
        let filter = "main";
        let title = if filter.is_empty() {
            " Filter (type to search) "
        } else {
            " Filter "
        };
        assert_eq!(title, " Filter ");
    }

    // ══════════════════════════════════════════════════════════════════
    // Title formatting (from draw_branch_dialog)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_branch_title_format() {
        let filtered = 5usize;
        let total = 10usize;
        let title = format!(" Branches ({}/{}) ", filtered, total);
        assert_eq!(title, " Branches (5/10) ");
    }

    #[test]
    fn test_branch_title_format_all_shown() {
        let n = 3usize;
        let title = format!(" Branches ({}/{}) ", n, n);
        assert_eq!(title, " Branches (3/3) ");
    }

    // ══════════════════════════════════════════════════════════════════
    // Dialog title text selection (edit vs new)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_edit_title_run_command() {
        let editing_idx: Option<usize> = Some(0);
        let title = if editing_idx.is_some() {
            " Edit Run Command "
        } else {
            " New Run Command "
        };
        assert_eq!(title, " Edit Run Command ");
    }

    #[test]
    fn test_new_title_run_command() {
        let editing_idx: Option<usize> = None;
        let title = if editing_idx.is_some() {
            " Edit Run Command "
        } else {
            " New Run Command "
        };
        assert_eq!(title, " New Run Command ");
    }

    #[test]
    fn test_edit_title_preset() {
        let editing_idx: Option<usize> = Some(2);
        let title = if editing_idx.is_some() {
            " Edit Preset "
        } else {
            " New Preset "
        };
        assert_eq!(title, " Edit Preset ");
    }

    #[test]
    fn test_new_title_preset() {
        let editing_idx: Option<usize> = None;
        let title = if editing_idx.is_some() {
            " Edit Preset "
        } else {
            " New Preset "
        };
        assert_eq!(title, " New Preset ");
    }

    // ══════════════════════════════════════════════════════════════════
    // Enter label logic (from draw_run_command_dialog)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_enter_label_editing_name() {
        let editing_name = true;
        let field_mode = CommandFieldMode::Command;
        let label = if editing_name {
            "next"
        } else {
            match field_mode {
                CommandFieldMode::Command => "save",
                CommandFieldMode::Prompt => "generate",
            }
        };
        assert_eq!(label, "next");
    }

    #[test]
    fn test_enter_label_command_mode() {
        let editing_name = false;
        let field_mode = CommandFieldMode::Command;
        let label = if editing_name {
            "next"
        } else {
            match field_mode {
                CommandFieldMode::Command => "save",
                CommandFieldMode::Prompt => "generate",
            }
        };
        assert_eq!(label, "save");
    }

    #[test]
    fn test_enter_label_prompt_mode() {
        let editing_name = false;
        let field_mode = CommandFieldMode::Prompt;
        let label = if editing_name {
            "next"
        } else {
            match field_mode {
                CommandFieldMode::Command => "save",
                CommandFieldMode::Prompt => "generate",
            }
        };
        assert_eq!(label, "generate");
    }

    // ══════════════════════════════════════════════════════════════════
    // Constraint and Layout checks
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_constraint_length() {
        let c = Constraint::Length(3);
        assert_eq!(c, Constraint::Length(3));
    }

    #[test]
    fn test_constraint_min() {
        let c = Constraint::Min(5);
        assert_eq!(c, Constraint::Min(5));
    }

    #[test]
    fn test_layout_vertical_split() {
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(Rect::new(0, 0, 60, 7));
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].height, 3);
        assert_eq!(chunks[1].height, 3);
        assert_eq!(chunks[2].height, 1);
    }

    // ══════════════════════════════════════════════════════════════════
    // RunCommand
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_run_command_new() {
        let c = RunCommand::new("test", "cargo test", false);
        assert_eq!(c.name, "test");
        assert_eq!(c.command, "cargo test");
        assert!(!c.global);
    }

    #[test]
    fn test_run_command_global() {
        let c = RunCommand::new("deploy", "deploy.sh", true);
        assert!(c.global);
    }

    #[test]
    fn test_run_command_clone() {
        let c = RunCommand::new("build", "make", false);
        let cloned = c.clone();
        assert_eq!(cloned.name, c.name);
        assert_eq!(cloned.command, c.command);
        assert_eq!(cloned.global, c.global);
    }
}
