use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::wt::{BranchEntry, EntryKind, RemovalSafety};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadState {
    Branch,
    Detached,
    Unborn,
}

pub fn resolve_repo_root(start_dir: &str) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["-C", start_dir, "rev-parse", "--show-toplevel"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Some(PathBuf::from(path))
}

pub fn get_primary_worktree(repo_root: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["-C", repo_root, "worktree", "list", "--porcelain"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.starts_with("worktree ") {
            let path = line.strip_prefix("worktree ")?;
            return Some(path.to_string());
        }
    }
    None
}

pub fn head_state(repo_root: &str) -> Result<HeadState, String> {
    let head = Command::new("git")
        .args(["-C", repo_root, "rev-parse", "--verify", "--quiet", "HEAD"])
        .status()
        .map_err(|error| error.to_string())?;
    if !head.success() {
        return Ok(HeadState::Unborn);
    }

    let branch = Command::new("git")
        .args([
            "-C",
            repo_root,
            "symbolic-ref",
            "--quiet",
            "--short",
            "HEAD",
        ])
        .status()
        .map_err(|error| error.to_string())?;
    Ok(if branch.success() {
        HeadState::Branch
    } else {
        HeadState::Detached
    })
}

pub fn resolve_head(repo_root: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", repo_root, "rev-parse", "--verify", "HEAD"])
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn fetch_all(repo_root: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", repo_root, "fetch", "--all", "--prune"])
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn native_entries(repo_root: &str, include_remotes: bool) -> Result<Vec<BranchEntry>, String> {
    let worktrees = native_worktrees(repo_root)?;
    let mut entries = Vec::new();
    for line in git_ref_lines(repo_root, "refs/heads")? {
        let Some((reference, upstream)) = line.split_once('\0') else {
            continue;
        };
        let path = worktrees.get(reference).cloned();
        let kind = match path.as_deref() {
            Some(path) if path == repo_root => EntryKind::WorktreeCurrent,
            Some(_) => EntryKind::WorktreeOther,
            None => EntryKind::BranchLocal,
        };
        entries.push(BranchEntry {
            kind,
            branch: reference.into(),
            path,
            symbols: String::new(),
            remote: None,
            upstream: (!upstream.is_empty()).then(|| upstream.into()),
        });
    }
    if include_remotes {
        for line in git_ref_lines(repo_root, "refs/remotes")? {
            let Some((reference, _)) = line.split_once('\0') else {
                continue;
            };
            if reference.ends_with("/HEAD") {
                continue;
            }
            let Some((remote, branch)) = reference.split_once('/') else {
                continue;
            };
            entries.push(BranchEntry {
                kind: EntryKind::BranchRemote,
                branch: branch.into(),
                path: None,
                symbols: String::new(),
                remote: Some(remote.into()),
                upstream: None,
            });
        }
    }
    entries.sort_by_key(|entry| match entry.kind {
        EntryKind::WorktreeCurrent => 0,
        EntryKind::WorktreeMain => 1,
        EntryKind::WorktreeOther => 2,
        EntryKind::BranchLocal => 3,
        EntryKind::BranchRemote => 4,
    });
    Ok(entries)
}

fn git_ref_lines(repo_root: &str, namespace: &str) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .args([
            "-C",
            repo_root,
            "for-each-ref",
            "--format=%(refname:short)%00%(upstream:short)",
            namespace,
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_owned)
            .collect())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn native_worktrees(repo_root: &str) -> Result<std::collections::HashMap<String, String>, String> {
    let output = Command::new("git")
        .args(["-C", repo_root, "worktree", "list", "--porcelain"])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let mut worktrees = std::collections::HashMap::new();
    let mut path = None;
    for line in String::from_utf8_lossy(&output.stdout)
        .lines()
        .chain(std::iter::once(""))
    {
        if let Some(value) = line.strip_prefix("worktree ") {
            path = Some(value.to_string());
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            if let Some(path) = path.take() {
                worktrees.insert(branch.to_string(), path);
            }
        } else if line.is_empty() {
            path = None;
        }
    }
    Ok(worktrees)
}

pub fn native_removal_safety(repo_root: &str) -> RemovalSafety {
    let output = Command::new("git")
        .args(["-C", repo_root, "status", "--porcelain"])
        .output();
    match output {
        Ok(output) if output.status.success() && output.stdout.is_empty() => RemovalSafety::Safe,
        Ok(output) if output.status.success() => RemovalSafety::Dirty,
        _ => RemovalSafety::Unknown,
    }
}

pub fn validate_new_branch_name(repo_root: &str, branch: &str) -> Result<(), String> {
    if branch.is_empty() {
        return Err("Branch name is required".into());
    }
    let valid = Command::new("git")
        .args(["-C", repo_root, "check-ref-format", "--branch", branch])
        .output()
        .map_err(|error| error.to_string())?;
    if !valid.status.success() {
        return Err(String::from_utf8_lossy(&valid.stderr).trim().to_string());
    }
    let exists = Command::new("git")
        .args([
            "-C",
            repo_root,
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if exists.status.success() {
        Err(format!("Branch {branch} already exists locally"))
    } else {
        Ok(())
    }
}

pub fn normalize_branch_name(branch: &str, enabled: bool) -> String {
    if !enabled {
        return branch.to_string();
    }

    // Pattern: prefix-123 or prefix-123-description
    // Uppercase the prefix portion
    let parts: Vec<&str> = branch.splitn(2, '-').collect();
    if parts.len() >= 2 {
        let prefix = parts[0];
        let rest = &branch[prefix.len() + 1..];

        // Check if prefix is alphabetic-starting and rest starts with digits
        if prefix
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false)
            && prefix
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            && rest
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
        {
            return format!("{}-{}", prefix.to_ascii_uppercase(), rest);
        }
    }

    branch.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_jira_style() {
        assert_eq!(normalize_branch_name("jira-123", true), "JIRA-123");
        assert_eq!(
            normalize_branch_name("jira-123-fix-bug", true),
            "JIRA-123-fix-bug"
        );
        assert_eq!(normalize_branch_name("feature-456", true), "FEATURE-456");
    }

    #[test]
    fn test_no_normalize_when_disabled() {
        assert_eq!(normalize_branch_name("jira-123", false), "jira-123");
    }

    #[test]
    fn test_no_normalize_invalid_pattern() {
        // No digits after dash
        assert_eq!(normalize_branch_name("feature-abc", true), "feature-abc");
        // Starts with number
        assert_eq!(normalize_branch_name("123-feature", true), "123-feature");
        // No dash
        assert_eq!(normalize_branch_name("feature", true), "feature");
    }

    #[test]
    fn branch_name_validation_rejects_empty_names() {
        assert_eq!(
            validate_new_branch_name(".", "").unwrap_err(),
            "Branch name is required"
        );
    }
}
