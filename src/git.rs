use std::path::PathBuf;
use std::process::{Command, Stdio};

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

pub fn create_branch_from(
    repo_root: &str,
    branch: &str,
    base: &str,
    track: bool,
) -> Result<(), String> {
    let mut args = vec!["-C", repo_root, "branch"];
    if track {
        args.push("--track");
    }
    args.extend([branch, base]);
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
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
