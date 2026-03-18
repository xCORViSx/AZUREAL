//! Diff and patch parsing types and extraction
//!
//! Parses apply-patch format and unified diff format into structured line types,
//! and extracts preview strings (old/new) from edit tool inputs.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ApplyPatchLineKind {
    Header,
    Meta,
    Hunk,
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApplyPatchLine {
    pub kind: ApplyPatchLineKind,
    pub text: String,
}

pub fn extract_edit_preview_strings(input: &serde_json::Value) -> (String, String) {
    let old = input
        .get("old_string")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let new = input
        .get("new_string")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !old.is_empty() || !new.is_empty() {
        return (old.to_string(), new.to_string());
    }

    if let Some(diff) = input.get("unified_diff").and_then(|v| v.as_str()) {
        return extract_unified_diff_first_hunk(diff);
    }

    let Some(patch) = input.get("patch").and_then(|v| v.as_str()) else {
        return (String::new(), String::new());
    };

    extract_apply_patch_first_hunk(patch)
}

fn extract_apply_patch_first_hunk(patch: &str) -> (String, String) {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Section {
        None,
        Update,
        Add,
        Delete,
    }

    let mut section = Section::None;
    let mut old = Vec::new();
    let mut new = Vec::new();
    let mut started = false;

    for raw in patch.lines() {
        if raw == "*** Begin Patch" || raw == "*** End Patch" {
            continue;
        }

        if raw.starts_with("*** Update File: ") {
            if started {
                break;
            }
            section = Section::Update;
            continue;
        }
        if raw.starts_with("*** Add File: ") {
            if started {
                break;
            }
            section = Section::Add;
            continue;
        }
        if raw.starts_with("*** Delete File: ") {
            if started {
                break;
            }
            section = Section::Delete;
            continue;
        }
        if raw.starts_with("*** Move to: ") {
            continue;
        }
        if raw.starts_with("*** ") {
            if started {
                break;
            }
            continue;
        }
        if raw.starts_with("@@") {
            if started && (!old.is_empty() || !new.is_empty()) {
                break;
            }
            continue;
        }

        match section {
            Section::Update => {
                if let Some(rest) = raw.strip_prefix('+') {
                    new.push(rest.to_string());
                    started = true;
                } else if let Some(rest) = raw.strip_prefix('-') {
                    old.push(rest.to_string());
                    started = true;
                } else if let Some(rest) = raw.strip_prefix(' ') {
                    if started {
                        old.push(rest.to_string());
                        new.push(rest.to_string());
                    }
                }
            }
            Section::Add => {
                if let Some(rest) = raw.strip_prefix('+') {
                    new.push(rest.to_string());
                    started = true;
                } else if started {
                    break;
                }
            }
            Section::Delete => {
                if let Some(rest) = raw.strip_prefix('-') {
                    old.push(rest.to_string());
                    started = true;
                } else if started {
                    break;
                }
            }
            Section::None => {}
        }
    }

    (old.join("\n"), new.join("\n"))
}

fn extract_unified_diff_first_hunk(diff: &str) -> (String, String) {
    let mut old = Vec::new();
    let mut new = Vec::new();
    let mut in_hunk = false;
    let mut started = false;

    for raw in diff.lines() {
        if raw.starts_with("diff --git ") {
            if started {
                break;
            }
            continue;
        }
        if raw.starts_with("@@") {
            if started && (!old.is_empty() || !new.is_empty()) {
                break;
            }
            in_hunk = true;
            continue;
        }
        if !in_hunk || raw == "\\ No newline at end of file" {
            continue;
        }

        if raw.starts_with("+++") || raw.starts_with("---") {
            continue;
        }

        if let Some(rest) = raw.strip_prefix('+') {
            new.push(rest.to_string());
            started = true;
            continue;
        }
        if let Some(rest) = raw.strip_prefix('-') {
            old.push(rest.to_string());
            started = true;
            continue;
        }
        if let Some(rest) = raw.strip_prefix(' ') {
            if started {
                old.push(rest.to_string());
                new.push(rest.to_string());
            }
            continue;
        }
        if started {
            break;
        }
    }

    (old.join("\n"), new.join("\n"))
}

