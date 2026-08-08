use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn wt_switch(
    repo_root: &str,
    branch: &str,
    create: bool,
) -> Result<Value, String> {
    let mut args = vec![
        "switch",
        "--no-cd",
        "--format", "json",
        "-C", repo_root,
    ];
    if create {
        args.push("--create");
    }
    args.push(branch);

    let out = Command::new("wt")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}

pub fn wt_remove(repo_root: &str, checkout_path: &str) -> Result<Value, String> {
    let out = Command::new("wt")
        .args(["-C", repo_root, "remove", "--foreground", "--format", "json", checkout_path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}

pub fn wt_list(repo_root: &str, include_branches: bool, include_remotes: bool) -> Result<Value, String> {
    let mut args = vec![
        "list",
        "--format=json",
        "--config-set", "list.json-schema=2",
        "-C", repo_root,
    ];
    if include_branches {
        args.push("--branches");
    }
    if include_remotes {
        args.push("--remotes");
    }

    let out = Command::new("wt")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}

#[derive(Debug, Clone)]
pub struct BranchEntry {
    pub kind: EntryKind,
    pub branch: String,
    #[allow(dead_code)]
    pub path: Option<String>,
    pub symbols: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EntryKind {
    WorktreeCurrent,
    WorktreeMain,
    WorktreeOther,
    BranchLocal,
    BranchRemote,
    NewWorktree, // Synthetic entry for creating a new worktree
}

pub fn parse_wt_list(json: &Value) -> Vec<BranchEntry> {
    let mut entries = Vec::new();
    // Schema 2: items at top level, not under result
    let Some(items) = json.get("items").and_then(|v| v.as_array()) else {
        return entries;
    };

    for item in items {
        let Some(branch) = item.get("branch").and_then(|v| v.as_str()) else {
            continue;
        };
        
        let worktree = item.get("worktree");
        let kind = if let Some(wt) = worktree {
            let main = wt.get("main").and_then(|v| v.as_bool()).unwrap_or(false);
            let current = wt.get("current").and_then(|v| v.as_bool()).unwrap_or(false);
            if current {
                EntryKind::WorktreeCurrent
            } else if main {
                EntryKind::WorktreeMain
            } else {
                EntryKind::WorktreeOther
            }
        } else if item.get("remote").is_some() {
            EntryKind::BranchRemote
        } else {
            EntryKind::BranchLocal
        };

        let path: Option<String> = worktree
            .and_then(|wt| wt.get("path"))
            .and_then(|v| v.as_str())
            .map(String::from);

        // Skip prunable/deleted worktrees (path doesn't exist on disk)
        if matches!(kind, EntryKind::WorktreeCurrent | EntryKind::WorktreeMain | EntryKind::WorktreeOther) {
            if let Some(ref p) = path {
                if !Path::new(p).exists() {
                    continue; // Skip this entry - worktree no longer exists on disk
                }
            }
        }

        let symbols = item
            .pointer("/display/symbols")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        entries.push(BranchEntry {
            kind,
            branch: branch.to_string(),
            path,
            symbols,
        });
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_kind_match() {
        // Ensure EntryKind matches for worktrees
        let entry = BranchEntry {
            kind: EntryKind::WorktreeOther,
            branch: "test-branch".to_string(),
            path: Some("/some/path".to_string()),
            symbols: "".to_string(),
        };
        assert!(matches!(entry.kind, EntryKind::WorktreeCurrent | EntryKind::WorktreeMain | EntryKind::WorktreeOther));
    }
}
