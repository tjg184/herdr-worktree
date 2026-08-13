use std::env;
use std::process;

mod config;
mod git;
mod herdr;
mod model;
mod tui;
mod wt;

use config::{Config, WorktreeBackend};
use crossterm::{
    event::{self},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use git::{get_primary_worktree, head_state, resolve_repo_root};
use herdr::{
    close_plugin_pane, focus_plugin_pane, get_focused_workspace_id, get_own_pane_id_from_env,
    get_plugin_pane_id, git_delete_branch, herdr_json, open_confirm_remove_pane, show_notification,
    workspace_close, worktree_remove,
};
use serde_json::Value;
use tui::TuiResult;
use wt::{
    ensure_available as ensure_worktrunk_available, removal_safety, wt_list, wt_remove,
    BranchEntry, RemovalSafety,
};

// Also import ConfirmAction for the confirm-remove action
use tui::ConfirmAction;

extern crate libc;

const PICKER_LABEL: &str = "Worktree Picker";

fn main() {
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("open") => open_picker_action(),
        Some("ui") => run_ui(),
        Some("remove") => remove_action(),
        Some("confirm-remove") => confirm_remove_ui(),
        _ => {
            eprintln!("Usage: herdr-worktree <open|ui|remove|confirm-remove>");
            process::exit(1);
        }
    }
}

/// Print error and wait for keypress before exiting, so user can see it
fn show_error_and_exit(message: &str, code: i32) -> ! {
    eprintln!("Error: {}", message);
    eprintln!("Press any key to close...");

    // Try to read a keypress (best effort - may fail if terminal not setup)
    let _ = enable_raw_mode();
    let _ = event::read();
    let _ = disable_raw_mode();

    process::exit(code);
}

fn ensure_worktrunk_or_exit() {
    if let Err(error) = ensure_worktrunk_available() {
        let _ = show_notification("Worktrunk unavailable", &error);
        show_error_and_exit(&error, 1);
    }
}

fn remove_action() {
    let config = Config::load();
    // Get herdr snapshot to find focused workspace
    let snapshot = match herdr_json(["api", "snapshot"]) {
        Ok(json) => json,
        Err(e) => {
            let _ = show_notification("Failed to get herdr snapshot", &e);
            show_error_and_exit(&format!("Failed to get herdr snapshot: {}", e), 1);
        }
    };

    if config.backend == WorktreeBackend::Worktrunk {
        ensure_worktrunk_or_exit();
    }

    // Get focused workspace ID from snapshot
    let workspace_id = match get_focused_workspace_id(&snapshot) {
        Some(id) => id,
        None => {
            show_error_and_exit("No active Herdr workspace", 1);
        }
    };

    // Get CWD and worktree info for the focused workspace
    let (checkout_path, repo_name, branch) =
        match get_workspace_info_from_snapshot(&snapshot, &workspace_id, config.backend) {
            Ok(info) => info,
            Err(error) => {
                let _ = show_notification("Failed to get workspace worktree info", &error);
                show_error_and_exit(
                    &format!("Failed to get workspace worktree info: {error}"),
                    1,
                );
            }
        };

    // Resolve repo root (validates it's a git repo)
    let repo_root = match resolve_repo_root(&checkout_path) {
        Some(path) => path.to_string_lossy().to_string(),
        None => {
            show_error_and_exit("Not inside a Git worktree", 1);
        }
    };

    // Get primary worktree path
    let primary_worktree = match get_primary_worktree(&repo_root) {
        Some(path) => path,
        None => {
            show_error_and_exit("Failed to determine primary worktree", 1);
        }
    };

    // Refuse to remove the primary worktree
    if repo_root == primary_worktree {
        show_error_and_exit("Refusing to remove the primary worktree", 1);
    }

    // Build display string: repo_name: branch
    let display_text = format!("{}: {}", repo_name, branch);
    let removal_safety = match config.backend {
        WorktreeBackend::Worktrunk => wt_list(&repo_root, false, false)
            .map(|list| removal_safety(&list, &checkout_path))
            .unwrap_or(RemovalSafety::Unknown),
        WorktreeBackend::Native => git::native_removal_safety(&checkout_path),
    };

    // Check if confirm dialog is already open using global lock file with PID check
    let lock_file = "/tmp/herdr-worktree-confirm.lock";
    if std::path::Path::new(lock_file).exists() {
        // Check if the process holding the lock is still alive
        if let Ok(pid_str) = std::fs::read_to_string(lock_file) {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                // Check if process exists (kill 0 is POSIX "check if process exists")
                if unsafe { libc::kill(pid as i32, 0) } == 0 {
                    // Process exists, dialog already open
                    process::exit(0);
                }
            }
        }
        // Stale lock file, remove it
        let _ = std::fs::remove_file(lock_file);
    }

    let plugin_id = env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| "herdr-worktree".to_string());

    // Open confirm-remove pane
    let env_vars = [
        ("HERDR_REMOVE_WORKSPACE_ID", workspace_id.as_str()),
        ("HERDR_REMOVE_CHECKOUT_PATH", checkout_path.as_str()),
        ("HERDR_REMOVE_REPO_ROOT", repo_root.as_str()),
        ("HERDR_REMOVE_REPO_NAME", repo_name.as_str()),
        ("HERDR_REMOVE_BRANCH", branch.as_str()),
        ("HERDR_REMOVE_DISPLAY_TEXT", display_text.as_str()),
        ("HERDR_REMOVE_SAFETY", removal_safety.as_env_value()),
        (
            "HERDR_REMOVE_BACKEND",
            match config.backend {
                WorktreeBackend::Worktrunk => "worktrunk",
                WorktreeBackend::Native => "native",
            },
        ),
    ];
    if let Err(e) = open_confirm_remove_pane(&plugin_id, &env_vars) {
        // Clean up lock file on error
        let _ = std::fs::remove_file(lock_file);
        show_error_and_exit(&format!("Failed to open confirm dialog: {}", e), 1);
    }

    process::exit(0);
}

