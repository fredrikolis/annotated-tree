#!/usr/bin/env bash
# Concern: one-time npm bootstrap that publishes an initial placeholder package so later CI releases can authenticate | Non-concern: normal release publishing (RELEASING.md owns that) | IO: (npm credentials) -> published placeholder package
# exist before a trusted publisher can be attached — so the 6 packages must be
# published once by hand. This stages them from a published GitHub Release's binaries
# and publishes them under your logged-in npm account (`npm login` first; npm will
# prompt for your 2FA OTP per package). After this, configure a trusted publisher for
# each package on npmjs.com and every future release publishes via OIDC in CI — no
# token. Run ONCE.
#
# Usage:  scripts/bootstrap-npm.sh v0.1.0
set -euo pipefail

tag="${1:?usage: scripts/bootstrap-npm.sh <tag, e.g. v0.1.0>}"
version="${tag#v}"
repo="fredrikolis/annotated-tree"
cd "$(dirname "$0")/.."

targets="x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc"
for target in $targets; do
  mkdir -p "dist/${target}"
  case "$target" in *windows*) ext=zip ;; *) ext=tar.gz ;; esac
  archive="annotated-tree-${target}.${ext}"
  gh release download "$tag" -R "$repo" -p "$archive" -O "$archive" --clobber
  if [ "$ext" = zip ]; then
    unzip -o "$archive" -d "dist/${target}"
  else
    tar -xzf "$archive" -C "dist/${target}"
  fi
  rm -f "$archive"
done

node npm/scripts/build-npm.mjs "$version" dist

echo ">> Publishing 6 packages (npm will prompt for your 2FA OTP each time)…"
for plat in linux-x64-musl linux-arm64-musl darwin-x64 darwin-arm64 win32-x64; do
  npm publish "./npm/platforms/${plat}"
done
# Leading ./ is REQUIRED: `npm publish npm` treats `npm` as the package spec (and
# tries to republish the npm CLI); `./npm` publishes the local directory.
npm publish ./npm

cat <<'EOF'
>> Bootstrap publish complete.
   Next (one-time): add a Trusted Publisher to each of the 6 packages on npmjs.com
   (GitHub Actions provider — org: fredrikolis, repo: annotated-tree, workflow:
   release.yml). After that, all future releases publish via OIDC with no token.
EOF
