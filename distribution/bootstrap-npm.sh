#!/usr/bin/env bash
# Concern: one-time npm bootstrap that publishes the initial placeholder packages | Non-concern: normal release publishing (RELEASING.md owns it) | IO: (npm credentials) -> published placeholders
# exist before a trusted publisher can be attached.
# Usage:  distribution/bootstrap-npm.sh v0.1.0
set -euo pipefail

tag="${1:?usage: distribution/bootstrap-npm.sh <tag, e.g. v0.1.0>}"
version="${tag#v}"
repo="fredrikolis/annotated-tree"
cd "$(dirname "$0")/.."

targets="x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-apple-darwin aarch64-apple-darwin"
for target in $targets; do
  mkdir -p "dist/${target}"
  archive="annotated-tree-${target}.tar.gz"
  gh release download "$tag" -R "$repo" -p "$archive" -O "$archive" --clobber
  tar -xzf "$archive" -C "dist/${target}"
  rm -f "$archive"
done

node distribution/npm/scripts/build-npm.mjs "$version" dist

echo ">> Publishing 5 packages (npm will prompt for your 2FA OTP each time)…"
for plat in linux-x64-musl linux-arm64-musl darwin-x64 darwin-arm64; do
  npm publish "./distribution/npm/platforms/${plat}"
done
# Leading ./ required: a bare `npm publish npm` would republish the npm CLI itself.
npm publish ./distribution/npm

cat <<'EOF'
>> Bootstrap publish complete.
   Next (one-time): add a Trusted Publisher to each of the 5 packages on npmjs.com
   (GitHub Actions provider — org: fredrikolis, repo: annotated-tree, workflow:
   release.yml). After that, all future releases publish via OIDC with no token.
EOF
