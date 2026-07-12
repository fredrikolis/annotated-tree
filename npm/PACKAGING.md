<!-- Concern: how the npm `annotated-tree` channel is structured and published (maintainer internals) | Non-concern: tool usage (see the package README) or release mechanics for other channels (see RELEASING.md) | IO: none -->
# npm channel — packaging internals

Maintainer notes for how `npx annotated-tree` is packaged and published. The npmjs.com
package page is the canonical repo-root [`README.md`](../README.md) — one README for
every channel (GitHub, crates.io, npm), copied into `npm/` at publish time by
`build-npm.mjs`, never committed here.

`npx annotated-tree` runs the same prebuilt binary as every other channel — no
Rust toolchain needed. This directory is the packaging for that channel.

## Layout

```
npm/
  package.json              main package `annotated-tree` (the thin shim)
  bin/annotated-tree.js     launcher: resolves the platform binary, forwards argv/stdio/exit code
  platforms/<plat>/         one package per target (os/cpu/libc + the binary)
  scripts/build-npm.mjs     publish preparer (stamps version, injects binaries + README)
  README.md                 injected at publish from ../README.md (npmjs.com page); gitignored
  PACKAGING.md              this file (maintainer-only; not published to npm)
```

## How it works (optionalDependencies, no postinstall)

The main package declares each per-platform package as an **optionalDependency**.
npm evaluates every optional dep's `os`/`cpu`/`libc` fields and installs **only
the one** matching the host; the rest are skipped. At runtime the shim
(`bin/annotated-tree.js`) resolves the installed platform package via
`require.resolve('<pkg>/package.json')`, then `execFileSync`s the binary beside
it, forwarding argv and stdio and exiting with the binary's own exit code.

- **`#!/usr/bin/env node` shebang on line 1** — the npm-linked executable (and
  `npx`) exec the shim via its shebang; without it, `npx annotated-tree` breaks.
- **No `postinstall` script** — works under `npm install --ignore-scripts`, no
  install-time network access.
- **Missing binary is explicit** — if no platform package is installed (e.g.
  `--no-optional`, or an unsupported target) the shim prints the platform and how
  to fix it, then exits nonzero. Never a silent no-op.
- **Both Linux targets are static musl**, so one Linux package per arch serves
  both glibc and musl hosts (the `libc` field lists both).

The platform packages deliberately declare **no `bin`** — only the main package
owns the `annotated-tree` command, so there is no ambiguous symlink when several
platform packages theoretically resolve. They ship the binary via `files`.

## Platform matrix

| npm platform package | os | cpu | libc | release target | binary |
|---|---|---|---|---|---|
| `annotated-tree-linux-x64-musl` | linux | x64 | musl (+glibc) | `x86_64-unknown-linux-musl` | `annotated-tree` |
| `annotated-tree-linux-arm64-musl` | linux | arm64 | musl (+glibc) | `aarch64-unknown-linux-musl` | `annotated-tree` |
| `annotated-tree-darwin-x64` | darwin | x64 | — | `x86_64-apple-darwin` | `annotated-tree` |
| `annotated-tree-darwin-arm64` | darwin | arm64 | — | `aarch64-apple-darwin` | `annotated-tree` |
| `annotated-tree-win32-x64` | win32 | x64 | — | `x86_64-pc-windows-msvc` | `annotated-tree.exe` |

The `release` workflow builds these exact targets once for the GitHub Release;
the npm channel reuses those same binaries (single source of truth).

## Publish flow (CI)

Binaries are **not committed** — `scripts/build-npm.mjs` injects them at publish
time from the release artifacts:

1. `release.yml` builds + uploads `annotated-tree-<target>.tar.gz` per target
   (the existing `upload-assets` job).
2. The `publish-npm` job downloads those tarballs, extracts each into
   `dist/<target>/annotated-tree[.exe]`.
3. `node scripts/build-npm.mjs <version> dist` stamps `<version>` across the main
   package (and pins its `optionalDependencies`) and every platform package, copies
   each binary into its `platforms/<plat>/` dir, and copies the canonical repo-root
   `README.md` into `npm/` as the package page.
4. Platform packages are published **first**, then the main package (so the pinned
   optional deps already exist when the main package is installed).

Published via **OIDC trusted publishing** — no stored token. Each of the 6 packages
needs a GitHub Actions trusted publisher configured once on npmjs.com (org
`fredrikolis`, repo `annotated-tree`, workflow `release.yml`); see
[`scripts/bootstrap-npm.sh`](../scripts/bootstrap-npm.sh) for the one-time
bootstrap.

## Local check (no publish)

```sh
# main package contents (README.md is injected by build-npm.mjs, so it only
# appears in the tarball after that runs — e.g. `cp ../README.md ./npm/` first)
npm pack --dry-run ./npm

# prove the shim resolves + forwards (host = linux x64):
cargo build --release
mkdir -p npm/node_modules/annotated-tree-linux-x64-musl
cp target/release/annotated-tree npm/node_modules/annotated-tree-linux-x64-musl/
cp npm/platforms/linux-x64-musl/package.json npm/node_modules/annotated-tree-linux-x64-musl/
node npm/bin/annotated-tree.js --version           # prints the version
node npm/bin/annotated-tree.js --strict-check <bad> # forwards exit 1

# prove the missing-binary path errors (nonzero, not silent):
rm -rf npm/node_modules
node npm/bin/annotated-tree.js --version           # exits nonzero with a clear message
```
