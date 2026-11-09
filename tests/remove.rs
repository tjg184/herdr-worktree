#![cfg(unix)]

use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn script(path: &Path, content: &str) {
    fs::write(path, format!("#!/bin/sh\nset -eu\n{content}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn remove_action(detached: bool) -> (tempfile::TempDir, std::process::Output) {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    fs::create_dir(root.join("linked")).unwrap();
    let snapshot = serde_json::json!({"result": {"snapshot": {
        "focused_workspace_id": "linked",
        "workspaces": [{"workspace_id": "linked", "worktree": {
            "checkout_path": root.join("linked"), "repo_root": root.join("main"),
            "repo_name": "example"
        }}]
    }}});
    fs::write(root.join("snapshot.json"), snapshot.to_string()).unwrap();
    script(
        &root.join("herdr"),
        r#"
case "$*" in
  'api snapshot') cat "$FIXTURE/snapshot.json" ;;
  'plugin pane open '*) printf '%s\n' "$*" > "$FIXTURE/pane" ;;
  *) exit 1 ;;
esac
"#,
    );
    script(
        &root.join("git"),
        r#"
case "$*" in
  *'rev-parse --show-toplevel') printf '%s/linked\n' "$FIXTURE" ;;
  *'worktree list --porcelain') printf 'worktree %s/main\n\nworktree %s/linked\nbranch refs/heads/feature\n' "$FIXTURE" "$FIXTURE" ;;
  *'rev-parse --abbrev-ref HEAD')
    if [ "$DETACHED" = true ]; then printf 'HEAD\n'; else printf 'feature\n'; fi ;;
  *'symbolic-ref --quiet --short HEAD')
    [ "$DETACHED" = false ] || exit 1
    printf 'feature\n' ;;
  *) exit 1 ;;
esac
"#,
    );
    script(
        &root.join("wt"),
        r#"
printf '%s\n' "$*" >> "$FIXTURE/wt-calls"
case "$*" in
  --version) printf 'wt test\n' ;;
  *) printf '{"items":[{"branch":"feature","worktree":{"path":"%s/linked"},"display":{"state":"empty"}}]}\n' "$FIXTURE" ;;
esac
"#,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_herdr-worktree"))
        .arg("remove")
        .env("FIXTURE", root)
        .env("DETACHED", detached.to_string())
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root)
        .env("HERDR_BIN_PATH", root.join("herdr"))
        .env("PATH", format!("{}:/usr/bin:/bin", root.display()))
        .output()
        .unwrap();
    (fixture, output)
}

#[test]
fn removal_opens_confirmation_without_collecting_worktrunk_status() {
    let (fixture, output) = remove_action(false);
    let root = fixture.path();
    assert!(output.status.success(), "{:?}", output);
    assert!(fs::read_to_string(root.join("pane"))
        .unwrap()
        .contains("confirm-remove"));
    assert_eq!(
        fs::read_to_string(root.join("wt-calls")).unwrap(),
        "--version\n"
    );
}

#[test]
fn removal_rejects_detached_head_before_offering_to_delete_the_worktree() {
    let (fixture, output) = remove_action(true);
    assert!(!output.status.success());
    assert!(!fixture.path().join("pane").exists());
}
