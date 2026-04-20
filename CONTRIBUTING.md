# Contributing

Contributions are welcome.

This repo is pinned to Rust 1.95.0 via `rust-toolchain.toml`.

Enable the repo-managed Git hooks in your clone:

```bash
git config core.hooksPath .githooks
```

Install `cargo-deny` if you do not already have it:

```bash
cargo install --locked cargo-deny
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

The configured pre-commit hook and GitHub Actions CI run the same verification and will block merges or local commits if any of those checks fail.

## AI Assistance

AI-assisted development is allowed for this project. If you use AI coding or review tools while preparing a contribution, disclose that in the pull request description.

All submitted changes are expected to be reviewed and understood by the human contributor, who remains responsible for the code they propose.
