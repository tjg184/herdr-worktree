# Herdr Worktree Plugin

Interactive worktree picker for Herdr terminal workspace manager.

## Features

- Browse existing worktrees, local branches, and remote branches
- Create new worktrees from branches, pull requests, merge requests, or URLs
- Jira-style branch name normalization (optional): `ticket-123` → `TICKET-123`
- Toggle remote branches with `alt+r`
- Refresh remote branches with `ctrl+r`
- Create a branch from the current HEAD or a selected local/remote base
- Auto-focus new worktree after creation

## Installation

### Prerequisite

This plugin requires [Worktrunk](https://worktrunk.dev/). Install it before using the picker or worktree removal action:

```bash
brew install worktrunk
# or
cargo install worktrunk
```

```bash
# Link local plugin
git clone <repo-url> ~/dev/herdr-worktree
cd ~/dev/herdr-worktree
herdr plugin link $(pwd)

# Or install a release from GitHub
<!-- x-release-please-start-version -->
herdr plugin install tjg184/herdr-worktree --ref v0.1.0
<!-- x-release-please-end -->
```

## Configuration

Create `~/.config/herdr/plugins/config/herdr-worktree/config.toml`:

```toml
# Normalize Jira-style branch names (ticket-123 → TICKET-123)
normalize_jira_prefix = true

[keybindings]
confirm = "enter"
cancel = "esc"
toggle_remotes = "alt+r"
refresh = "ctrl+r"
```

Bindings accept `alt`, `ctrl`, and `shift` modifiers, such as `ctrl+o`. Supported named keys are `enter`, `esc`, `tab`, `backspace`, `up`, and `down`.

## Keybindings

Add to `~/.config/herdr/config.toml`:

```toml
# Remove default new_worktree binding (optional)
new_worktree = ""

# Wire plugin to prefix+shift+g
[[keys.command]]
key = "prefix+shift+g"
type = "plugin_action"
command = "herdr-worktree.open"
description = "new worktree"

# Remove the focused non-primary worktree
[[keys.command]]
key = "prefix+shift+x"
type = "plugin_action"
command = "herdr-worktree.remove"
description = "remove worktree"
```

## Usage

- `prefix+shift+g` - Open the worktree picker
- Type to filter branches
- `alt+r` - Toggle remote branches
- `ctrl+r` - Fetch all remotes and prune deleted refs
- `ctrl+u` - Clear the active filter or branch-name input
- `enter` - Select an existing worktree or branch
- Select `New worktree...`, then choose one of:
- `New branch from current HEAD` for a new branch name
- `New branch from another base` to select a local or remote base, then enter a new branch name
- `Open pull or merge request` for GitHub `pr:123`, GitLab `mr:123`, or a request URL
- `esc` - Cancel; while entering a new worktree, return to the picker
- `prefix+shift+x` - Confirm removal of the focused non-primary worktree

## Releasing

Release Please maintains a release PR from conventional commits on `main`:

- `fix:` creates a patch release.
- `feat:` creates a minor release.
- A `!` after the type/scope or a `BREAKING CHANGE:` footer creates a major release.
- Other commit types do not create a release.

Merging the release PR updates the Cargo, plugin, and README versions, creates the tag, and publishes the GitHub release.
