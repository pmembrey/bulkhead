# Bulkhead

`bulkhead` is a small Rust CLI for running local coding agents inside a hardened devcontainer.

The bundled template currently reflects a Rust-heavy maintainer workflow: `rustup`, `zellij`, `vim`, GitHub CLI, and a few audit-oriented terminal tools are available by default.

The basic model is simple:

- the current project directory is writable inside the container
- the rest of your laptop is not exposed unless you explicitly add mounts
- `.devcontainer` is mounted read-only inside the container so code running in-container cannot rewrite the host-executed container config during rebuild
- `bulkhead.toml` is mounted read-only inside the container so code running in-container cannot rewrite its own host-side policy

## Status

This is still under active development. The config format and CLI are not treated as stable yet.

Current design notes:

- [Clone mode spec](docs/clone-mode.md)
- [Clone mode workflows](docs/examples/clone-mode-workflows.md)

## Why

Local agent workflows are useful, but running them directly on the host with broad access is a bad default. `bulkhead` aims to make the safe path easy:

- generate a devcontainer workspace
- keep host exposure narrow by default
- manage extra mounts explicitly
- make common lifecycle operations simple from one CLI

The current default remains the simplest flow:

```bash
bulkhead shell
```

Git-aware isolation support:

- clone mode for stronger isolation under `.bulkhead/clones/`
- Git worktrees inside those isolated clones when you want lighter-weight parallel checkouts

## Quick Start

Prerequisites:

- a Docker runtime such as Docker Desktop, OrbStack, or Colima
- the Dev Container CLI

Install with Homebrew:

```bash
brew tap pmembrey/bulkhead
brew install bulkhead
```

Or build and install the binary from this repo:

```bash
cargo install --path .
```

Create a workspace:

```bash
mkdir my-project
cd my-project
bulkhead shell
```

Create and enter an isolated clone from an existing repository:

```bash
cd my-repo
bulkhead clone shell feature-x --create
```

If `bulkhead.toml` does not exist yet, `bulkhead` will offer to create it and let you choose a preset. If the Dev Container CLI is missing, use:

```bash
bulkhead doctor --fix
```

## Common Commands

The normal entrypoint is:

```bash
bulkhead shell
```

That will bootstrap the workspace if needed, start the container if needed, and open a `bash` shell inside it.

Other useful commands:

- `bulkhead up`
  Start or ensure the container is running without opening a shell.
- `bulkhead rebuild`
  Rebuild the container after changing `bulkhead.toml`, mounts, or managed files.
- `bulkhead down`
  Stop the running container but keep its managed resources.
- `bulkhead status`
  Show workspace config, remote user, mount count, and current container state.
- `bulkhead logs`
  Show Docker logs for the workspace container.
- `bulkhead logs --tail 200 -f`
  Follow recent container logs live.
- `bulkhead exec -- pwd`
  Run a one-off command inside the container without opening an interactive shell.
- `bulkhead clone shell feature-x`
  Re-enter an existing Bulkhead-managed isolated clone by name.
- `bulkhead clone shell feature-x --create`
  Create `.bulkhead/clones/feature-x`, bootstrap Bulkhead there when safe, and open a shell inside it.
- `bulkhead clone shell review-fix --create --branch fix/review`
  Use a simple managed clone name on disk but create a different branch inside the clone.
- `bulkhead clone list`
  Show the managed isolated clones for the current repository.
- `bulkhead clone remove feature-x`
  Delete one managed clone without touching the source checkout.
- `bulkhead mount list`
  Show the extra host path mounts currently configured in `bulkhead.toml`.
- `bulkhead mount add ~/drop /drop --rw`
  Add a writable host mount.
- `bulkhead mount add ~/secrets /secrets --access ro`
  Add a read-only host mount.
- `bulkhead mount remove /drop`
  Remove a configured host mount by source or target.
- `bulkhead config git status`
  Show whether the managed host `~/.gitconfig` mount is enabled.
- `bulkhead config git disable`
  Disable the managed host `~/.gitconfig` mount.
- `bulkhead destroy`
  Remove the container and Bulkhead-managed Docker resources for the workspace.

## Config

`bulkhead.toml` is the source of truth. `bulkhead` generates `.devcontainer/devcontainer.json` from it.

Example:

```toml
name = "Bulkhead Agent Sandbox"
workspace_folder = "/workspace"
remote_user = "miggyx"
agents = ["claude", "codex", "pi"]

[build]
dockerfile = ".devcontainer/Dockerfile"
context = ".devcontainer"

[git]
enabled = true

[[path]]
source = "~/drop"
target = "/drop"
access = "rw"
```

A few important points:

