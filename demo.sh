#!/usr/bin/env bash
# Concern: builds annotated-tree if needed, then shows its features against sample/ | Non-concern: the tool's own logic | IO: none
# Run: ./demo.sh
set -euo pipefail

cd "$(dirname "$0")"

BIN="target/release/annotated-tree"

bold() { printf '\n\033[1;36m%s\033[0m\n' "$*"; }
run()  { printf '\033[2m$ %s\033[0m\n' "$*"; eval "$*"; }

# --- Build only if the binary is missing or older than the sources -----------
if [ ! -x "$BIN" ] || [ -n "$(find src Cargo.toml default_config.toml -newer "$BIN" 2>/dev/null)" ]; then
  bold "Building annotated-tree (release)…"
  cargo build --release
else
  bold "Using existing build: $BIN"
fi

bold "1) Annotated tree of the sample monorepo"
echo "   Per-file annotations + cross-ecosystem dependency graph (Python/npm/Cargo/Go)."
run "$BIN sample"

bold "2) Depth-limited overview (-L 2): just packages and their dependency edges"
run "$BIN -L 2 sample"

bold "3) Reveal gitignored files (--no-gitignore): sample/build/ reappears"
run "$BIN --no-gitignore -L 2 sample"

bold "4) ASCII glyphs + exclude globs (--ascii -I 'frontend|services')"
run "$BIN --ascii -I 'frontend|services' -L 2 sample"

bold "5) Strict-check lint mode (nonzero exit flags non-conforming files)"
run "$BIN --strict-check sample" || echo "   (exit $? — offenders found, as expected)"

bold "Done."
echo "Try it on this repo:   $BIN --no-gitignore -I 'target' ."
