# Repository Guidelines

## What This Repo Is
`bulkhead` is a Rust CLI for creating and managing hardened devcontainer workspaces for local coding agents.

The main model to preserve:
- `bulkhead.toml` is the source of truth for a workspace
- `.devcontainer/devcontainer.json` is generated from that config
- the current workspace is writable in-container, but Bulkhead tries to keep other host exposure narrow by default

## Project Layout
- `src/main.rs`: thin binary entrypoint
- `src/lib.rs`: parses CLI input and dispatches into the library
- `src/cli.rs`: Clap command and argument definitions
- `src/commands/`: subcommand handlers
- `src/config.rs`: config schema, parsing, validation helpers, path/volume helpers, template support
- `src/devcontainer.rs`: devcontainer validation and JSON generation
- `src/docker.rs`: Docker resource discovery and cleanup helpers
- `src/system.rs`: external command execution and environment/tool checks
- `templates/`: managed workspace template files, including the bundled Dockerfile and post-create bootstrap script

## Working Rules
- Prefer changing generator code and templates over editing local workspace artifacts directly.
- Treat repo-root `.devcontainer/` and `bulkhead.toml` as ignored local files, not authoritative source files.
- Preserve the security posture when changing config validation or mount behavior. In particular, do not weaken the existing Docker socket, `SYS_ADMIN`, reserved mount target, or broad host-mount protections by accident.
- Keep CLI definitions in `src/cli.rs`, command behavior in `src/commands/`, and README examples in sync.
- If you change agent support, update all of the relevant surfaces together:
  - `PreinstalledAgent` and config parsing in `src/config.rs`
  - env, mounts, and generated devcontainer behavior in `src/devcontainer.rs`
  - bootstrap/install behavior in `templates/bulkhead-post-create.sh`
  - user-facing docs in `README.md`

## Testing And Validation
- Enable the repo hook in a clone with `git config core.hooksPath .githooks`.
- Commits are expected to pass the repo-managed pre-commit hook at `.githooks/pre-commit`.
- The canonical local and CI verification entrypoint is `./scripts/verify.sh`.
- Dependency policy is defined in `deny.toml` and enforced with `cargo-deny`.
- `./scripts/verify.sh` runs:
  - `cargo fmt --all -- --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test`
  - `cargo deny check`
- GitHub Actions runs the same verification in `.github/workflows/ci.yml`.
- Add or update unit tests alongside the touched module. The main test coverage for config and generation behavior lives in:
  - `src/config.rs`
  - `src/devcontainer.rs`
  - `src/docker.rs`
  - `src/system.rs`

## Documentation Notes
- If you change the config format, generated mounts/env, supported agents, or user-visible commands, update `README.md`.
- If you change contributor workflow or required validation steps, update `CONTRIBUTING.md`.