- `remote_user` is set from the host username when a template is created
- the bundled Dockerfile makes `remote_user` the actual non-root account in the container, not just the exec target
- `[build]` points at the Dockerfile and build context to use, relative to the workspace root
- the bundled Dockerfile is Rust-oriented by default, pins its base image and bundled tool versions, and keeps the base devcontainer bash setup intact, but you can point `[build]` at another Dockerfile in your repo if your workflow is different
- if you replace the bundled Dockerfile, your custom build is responsible for creating whatever `remote_user` you configure
- `features` is allowlisted to Bulkhead-supported Dev Container Features, because features can carry their own runtime metadata
- `agents` preinstalls pinned supported agent CLIs inside the container and attaches persistent config volumes for them
- currently supported agents are `claude`, `codex`, and `pi`
- `claude` forwards `ANTHROPIC_API_KEY` and `CLAUDE_CODE_OAUTH_TOKEN` from the host and persists config under `~/.claude` in the container
- if `CLAUDE_CODE_OAUTH_TOKEN` is set, Bulkhead also tries to seed Claude auth during post-create so headless setup can avoid the browser login flow
- if neither `ANTHROPIC_API_KEY` nor `CLAUDE_CODE_OAUTH_TOKEN` is set, expect a one-time Claude login inside the container; the persisted `~/.claude` volume keeps that auth state across rebuilds
- `codex` forwards `OPENAI_API_KEY` from the host and persists config under `~/.codex` in the container
- `pi` forwards `OPENAI_API_KEY` and `ANTHROPIC_API_KEY`, persists config under `~/.pi`, and installs `@mariozechner/pi-coding-agent` after bootstrapping pinned `nvm` and Node versions during post-create
- `[git]` is a dedicated managed feature for mounting host `~/.gitconfig` read-only into the container user's home
- extra host paths live under `[[path]]`
- `access` defaults to read-only unless you explicitly request write access
- `[[path]]` sources must resolve to plain host paths; variable-based sources such as `${localEnv:...}` are not allowed
- mount targets are normalized and may not point at or under Bulkhead's read-only `/workspace/.devcontainer` or `/workspace/bulkhead.toml` mounts
- `run_args` is allowed for narrow Docker options, but Bulkhead rejects flags that would bypass its host-access policy, including privileged mode, host bind mounts, devices, host namespaces, Docker security options, and any `--cap-add` value outside the `NET_ADMIN` / `NET_RAW` allowlist

## Isolated Clones

Clone mode is the recommended Git-aware isolation workflow:

```bash
bulkhead clone shell feature-x --create
```

That command:

- creates an independent local clone under `.bulkhead/clones/feature-x` when it does not exist yet
- bootstraps Bulkhead there when safe
- starts the normal Bulkhead shell flow inside that clone
- keeps the source checkout and source repository metadata out of the container
- warns if the source repository is dirty, since clone mode starts from committed Git state only

Useful workflows:

```bash
# Re-enter an existing managed clone
bulkhead clone shell feature-x

# Create a fresh isolated clone and branch from origin/main
bulkhead clone shell feature-x --create --base origin/main

# Create a detached scratch clone
bulkhead clone shell scratch --create --detach

# See what managed clones already exist
bulkhead clone list

# Remove one managed clone
bulkhead clone remove feature-x
```

Managed clone names are simple directory names under `.bulkhead/clones/`. If you
want a different branch name inside the clone, set it explicitly:

```bash
bulkhead clone shell review-fix --create --branch fix/review
```

The clone name is also the default Git branch name. If the on-disk clone name is
not a valid Git branch name, pass `--branch <name>` or `--detach`.

Because clone mode uses a normal independent Git clone, you can still create Git
worktrees inside the clone later if you want them:

```bash
bulkhead clone shell feature-x --create
# inside the clone:
git worktree add ../feature-x-scratch
```

## Safety Model

`bulkhead` is trying to give you a practical host-protection boundary, not perfect sandboxing.

Defaults:

- current workspace mounted read-write
- `.devcontainer` mounted read-only
- `bulkhead.toml` mounted read-only
- no Docker socket mount
- dangerous Docker runtime flags rejected, including privileged mode, host bind mounts, devices, host namespaces, Docker security options, and any `--cap-add` value outside the `NET_ADMIN` / `NET_RAW` allowlist
- minimal host mounts unless explicitly configured

Still true:

- code inside the container can fully modify the repo you launched from
- network access is not blocked by default
- adding broad writable host mounts weakens the model

Clone mode changes that tradeoff for Git-based workflows by moving the
container into a Bulkhead-managed isolated clone under `.bulkhead/clones/`.
That design is documented in [docs/clone-mode.md](docs/clone-mode.md).

## Diagnostics

`bulkhead doctor` checks:

- Docker installed
- Docker daemon reachable
- Dev Container CLI installed
- Docker buildx health

It also tries to surface the common Docker buildx permission problem early, including the `~/.docker/buildx/activity/... operation not permitted` failure that can otherwise show up later during `devcontainer up`.

## Inspiration

`bulkhead` is heavily inspired by Trail of Bits' `claude-code-devcontainer` project:

- https://github.com/trailofbits/claude-code-devcontainer

This repo started by studying and borrowing the security posture of that project, then rebuilding the operator layer as an agent-agnostic Rust CLI instead of a Claude-specific Bash wrapper.

## AI Assistance

This project was developed with assistance from AI coding and review tools. All code, design decisions, and releases are reviewed and approved by the maintainer(s), who remain responsible for the software.

## License

Licensed under either of:

- MIT license ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.
