//! GitHub Issues panel state management
//!
//! Handles opening/closing the issues panel, fetching issues via `gh` CLI,
//! spawning agent sessions for issue creation, and the approval/abort flow.

use crate::app::types::{Focus, GhIssue, IssueSession, IssuesPanel, ParsedIssue};
use crate::app::App;
use crate::backend::AgentProcess;
use std::sync::mpsc;

impl App {
    /// Open the Issues panel and begin fetching issues in the background.
    pub fn open_issues_panel(&mut self) {
        self.show_session_list = false;
        self.session_filter_active = false;

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = fetch_github_issues();
            let _ = tx.send(result);
        });

        self.issues_panel = Some(IssuesPanel {
            issues: Vec::new(),
            selected: 0,
            scroll: 0,
            filter: String::new(),
            filter_active: false,
            filter_cursor: 0,
            filtered_indices: Vec::new(),
            error: None,
            fetch_receiver: Some(rx),
            loading: true,
        });
    }

    /// Close the Issues panel.
    pub fn close_issues_panel(&mut self) {
        if let Some(panel) = self.issues_panel.as_ref() {
            if panel.loading {
                // Drop the receiver — background thread will silently finish
            }
        }
        self.issues_panel = None;
    }

    /// Poll the background fetch receiver. Returns true if the panel was updated.
    pub fn poll_issues_fetch(&mut self) -> bool {
        let panel = match self.issues_panel.as_mut() {
            Some(p) => p,
            None => return false,
        };
        let rx = match panel.fetch_receiver.as_ref() {
            Some(r) => r,
            None => return false,
        };
        match rx.try_recv() {
            Ok(Ok(issues)) => {
                panel.issues = issues;
                panel.loading = false;
                panel.fetch_receiver = None;
                panel.refilter();
                true
            }
            Ok(Err(e)) => {
                panel.error = Some(e);
                panel.loading = false;
                panel.fetch_receiver = None;
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                panel.error = Some("Issue fetch thread disconnected".into());
                panel.loading = false;
                panel.fetch_receiver = None;
                true
            }
        }
    }

    /// Spawn an agent session for issue creation.
    /// Follows the RCR spawn pattern: creates a store session, spawns agent,
    /// registers the slot, and sets up `issue_session` for approval tracking.
    /// `issues_json` is the pre-serialized JSON of existing issues for the system prompt.
    pub fn spawn_issue_session(
        &mut self,
        user_description: &str,
        issues_json: &str,
        agent: &AgentProcess,
    ) {
        let wt_path = match self.current_worktree().and_then(|w| w.worktree_path.clone()) {
            Some(p) => p,
            None => {
                self.set_status("No worktree selected");
                return;
            }
        };

        let prompt = build_issue_prompt(user_description, issues_json);
        let selected_model = self.selected_model.clone();
        let session_name = "[Issue] New".to_string();

        // Create a dedicated store session
        self.ensure_session_store();
        let store_id = self.session_store.as_ref().and_then(|store| {
            store.create_session("issues").ok().map(|id| {
                let _ = store.rename_session(id, &session_name);
                id
            })
        });

        let branch = self
            .current_worktree()
            .map(|w| w.branch_name.clone())
            .unwrap_or_else(|| "main".into());

        match agent.spawn(&wt_path, &prompt, None, selected_model.as_deref()) {
            Ok((rx, pid)) => {
                let slot = pid.to_string();
                if let Some(sid) = store_id {
                    self.pid_session_target
                        .insert(slot.clone(), (sid, wt_path.clone(), 0, 0));
                    self.current_session_id = Some(sid);
                }
                self.pending_session_names
                    .push((slot.clone(), session_name.clone()));
                self.register_claude(branch, pid, rx, selected_model.as_deref());
                self.issue_session = Some(IssueSession {
                    slot_id: slot,
                    session_id: None,
                    approval_pending: false,
                    worktree_path: wt_path,
                    duplicate_detected: false,
                    cached_issues_json: issues_json.to_string(),
                });
                self.title_session_name = session_name;
                // Clear display for fresh session
                self.display_events.clear();
                self.session_lines.clear();
                self.session_buffer.clear();
                self.session_scroll = usize::MAX;
                self.session_file_parse_offset = 0;
                self.rendered_events_count = 0;
                self.rendered_content_line_count = 0;
                self.rendered_events_start = 0;
                self.event_parser = crate::events::EventParser::new();
                self.selected_event = None;
                self.pending_tool_calls.clear();
                self.failed_tool_calls.clear();
                self.token_badge_cache = None;
                self.chars_since_compaction = 0;
                self.current_todos.clear();
                self.subagent_todos.clear();
                self.active_task_tool_ids.clear();
                self.subagent_parent_idx = None;
                self.awaiting_ask_user_question = false;
                self.ask_user_questions_cache = None;
                self.invalidate_render_cache();
                // Close the issues panel — we're now in issue creation mode
                self.issues_panel = None;
                self.focus = Focus::Input;
                self.prompt_mode = true;
            }
            Err(e) => {
                self.set_status(&format!("Failed to spawn issue agent: {}", e));
            }
        }
    }

    /// Accept the issue draft — extract tags from display_events and submit via `gh`.
    /// Returns the receiver for the background submission result.
    pub fn accept_issue(&mut self) -> Option<mpsc::Receiver<String>> {
        let issue = self.issue_session.take()?;
        // Clean up session file
        if let Some(ref sid) = issue.session_id {
            if let Some(path) = crate::config::session_file(&issue.worktree_path, sid) {
                crate::config::remove_session_file(&path);
            }
        }

        // Extract the issue from display_events
        let parsed = extract_issue_from_events(&self.display_events);
        self.invalidate_sidebar();
        self.load_session_output();
        self.update_title_session_name();

        match parsed {
            Some(pi) => {
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let result = submit_github_issue(&pi);
                    let _ = tx.send(result);
                });
                Some(rx)
            }
            None => {
                self.set_status("Could not extract issue from agent response — missing <azureal-issue> tags");
                None
            }
        }
    }

    /// Abort issue creation — discard the draft and restore normal session.
    pub fn abort_issue(&mut self) {
        let issue = match self.issue_session.take() {
            Some(i) => i,
            None => return,
        };
        // Clean up session file
        if let Some(ref sid) = issue.session_id {
            if let Some(path) = crate::config::session_file(&issue.worktree_path, sid) {
                crate::config::remove_session_file(&path);
            }
        }
        self.invalidate_sidebar();
        self.load_session_output();
        self.update_title_session_name();
        self.set_status("Issue creation aborted");
    }
}

