#!/usr/bin/env bash
# Local release validation: runs every release gate that does not require pushing
# anything. Mirrors the CI + release pipeline so a release can be validated on a
# workstation before tagging. See RELEASING.md for the parts that need external
# toolchains (macOS/Windows legs, Homebrew, npm).
set -euo pipefail

cd "$(dirname "$0")/.."

say() { printf '\n\033[1;36m== %s ==\033[0m\n' "$1"; }

say "format"
cargo fmt --check

say "clippy (deny warnings)"
cargo clippy --all-targets -- -D warnings

say "tests"
cargo test --all

say "self-check (dogfood the convention)"
cargo run --release --quiet -- --strict-check src

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
