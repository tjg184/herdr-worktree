use std::env;
use std::process;

mod config;
mod git;
mod herdr;
mod model;
mod tui;
mod wt;

use config::Config;
use git::resolve_repo_root;
use herdr::{get_plugin_pane_id, herdr_json, open_plugin_pane, focus_plugin_pane};

const PICKER_LABEL: &str = "Worktree Picker";

fn main() {
    let args: Vec<String> = env::args().collect();
    
    match args.get(1).map(|s| s.as_str()) {
        Some("open") => open_picker_action(),
        Some("ui") => run_ui(),
        _ => {
            eprintln!("Usage: herdr-worktree <open|ui>");
            process::exit(1);
        }
    }
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

    if let Some(pane_id) = get_plugin_pane_id(&pane_json, PICKER_LABEL) {
        // Focus existing picker
        if let Err(e) = focus_plugin_pane(&pane_id) {
            eprintln!("Failed to focus picker: {}", e);
            process::exit(1);
        }
    } else {
        // Open new picker
        let plugin_id = env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| "herdr-worktree".to_string());
        if let Err(e) = open_plugin_pane(&plugin_id, "picker") {
            eprintln!("Failed to open picker: {}", e);
            process::exit(1);
        }
    }

    process::exit(0);
}

fn run_ui() {
    let config = Config::load();

    // Resolve repo root from herdr's active pane CWD
    let start_dir = env::var("HERDR_ACTIVE_PANE_CWD")
        .or_else(|_| env::var("PWD"))
        .unwrap_or_else(|_| ".".to_string());

    let repo_root = match resolve_repo_root(&start_dir) {
        Some(path) => path.to_string_lossy().to_string(),
        None => {
            eprintln!("Not inside a Git repository");
            process::exit(1);
        }
    };

    if let Err(e) = tui::run_tui(repo_root, config) {
        eprintln!("TUI error: {}", e);
        process::exit(1);
    }
}