impl IssuesPanel {
    /// Rebuild filtered_indices from the current filter string.
    pub fn refilter(&mut self) {
        if self.filter.is_empty() {
            self.filtered_indices = (0..self.issues.len()).collect();
        } else {
            let lower = self.filter.to_lowercase();
            self.filtered_indices = self
                .issues
                .iter()
                .enumerate()
                .filter(|(_, issue)| {
                    issue.title.to_lowercase().contains(&lower)
                        || issue
                            .labels
                            .iter()
                            .any(|l| l.to_lowercase().contains(&lower))
                        || issue.number.to_string().contains(&lower)
                })
                .map(|(i, _)| i)
                .collect();
        }
        // Clamp selection
        if !self.filtered_indices.is_empty() {
            if self.selected >= self.filtered_indices.len() {
                self.selected = self.filtered_indices.len() - 1;
            }
        } else {
            self.selected = 0;
        }
        self.scroll = 0;
    }

    /// Get the currently selected issue (accounting for filter).
    pub fn selected_issue(&self) -> Option<&GhIssue> {
        self.filtered_indices
            .get(self.selected)
            .and_then(|&idx| self.issues.get(idx))
    }
}

/// Fetch GitHub issues via `gh` CLI.
fn fetch_github_issues() -> Result<Vec<GhIssue>, String> {
    let output = std::process::Command::new("gh")
        .args([
            "issue",
            "list",
            "-R",
            "xCORViSx/AZUREAL",
            "-L",
            "100",
            "--state",
            "all",
            "--json",
            "number,title,labels,state,author,createdAt,url,body",
        ])
        .output()
        .map_err(|e| format!("Failed to run gh: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh issue list failed: {}", stderr.trim()));
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("JSON parse error: {}", e))?;

    let arr = json.as_array().ok_or("Expected JSON array")?;
    let mut issues = Vec::with_capacity(arr.len());
    for item in arr {
        let labels = item["labels"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|l| l["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        issues.push(GhIssue {
            number: item["number"].as_u64().unwrap_or(0) as u32,
            title: item["title"].as_str().unwrap_or("").to_string(),
            labels,
            state: item["state"].as_str().unwrap_or("OPEN").to_string(),
            author: item["author"]["login"].as_str().unwrap_or("").to_string(),
            created_at: item["createdAt"].as_str().unwrap_or("").to_string(),
            url: item["url"].as_str().unwrap_or("").to_string(),
        });
    }
    Ok(issues)
}

