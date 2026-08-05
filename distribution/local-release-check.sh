#!/usr/bin/env bash
# Concern: runs every local release gate that does not require pushing (fmt, clippy, tests, build) | Non-concern: gates that require network/push (CI owns those) | IO: (working tree) -> pass/fail
# anything. Mirrors the CI + release pipeline so a release can be validated on a
# workstation before tagging. See RELEASING.md for the parts that need external
# toolchains (the macOS legs, npm).
set -euo pipefail

cd "$(dirname "$0")/.."

say() { printf '\n\033[1;36m== %s ==\033[0m\n' "$1"; }

say "CHANGELOG has a section for the Cargo.toml version"
# .github/workflows/release.yml hands CHANGELOG.md to create-gh-release-action, which looks the
# tag's version up by heading and fails when it finds none — AFTER the tag is pushed, taking
# upload-assets, publish-crate and publish-npm down with it. Catch it here, before the tag.
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
# Whole repo except sample/ (its loose annotations are pinned by tests/golden.rs), matching
# .githooks/pre-commit and CI. Two invocations: `walk::keep_entry` prunes any directory named
# `tests` unless `--include-tests` (default off), so the first cannot see tests/ at all.
# tests/fixtures stays excluded — charter_malformed is deliberately malformed. `--hidden` is
# that prune at the other end: without it nothing under .githooks/ or .github/ is walked.
cargo run --release --quiet -- --strict-check . --hidden --ignore sample --max-length 200
cargo run --release --quiet -- --strict-check . --hidden --ignore sample --include-tests -I 'tests/fixtures' --max-length 200

say "crate is publishable"
cargo package --list --allow-dirty >/dev/null
cargo publish --dry-run --allow-dirty

# Static Linux binaries via zig (no Docker). Skipped if cargo-zigbuild is absent —
# install with: cargo install cargo-zigbuild && (brew install zig | apt install zig)
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
