use std::env;
use std::process;

mod config;
mod git;
mod herdr;
mod model;
mod tui;
mod wt;

use config::Config;
use git::{resolve_repo_root, get_primary_worktree};
use herdr::{get_own_pane_id_from_env, close_plugin_pane, get_plugin_pane_id, herdr_json, focus_plugin_pane, worktree_open, workspace_close, show_notification, get_focused_workspace_id, open_confirm_remove_pane};
use wt::{wt_list, wt_switch, wt_remove, BranchEntry, EntryKind};
use git::normalize_branch_name;
use tui::TuiResult;
use serde_json::Value;
use crossterm::{
    event::{self, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode},
};

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

fn remove_action() {
    // Get herdr snapshot to find focused workspace
    let snapshot = match herdr_json(["api", "snapshot"]) {
        Ok(json) => json,
        Err(e) => {
            let _ = show_notification("Failed to get herdr snapshot", &e);
            show_error_and_exit(&format!("Failed to get herdr snapshot: {}", e), 1);
        }
    };

    // Get focused workspace ID from snapshot
    let workspace_id = match get_focused_workspace_id(&snapshot) {
        Some(id) => id,
        None => {
            show_error_and_exit("No active Herdr workspace", 1);
        }
    };

    // Get CWD and worktree info for the focused workspace
    let (checkout_path, repo_name, branch) = match get_workspace_info_from_snapshot(&snapshot, &workspace_id) {
        Some(info) => info,
        None => {
            show_error_and_exit("Failed to get workspace worktree info", 1);
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
    ];
    if let Err(e) = open_confirm_remove_pane(&plugin_id, &env_vars) {
        // Clean up lock file on error
        let _ = std::fs::remove_file(&lock_file);
        show_error_and_exit(&format!("Failed to open confirm dialog: {}", e), 1);
    }

    process::exit(0);
}

fn get_workspace_info_from_snapshot(snapshot: &Value, workspace_id: &str) -> Option<(String, String, String)> {
    // Get worktree info: (checkout_path, repo_name, branch)
    let workspaces = snapshot.pointer("/result/snapshot/workspaces")?.as_array()?;
    let workspace = workspaces.iter().find(|w| {
        w.get("workspace_id").and_then(|v| v.as_str()) == Some(workspace_id)
    })?;
    
    let worktree = workspace.get("worktree")?;
    let checkout_path = worktree.get("checkout_path")?.as_str()?.to_string();
    let repo_name = worktree.get("repo_name")?.as_str()?.to_string();
    
    // Get branch from worktree list
    let repo_root = worktree.get("repo_root")?.as_str()?;
    let entries = wt_list(repo_root, true, false)
        .ok()
        .map(|json| wt::parse_wt_list(&json))?;
    
    let branch = entries.iter()
        .find(|e| e.path.as_deref() == Some(&checkout_path))
        .map(|e| e.branch.clone())?;
    
    Some((checkout_path, repo_name, branch))
}



fn open_picker_action() {
    // Check if picker already exists in focused workspace
    let pane_json = match herdr_json(["pane", "list"]) {
        Ok(json) => json,
        Err(e) => {
            eprintln!("Failed to list panes: {}", e);
            process::exit(1);
        }
    };

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
        let plugin_id = env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| "herdr-worktree".to_string());
        if let Err(e) = herdr::open_plugin_pane_with_cwd(&plugin_id, "picker", &current_cwd) {
            eprintln!("Failed to open picker: {}", e);
            process::exit(1);
        }
    }

    process::exit(0);
}

fn load_entries(repo_root: &str, include_remotes: bool) -> Result<Vec<BranchEntry>, String> {
    let mut entries = wt_list(repo_root, true, false)
        .map(|json| wt::parse_wt_list(&json))?;
    
    if include_remotes {
        let remote_entries = wt_list(repo_root, false, true)
            .map(|json| wt::parse_wt_list(&json))?;
        entries.extend(remote_entries);
    }
    
    Ok(entries)
}

