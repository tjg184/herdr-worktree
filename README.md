# Herdr Worktree Plugin

Interactive worktree picker for Herdr terminal workspace manager.

## Features

- Browse existing worktrees, local branches, and remote branches
- Create new worktrees from branches or free-form names
- Jira-style branch name normalization (optional): `ticket-123` → `TICKET-123`
- Toggle remote branches with `alt+r`
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

# Or install from GitHub
herdr plugin install <github-org>/<repo>@<version>
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
```

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
```

## Usage

- `prefix+shift+g` - Open the worktree picker
- Type to filter branches
- `alt+r` - Toggle remote branches
- `enter` - Select existing or create new branch
- `esc` - Cancel and close