fn get_workspace_info_from_snapshot(
    snapshot: &Value,
    workspace_id: &str,
    backend: WorktreeBackend,
) -> Result<(String, String, String), String> {
    // Get worktree info: (checkout_path, repo_name, branch)
    let workspaces = snapshot
        .pointer("/result/snapshot/workspaces")
        .and_then(|value| value.as_array())
        .ok_or("Herdr snapshot has no workspaces")?;
    let workspace = workspaces
        .iter()
        .find(|w| w.get("workspace_id").and_then(|v| v.as_str()) == Some(workspace_id))
        .ok_or("Focused workspace was not found")?;

    let worktree = workspace
        .get("worktree")
        .ok_or("Focused workspace has no worktree")?;
    let checkout_path = worktree
        .get("checkout_path")
        .and_then(|value| value.as_str())
        .ok_or("Focused workspace has no checkout path")?
        .to_string();
    let repo_name = worktree
        .get("repo_name")
        .and_then(|value| value.as_str())
        .ok_or("Focused workspace has no repository name")?
        .to_string();

    // Get branch from worktree list
    let repo_root = worktree
        .get("repo_root")
        .and_then(|value| value.as_str())
        .ok_or("Focused workspace has no repository root")?;
    let entries = match backend {
        WorktreeBackend::Worktrunk => {
            wt_list(repo_root, true, false).map(|json| wt::parse_wt_list(&json))?
        }
        WorktreeBackend::Native => git::native_entries(repo_root, false)?,
    };

    let branch = entries
        .iter()
        .find(|e| e.path.as_deref() == Some(&checkout_path))
        .map(|e| e.branch.clone())
        .ok_or("Could not determine the worktree branch")?;

    Ok((checkout_path, repo_name, branch))
}

