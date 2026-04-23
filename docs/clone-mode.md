# Clone Mode Spec

## Summary

Bulkhead's default flow stays unchanged:

```bash
bulkhead shell
```

Clone mode adds an explicitly isolated Git workflow for users who want to keep
agent activity away from the original repository checkout and its Git metadata.

The key idea is simple:

- the source repository stays untouched by the container
- Bulkhead creates a separate clone under `.bulkhead/clones/`
- the container mounts only that clone
- users can still use normal Git, including Git worktrees, inside the isolated clone

## Goals

- Preserve the current `bulkhead shell` workflow as the primary UX.
- Offer a stronger isolation mode than the current-directory workflow.
- Avoid native linked-worktree wiring against the source repository for the first isolated Git feature.
- Keep export simple by storing isolated clones on the host, not only in a container volume.
- Let advanced users create Git worktrees inside the isolated clone if they want lightweight parallel checkouts.

## Non-Goals

- Do not change the behavior of `bulkhead shell`.
- Do not make clone mode the default.
- Do not attempt to include uncommitted source-repo changes in the initial clone.
- Do not provide a special Git metadata bridge back to the source repository.
- Do not ship host-side linked-worktree support for the source repository as the primary isolation story.

## Why Clone Mode

Bulkhead's current mode is convenient but direct:

- the current directory is mounted writable
- if that directory is the main repo checkout, an agent can damage it directly

Host-side Git worktrees only partially help:

- they isolate checkout files
- but linked worktrees still share Git metadata with the source repository

Clone mode gives a cleaner safety boundary:

- checkout files are isolated
- Git metadata is isolated
- the agent can still use standard Git commands normally

## Mode Comparison

### Normal Mode

```bash
bulkhead shell
bulkhead shell --workspace /some/path
```

- Operates on the current directory or an explicit directory path.
- Fastest path.
- Lowest isolation.

### Clone Mode

```bash
bulkhead clone shell feature-x
bulkhead clone shell feature-x --create
```

- Operates on a Bulkhead-managed clone under `.bulkhead/clones/feature-x`.
- Protects the source checkout and source repo metadata from the container session.
- Higher disk usage than normal mode.
- Clearer safety story than host-side linked worktrees.

### Git Worktrees

Git worktrees remain relevant, but the preferred place to use them is inside the
isolated clone, not against the original host repository.

That means:

- no special `.git` metadata wiring against the source repo
- worktree users still get their preferred Git workflow
- any worktree-induced damage stays inside the isolated clone's repository

## Storage Layout

Bulkhead-managed isolated clones live under:

```text
.bulkhead/
  clones/
    <name>/
```

Example:

```text
/repos/app/.bulkhead/clones/feature-x
```

Properties:

- predictable
- easy to clean up
- local to the project
- easy to ignore in the source repo

## Git Ignore

The source repository should ignore `.bulkhead/`.

Recommendation:

```gitignore
.bulkhead/
```

This keeps clone-mode state out of the main repo's status output.

## Command Surface

### `bulkhead clone shell <name>`

Primary entrypoint for isolated Git work.

Behavior:

- If `.bulkhead/clones/<name>` exists, use it.
- If it does not exist:
  - in an interactive terminal, prompt before creation
  - in a non-interactive context, fail unless `--create` is passed
- After resolving or creating the clone, run the normal Bulkhead shell flow there.

Useful options:

- `--create`
  Create the clone immediately when missing.
- `--base <rev>`
  When creating, use a specific revision or branch as the base after cloning.
  Without this, use the exact commit that the source repository's `HEAD`
  points to; do not infer the source branch's tracked branch name.
- `--branch <name>`
  Create or force-move a specific branch name inside the new clone. This is
  useful when the managed clone name and the Git branch name should differ.
- `--detach`
  When creating, leave the clone in detached HEAD mode.
- `--allow-dirty-source`
  Continue even if the source repository has uncommitted changes.

Examples:

```bash
bulkhead clone shell feature-x
bulkhead clone shell feature-x --create
bulkhead clone shell feature-x --create --base origin/main
bulkhead clone shell review-fix --create --branch fix/review
bulkhead clone shell scratch --create --detach
```

### `bulkhead clone list`

List Bulkhead-managed clones for the current source repository.

Output should include:

- clone name
- path
- current branch or detached state
- dirty/clean state if cheap to compute

### `bulkhead clone remove <name>`

Delete a Bulkhead-managed clone.

Behavior:

- refuse if the name does not exist
- prompt before deletion unless `--force`
- remove the clone directory only

### Optional Later Commands

- `bulkhead clone path <name>`
- `bulkhead clone exec <name> -- <cmd>`
- `bulkhead clone status`
- `bulkhead clone prune`

These are not necessary for the first version.

## Clone Creation Semantics

Clone mode must create a repository that is independent from the source repo.

Important Git detail:

- local `git clone /path/to/repo` optimizes by default
- that can use local clone behavior and hardlinks
- shared or hardlinked object storage weakens the isolation story

Implementation requirement:

- do not use `--shared`
- avoid shared-object assumptions entirely
- prefer an explicitly independent local clone invocation such as:

```bash
git clone --no-local --no-hardlinks <source-repo> <destination>
```

This is intentionally conservative. Disk use is a better trade than a clone that
quietly shares storage with the source repo.

## Source Repository Preconditions

Clone mode should require that the current directory is inside a Git repository.

Creation behavior:

- resolve the source repository root with Git
- create `.bulkhead/clones/` under that source repo root if absent
- clone from the source repository root, not from the current subdirectory

## Dirty Source Repository Behavior

This needs to be explicit because it is easy to misunderstand.

Initial version:

- clone mode only captures committed Git state
- uncommitted source-repo changes are not copied into the clone

Recommended UX:

- if the source repo is dirty and the user creates a new clone:
  - interactive terminal: warn and ask whether to continue
  - non-interactive: fail unless an explicit `--allow-dirty-source` or similar flag is passed

Suggested warning text:

> The source repository has uncommitted changes. Clone mode starts from committed Git state only. Continue?

## Branch and Checkout Behavior

Recommended default:

- plain `bulkhead clone shell feature-x --create`
  - clone the repo
  - if no `--base` is given, start from the exact commit that the source repo's
    `HEAD` currently points to
  - create or force-move a branch named `feature-x` to that base commit unless
    `--detach` is used

Here, "create or force-move" means the tool creates the branch if it is missing
or moves the branch pointer to the chosen base commit, equivalent to
`git branch -f feature-x <base>`, then checks out that branch. Bulkhead should
not run a separate working-tree `git reset --hard` for this branch pointer
update; requested checkout behavior still uses normal Git checkout semantics.

When no `--base` is supplied, the base is the commit that the source
repository's `HEAD` points to at clone creation time. If the source is in
detached HEAD, use that detached commit hash. If the source `HEAD` is a branch,
do not implicitly use its tracked branch name; users who want a named branch as
the base should pass it explicitly, for example `--base origin/main`.

This keeps the command ergonomic:

- the clone name is the human handle
- the branch name defaults to the same name

Managed clone names are intentionally restricted to a simple on-disk subset:

- ASCII letters and digits
- `-`
- `_`
- `.`
- 1 to 255 characters total
- not exactly `.` or `..`
- no `..` substring
- no leading `.`

These rules keep names as direct children of `.bulkhead/clones/` and reject path
traversal inputs such as `..` or `../../../etc` before they are used on disk.

If users want a richer branch name such as `fix/review`, they should use a safe
managed clone name on disk and pass `--branch fix/review`.

## Runtime Behavior

Once the clone exists, Bulkhead should treat it like any other project directory:

- `bulkhead.toml` lives inside the clone if Bulkhead is bootstrapped there
- `.devcontainer/` is generated there
- the container mounts only the isolated clone directory as `/workspace`

That means:

- deleting files in `/workspace` damages only the isolated clone
- deleting `/workspace/.git` damages only the isolated clone
- the source repo checkout and metadata stay out of scope

## Bootstrap Behavior

When creating a clone, Bulkhead should install its own files there when safe.

Rules:

- if `bulkhead.toml` already exists in the clone, leave it alone
- if `.devcontainer/` already exists in the clone, leave it alone
- otherwise install the selected Bulkhead preset

This uses the same conservative bootstrap rules as any other Bulkhead-managed
directory: leave existing config alone, and only install Bulkhead files when it
is safe to do so.

## Working With Changes Later

Clone mode should not require a custom export step just to keep work.

Because the clone lives on the host, users can:

- open the clone directly
- inspect diffs there
- commit there
- push from there
- cherry-pick from there into the source repo
- generate patches or bundles there if desired

That means no dedicated export command is required for version one.

## Relationship to Worktree Support

Decision:

- clone mode is the primary isolated Git feature
- host-side linked-worktree support for the source repository is not part of the current CLI

Rationale:

- linked worktrees against the source repo require special `.git` metadata wiring
- even with that extra work, linked worktrees still share Git metadata with the source repo
- clone mode gives a cleaner safety model and simpler implementation story

Advanced users can still run:

```bash
git worktree add ../scratch
```

inside the isolated clone if they want lightweight parallel checkouts there.

## Security Model

### Clone Mode Protects

- source repo checkout files
- source repo `.git` metadata
- sibling clones and unrelated host paths, unless explicitly mounted

### Clone Mode Does Not Protect

- the isolated clone from itself
- any extra host mounts the user adds
- network side effects

## Phased Rollout

### Phase 1

- `bulkhead clone shell <name>`
- `bulkhead clone list`
- `bulkhead clone remove <name>`
- `.bulkhead/` ignored in the source repo
- dirty-source warning
- independent clone creation

### Phase 2

- richer status output
- convenience commands for path or exec
- optional cleanup/prune flows

### Phase 3

- revisit whether native host-side worktree support is worth adding later

## Open Questions

- Should dirty-source creation warn by default or fail by default?
- Should `clone remove` refuse when the clone has uncommitted changes unless `--force`?
- Do we want a future `bulkhead clone shell <name> --reset` flow for rebuilding a managed clone from scratch?
