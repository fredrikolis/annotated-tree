<!-- Concern: how to cut a release and validate the pipeline locally first | Non-concern: tool usage (see README) | IO: none -->
# Releasing

`annotated-tree` ships a single static binary through several channels from one
tag. The pipeline is intentionally **hand-rolled** (GitHub Actions +
`taiki-e/*` actions) rather than `cargo-dist`, so the release shape is fully under
our control and every action is independently maintained. `cargo-dist` remains a
viable batteries-included alternative (see the end of this doc).

## Distribution channels

| Channel | User command | Produced by |
|---|---|---|
| crates.io | `cargo install annotated-tree` | `publish-crate` job |
| Prebuilt binary | `cargo binstall annotated-tree` | `upload-assets` (binstall-compatible names) |
| GitHub Releases | download tarball + `.sha256` | `upload-assets` |
| Shell installer | `curl … annotated-tree-installer.sh \| sh` | `create-release` (uploads `installer/install.sh`) |
| npm | `npx annotated-tree` | `publish-npm` job (reuses `upload-assets` binaries) |

Target matrix: linux `{x86_64,aarch64} × {gnu,musl}`, macOS `{x86_64,aarch64}`,
Windows `x86_64-msvc`. The linux cross targets build with `cargo-zigbuild`.

## Validate locally FIRST

Run the full local gate before tagging:

```sh
./scripts/local-release-check.sh
```

This runs fmt, clippy (`-D warnings`), the test suite, the dogfood self-check,
`cargo publish --dry-run`, and — if `cargo-zigbuild` is installed — the static
musl/aarch64 Linux builds. It does everything the pipeline does that doesn't
require pushing.

### Extra local checks that need external toolchains

- **All cross targets from Linux** (musl + macOS), via Zig:
  ```sh
  cargo install cargo-zigbuild
  rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl universal2-apple-darwin
  cargo zigbuild --release --target x86_64-unknown-linux-musl
  cargo zigbuild --release --target universal2-apple-darwin   # needs macOS SDK; see note
  ```
  A macOS target from Linux needs a macOS SDK (`SDKROOT`) only if the binary links
  Apple frameworks — this pure-Rust CLI does not, so the darwin legs build without
  an SDK. Validate against the real `Cargo.lock`, not a hello-world.
- **Run the release workflow's Linux legs** with [`act`](https://github.com/nektos/act):
  ```sh
  act -W .github/workflows/release.yml push --dryrun
  ```
  `act` cannot exercise the macOS/Windows matrix legs or real asset upload/OIDC —
  validate those with the zig builds above and the binary smoke test.
- **Shell installer** (`installer/install.sh`) — validate the download → verify →
  install path against a local file server before trusting it in a release:
  ```sh
  shellcheck installer/install.sh          # or: sh -n installer/install.sh
  cargo build --release
  # Package under the exact release asset name the installer requests for this
  # host (linux x86_64 → the musl name) and generate its .sha256:
  target=x86_64-unknown-linux-musl
  mkdir -p srv
  tar czf srv/annotated-tree-$target.tar.gz -C target/release annotated-tree
  ( cd srv && sha256sum annotated-tree-$target.tar.gz > annotated-tree-$target.tar.gz.sha256 )
  ( cd srv && python3 -m http.server 8137 & )
  INSTALL_DIR=$(mktemp -d) ANNOTATED_TREE_BASE_URL=http://localhost:8137 \
    sh installer/install.sh
  "$INSTALL_DIR"/annotated-tree --version    # must print the version
  # Then corrupt srv/*.sha256 and re-run: the installer MUST exit nonzero and
  # install nothing. This is the same flow CI's `installer-e2e` job runs.
  ```

## Cut a release

1. Bump `version` in `Cargo.toml`; move the `CHANGELOG.md` `[Unreleased]` section
   under the new version heading.
2. `./scripts/local-release-check.sh` — must pass.
3. Commit, then tag: `git tag vX.Y.Z && git push --tags`.
4. The `release` workflow creates the GitHub Release, uploads binaries + checksums,
   publishes to crates.io, and publishes the npm channel.

## Required repository secrets

- `CARGO_REGISTRY_TOKEN` — crates.io API token (`publish-crate`).

The npm channel (`publish-npm`) uses **OIDC trusted publishing** — no stored token.
Each of the 6 packages (`annotated-tree` + the five `annotated-tree-*` platform
packages) needs a GitHub Actions trusted publisher configured once on npmjs.com
(org `fredrikolis`, repo `annotated-tree`, workflow `release.yml`); see
`scripts/bootstrap-npm.sh` for the one-time bootstrap.

## npm channel (`npx annotated-tree`)

Packaging is documented in [`npm/PACKAGING.md`](npm/PACKAGING.md); the `publish-npm` job wires it into
the release. The design is `optionalDependencies` + a thin JS shim (no
`postinstall` downloader — works under `--ignore-scripts`): the main package
`annotated-tree` depends on per-platform packages
`annotated-tree-{linux-x64-musl,linux-arm64-musl,darwin-x64,darwin-arm64,win32-x64}`,
each carrying one prebuilt binary with `os`/`cpu`/`libc` fields so npm installs
exactly one. `npm/bin/annotated-tree.js` resolves that binary and forwards
argv/stdio/exit code (missing binary → explicit nonzero error, never a silent
no-op). This matters for agent sandboxes that have Node but not Rust.

**Flow (in `release.yml`, after `upload-assets`):**

1. Download each `annotated-tree-<target>.tar.gz` release asset and extract its
   binary into `dist/<target>/` — **the same binaries the GitHub Release ships**
   (one artifact, many channels; no parallel build).
2. `node npm/scripts/build-npm.mjs <version> dist` stamps the version across the
   main package (pinning its `optionalDependencies`) and every platform package,
   and copies each binary into its `npm/platforms/<plat>/` dir. Binaries are
   never committed.
3. `npm publish` each platform package first, then the main package.

**Validate locally** (Node ≥ 18; no publish needed) — see
[`npm/PACKAGING.md`](npm/PACKAGING.md#local-check-no-publish): `npm pack --dry-run`
the packages, stage the built binary into a fake `node_modules` platform
package, then drive the shim (`--version`, an exit-code-forwarding
`--strict-check`, and the missing-binary error path). CI's `npm-shim-e2e` job
runs exactly this on every push.

## Alternative: cargo-dist

`cargo-dist` (invoked as `dist`, currently community-maintained under the
`axodotdev` org) generates the whole matrix + shell/PowerShell installers +
npm shim from one config, and its default artifact names are already
binstall-compatible. Trade-off: low bus-factor. To adopt: `cargo install
cargo-dist --locked && dist init`, then validate with `dist plan` and `dist build`
before tagging.