fn open_picker_action() {
    let config = Config::load();
    // Check if picker already exists in focused workspace
    let pane_json = match herdr_json(["pane", "list"]) {
        Ok(json) => json,
        Err(e) => {
            eprintln!("Failed to list panes: {}", e);
            process::exit(1);
        }
    };

    if config.backend == WorktreeBackend::Worktrunk {
        ensure_worktrunk_or_exit();
    }

    // Get workspace CWD from pane list (find shell pane in focused workspace)
    let current_cwd = herdr::get_workspace_cwd(&pane_json)
        .or_else(|| env::var("HERDR_ACTIVE_PANE_CWD").ok())
        .or_else(|| env::var("PWD").ok())
        .unwrap_or_else(|| ".".to_string());

    if let Some(pane_id) = get_plugin_pane_id(&pane_json, PICKER_LABEL) {
        // Focus existing picker
        if let Err(e) = focus_plugin_pane(&pane_id) {
            eprintln!("Failed to focus picker: {}", e);
            process::exit(1);
        }
    } else {
        // Open new picker with CWD from invoking workspace
        let plugin_id =
            env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| "herdr-worktree".to_string());
        if let Err(e) = herdr::open_plugin_pane_with_cwd(&plugin_id, "picker", &current_cwd) {
            eprintln!("Failed to open picker: {}", e);
            process::exit(1);
        }
    }

    process::exit(0);
}

fn load_entries(
    repo_root: &str,
    include_remotes: bool,
    backend: WorktreeBackend,
) -> Result<Vec<BranchEntry>, String> {
    match backend {
        WorktreeBackend::Worktrunk => {
            load_entries_with(include_remotes, |include_branches, include_remotes| {
                wt_list(repo_root, include_branches, include_remotes)
            })
        }
        WorktreeBackend::Native => git::native_entries(repo_root, include_remotes),
    }
}

fn load_entries_with(
    include_remotes: bool,
    mut list: impl FnMut(bool, bool) -> Result<Value, String>,
) -> Result<Vec<BranchEntry>, String> {
    list(true, include_remotes).map(|json| wt::parse_wt_list(&json))
}

fn run_ui() {
    let config = Config::load();

    if config.backend == WorktreeBackend::Worktrunk {
        ensure_worktrunk_or_exit();
    }

    // Resolve repo root from HERDR_WORKTREE_CWD (set by open action) or fallbacks
    let start_dir = env::var("HERDR_WORKTREE_CWD")
        .or_else(|_| env::var("HERDR_ACTIVE_PANE_CWD"))
        .or_else(|_| env::var("PWD"))
        .unwrap_or_else(|_| ".".to_string());

    let repo_root = match resolve_repo_root(&start_dir) {
        Some(path) => path.to_string_lossy().to_string(),
        None => {
            show_error_and_exit("Not inside a Git repository", 1);
        }
    };

    // Get our own pane ID from env (set by herdr when launching the pane)
    let pane_id: Option<String> = get_own_pane_id_from_env();

    let head = match head_state(&repo_root) {
        Ok(head) => head,
        Err(error) => show_error_and_exit(&format!("Failed to inspect HEAD: {error}"), 1),
    };

    // Load local branches immediately; remotes load in background
    let entries = match load_entries(&repo_root, false, config.backend) {
        Ok(e) => e,
        Err(e) => {
            show_error_and_exit(&format!("Failed to list worktrees: {}", e), 1);
        }
    };

    match tui::run_tui(repo_root, config, entries, head) {
        Ok(TuiResult::Cancelled) => {}
        Ok(TuiResult::Created) => {
            if let Some(id) = pane_id.as_deref() {
                let _ = close_plugin_pane(id);
            }
        }
        Err(e) => {
            show_error_and_exit(&format!("TUI error: {}", e), 1);
        }
    }

    process::exit(0);
}

