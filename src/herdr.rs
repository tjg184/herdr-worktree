use serde_json::Value;
use std::env;
use std::process::{Command, Stdio};

pub fn herdr_bin() -> String {
    env::var("HERDR_BIN_PATH").unwrap_or_else(|_| "herdr".into())
}

pub fn herdr_json<const N: usize>(args: [&str; N]) -> Result<Value, String> {
    let out = Command::new(herdr_bin())
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}

pub fn run_herdr<const N: usize>(args: [&str; N]) -> Result<(), String> {
    let status = Command::new(herdr_bin())
        .args(args)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("herdr exited with {status}"))
    }
}

pub fn open_plugin_pane(plugin: &str, entrypoint: &str) -> Result<(), String> {
    run_herdr([
        "plugin", "pane", "open",
        "--plugin", plugin,
        "--entrypoint", entrypoint,
        "--focus"
    ])
}

pub fn focus_plugin_pane(pane_id: &str) -> Result<(), String> {
    run_herdr(["plugin", "pane", "focus", pane_id])
}

pub fn close_plugin_pane(pane_id: &str) -> Result<(), String> {
    run_herdr(["plugin", "pane", "close", pane_id])
}

pub fn get_plugin_pane_id(pane_json: &Value, label: &str) -> Option<String> {
    let panes = pane_json.pointer("/result/panes")?.as_array()?;
    let focused = panes.iter().find(|p| {
        p.get("focused").and_then(|v| v.as_bool()) == Some(true)
    })?;
    let workspace = focused.get("workspace_id")?.as_str()?;
    
    panes.iter().find(|p| {
        p.get("label").and_then(|v| v.as_str()) == Some(label)
            && p.get("workspace_id").and_then(|v| v.as_str()) == Some(workspace)
    })
    .and_then(|p| p.get("pane_id")?.as_str().map(String::from))
}

pub fn worktree_open(repo_root: &str, path: &str, focus: bool) -> Result<Value, String> {
    let mut args = vec![
        "worktree", "open",
        "--cwd", repo_root,
        "--path", path,
        "--json"
    ];
    if focus {
        args.push("--focus");
    } else {
        args.push("--no-focus");
    }
    
    let out = Command::new(herdr_bin())
        .args(&args)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}
