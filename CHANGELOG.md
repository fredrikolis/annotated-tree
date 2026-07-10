<!-- Covers: Version history and notable changes. Not: Usage or roadmap. Use when: Checking what changed between releases. -->
# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres
to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Initial Rust implementation of `annotated-tree`.
- Annotated `tree` view: per-file first-line annotations extracted via a
  configurable, per-language engine (structured comment tokens + a regex escape
  hatch).
- Cross-ecosystem directory dependency graph across `pyproject.toml`,
  `package.json`, `Cargo.toml`, and `go.mod` — internal deps, external deps, and
  reverse "used by" edges.
- `--strict-check` lint mode with configurable convention (regex + hint).
- Layered configuration: built-in defaults < `~/.config/annotated-tree/config.toml`
  < `./.annotated-tree.toml` < CLI flags.
- Flags: `-L/--max-depth`, `--include-tests`, `--no-gitignore`, `--age`,
  `--ascii`, `-I/--ignore`, `--config`, `--ignore-parsing-errors`.
- Non-fatal stderr warnings for unparseable manifests (silence with
  `--ignore-parsing-errors`); a corrupt manifest never aborts the run and never
  affects stdout.
- Golden-file test suite pinning behaviour against the in-repo `sample/` fixture.
- `curl | sh` installer (`installer/install.sh`): detects the host platform,
  downloads the matching prebuilt release tarball, verifies its `.sha256`
  (aborting on mismatch), and installs into `$INSTALL_DIR` (default
  `~/.local/bin`). Published as the `annotated-tree-installer.sh` release asset
  and covered by a CI shellcheck lint + download→verify→install e2e job.
