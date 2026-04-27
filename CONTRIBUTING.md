# Contributing

Contributions are welcome.

This repo is pinned to Rust 1.95.0 via `rust-toolchain.toml`.

Enable the repo-managed Git hooks in your clone:

```bash
git config core.hooksPath .githooks
```

Install `cargo-deny` and `gitleaks` if you do not already have them:

```bash
cargo install --locked cargo-deny
brew install gitleaks
```

Before opening a pull request, run:

```bash
./scripts/verify.sh
```

`./scripts/verify.sh` runs:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo deny check
```

The configured pre-commit hook scans staged changes with `gitleaks git --staged --redact --no-banner` before running `./scripts/verify.sh`. GitHub Actions CI runs `./scripts/verify.sh` and will block merges if any verification check fails.

## AI Assistance

AI-assisted development is allowed for this project. If you use AI coding or review tools while preparing a contribution, disclose that in the pull request description.

All submitted changes are expected to be reviewed and understood by the human contributor, who remains responsible for the code they propose.
