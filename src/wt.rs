use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};

const INSTALL_INSTRUCTIONS: &str =
    "Install Worktrunk with: brew install worktrunk (or cargo install worktrunk)";

pub fn ensure_available() -> Result<(), String> {
    let mut command = Command::new("wt");
    command.arg("--version");
    ensure_available_with(&mut command)
}

fn ensure_available_with(command: &mut Command) -> Result<(), String> {
    let output = command.output().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            format!("Worktrunk (wt) is required but was not found. {INSTALL_INSTRUCTIONS}")
        } else {
            format!("Could not run Worktrunk (wt): {error}")
        }
    })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        Err(format!(
            "Worktrunk (wt) exited with {}. {INSTALL_INSTRUCTIONS}",
            output.status
        ))
    } else {
        Err(format!("Worktrunk (wt) could not run: {stderr}"))
    }
}

pub fn wt_switch(repo_root: &str, branch: &str, create: bool) -> Result<Value, String> {
    let mut args = vec!["switch", "--no-cd", "--format", "json", "-C", repo_root];
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
        .args([
            "-C",
            repo_root,
            "remove",
            "--foreground",
            "--format",
            "json",
            checkout_path,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}

pub fn wt_list(
    repo_root: &str,
    include_branches: bool,
    include_remotes: bool,
) -> Result<Value, String> {
    let mut args = vec![
        "list",
        "--format=json",
        "--config-set",
        "list.json-schema=2",
        "-C",
        repo_root,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemovalSafety {
    Safe,
    Dirty,
    BranchCheckedOutElsewhere,
    BranchNotIntegrated,
    Unknown,
}

impl RemovalSafety {
    pub fn allows_removal(&self) -> bool {
        matches!(
            self,
            Self::Safe | Self::BranchCheckedOutElsewhere | Self::BranchNotIntegrated
        )
    }

    pub fn as_env_value(&self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Dirty => "dirty",
            Self::BranchCheckedOutElsewhere => "branch-checked-out-elsewhere",
            Self::BranchNotIntegrated => "branch-not-integrated",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_env_value(value: &str) -> Option<Self> {
        match value {
            "safe" => Some(Self::Safe),
            "dirty" => Some(Self::Dirty),
            "branch-checked-out-elsewhere" => Some(Self::BranchCheckedOutElsewhere),
            "branch-not-integrated" => Some(Self::BranchNotIntegrated),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

pub fn removal_safety(list: &Value, checkout_path: &str) -> RemovalSafety {
    let Some(item) = list
        .get("items")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find(|item| {
                item.pointer("/worktree/path").and_then(Value::as_str) == Some(checkout_path)
            })
        })
    else {
        return RemovalSafety::Unknown;
    };

    let Some(worktree) = item.get("worktree") else {
        return RemovalSafety::Unknown;
    };

    let dirty = ["staged", "modified", "untracked", "renamed", "deleted", "conflicted"]
        .iter()
        .any(|key| {
            worktree
                .pointer(&format!("/changes/{key}"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        });
    if dirty {
        return RemovalSafety::Dirty;
    }

    if worktree
        .get("duplicate_branch")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return RemovalSafety::BranchCheckedOutElsewhere;
    }

    match item.pointer("/display/state").and_then(Value::as_str) {
        Some("empty") | Some("integrated") => RemovalSafety::Safe,
        Some(_) => RemovalSafety::BranchNotIntegrated,
        None => RemovalSafety::Unknown,
    }
}

#[derive(Debug, Clone)]
pub struct BranchEntry {
    pub kind: EntryKind,
    pub branch: String,
    pub path: Option<String>,
    pub symbols: String,
    pub remote: Option<String>,
    pub upstream: Option<String>,
}

impl BranchEntry {
    pub fn reference(&self) -> String {
        match &self.remote {
            Some(remote) => format!("{remote}/{}", self.branch),
            None => self.branch.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
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
        if matches!(
            kind,
            EntryKind::WorktreeCurrent | EntryKind::WorktreeMain | EntryKind::WorktreeOther
        ) {
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
        let remote = item
            .get("remote")
            .and_then(|value| value.as_str())
            .map(String::from);
        let upstream = item
            .pointer("/upstream")
            .and_then(|value| value.as_object())
            .and_then(|upstream| {
                let remote = upstream.get("remote")?.as_str()?;
                let branch = upstream.get("branch")?.as_str()?;
                Some(format!("{remote}/{branch}"))
            });

        entries.push(BranchEntry {
            kind,
            branch: branch.to_string(),
            path,
            symbols,
            remote,
            upstream,
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
            remote: None,
            upstream: None,
        };
        assert!(matches!(
            entry.kind,
            EntryKind::WorktreeCurrent | EntryKind::WorktreeMain | EntryKind::WorktreeOther
        ));
    }

    #[test]
    fn availability_check_accepts_a_successful_command() {
        let mut command = Command::new("true");
        assert!(ensure_available_with(&mut command).is_ok());
    }

    #[test]
    fn remote_entries_preserve_their_qualified_reference() {
        let entries = parse_wt_list(&serde_json::json!({
            "items": [{"branch": "feature/auth", "remote": "upstream"}]
        }));

        assert_eq!(entries[0].reference(), "upstream/feature/auth");
    }

    #[test]
    fn local_entries_parse_their_upstream() {
        let entries = parse_wt_list(&serde_json::json!({
            "items": [{
                "branch": "feature/auth",
                "upstream": {"remote": "origin", "branch": "feature/auth"}
            }]
        }));

        assert_eq!(entries[0].upstream.as_deref(), Some("origin/feature/auth"));
    }

    #[test]
    fn availability_check_explains_how_to_install_a_missing_command() {
        let mut command = Command::new("herdr-worktree-test-command-that-does-not-exist");
        let error = ensure_available_with(&mut command).unwrap_err();

        assert!(error.contains("Worktrunk (wt) is required but was not found"));
        assert!(error.contains("brew install worktrunk"));
    }

    #[test]
    fn availability_check_rejects_an_unsuccessful_command() {
        let mut command = Command::new("false");
        let error = ensure_available_with(&mut command).unwrap_err();

        assert!(error.contains("Worktrunk (wt) exited"));
        assert!(error.contains("cargo install worktrunk"));
    }

    #[test]
    fn removal_safety_requires_a_clean_integrated_worktree() {
        let list = serde_json::json!({
            "items": [{
                "worktree": {
                    "path": "/repo.feature",
                    "changes": {"modified": false},
                    "duplicate_branch": false
                },
                "display": {"state": "integrated"}
            }]
        });

        assert_eq!(removal_safety(&list, "/repo.feature"), RemovalSafety::Safe);
    }

    #[test]
    fn removal_safety_rejects_dirty_and_unmerged_worktrees() {
        let dirty = serde_json::json!({
            "items": [{
                "worktree": {"path": "/repo.feature", "changes": {"untracked": true}},
                "display": {"state": "integrated"}
            }]
        });
        let unmerged = serde_json::json!({
            "items": [{
                "worktree": {"path": "/repo.feature", "changes": {}},
                "display": {"state": "ahead"}
            }]
        });

        assert_eq!(removal_safety(&dirty, "/repo.feature"), RemovalSafety::Dirty);
        assert_eq!(
            removal_safety(&unmerged, "/repo.feature"),
            RemovalSafety::BranchNotIntegrated
        );
    }

    #[test]
    fn removal_safety_rejects_a_branch_checked_out_elsewhere() {
        let list = serde_json::json!({
            "items": [{
                "worktree": {
                    "path": "/repo.feature",
                    "changes": {},
                    "duplicate_branch": true
                },
                "display": {"state": "integrated"}
            }]
        });

        assert_eq!(
            removal_safety(&list, "/repo.feature"),
            RemovalSafety::BranchCheckedOutElsewhere
        );
    }

    #[test]
    fn clean_unintegrated_worktrees_can_be_removed_without_deleting_the_branch() {
        assert!(RemovalSafety::BranchNotIntegrated.allows_removal());
        assert!(RemovalSafety::BranchCheckedOutElsewhere.allows_removal());
        assert!(!RemovalSafety::Dirty.allows_removal());
        assert!(!RemovalSafety::Unknown.allows_removal());
    }
}
