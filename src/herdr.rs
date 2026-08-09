use serde_json::Value;
use std::env;
use std::process::Command;

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

pub fn open_plugin_pane_with_cwd(plugin: &str, entrypoint: &str, cwd: &str) -> Result<(), String> {
    let status = Command::new(herdr_bin())
        .args([
            "plugin",
            "pane",
            "open",
            "--plugin",
            plugin,
            "--entrypoint",
            entrypoint,
            "--env",
            &format!("HERDR_WORKTREE_CWD={}", cwd),
            "--focus",
        ])
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("herdr exited with {status}"))
    }
}

pub fn open_confirm_remove_pane(plugin: &str, env_vars: &[(&str, &str)]) -> Result<(), String> {
    let mut args: Vec<String> = vec![
        "plugin".to_string(),
        "pane".to_string(),
        "open".to_string(),
        "--plugin".to_string(),
        plugin.to_string(),
        "--entrypoint".to_string(),
        "confirm-remove".to_string(),
        "--focus".to_string(),
    ];
    for (key, value) in env_vars {
        args.push("--env".to_string());
        args.push(format!("{}={}", key, value));
    }
    let status = Command::new(herdr_bin())
        .args(&args)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("herdr exited with {status}"))
    }
}

pub fn focus_plugin_pane(pane_id: &str) -> Result<(), String> {
    run_herdr(["plugin", "pane", "focus", pane_id])
}

pub fn close_plugin_pane(pane_id: &str) -> Result<(), String> {
    run_herdr(["plugin", "pane", "close", pane_id])
}

pub fn get_plugin_pane_id(pane_json: &Value, label: &str) -> Option<String> {
    let panes = pane_json.pointer("/result/panes")?.as_array()?;
    let focused = panes
        .iter()
        .find(|p| p.get("focused").and_then(|v| v.as_bool()) == Some(true))?;
    let workspace = focused.get("workspace_id")?.as_str()?;

    panes
        .iter()
        .find(|p| {
            p.get("label").and_then(|v| v.as_str()) == Some(label)
                && p.get("workspace_id").and_then(|v| v.as_str()) == Some(workspace)
        })
        .and_then(|p| p.get("pane_id")?.as_str().map(String::from))
}

pub fn workspace_close(workspace_id: &str) -> Result<(), String> {
    run_herdr(["workspace", "close", workspace_id])
}

pub fn show_notification(title: &str, body: &str) -> Result<(), String> {
    run_herdr(["notification", "show", title, "--body", body])
}

pub fn worktree_open(repo_root: &str, path: &str, focus: bool) -> Result<Value, String> {
    let mut args = vec![
        "worktree", "open", "--cwd", repo_root, "--path", path, "--json",
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

pub fn worktree_create(repo_root: &str, branch: &str, base: Option<&str>) -> Result<Value, String> {
    let mut args = vec![
        "worktree", "create", "--cwd", repo_root, "--branch", branch, "--focus", "--json",
    ];
    if let Some(base) = base {
        args.extend(["--base", base]);
    }
    let out = Command::new(herdr_bin())
        .args(&args)
        .output()
        .map_err(|error| error.to_string())?;
    if !out.status.success() {
        return Err(command_error(&out));
    }
    serde_json::from_slice(&out.stdout).map_err(|error| error.to_string())
}

pub fn worktree_remove(workspace_id: &str) -> Result<(), String> {
    let out = Command::new(herdr_bin())
        .args(["worktree", "remove", "--workspace", workspace_id])
        .output()
        .map_err(|error| error.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(command_error(&out))
    }
}

fn command_error(out: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    for text in [&stderr, &stdout] {
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
                return message.to_string();
            }
        }
        if !text.trim().is_empty() {
            return text.trim().to_string();
        }
    }
    format!("herdr exited with {}", out.status)
}

// Get own pane ID from HERDR_PLUGIN_CONTEXT_JSON env var
pub fn get_own_pane_id_from_env() -> Option<String> {
    env::var("HERDR_PLUGIN_CONTEXT_JSON")
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("pane_id")?.as_str().map(String::from))
}

// Get focused workspace ID from herdr snapshot
pub fn get_focused_workspace_id(snapshot: &Value) -> Option<String> {
    snapshot
        .pointer("/result/snapshot/focused_workspace_id")
        .and_then(|v| v.as_str())
        .map(String::from)
}

// Get the CWD of the current workspace from pane list JSON:
// 1. Find focused pane's workspace
// 2. Find a "shell" pane in that workspace -> use its foreground_cwd
// 3. Fall back to any pane in that workspace
pub fn get_workspace_cwd(pane_json: &Value) -> Option<String> {
    let panes = pane_json.pointer("/result/panes")?.as_array()?;

    // Get focused pane's workspace
    let focused = panes
        .iter()
        .find(|p| p.get("focused").and_then(|v| v.as_bool()) == Some(true))?;
    let workspace_id = focused.get("workspace_id")?.as_str()?;

    // Find a shell pane in that workspace
    let shell_pane = panes.iter().find(|p| {
        p.get("workspace_id").and_then(|v| v.as_str()) == Some(workspace_id)
            && p.get("label").and_then(|v| v.as_str()) == Some("shell")
    });

    if let Some(pane) = shell_pane {
        return pane
            .get("foreground_cwd")
            .or_else(|| pane.get("cwd"))
            .and_then(|v| v.as_str())
            .map(String::from);
    }

    // Fall back to any pane in the workspace
    let any_pane = panes
        .iter()
        .find(|p| p.get("workspace_id").and_then(|v| v.as_str()) == Some(workspace_id))?;

    any_pane
        .get("foreground_cwd")
        .or_else(|| any_pane.get("cwd"))
        .and_then(|v| v.as_str())
        .map(String::from)
}
