#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "${repo_root}"

run_cargo_deny() {
  if [[ -x "${repo_root}/target/cargo-deny/bin/cargo-deny" ]]; then
    "${repo_root}/target/cargo-deny/bin/cargo-deny" "$@"
  else
    cargo deny "$@"
  fi
}

if [[ ! -x "${repo_root}/target/cargo-deny/bin/cargo-deny" ]] && ! cargo deny --version >/dev/null 2>&1; then
  echo "verify: cargo-deny is required but was not found" >&2
  echo "install it with: cargo install --locked cargo-deny" >&2
  exit 1
fi

echo "verify: cargo fmt --all -- --check"
cargo fmt --all -- --check

echo "verify: cargo clippy --all-targets -- -D warnings"
cargo clippy --all-targets -- -D warnings

echo "verify: cargo test"
cargo test

echo "verify: cargo deny check"
run_cargo_deny check
