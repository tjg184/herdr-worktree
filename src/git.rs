use std::path::PathBuf;
use std::process::{Command, Stdio};

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
        if prefix.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false)
            && prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && rest.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
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
        assert_eq!(normalize_branch_name("jira-123-fix-bug", true), "JIRA-123-fix-bug");
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
}