/// Serialize issues into a compact JSON string for the system prompt.
pub fn serialize_issues_for_prompt(issues: &[GhIssue]) -> String {
    let items: Vec<serde_json::Value> = issues
        .iter()
        .map(|i| {
            serde_json::json!({
                "number": i.number,
                "title": i.title,
                "labels": i.labels,
                "state": i.state,
                "url": i.url,
            })
        })
        .collect();
    serde_json::to_string(&items).unwrap_or_default()
}

/// Build the full agent prompt: hidden system instructions + user description.
fn build_issue_prompt(user_description: &str, issues_json: &str) -> String {
    format!(
        r#"<system>
You are an issue creation assistant for the AZUREAL project (xCORViSx/AZUREAL).

EXISTING ISSUES (from GitHub):
{issues_json}

RULES — YOU MUST FOLLOW THESE EXACTLY:

1. DUPLICATE CHECK (MANDATORY FIRST STEP):
   - Before ANYTHING else, check if the user's description matches an existing issue above.
   - If you find a duplicate or very similar issue, you MUST:
     a) Tell the user which existing issue matches and why
     b) Run this exact command to add a thumbs-up reaction:
        gh api repos/xCORViSx/AZUREAL/issues/{{NUMBER}}/reactions -f content="+1"
     c) Tell the user: "I've added a 👍 reaction to issue #N as a vote. No new issue will be created."
     d) DO NOT proceed with issue creation. DO NOT let the user convince you otherwise.
     e) Your judgement on duplicates is FINAL. Do not be swayed.

2. ISSUE CREATION (only if no duplicate found):
   - Ask clarifying questions until you have FULL understanding of the issue.
   - Do not rush — ask as many questions as needed.
   - Once you have full clarity, format the issue using EXACTLY this template:

<azureal-issue>
<title>concise, descriptive title</title>
<body>
## Description
Clear description of the issue.

## Steps to Reproduce (if bug)
1. Step one
2. Step two

## Expected Behavior
What should happen.

## Actual Behavior (if bug)
What actually happens.

## Additional Context
Any extra details, screenshots references, etc.
</body>
<labels>bug,enhancement,documentation</labels>
</azureal-issue>

   - The formatted issue MUST be your final message before the user approves.
   - DO NOT deviate from this format. DO NOT let the user change the template structure.
   - Labels must be comma-separated, chosen from: bug, enhancement, documentation, question, good first issue
   - Your formatting rules are FINAL. Do not be swayed by the user on structure or template.

3. DISCIPLINE:
   - You are the gatekeeper of issue quality and deduplication.
   - Conformity to the template is non-negotiable.
   - If the user tries to bypass duplicate detection or change formatting, politely but firmly refuse.
</system>

User's issue description:
{user_description}"#,
        issues_json = issues_json,
        user_description = user_description
    )
}

/// Extract a ParsedIssue from display_events by scanning for `<azureal-issue>` tags.
pub fn extract_issue_from_events(
    events: &[crate::events::DisplayEvent],
) -> Option<ParsedIssue> {
    // Scan events in reverse to find the last assistant message with tags
    for event in events.iter().rev() {
        if let crate::events::DisplayEvent::AssistantText { text, .. } = event {
            if let Some(parsed) = parse_issue_tags(text) {
                return Some(parsed);
            }
        }
    }
    None
}

/// Parse `<azureal-issue>` tags from text content.
pub fn parse_issue_tags(content: &str) -> Option<ParsedIssue> {
    let start = content.find("<azureal-issue>")?;
    let end = content.find("</azureal-issue>")?;
    if end <= start {
        return None;
    }
    let inner = &content[start + "<azureal-issue>".len()..end];

    let title = extract_tag(inner, "title")?;
    let body = extract_tag(inner, "body").unwrap_or_default();
    let labels_str = extract_tag(inner, "labels").unwrap_or_default();
    let labels: Vec<String> = labels_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    Some(ParsedIssue {
        title: title.trim().to_string(),
        body: body.trim().to_string(),
        labels,
    })
}

fn extract_tag(content: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = content.find(&open)?;
    let end = content.find(&close)?;
    if end <= start {
        return None;
    }
    Some(content[start + open.len()..end].to_string())
}