fn confirm_remove_ui() {
    let backend = match env::var("HERDR_REMOVE_BACKEND").as_deref() {
        Ok("native") => WorktreeBackend::Native,
        _ => WorktreeBackend::Worktrunk,
    };
    if backend == WorktreeBackend::Worktrunk {
        ensure_worktrunk_or_exit();
    }

    // Read env vars passed from remove_action
    let workspace_id =
        env::var("HERDR_REMOVE_WORKSPACE_ID").expect("HERDR_REMOVE_WORKSPACE_ID not set");
    let checkout_path =
        env::var("HERDR_REMOVE_CHECKOUT_PATH").expect("HERDR_REMOVE_CHECKOUT_PATH not set");
    let repo_root = env::var("HERDR_REMOVE_REPO_ROOT").expect("HERDR_REMOVE_REPO_ROOT not set");
    let repo_name = env::var("HERDR_REMOVE_REPO_NAME").expect("HERDR_REMOVE_REPO_NAME not set");
    let display_text =
        env::var("HERDR_REMOVE_DISPLAY_TEXT").expect("HERDR_REMOVE_DISPLAY_TEXT not set");
    let removal_safety = env::var("HERDR_REMOVE_SAFETY")
        .ok()
        .and_then(|value| RemovalSafety::from_env_value(&value))
        .unwrap_or(RemovalSafety::Unknown);

    // Write lock file with our PID
    let lock_file = "/tmp/herdr-worktree-confirm.lock";
    if let Err(e) = std::fs::write(lock_file, std::process::id().to_string()) {
        show_error_and_exit(&format!("Failed to create lock file: {}", e), 1);
    }

    // Run confirm dialog TUI inline
    let action = tui::run_confirm_tui(&display_text, &removal_safety, backend)
        .unwrap_or(ConfirmAction::Cancel);

    match action {
        ConfirmAction::Remove => {
            // Remove the worktree
            let result = match backend {
                WorktreeBackend::Worktrunk => wt_remove(&repo_root, &checkout_path).map(|value| {
                    value
                        .get("branch")
                        .and_then(|value| value.as_str())
                        .unwrap_or("<unknown>")
                        .to_string()
                }),
                WorktreeBackend::Native => {
                    worktree_remove(&workspace_id).map(|_| {
                        env::var("HERDR_REMOVE_BRANCH").unwrap_or_else(|_| "<unknown>".into())
                    })
                }
            };
            match result {
                Ok(removed_branch) => {
                    // On Native + Safe, also delete the branch (worktrunk handles this itself)
                    if backend == WorktreeBackend::Native
                        && removal_safety == RemovalSafety::Safe
                    {
                        let _ = git_delete_branch(&repo_root, &removed_branch);
                    }

                    let notification_body = format!("{}: {}", repo_name, removed_branch);

                    // Close the workspace
                    if backend == WorktreeBackend::Worktrunk {
                        let _ = workspace_close(&workspace_id);
                    }

                    // Show success notification
                    let _ = show_notification("Worktree removed", &notification_body);
                }
                Err(e) => {
                    let _ = show_notification("Failed to remove worktree", &e);
                }
            }
        }
        ConfirmAction::CloseWorkspace => {
            // Close the workspace without removing anything
            let _ = workspace_close(&workspace_id);
        }
        ConfirmAction::Cancel => {
            // Do nothing - just close the pane
        }
    }

    // Clean up lock file
    let _ = std::fs::remove_file(lock_file);

    // Get own pane ID and close it
    if let Some(pane_id) = get_own_pane_id_from_env() {
        let _ = close_plugin_pane(&pane_id);
    }

    process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wt::EntryKind;
    use serde_json::json;

    #[test]
    fn loading_remotes_uses_one_combined_list_without_duplicate_worktrees() {
        let mut calls = Vec::new();
        let entries = load_entries_with(true, |include_branches, include_remotes| {
            calls.push((include_branches, include_remotes));
            Ok(json!({
                "items": [
                    {
                        "branch": "main",
                        "worktree": {"path": ".", "current": true, "main": true}
                    },
                    {"branch": "feature", "remote": "origin"}
                ]
            }))
        })
        .unwrap();

        assert_eq!(calls, vec![(true, true)]);
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.kind == EntryKind::WorktreeCurrent)
                .count(),
            1
        );
        assert!(entries
            .iter()
            .any(|entry| entry.kind == EntryKind::BranchRemote));
    }
}
