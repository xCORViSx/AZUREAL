//! Branch selection dialog state and input handling

/// State for the branch selection dialog
pub struct BranchDialog {
    pub branches: Vec<String>,
    /// Per-branch worktree count (active + archived)
    pub worktree_counts: Vec<usize>,
    /// Branches already checked out in an active worktree
    pub checked_out: Vec<String>,
    /// 0 = "Create new" row, 1..=N = branch rows
    pub selected: usize,
    pub filter: String,
    /// Cursor byte offset within `filter` (always on a char boundary)
    pub cursor_pos: usize,
    pub filtered_indices: Vec<usize>,
}

impl BranchDialog {
    pub fn new(
        branches: Vec<String>,
        checked_out: Vec<String>,
        worktree_counts: Vec<usize>,
    ) -> Self {
        let filtered_indices: Vec<usize> = (0..branches.len()).collect();
        Self {
            branches,
            worktree_counts,
            checked_out,
            selected: 0,
            filter: String::new(),
            cursor_pos: 0,
            filtered_indices,
        }
    }

    /// True if "Create new" row is selected
    pub fn on_create_new(&self) -> bool {
        self.selected == 0
    }

    /// Total display rows: 1 ("Create new") + filtered branches
    pub fn display_len(&self) -> usize {
        1 + self.filtered_indices.len()
    }

    /// Worktree count for a branch index
    pub fn worktree_count(&self, branch_idx: usize) -> usize {
        self.worktree_counts.get(branch_idx).copied().unwrap_or(0)
    }

    /// True if the branch is already checked out in a worktree
    pub fn is_checked_out(&self, branch: &str) -> bool {
        let local_name = if branch.contains('/') {
            branch.split('/').skip(1).collect::<Vec<_>>().join("/")
        } else {
            branch.to_string()
        };
        self.checked_out
            .iter()
            .any(|co| co == branch || co == &local_name)
    }

    pub fn apply_filter(&mut self) {
        let filter_lower = self.filter.to_lowercase();
        self.filtered_indices = self
            .branches
            .iter()
            .enumerate()
            .filter(|(_, b)| b.to_lowercase().contains(&filter_lower))
            .map(|(i, _)| i)
            .collect();
        if self.selected >= self.display_len() {
            self.selected = 0;
        }
    }

    /// Get the selected branch (None if on "Create new" row)
    pub fn selected_branch(&self) -> Option<&String> {
        if self.selected == 0 {
            return None;
        }
        let branch_idx = self.selected - 1;
        self.filtered_indices
            .get(branch_idx)
            .and_then(|&idx| self.branches.get(idx))
    }

    pub fn select_next(&mut self) {
        if self.selected + 1 < self.display_len() {
            self.selected += 1;
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn filter_char(&mut self, c: char) {
        if is_git_safe_char(c) {
            self.filter.insert(self.cursor_pos, c);
            self.cursor_pos += c.len_utf8();
            self.apply_filter();
        }
    }

    pub fn filter_backspace(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.filter[..self.cursor_pos]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.filter.remove(prev);
            self.cursor_pos = prev;
            self.apply_filter();
        }
    }

    pub fn cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos = self.filter[..self.cursor_pos]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    pub fn cursor_right(&mut self) {
        if self.cursor_pos < self.filter.len() {
            self.cursor_pos = self.filter[self.cursor_pos..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor_pos + i)
                .unwrap_or(self.filter.len());
        }
    }
}

/// Check if a character is valid in a git branch/worktree name
pub fn is_git_safe_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '+' | '@' | '/' | '!')
}
