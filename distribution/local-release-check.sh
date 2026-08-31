#!/usr/bin/env bash
# Concern: runs every local release gate that does not require pushing (fmt, clippy, tests, build) | Non-concern: gates that require network/push (CI owns those) | IO: (working tree) -> pass/fail
set -euo pipefail

cd "$(dirname "$0")/.."

say() { printf '\n\033[1;36m== %s ==\033[0m\n' "$1"; }

say "CHANGELOG has a section for the Cargo.toml version"
# create-gh-release-action fails on a missing heading AFTER the tag push, breaking the release.
version=$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)
if ! grep -q "^## \[$version\]" CHANGELOG.md; then
  echo "CHANGELOG.md has no '## [$version]' heading — the release would abort after tagging" >&2
  exit 1
fi

say "format"
cargo fmt --check

say "clippy (deny warnings)"
cargo clippy --all-targets -- -D warnings

say "tests"
cargo test --all

say "self-check (dogfood the convention)"
# Second run adds --include-tests but skips tests/fixtures (deliberately malformed).
bash .githooks/pre-commit

say "crate is publishable"
cargo package --list --allow-dirty >/dev/null
cargo publish --dry-run --allow-dirty

if command -v cargo-zigbuild >/dev/null 2>&1; then
  for target in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
    say "zigbuild $target"
    rustup target add "$target" >/dev/null 2>&1 || true
    cargo zigbuild --release --target "$target"
    file "target/$target/release/annotated-tree"
  done
else
  echo "note: cargo-zigbuild not installed — skipping musl/aarch64 cross builds"
fi

say "ALL LOCAL RELEASE CHECKS PASSED"