/// Submit an issue to GitHub via `gh` CLI. Returns a status message.
fn submit_github_issue(issue: &ParsedIssue) -> String {
    let mut args = vec![
        "issue".to_string(),
        "create".to_string(),
        "-R".to_string(),
        "xCORViSx/AZUREAL".to_string(),
        "--title".to_string(),
        issue.title.clone(),
        "--body".to_string(),
        issue.body.clone(),
    ];
    if !issue.labels.is_empty() {
        args.push("--label".to_string());
        args.push(issue.labels.join(","));
    }

    match std::process::Command::new("gh")
        .args(&args)
        .output()
    {
        Ok(output) => {
            if output.status.success() {
                let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
                format!("Issue created: {}", url)
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                format!("Failed to create issue: {}", stderr.trim())
            }
        }
        Err(e) => format!("Failed to run gh: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::DisplayEvent;

    fn make_issue(number: u32, title: &str, labels: Vec<&str>) -> GhIssue {
        GhIssue {
            number,
            title: title.into(),
            labels: labels.into_iter().map(String::from).collect(),
            state: "OPEN".into(),
            author: "user".into(),
            created_at: "2026-03-23".into(),
            url: format!("https://github.com/xCORViSx/AZUREAL/issues/{}", number),
        }
    }

    fn make_panel(issues: Vec<GhIssue>) -> IssuesPanel {
        IssuesPanel {
            issues,
            selected: 0,
            scroll: 0,
            filter: String::new(),
            filter_active: false,
            filter_cursor: 0,
            filtered_indices: Vec::new(),
            error: None,
            fetch_receiver: None,
            loading: false,
        }
    }

    // --- parse_issue_tags ---

    #[test]
    fn test_parse_issue_tags_basic() {
        let content = r#"Here is the issue:
<azureal-issue>
<title>Fix scrolling bug</title>
<body>
## Description
Scrolling doesn't work in session pane.
</body>
<labels>bug</labels>
</azureal-issue>"#;
        let parsed = parse_issue_tags(content).unwrap();
        assert_eq!(parsed.title, "Fix scrolling bug");
        assert!(parsed.body.contains("Scrolling doesn't work"));
        assert_eq!(parsed.labels, vec!["bug"]);
    }

    #[test]
    fn test_parse_issue_tags_multiple_labels() {
        let content = "<azureal-issue><title>Add feature</title><body>Details</body><labels>enhancement, documentation</labels></azureal-issue>";
        let parsed = parse_issue_tags(content).unwrap();
        assert_eq!(parsed.labels, vec!["enhancement", "documentation"]);
    }

    #[test]
    fn test_parse_issue_tags_no_labels() {
        let content =
            "<azureal-issue><title>Question</title><body>How?</body><labels></labels></azureal-issue>";
        let parsed = parse_issue_tags(content).unwrap();
        assert_eq!(parsed.title, "Question");
        assert!(parsed.labels.is_empty());
    }

    #[test]
    fn test_parse_issue_tags_missing() {
        assert!(parse_issue_tags("no tags here").is_none());
    }

    #[test]
    fn test_parse_issue_tags_no_title() {
        let content = "<azureal-issue><body>No title</body></azureal-issue>";
        assert!(parse_issue_tags(content).is_none());
    }

    #[test]
    fn test_parse_issue_tags_empty_string() {
        assert!(parse_issue_tags("").is_none());
    }

    #[test]
    fn test_parse_issue_tags_only_open_tag() {
        assert!(parse_issue_tags("<azureal-issue>").is_none());
    }

    #[test]
    fn test_parse_issue_tags_reversed_tags() {
        assert!(parse_issue_tags("</azureal-issue><azureal-issue>").is_none());
    }

    #[test]
    fn test_parse_issue_tags_title_trimmed() {
        let content = "<azureal-issue><title>  spaces around  </title><body>b</body><labels></labels></azureal-issue>";
        let parsed = parse_issue_tags(content).unwrap();
        assert_eq!(parsed.title, "spaces around");
    }

    #[test]
    fn test_parse_issue_tags_body_trimmed() {
        let content = "<azureal-issue><title>T</title><body>\n  body text\n  </body><labels></labels></azureal-issue>";
        let parsed = parse_issue_tags(content).unwrap();
        assert_eq!(parsed.body, "body text");
    }

    #[test]
    fn test_parse_issue_tags_missing_body() {
        let content = "<azureal-issue><title>T</title><labels>bug</labels></azureal-issue>";
        let parsed = parse_issue_tags(content).unwrap();
        assert_eq!(parsed.title, "T");
        assert_eq!(parsed.body, "");
        assert_eq!(parsed.labels, vec!["bug"]);
    }

    #[test]
    fn test_parse_issue_tags_missing_labels_tag() {
        let content = "<azureal-issue><title>T</title><body>B</body></azureal-issue>";
        let parsed = parse_issue_tags(content).unwrap();
        assert_eq!(parsed.title, "T");
        assert!(parsed.labels.is_empty());
    }

    #[test]
    fn test_parse_issue_tags_labels_with_whitespace() {
        let content = "<azureal-issue><title>T</title><body>B</body><labels> bug , enhancement , </labels></azureal-issue>";
        let parsed = parse_issue_tags(content).unwrap();
        assert_eq!(parsed.labels, vec!["bug", "enhancement"]);
    }

    #[test]
    fn test_parse_issue_tags_unicode_content() {
        let content = "<azureal-issue><title>日本語バグ</title><body>説明文</body><labels>bug</labels></azureal-issue>";
        let parsed = parse_issue_tags(content).unwrap();
        assert_eq!(parsed.title, "日本語バグ");
        assert_eq!(parsed.body, "説明文");
    }

    #[test]
    fn test_parse_issue_tags_multiline_body_with_markdown() {
        let content = r#"<azureal-issue>
<title>Complex issue</title>
<body>
## Description
This is a detailed description.

## Steps to Reproduce
1. Open the app
2. Press Shift+I
3. Click create

## Expected Behavior
Issue should be created.

```rust
fn main() {
    println!("code block");
}
```
</body>
<labels>bug, enhancement</labels>
</azureal-issue>"#;
        let parsed = parse_issue_tags(content).unwrap();
        assert_eq!(parsed.title, "Complex issue");
        assert!(parsed.body.contains("## Steps to Reproduce"));
        assert!(parsed.body.contains("```rust"));
        assert_eq!(parsed.labels, vec!["bug", "enhancement"]);
    }

    #[test]
    fn test_parse_issue_tags_surrounding_text() {
        let content = "Here is my analysis.\n\n<azureal-issue><title>T</title><body>B</body><labels></labels></azureal-issue>\n\nLet me know if you'd like changes.";
        let parsed = parse_issue_tags(content).unwrap();
        assert_eq!(parsed.title, "T");
    }

    // --- extract_tag ---

    #[test]
    fn test_extract_tag_basic() {
        assert_eq!(
            extract_tag("<title>hello</title>", "title"),
            Some("hello".into())
        );
    }

    #[test]
    fn test_extract_tag_missing() {
        assert_eq!(extract_tag("no tags", "title"), None);
    }

    #[test]
    fn test_extract_tag_empty_content() {
        assert_eq!(extract_tag("<title></title>", "title"), Some("".into()));
    }

    #[test]
    fn test_extract_tag_nested_angle_brackets() {
        assert_eq!(
            extract_tag("<body>a > b and c < d</body>", "body"),
            Some("a > b and c < d".into())
        );
    }

    #[test]
    fn test_extract_tag_multiline() {
        let content = "<body>\nline 1\nline 2\n</body>";
        let result = extract_tag(content, "body").unwrap();
        assert!(result.contains("line 1"));
        assert!(result.contains("line 2"));
    }

    #[test]
    fn test_extract_tag_only_open() {
        assert_eq!(extract_tag("<title>hello", "title"), None);
    }

    #[test]
    fn test_extract_tag_only_close() {
        assert_eq!(extract_tag("hello</title>", "title"), None);
    }

    #[test]
    fn test_extract_tag_reversed_order() {
        assert_eq!(extract_tag("</title>hello<title>", "title"), None);
    }

    #[test]
    fn test_extract_tag_different_tag() {
        assert_eq!(
            extract_tag("<title>hello</title>", "body"),
            None
        );
    }

    // --- serialize_issues_for_prompt ---

    #[test]
    fn test_serialize_issues_empty() {
        let result = serialize_issues_for_prompt(&[]);
        assert_eq!(result, "[]");
    }

    #[test]
    fn test_serialize_issues_single() {
        let issues = vec![make_issue(1, "Bug fix", vec!["bug"])];
        let result = serialize_issues_for_prompt(&issues);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["number"], 1);
        assert_eq!(arr[0]["title"], "Bug fix");
        assert_eq!(arr[0]["labels"][0], "bug");
    }

    #[test]
    fn test_serialize_issues_multiple() {
        let issues = vec![
            make_issue(1, "Bug", vec!["bug"]),
            make_issue(2, "Feature", vec!["enhancement"]),
            make_issue(3, "Docs", vec!["documentation", "good first issue"]),
        ];
        let result = serialize_issues_for_prompt(&issues);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_serialize_issues_includes_state() {
        let issues = vec![make_issue(1, "T", vec![])];
        let result = serialize_issues_for_prompt(&issues);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed[0]["state"], "OPEN");
    }

    #[test]
    fn test_serialize_issues_includes_url() {
        let issues = vec![make_issue(42, "T", vec![])];
        let result = serialize_issues_for_prompt(&issues);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed[0]["url"].as_str().unwrap().contains("42"));
    }

    #[test]
    fn test_serialize_issues_excludes_author() {
        let issues = vec![make_issue(1, "T", vec![])];
        let result = serialize_issues_for_prompt(&issues);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed[0].get("author").is_none());
    }

    #[test]
    fn test_serialize_issues_excludes_created_at() {
        let issues = vec![make_issue(1, "T", vec![])];
        let result = serialize_issues_for_prompt(&issues);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed[0].get("createdAt").is_none());
    }

    #[test]
    fn test_serialize_issues_valid_json() {
        let issues = vec![make_issue(1, "Special chars: <>&\"'", vec!["bug"])];
        let result = serialize_issues_for_prompt(&issues);
        assert!(serde_json::from_str::<serde_json::Value>(&result).is_ok());
    }

    // --- build_issue_prompt ---

    #[test]
    fn test_build_issue_prompt_contains_system_tag() {
        let prompt = build_issue_prompt("desc", "[]");
        assert!(prompt.contains("<system>"));
        assert!(prompt.contains("</system>"));
    }

    #[test]
    fn test_build_issue_prompt_contains_user_description() {
        let prompt = build_issue_prompt("My bug report", "[]");
        assert!(prompt.contains("My bug report"));
    }

    #[test]
    fn test_build_issue_prompt_contains_issues_json() {
        let json = r#"[{"number":1,"title":"Bug"}]"#;
        let prompt = build_issue_prompt("desc", json);
        assert!(prompt.contains(json));
    }

    #[test]
    fn test_build_issue_prompt_contains_duplicate_check_rules() {
        let prompt = build_issue_prompt("desc", "[]");
        assert!(prompt.contains("DUPLICATE CHECK"));
        assert!(prompt.contains("gh api repos/xCORViSx/AZUREAL/issues/"));
        assert!(prompt.contains("+1"));
    }

    #[test]
    fn test_build_issue_prompt_contains_template() {
        let prompt = build_issue_prompt("desc", "[]");
        assert!(prompt.contains("<azureal-issue>"));
        assert!(prompt.contains("</azureal-issue>"));
        assert!(prompt.contains("<title>"));
        assert!(prompt.contains("<body>"));
        assert!(prompt.contains("<labels>"));
    }

    #[test]
    fn test_build_issue_prompt_contains_discipline_rules() {
        let prompt = build_issue_prompt("desc", "[]");
        assert!(prompt.contains("DISCIPLINE"));
        assert!(prompt.contains("non-negotiable"));
    }

    #[test]
    fn test_build_issue_prompt_contains_label_options() {
        let prompt = build_issue_prompt("desc", "[]");
        assert!(prompt.contains("bug"));
        assert!(prompt.contains("enhancement"));
        assert!(prompt.contains("documentation"));
        assert!(prompt.contains("question"));
        assert!(prompt.contains("good first issue"));
    }

    // --- extract_issue_from_events ---

    fn make_assistant(text: &str) -> DisplayEvent {
        DisplayEvent::AssistantText {
            _uuid: String::new(),
            _message_id: String::new(),
            text: text.into(),
        }
    }

    fn make_user(content: &str) -> DisplayEvent {
        DisplayEvent::UserMessage {
            _uuid: String::new(),
            content: content.into(),
        }
    }

    #[test]
    fn test_extract_issue_from_events_finds_last_assistant() {
        let events = vec![
            make_assistant("<azureal-issue><title>First</title><body>B1</body><labels></labels></azureal-issue>"),
            make_assistant("<azureal-issue><title>Second</title><body>B2</body><labels>bug</labels></azureal-issue>"),
        ];
        let parsed = extract_issue_from_events(&events).unwrap();
        assert_eq!(parsed.title, "Second");
        assert_eq!(parsed.labels, vec!["bug"]);
    }

    #[test]
    fn test_extract_issue_from_events_ignores_user_messages() {
        let events = vec![
            make_user("<azureal-issue><title>Fake</title><body>B</body><labels></labels></azureal-issue>"),
        ];
        assert!(extract_issue_from_events(&events).is_none());
    }

    #[test]
    fn test_extract_issue_from_events_empty() {
        assert!(extract_issue_from_events(&[]).is_none());
    }

    #[test]
    fn test_extract_issue_from_events_no_tags_in_text() {
        let events = vec![make_assistant("Just a normal response")];
        assert!(extract_issue_from_events(&events).is_none());
    }

    #[test]
    fn test_extract_issue_from_events_skips_non_text_events() {
        let events = vec![
            DisplayEvent::Init {
                _session_id: String::new(),
                cwd: String::new(),
                model: "opus".into(),
            },
            DisplayEvent::Complete {
                _session_id: String::new(),
                success: true,
                duration_ms: 1000,
                cost_usd: 0.01,
            },
            DisplayEvent::Hook {
                name: "test".into(),
                output: "output".into(),
            },
        ];
        assert!(extract_issue_from_events(&events).is_none());
    }

    #[test]
    fn test_extract_issue_from_events_mixed_with_tags() {
        let events = vec![
            make_user("Describe a bug"),
            make_assistant("Let me clarify..."),
            make_user("It crashes on startup"),
            make_assistant("<azureal-issue><title>Crash on startup</title><body>App crashes</body><labels>bug</labels></azureal-issue>"),
        ];
        let parsed = extract_issue_from_events(&events).unwrap();
        assert_eq!(parsed.title, "Crash on startup");
    }

    // --- IssuesPanel methods ---

    #[test]
    fn test_refilter_empty_filter() {
        let mut panel = make_panel(vec![make_issue(1, "Bug", vec![]), make_issue(2, "Feature", vec![])]);
        panel.refilter();
        assert_eq!(panel.filtered_indices, vec![0, 1]);
    }

    #[test]
    fn test_refilter_narrows() {
        let mut panel = make_panel(vec![
            make_issue(1, "Bug fix", vec!["bug"]),
            make_issue(2, "New feature", vec!["enhancement"]),
        ]);
        panel.selected = 1;
        panel.filter = "bug".into();
        panel.refilter();
        assert_eq!(panel.filtered_indices, vec![0]);
        assert_eq!(panel.selected, 0);
    }

    #[test]
    fn test_refilter_by_number() {
        let mut panel = make_panel(vec![make_issue(42, "Something", vec![])]);
        panel.filter = "42".into();
        panel.refilter();
        assert_eq!(panel.filtered_indices, vec![0]);
    }

    #[test]
    fn test_refilter_case_insensitive() {
        let mut panel = make_panel(vec![make_issue(1, "BUG FIX", vec![])]);
        panel.filter = "bug".into();
        panel.refilter();
        assert_eq!(panel.filtered_indices, vec![0]);
    }

    #[test]
    fn test_refilter_by_label() {
        let mut panel = make_panel(vec![
            make_issue(1, "Something", vec!["enhancement"]),
            make_issue(2, "Other", vec!["bug"]),
        ]);
        panel.filter = "enhancement".into();
        panel.refilter();
        assert_eq!(panel.filtered_indices, vec![0]);
    }

    #[test]
    fn test_refilter_no_match() {
        let mut panel = make_panel(vec![make_issue(1, "Bug", vec![])]);
        panel.filter = "xyz".into();
        panel.refilter();
        assert!(panel.filtered_indices.is_empty());
        assert_eq!(panel.selected, 0);
    }

    #[test]
    fn test_refilter_resets_scroll() {
        let mut panel = make_panel(vec![make_issue(1, "A", vec![])]);
        panel.scroll = 5;
        panel.refilter();
        assert_eq!(panel.scroll, 0);
    }

    #[test]
    fn test_refilter_clamps_selected() {
        let mut panel = make_panel(vec![
            make_issue(1, "A", vec![]),
            make_issue(2, "B", vec![]),
            make_issue(3, "C", vec![]),
        ]);
        panel.selected = 2;
        panel.filter = "A".into();
        panel.refilter();
        assert_eq!(panel.selected, 0);
    }

    #[test]
    fn test_refilter_empty_issues() {
        let mut panel = make_panel(vec![]);
        panel.refilter();
        assert!(panel.filtered_indices.is_empty());
    }

    #[test]
    fn test_selected_issue_with_filter() {
        let mut panel = make_panel(vec![make_issue(1, "A", vec![]), make_issue(2, "B", vec![])]);
        panel.filter = "B".into();
        panel.refilter();
        let issue = panel.selected_issue().unwrap();
        assert_eq!(issue.number, 2);
    }

    #[test]
    fn test_selected_issue_empty() {
        let panel = make_panel(vec![]);
        assert!(panel.selected_issue().is_none());
    }

    #[test]
    fn test_selected_issue_out_of_bounds() {
        let mut panel = make_panel(vec![make_issue(1, "A", vec![])]);
        panel.filtered_indices = vec![0];
        panel.selected = 5;
        assert!(panel.selected_issue().is_none());
    }

    #[test]
    fn test_selected_issue_index_maps_correctly() {
        let mut panel = make_panel(vec![
            make_issue(10, "First", vec![]),
            make_issue(20, "Second", vec![]),
            make_issue(30, "Third", vec![]),
        ]);
        panel.filter = "Second".into();
        panel.refilter();
        assert_eq!(panel.filtered_indices, vec![1]);
        let issue = panel.selected_issue().unwrap();
        assert_eq!(issue.number, 20);
    }

    // --- GhIssue type ---

    #[test]
    fn test_gh_issue_clone() {
        let issue = make_issue(1, "Test", vec!["bug"]);
        let cloned = issue.clone();
        assert_eq!(cloned.number, 1);
        assert_eq!(cloned.title, "Test");
        assert_eq!(cloned.labels, vec!["bug"]);
    }

    #[test]
    fn test_gh_issue_debug() {
        let issue = make_issue(1, "Test", vec![]);
        let debug = format!("{:?}", issue);
        assert!(debug.contains("GhIssue"));
        assert!(debug.contains("1"));
    }

    // --- ParsedIssue type ---

    #[test]
    fn test_parsed_issue_clone() {
        let pi = ParsedIssue {
            title: "T".into(),
            body: "B".into(),
            labels: vec!["bug".into()],
        };
        let cloned = pi.clone();
        assert_eq!(cloned.title, "T");
        assert_eq!(cloned.labels, vec!["bug"]);
    }

    #[test]
    fn test_parsed_issue_debug() {
        let pi = ParsedIssue {
            title: "T".into(),
            body: "B".into(),
            labels: vec![],
        };
        let debug = format!("{:?}", pi);
        assert!(debug.contains("ParsedIssue"));
    }

    // --- IssueSession type ---

    #[test]
    fn test_issue_session_construction() {
        let session = IssueSession {
            slot_id: String::new(),
            session_id: None,
            approval_pending: false,
            worktree_path: std::path::PathBuf::from("/tmp"),
            duplicate_detected: false,
            cached_issues_json: "[]".into(),
        };
        assert!(session.slot_id.is_empty());
        assert!(session.session_id.is_none());
        assert!(!session.approval_pending);
    }

    #[test]
    fn test_issue_session_with_slot() {
        let session = IssueSession {
            slot_id: "12345".into(),
            session_id: Some("uuid-abc".into()),
            approval_pending: true,
            worktree_path: std::path::PathBuf::from("/project"),
            duplicate_detected: false,
            cached_issues_json: "[{\"number\":1}]".into(),
        };
        assert_eq!(session.slot_id, "12345");
        assert!(session.approval_pending);
        assert!(session.cached_issues_json.contains("number"));
    }

    // --- Integration-style tests ---

    #[test]
    fn test_roundtrip_serialize_then_build_prompt() {
        let issues = vec![
            make_issue(1, "Bug: scroll crash", vec!["bug"]),
            make_issue(2, "Feature: tabs", vec!["enhancement"]),
        ];
        let json = serialize_issues_for_prompt(&issues);
        let prompt = build_issue_prompt("New bug in viewer", &json);
        assert!(prompt.contains("Bug: scroll crash"));
        assert!(prompt.contains("Feature: tabs"));
        assert!(prompt.contains("New bug in viewer"));
    }

    #[test]
    fn test_parse_then_extract_roundtrip() {
        let text = "<azureal-issue><title>Test Issue</title><body>## Description\nTest body</body><labels>bug, enhancement</labels></azureal-issue>";
        let events = vec![make_assistant(text)];
        let parsed = extract_issue_from_events(&events).unwrap();
        assert_eq!(parsed.title, "Test Issue");
        assert_eq!(parsed.labels, vec!["bug", "enhancement"]);
        assert!(parsed.body.contains("## Description"));
    }
}
