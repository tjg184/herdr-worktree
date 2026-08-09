# Herdr Worktree Plugin

Create, switch, and remove Git worktrees from Herdr.

## Installation

### Prerequisite

This plugin uses [Worktrunk](https://worktrunk.dev/) by default. It provides hooks, path templates, pull/merge request opening, and conditional branch cleanup. Install it for the default backend:

```bash
brew install worktrunk
# or
cargo install worktrunk
```

Install from GitHub:

```bash
herdr plugin install tjg184/herdr-worktree
```

For local development:

```bash
herdr plugin link /path/to/herdr-worktree
```

## Configuration

The defaults work without configuration. To normalize Jira-style branch names,
create `~/.config/herdr/plugins/config/herdr-worktree/config.toml`:

```toml
normalize_jira_prefix = true
```

See [`config.example.toml`](config.example.toml) to customize picker keybindings.

Set `backend = "native"` to use Herdr's built-in worktree commands instead. Native mode requires Herdr 0.8.0+, does not require Worktrunk, keeps branches when removing worktrees, and does not support pull/merge request references.

## Keybindings

Add to `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "prefix+shift+g"
type = "plugin_action"
command = "herdr-worktree.open"
description = "worktree picker"
```

## Usage

- Run the bound command to open the picker.
- Select an existing worktree to switch to it, or select `New worktree...`.
- Create a branch from the current checkout or another branch, or open a GitHub pull request, GitLab merge request, or request URL.
- Type to filter. The picker displays available controls and prompts at each step.

## Releasing

Release Please maintains a release PR from conventional commits on `main`:

- `fix:` creates a patch release.
- `feat:` creates a minor release.
- A `!` after the type/scope or a `BREAKING CHANGE:` footer creates a major release.
- Other commit types do not create a release.

Merging the release PR updates the Cargo and plugin versions, creates the tag, and publishes the GitHub release.
