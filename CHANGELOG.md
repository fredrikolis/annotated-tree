<!-- Covers: Version history and notable changes. Not: Usage or roadmap. Use when: Checking what changed between releases. -->
# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres
to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.1] - 2026-07-10

### Fixed
- **`npx annotated-tree` / `npm install` failed** — the launcher shim
  (`bin/annotated-tree.js`) opened with its annotation comment instead of a
  `#!/usr/bin/env node` shebang, so the npm-linked executable could not run. The
  shebang is restored and CI now asserts it on line 1.
- Strip a leading UTF-8 BOM when reading a file's annotation head, so a
  BOM-prefixed shebang file is no longer mis-read as lacking a first-line shebang.

### Added
- **Shell script support** — `.sh` / `.bash` files are now recognized by the
  annotation engine (shebang skipped, annotation read from the first comment
  below it).

## [0.1.0] - 2026-07-10

Initial release.

### Added
- **Annotated tree view** — a directory tree where every source file shows its
  first-line responsibility annotation, extracted by a configurable, per-language
  engine (structured comment tokens plus a regex escape hatch).
- **Cross-ecosystem dependency graph** in the tree — `pyproject.toml`,
  `package.json`, `Cargo.toml`, and `go.mod` are cross-referenced into internal
  deps, external deps, and reverse "used by" edges; unresolved workspace/path deps
  are flagged.
- **`--strict-check`** lint mode — nonzero exit on any code file lacking a
  conforming annotation. Enforces architectural dependency `[rules]` (deny edges,
  forbid cycles, forbid orphans) declared in `.annotated-tree.toml`.
- **`--format json`** (versioned, stable schema) and **`--format md`** output for
  tooling and agents.
- **`--symbols`** — per-file top-level definition outline via tree-sitter
  (feature-gated: build with `--features symbols`).
- **`--mcp`** — serve the map, dependency, and strict-check tools over stdio as a
  Model Context Protocol server (feature-gated: build with `--features mcp`).
- **`--changed` / `--since <ref>`** — restrict the view to files changed versus a
  git ref plus their reverse-dependency blast radius.
- **`--tokens`** rough per-file/package token estimate; **`--age`** modification
  times; **`--max-files`** runaway-scope safety valve (aborts with exit 2 before
  any output).
- Flags: `-L/--max-depth`, `--include-tests`, `--no-gitignore`, `--ascii`,
  `-I/--ignore`, `--config`, `--no-limit`, `--ignore-parsing-errors`.
- **Layered configuration** — built-in defaults < `~/.config/annotated-tree/config.toml`
  < repo `./.annotated-tree.toml` < CLI flags. Regex-configurable extraction and
  validation convention per language.
- Non-fatal stderr warnings for unparseable manifests (silence with
  `--ignore-parsing-errors`); a corrupt manifest never aborts the run.
- **Distribution** — crates.io, cargo-binstall, Homebrew, npm/npx, and a
  checksum-verifying `curl | sh` installer.
- Golden-file and integration test suite; CI across Linux, macOS, and Windows.

[Unreleased]: https://github.com/fredrikolis/annotated-tree/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/fredrikolis/annotated-tree/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/fredrikolis/annotated-tree/releases/tag/v0.1.0
