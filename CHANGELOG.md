<!-- Concern: version history and notable changes | Non-concern: usage or roadmap (see README) | IO: none -->
# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres
to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **`--include <GLOB>`** — a positive glob selector, the counterpart to `-I/--ignore`:
  it adds files of any type to the tree even when their extension maps to no known
  language (repeatable, pipe-separated; `--include '*'` shows every file). An included
  file's annotation is read *marker-agnostically* (keyed on the invariant `Concern:`
  opener), so extensionless and unrecognized files still surface their one-line
  annotation. Config-enablable via `[display] include = ["*.sh", "Dockerfile"]`.
  `--strict-check` is unaffected — it stays recognized-languages-only (an unknown comment
  grammar cannot be validated).
- **Library API** — the crate now exposes its low-level primitives so another program can
  reuse the `ignore`-based walk (`walk::configured_walk`, `walk::collect_code_files`) and
  the annotation grammar (`annotation::extract`, the marker-agnostic `annotation::extract_any`,
  `annotation::analyze`) over files of any shape, driving its own rendering. The `config`,
  `walk`, and `annotation` modules are public, plus the `build_globset` glob-compile helper;
  the tree model, graph, renderers, and strict-check stay crate-internal.

## [0.2.1] - 2026-07-13

Docs-only release: no change to the binary. Cut to refresh the README shipped
to crates.io and npmjs.

### Changed
- **README.md rewritten around adoption** — what the tool is, intended usage
  (annotate, enforce via a local git hook, read the map every session), a TL;DR
  for humans, the rationale for agents, and install/wire/enforce/configure
  steps. Roughly half its former length.
- **README_APPENDIX.md** (new) — the extended argument (the infinite-context
  objection, related work, what is still unproven) and the full bibliography
  for every inline citation, cross-linked from the README.
- **Annotation guide** — the Non-concern owner may now be an external system or
  out of the repo's scope, not only a named sibling; "true of every file" is
  called out as a truism, not a boundary.
- The repo now carries its own root `.annotation` charter, a
  `docs/communication-style.md` review rubric, and a fixed executable bit on
  `.githooks/pre-commit` (the strict-check gate was being silently skipped).

## [0.2.0] - 2026-07-12

### Added
- **Annotation guide on `--help` and failing `--strict-check`** — the format, a
  GOOD/FAILS contrast, and how to find the Non-concern, shown inline where you fix an
  annotation. Opt out with `--no-guide`.
- **Single-file `--strict-check`** — lint one file, not just a directory (pre-commit
  hook, or the file you just wrote).
- **Directory charters** — a directory carries its own `Concern | Non-concern | IO`
  line (a `.annotation` breadcrumb, else its entry file), promoted onto its tree row.
- **Actionable `--strict-check` diagnostics** — each violation now names the file's
  language, the exact comment marker to use, the *real* line number (past any
  shebang/blank lines — no more hardcoded `:1` that led fixes to clobber a shebang),
  the offending content, and a copy-pasteable conformant example. Missing vs.
  non-conforming annotations get distinct wording, and a wrong-marker line is echoed
  as `found: '…'`. The machine-parseable `path:LINE:` prefix is preserved. MCP
  `strict_check` inherits the richer messages.
- **Machine-readable `--strict-check --format json`** — the strict check now emits a
  structured document (`{passed, error_count, files_checked, violations,
  rule_violations}`) when `--format json` is passed, one record per violation with
  `path`/`line`/`language`/`category`/`marker`/`hint`/`example`/`found`. The default
  TEXT report is unchanged. MCP `strict_check` returns this same structured object
  (byte-for-byte the CLI's `--format json`) instead of a flat text report.
- **`ANNOTATION FORMAT` in `--help`** — `--help` now shows what a conforming
  annotation looks like, with a verbatim example sourced from the built-in
  `[convention].example` so help and enforcement can never disagree.
- **`[convention].example` config field** — a full, conformant annotation line
  (per-language overridable, like `require`/`hint`) that feeds both `--help` and the
  strict-check diagnostics. A test proves every built-in language's example passes
  the lint it advertises.
- **Per-directory display cap** — `--max-per-node <N>` (default 50) shows at most
  N subdirectories and N files per directory, folding the overflow into a single
  `[+N folders and F files, use --full to expand]` marker. Keeps signal-dense
  source trees fully visible while collapsing massive test/corpus folders to one
  line — the overview an agent wants without the token noise. Aggregate `--tokens`
  totals still reflect the full (untruncated) subtree, so a collapsed folder still
  reports its true size. Expand everything with `--full` (or `--max-per-node 0`).
  Display-only: the walk still visits every file, so `--max-files` is unaffected.
  JSON/MCP carry the breakdown as `elided_dirs` / `elided_files` (omitted when
  zero — no schema bump).

### Changed
- **One invariant annotation format** — the three-field `Concern | Non-concern | IO`
  grammar is now fixed (not configurable); the only per-language knob is the comment
  marker.
- **Stricter vacuity enforcement** — a filler `Concern` (`utils`/`helpers`/…) and an
  inward `Non-concern` (`this file's own …`) now fail, matching what the guide teaches.

### Removed
- **`--explain`** — superseded by the annotation guide, now shown inline on a failing
  `--strict-check` and in `--help`.

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
- **Distribution** — crates.io, cargo-binstall, npm/npx, and a
  checksum-verifying `curl | sh` installer.
- Golden-file and integration test suite; CI across Linux, macOS, and Windows.

[Unreleased]: https://github.com/fredrikolis/annotated-tree/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/fredrikolis/annotated-tree/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/fredrikolis/annotated-tree/releases/tag/v0.1.0