fn run_ui() {
    let config = Config::load();

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

    // PHASE 1+2+3: Loop over TUI, reloading on ToggleRemotes
    let mut show_remotes = false;
    let selection = loop {
        // Load entries (with or without remotes)
        let entries = match load_entries(&repo_root, show_remotes) {
            Ok(e) => e,
            Err(e) => {
                show_error_and_exit(&format!("Failed to list worktrees: {}", e), 1);
            }
        };

        // Run TUI
        match tui::run_tui(repo_root.clone(), config.clone(), entries, show_remotes) {
            Ok(TuiResult::Selected(entry)) => break Some(entry),
            Ok(TuiResult::Cancelled) => break None,
            Ok(TuiResult::ToggleRemotes) => {
                show_remotes = !show_remotes;
                continue; // Reload and re-enter TUI
            }
            Err(e) => {
                show_error_and_exit(&format!("TUI error: {}", e), 1);
            }
        }
    };

    // PHASE 3: After final TUI teardown, perform side effects
    if let Some(entry) = selection {
        let normalized = normalize_branch_name(&entry.branch, config.normalize_jira_prefix);
        
        // Handle based on entry kind
        match entry.kind {
            // For existing worktrees, just open them in herdr (skip wt switch)
            EntryKind::WorktreeCurrent | EntryKind::WorktreeMain | EntryKind::WorktreeOther => {
                if let Some(path) = entry.path {
                    match worktree_open(&repo_root, &path, true) {
                        Ok(_) => {
                            if let Some(id) = pane_id {
                                let _ = close_plugin_pane(&id);
                            }
                        }
                        Err(e) => {
                            show_error_and_exit(&format!("Failed to open worktree: {}", e), 1);
                        }
                    }
                } else {
                    show_error_and_exit("Worktree entry missing path", 1);
                }
            }
            // For branches (local/remote), use wt switch to create/switch
            EntryKind::BranchLocal | EntryKind::BranchRemote => {
                // Check if branch exists (to determine if we need --create)
                let exists = load_entries(&repo_root, false)
                    .map(|entries| entries.iter().any(|e| e.branch == normalized))
                    .unwrap_or(false);

                // Run wt switch
                match wt_switch(&repo_root, &normalized, !exists) {
                    Ok(result) => {
                        let path = result
                            .get("path")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&normalized);

                        // Open worktree in herdr (with focus)
                        match worktree_open(&repo_root, path, true) {
                            Ok(_) => {
                                if let Some(id) = pane_id {
                                    let _ = close_plugin_pane(&id);
                                }
                            }
                            Err(e) => {
                                show_error_and_exit(&format!("Failed to open worktree: {}", e), 1);
                            }
                        }
                    }
                    Err(e) => {
                        show_error_and_exit(&format!("Failed to switch/create worktree: {}", e), 1);
                    }
                }
            }
            // For new worktree (from "+ New worktree..."), use wt switch with smart --create
            EntryKind::NewWorktree => {
                let input = &normalized;
                let is_ref = input.starts_with("pr:")
                    || input.starts_with("mr:")
                    || input.starts_with("http://")
                    || input.starts_with("https://");
                
                // For pr:/mr:/URLs, don't use --create (wt handles them)
                // For plain branch names, check if it exists locally
                let create = if is_ref {
                    false
                } else {
                    // Check if this branch name exists locally
                    !load_entries(&repo_root, false)
                        .map(|entries| entries.iter().any(|e| e.branch == *input))
                        .unwrap_or(false)
                };

                // Run wt switch (wt handles pr:N, URLs, existing branches, and new branches)
                match wt_switch(&repo_root, input, create) {
                    Ok(result) => {
                        let path = result
                            .get("path")
                            .and_then(|v| v.as_str())
                            .unwrap_or(input);

                        // Open worktree in herdr (with focus)
                        match worktree_open(&repo_root, path, true) {
                            Ok(_) => {
                                if let Some(id) = pane_id {
                                    let _ = close_plugin_pane(&id);
                                }
                            }
                            Err(e) => {
                                show_error_and_exit(&format!("Failed to open worktree: {}", e), 1);
                            }
                        }
                    }
                    Err(e) => {
                        show_error_and_exit(&format!("Failed to switch/create worktree: {}", e), 1);
                    }
                }
            }
        }
    }
    
    process::exit(0);
}

fn confirm_remove_ui() {
    // Read env vars passed from remove_action
    let workspace_id = env::var("HERDR_REMOVE_WORKSPACE_ID")
        .expect("HERDR_REMOVE_WORKSPACE_ID not set");
    let checkout_path = env::var("HERDR_REMOVE_CHECKOUT_PATH")
        .expect("HERDR_REMOVE_CHECKOUT_PATH not set");
    let repo_root = env::var("HERDR_REMOVE_REPO_ROOT")
        .expect("HERDR_REMOVE_REPO_ROOT not set");
    let repo_name = env::var("HERDR_REMOVE_REPO_NAME")
        .expect("HERDR_REMOVE_REPO_NAME not set");
    let display_text = env::var("HERDR_REMOVE_DISPLAY_TEXT")
        .expect("HERDR_REMOVE_DISPLAY_TEXT not set");

    // Write lock file with our PID
    let lock_file = "/tmp/herdr-worktree-confirm.lock";
    if let Err(e) = std::fs::write(lock_file, std::process::id().to_string()) {
        show_error_and_exit(&format!("Failed to create lock file: {}", e), 1);
    }

    // Run confirm dialog TUI inline
    let confirmed = tui::run_confirm_tui(&display_text).unwrap_or(false);

    if confirmed {
        // Remove the worktree
        match wt_remove(&repo_root, &checkout_path) {
            Ok(remove_result) => {
                // Extract branch for notification
                let removed_branch = remove_result
                    .get("branch")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>");
                let notification_body = format!("{}: {}", repo_name, removed_branch);

                // Close the workspace
                let _ = workspace_close(&workspace_id);

                // Show success notification
                let _ = show_notification("Worktree removed", &notification_body);
            }
            Err(e) => {
                let _ = show_notification("Failed to remove worktree", &e);
            }
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
