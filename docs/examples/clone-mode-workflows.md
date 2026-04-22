# Clone Mode Workflows

This document shows the intended user experience for clone mode.

The commands below match the isolated-clone workflow described in
[Clone Mode](../clone-mode.md).

## Why Clone Mode

Bulkhead's current flow is intentionally simple:

```bash
bulkhead shell
```

That should stay the default.

Clone mode is for users who want a stronger isolation boundary for Git work:

- the source checkout stays out of the container
- the source repository's `.git` metadata stays out of the container
- the agent works in a Bulkhead-managed clone under `.bulkhead/clones/`

## Basic Day-To-Day Flow

Start an isolated clone session:

```bash
bulkhead clone shell feature-x --create
```

Behavior:

- create `.bulkhead/clones/feature-x` if it does not exist
- clone from the current repository into that path
- bootstrap Bulkhead files there when safe
- start the normal Bulkhead shell in the clone

Re-enter the same clone later:

```bash
bulkhead clone shell feature-x
```

## Create From A Specific Base

Start from `origin/main` instead of whatever the source repository currently has checked out:

```bash
bulkhead clone shell feature-x --create --base origin/main
```

If the managed clone name and the Git branch name should differ:

```bash
bulkhead clone shell review-fix --create --branch fix/review
```

## Detached Scratch Clone

For experimentation without a branch:

```bash
bulkhead clone shell scratch --create --detach
```

## See What Clones Already Exist

```bash
bulkhead clone list
```

Output includes:

- clone name
- path
- current branch or detached state
- possibly dirty/clean state

## Remove A Managed Clone

```bash
bulkhead clone remove feature-x
```

Behavior:

- prompt before deletion unless `--force`
- remove only `.bulkhead/clones/feature-x`

## Dirty Source Repositories

Clone mode starts from committed Git state only. If the source repository has
uncommitted changes:

- interactive runs warn before creating the clone
- non-interactive runs require `--allow-dirty-source`

## How This Fits With The Existing Flow

Current default:

```bash
bulkhead shell
```

Isolated Git workflow:

```bash
bulkhead clone shell feature-x --create
```

So the mental model stays simple:

- `bulkhead shell`
  Work directly in the current directory.
- `bulkhead clone shell <name>`
  Work in a Bulkhead-managed isolated clone.

## Using Git Worktrees Inside The Clone

Clone mode does not remove Git worktrees from the picture. It just relocates
them to the isolated clone, where they no longer share metadata with the source
repository.

Example:

```bash
bulkhead clone shell feature-x --create
# inside the isolated clone:
git worktree add ../feature-x-scratch
```

That way:

- the source repository stays isolated from the container
- worktree users still get a normal Git workflow inside the isolated clone

## What Changes Later

Because the clone lives on the host, users can later:

- inspect diffs there
- commit there
- push from there
- cherry-pick from there into the source repository
- create patches or bundles there if they want an explicit handoff

The important point is that clone mode should not require a special export step
just to keep the work.