pub(crate) fn parse_apply_patch_lines(patch: &str) -> Vec<ApplyPatchLine> {
    let mut lines = Vec::new();

    for raw in patch.lines() {
        if raw == "*** Begin Patch" || raw == "*** End Patch" {
            continue;
        }

        let (kind, text) = if let Some(rest) = raw.strip_prefix("*** Update File: ") {
            (
                ApplyPatchLineKind::Header,
                format!("Update File: {}", rest.trim()),
            )
        } else if let Some(rest) = raw.strip_prefix("*** Add File: ") {
            (
                ApplyPatchLineKind::Header,
                format!("Add File: {}", rest.trim()),
            )
        } else if let Some(rest) = raw.strip_prefix("*** Delete File: ") {
            (
                ApplyPatchLineKind::Header,
                format!("Delete File: {}", rest.trim()),
            )
        } else if let Some(rest) = raw.strip_prefix("*** Move to: ") {
            (
                ApplyPatchLineKind::Meta,
                format!("Move to: {}", rest.trim()),
            )
        } else if raw.starts_with("@@") {
            (ApplyPatchLineKind::Hunk, raw.to_string())
        } else if raw.starts_with('+') {
            (ApplyPatchLineKind::Added, raw.to_string())
        } else if raw.starts_with('-') {
            (ApplyPatchLineKind::Removed, raw.to_string())
        } else if raw.starts_with(' ') {
            (ApplyPatchLineKind::Context, raw.to_string())
        } else {
            (ApplyPatchLineKind::Meta, raw.to_string())
        };

        lines.push(ApplyPatchLine { kind, text });
    }

    lines
}

pub(crate) fn parse_unified_diff_lines(diff: &str) -> Vec<ApplyPatchLine> {
    let mut lines = Vec::new();

    for raw in diff.lines() {
        let kind = if raw.starts_with("diff --git ") {
            ApplyPatchLineKind::Header
        } else if raw.starts_with("index ") || raw.starts_with("--- ") || raw.starts_with("+++ ") {
            ApplyPatchLineKind::Meta
        } else if raw.starts_with("@@") {
            ApplyPatchLineKind::Hunk
        } else if raw.starts_with('+') {
            ApplyPatchLineKind::Added
        } else if raw.starts_with('-') {
            ApplyPatchLineKind::Removed
        } else if raw.starts_with(' ') {
            ApplyPatchLineKind::Context
        } else {
            ApplyPatchLineKind::Meta
        };

        lines.push(ApplyPatchLine {
            kind,
            text: raw.to_string(),
        });
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_edit_preview_strings_prefers_explicit_fields() {
        let input = json!({
            "old_string": "before",
            "new_string": "after",
            "patch": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-before\n+after\n*** End Patch"
        });
        let (old, new) = extract_edit_preview_strings(&input);
        assert_eq!(old, "before");
        assert_eq!(new, "after");
    }

    #[test]
    fn extract_edit_preview_strings_from_update_patch() {
        let input = json!({
            "patch": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n fn main() {\n-    old_call();\n+    new_call();\n }\n*** End Patch"
        });
        let (old, new) = extract_edit_preview_strings(&input);
        assert_eq!(old, "    old_call();\n}");
        assert_eq!(new, "    new_call();\n}");
    }

    #[test]
    fn extract_edit_preview_strings_from_add_patch() {
        let input = json!({
            "patch": "*** Begin Patch\n*** Add File: src/new.rs\n+fn main() {}\n+println!(\"hi\");\n*** End Patch"
        });
        let (old, new) = extract_edit_preview_strings(&input);
        assert!(old.is_empty());
        assert_eq!(new, "fn main() {}\nprintln!(\"hi\");");
    }

    #[test]
    fn extract_edit_preview_strings_from_unified_diff() {
        let input = json!({
            "unified_diff": "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,3 @@\n fn main() {\n-    old_call();\n+    new_call();\n }\n"
        });
        let (old, new) = extract_edit_preview_strings(&input);
        assert_eq!(old, "    old_call();\n}");
        assert_eq!(new, "    new_call();\n}");
    }
}
