<!-- Concern: version history and notable changes | Non-concern: usage or roadmap (see README) | IO: none -->
# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres
to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Per-file `.annotation` **sidecars** (#1). A file that maps to no comment marker — a CSV, a
  dataset, a binary — carries its contract in a `<name>.annotation` file beside it, holding the
  same bare three-field line a folder's `.annotation` holds. Three consequences:
  a file carrying a sidecar is **listed whatever its extension** (writing the sidecar is the
  opt-in, so no `--include` is needed); the sidecar's own row is **suppressed**, under a criterion
  the report now states (see Changed); and a sidecar is only ever read for a file that cannot hold
  a first-line comment, so `foo.rs.annotation` beside a `foo.rs` is an ordinary file, not a
  sidecar, and an annotation's location stays determined by the path it annotates.
- `orphan_sidecars` on the `--strict-check` report: a `<name>.annotation` whose named file does
  not exist annotates nothing, and is reported as `path: message` (its own list — a dangling path
  is not an issue about an Annotation, and no `[rules]` table configures it). It FAILS the check.
  Nothing is deleted or rewritten: the tool still makes no write of any kind.
- `--strict-check` enforces a sidecar body with the same grammar as a folder charter, reported at
  the sidecar's own path with `language: "sidecar"`.
- `FileNode.sidecar` in the JSON map (omitted when false): the row's annotation came from the
  sidecar beside it. **BREAKING** for a consumer that builds a `FileNode` by struct literal.
- A leading **YAML frontmatter** block is skipped when looking for a file's annotation, exactly as
  a `#!` shebang already was (#16). A Claude Code skill/agent/command, or any static-site page,
  must keep its frontmatter on line 1; before this, such a file could not carry an annotation at
  all, so shipping skills and enforcing `--strict-check` were mutually exclusive. Only a CLOSED
  block at the very start is a prefix — a `---` further down stays a horizontal rule.
- Two accessory binaries, built by `cargo install annotated-tree` (cargo route only; the
  prebuilt channels still ship a single binary):
  `annotated-bash-wrapper` reads a tool's output and appends each printed path's first-line
  annotation to the line it appeared on; `annotated-toolcall-rewrite` is a Claude Code
  `PreToolUse` hook that pipes eligible `grep`, `find` and `ls` calls through it, so an
  agent's own search results carry the contracts. The tool itself is never substituted and
  its exit code is preserved. Neither changes `annotated-tree`.
  See [toolcall-rewrite/README.md](toolcall-rewrite/README.md).
- `annotated-toolcall-rewrite --install-hook [FILE]` and `--uninstall-hook [FILE]`. Cargo has
  no post-install or pre-uninstall step — the only code it runs is `build.rs`, at build time —
  so switching the hook on is an explicit command. It MERGES one `PreToolUse` entry into the
  settings file, keeping every other key: that file holds the permissions a user has accepted,
  and a setup step that overwrote it would cost them all of them. Defaults to
  `~/.claude/settings.json`; pass `.claude/settings.local.json` for a single repo. Idempotent,
  writes atomically, refuses a file that does not parse rather than replacing it, and
  `--uninstall-hook` removes only the entry it added.
- `annotated_tree::resolve_charter` — resolve a directory's charter through the public API.
  `Charter` was already exported with no way to obtain one.

### Changed
- `-L LEVEL` caps the **walk**, not just the render (#15). The traversal stops at the deepest
  level the output can show, so `annotated-tree -L 1 ~` no longer walks an entire home directory
  to print one level (measured on one: 2.4 s warm and 253k directory reads, down to 17 ms and
  88). Three user-visible consequences:
  **empty directories are listed** — a directory earns its row by being VISITED, not by holding
  a listable file somewhere beneath it. Below the cutoff nothing is visited, so "has a listable
  descendant" is a question the deepest rows can no longer answer, and answering it at one depth
  but not another would be the worse rule; a folder whose contents are all unlistable (only a
  `notes.txt`, or nothing at all) now gets a row at every depth, where it used to be invisible.
  The `-L` cap **cuts the input to the dependency graph**, so a shallow render shows a shallower
  graph of the same tree instead of edges drawn from manifests the caller asked not to see. The
  manifest walk runs exactly ONE level below the deepest row, because a package's manifest lives
  INSIDE the package, one level under the row that names it — so every directory the map
  DISPLAYS still states its own `<- depends on […]` / `used by: […]` facts, while a package
  below the cutoff is no row, is never read, and contributes no edge (a path/workspace
  dependency on one now renders as `(unresolved)`). The extra level reads manifests only and
  can never add a row.
  And **`--strict-check` is not capped**: a gate is not a rendered view, so it still lints every
  file at every depth, `-L` or no `-L`.
- The text map states the one exclusion criterion it applies to `.annotation` files, on stderr
  and only when a sidecar row was actually suppressed. The JSON map states it structurally
  instead, as `"sidecar": true` on the row that took the contract.
- A malformed `.annotation` body that is a **conforming line wrapped in a comment marker** is
  now diagnosed as exactly that — "remove the `<!--` and `-->`" — instead of "the ` | ` field
  separators are missing", which was the one explanation that could not be true (#17). The
  printed `suggestion` is the line from inside the wrapper, so it is usable as printed; it used
  to embed the malformed text and could not be pasted. Same verdict, same parts reported.
- `SPEC.md` gains an **accessory tool** vocabulary entry, stating that a program which
  helps an agent consume Annotations performs no run, emits no Report, and is therefore
  governed by none of the invariants.

### Fixed
- `--max-files` aborted runs over files that would never have been shown (#15). `-L 1
  --max-files 5` on a tree holding 41 files three levels down exited 3 with nothing written,
  and on any large tree the DEFAULT `--max-files 10000` failed a one-level render outright.
  The count is now over what the capped walk visits, so it is bounded by `-L` like everything
  else.

## [0.5.0] - 2026-07-30

### Added
- `[rules] max_annotation_length` and `--max-length <N>`: fail any annotation field
  (`Concern`, `Non-concern`, `IO`) whose MAXIMAL SPAN runs over N characters — on a conforming
  line that span is exactly the trimmed value (see Fixed, below). N itself passes. 200 by
  default (see Changed); `--max-length 0` normalizes to no bound, like `--max-per-node 0`.
- `annotation_too_long` category, carrying `defect.too_long` (each offending part and its
  length) and `defect.max` (the bound).
- `annotation::Outcome::TooLong` variant. `Outcome` is a `pub enum` with no
  `#[non_exhaustive]`, so a downstream exhaustive `match` must handle it.

### Changed
- BREAKING: the annotation length bound ships ON at 200 characters per field —
  `default_config.toml` now sets `[rules] max_annotation_length = 200`, the built-in layer every
  other layer overrides. A repo that passed `--strict-check` on 0.4.0 with longer annotations now
  FAILS with no config change. A bound nobody enables catches nothing: 0.4.0 in real repos let
  agents write 500–1600 character fields. Raise it with `[rules] max_annotation_length = <N>` or
  `--max-length <N>`; `--max-length 0` disables it. The failing TEXT report names that escape once.
- `--strict-check` is form-only: every field present, non-empty, within the length
  bound. Filler `Concern`s, inward `Non-concern`s, `<placeholder>` slots and an empty IO
  operand (`IO: (a) ->`) all pass. The annotation guide still advises against filler.
- An empty field is fatal as `malformed_annotation`, naming the field in `defect.missing`. A
  line whose three keys are present but unextractable (`Concern: a|Non-concern: b|IO: c`, or
  keys out of order) reports all three parts. Both carry a human `detail` clause.
- `annotation::Outcome::Malformed` gains a `detail` field: any struct-variant pattern on
  `Malformed` must be updated.
- `annotation::analyze` / `analyze_charter` / `analyze_file` take the length bound as a
  trailing `Option<usize>` (`None` = no bound).
- `config::CliOverrides` gains a public `max_annotation_length` field: a struct-literal
  construction must supply it, or fall back to `..Default::default()`.
- `Cli` gains a public `max_length` field. `Cli` derives no `Default`, so a struct-literal
  construction must supply it.
- `suggestion` is absent for `annotation_too_long`. The TEXT message never carried one for
  this category either, so TEXT-only consumers see no change.
- The `suggestion` stub passes the form check as printed. Its `<…>` slots are still judgments
  an agent has to write out, and a configured `max_annotation_length` applies to it
  (`<concern owned elsewhere>` alone is 25 characters).
- `--help` output: the embedded annotation guide declares the `Non-concern`'s where-it-lives
  pointer OPTIONAL and carries a BREVITY section.
- `--githook-guide` recipe and the commit-message attestation format: every per-principle line
  carries a severity — `none`, `N/A — reason`, or `MAJOR`/`MODERATE`/`MINOR` plus the finding —
  in place of a numeric score. `MEDIUM:` is renamed `MODERATE:`, no alias. A numeric `MINOR:`
  count is now required, and never gated. The gate cross-checks the lines against the counts: a
  line carrying `MAJOR` under a declared `MAJOR: 0` fails. BREAKING for a shipped recipe: a repo
  that wired the example hook in has every commit message rejected until it adopts the format.
- `--githook-guide` defines the three severity tiers (MAJOR / MODERATE / MINOR).

### Removed
- `--symbols`, the `[display] show_symbols` key, the `FileNode.symbols` JSON field (and its
  `--schema` lines), the `symbols` module, and the `symbols` Cargo feature with its
  `tree-sitter`, `tree-sitter-python`, `tree-sitter-rust`, `tree-sitter-go`,
  `tree-sitter-typescript` and `streaming-iterator` optional dependencies. The tool reports on a
  file as a NODE — its declared contract — never on its body; a parsed declaration list is a
  second, derived map that can disagree with the annotated one.
- BREAKING: the `Symbol` and `SymbolKind` crate-root re-exports are gone, and `Display`,
  `CliOverrides`, `Cli` and `FileNode` lose their public `show_symbols`/`symbols` fields; none is
  `#[non_exhaustive]`, so a struct literal or field read must drop it. A repo config still
  carrying `show_symbols` now fails to parse (`deny_unknown_fields`) — delete the key. `mcp` is
  the only remaining Cargo feature.
- `--tokens`, the `[display] show_tokens` key, the `DirNode.tokens` / `FileNode.tokens` JSON
  fields (and their `--schema` lines), and the `tokens` module. It was a `~4 bytes/token`
  heuristic, and an unreliable estimate in a tool sold on ingest efficiency is worse than none
  — an agent may budget against it.
- BREAKING: `Display`, `CliOverrides`, `Cli`, `DirNode` and `FileNode` lose their public
  `show_tokens`/`tokens` fields; none is `#[non_exhaustive]`, so a struct literal or field read
  must drop it. A repo config still carrying `show_tokens` now fails to parse
  (`deny_unknown_fields`) — delete the key.
- The vacuity gate: the `annotation_vacuous` category, the `defect.vacuous` JSON key, and
  `annotation::Outcome::Vacuous`.
- The `annotation_on_orphan` advisory, the strict report's top-level `warnings` array (and its
  `Found N warning(s)` TEXT block), and `exit::code::ANNOTATION_ON_ORPHAN`. The opt-in
  `[rules] forbid_orphans` / `orphan_package` rule is untouched.

### Fixed
- `.githooks/commit-msg` read the FIRST match for each count, so a body line at column 0 reading
  `MAJOR: 0 blockers remained` shadowed the real trailer and a commit with unresolved blockers
  passed the gate. Every count now takes the LAST match.
- The length bound under-measured a field whose prose quoted the ` | Non-concern:` / ` | IO:`
  separators with their colons, in EITHER direction: the parser splits at the first occurrence, so
  a quote ahead of the real key hid every character after it (a 300-character `Concern` measured
  150 and passed `--max-length 200`), and splitting at the last occurrence instead merely moved the
  shortfall onto a quote that FOLLOWS the real key (a 207-character `IO` measured 100 and passed).
  The three fields partition the line, so choosing an occurrence only redistributes length; the
  bound therefore no longer measures the parsed values at all. It measures each field over its
  MAXIMAL extent — `Concern` up to the LAST ` | Non-concern:`, `Non-concern` from the first of
  those to the LAST ` | IO:`, `IO` from the first of those to the end of the line. On a conforming
  line each separator occurs once, so the spans are exactly the parsed fields and no reported
  length changes; a line that quotes a separator over-measures and fails loudly, and can no longer
  under-measure. Parsing and rendering are byte-for-byte unchanged.
- The `malformed_annotation` suggestion seeded its `Concern:` from the text before the first bare
  ` | `, so a `Concern` whose own prose held a pipe (a shell pipeline, a `|x|` closure, SQL `||`)
  was truncated mid-sentence in the suggested stub. It now cuts at the ` | Non-concern:` separator
  the parser splits on, so the seed is exactly the `Concern` the checker read.

## [0.4.0] - 2026-07-20

### Added
- Map + render library surface: the crate root re-exports the tree model and renderer, so a
  consumer can assemble a `CodebaseMap` from `DirNode` / `FileNode` by hand and render it via
  `for_format(Format, ascii)` + the `Renderer` trait, without the internal `build` pipeline.
  Access-only: no behavior or schema change.
- Node field types re-exported so every field is nameable: `Charter`, `DirDeps`,
  `InternalDep`, `Warning`, `Symbol`, `SymbolKind`. The graph, symbols and strict builder
  machinery stays crate-internal.

## [0.3.0] - 2026-07-20

### Added
- `--include <GLOB>`: positive glob selector, counterpart to `-I/--ignore`. Adds files of any
  type to the tree even when their extension maps to no known language (repeatable,
  pipe-separated; `--include '*'` shows every file). Config-enablable via
  `[display] include = ["*.sh", "Dockerfile"]`.
- An included file's annotation is read marker-agnostically (keyed on the invariant
  `Concern:` opener), so extensionless and unrecognized files surface theirs.
  `--strict-check` is unaffected: it stays recognized-languages-only.
- Library API: the `config`, `walk` and `annotation` modules are public, exposing
  `walk::configured_walk`, `walk::collect_code_files`, `annotation::extract`, the
  marker-agnostic `annotation::extract_any`, and `annotation::analyze`, plus the
  `build_globset` glob-compile helper. Tree model, graph, renderers and strict-check stay
  crate-internal.

## [0.2.1] - 2026-07-13

Docs and packaging. Cut to refresh the README shipped to crates.io and npmjs; the annotation
guide is embedded in `--help`, so its edits changed shipped output too.

### Added
- README_APPENDIX.md: the extended argument (the infinite-context objection, related work,
  what is still unproven) and the full bibliography, cross-linked from the README.

### Changed
- README.md rewritten around adoption: what the tool is, intended usage (annotate, enforce
  via a local git hook, read the map every session), a human TL;DR, the agent rationale, and
  install/wire/enforce/configure steps. Roughly half its former length.
- Annotation guide: the `Non-concern` owner may be an external system or out of the repo's
  scope, not only a named sibling; "true of every file" is called out as a truism.
- Repo carries its own root `.annotation` charter, a `docs/communication-style.md` review
  rubric, and a fixed executable bit on `.githooks/pre-commit` (the strict-check gate was
  being silently skipped).

## [0.2.0] - 2026-07-12

### Added
- Annotation guide on `--help` and on a failing `--strict-check`: the format, a GOOD/FAILS
  contrast, and how to find the `Non-concern`. Opt out with `--no-guide`.
- Single-file `--strict-check`: lint one file, not just a directory.
- Directory charters: a directory carries its own `Concern | Non-concern | IO` line (a
  `.annotation` breadcrumb, else its entry file), promoted onto its tree row.
- `--strict-check` diagnostics name the file's language, the exact comment marker, the real
  line number (past any shebang or blank lines, replacing the hardcoded `:1` that led fixes
  to clobber a shebang), the offending content, and a copy-pasteable conformant example.
  Missing vs non-conforming get distinct wording, a wrong-marker line is echoed as
  `found: '…'`, the `path:LINE:` prefix is preserved, and MCP `strict_check` inherits the
  richer messages.
- `--strict-check --format json`: `{passed, error_count, files_checked, violations,
  rule_violations}`, one record per violation with
  `path`/`line`/`language`/`category`/`marker`/`example`/`found`. The TEXT report is
  unchanged; MCP `strict_check` returns this same object byte-for-byte.
- `ANNOTATION FORMAT` in `--help`, with a verbatim example DERIVED from the language's comment
  marker plus the fixed format — never configured — so help and enforcement cannot disagree. A
  test proves every built-in language's example passes the lint it advertises.
- `--max-per-node <N>` (default 50): show at most N subdirectories and N files per directory,
  folding the overflow into one `[+N folders and F files, use --full to expand]` marker.
  Display-only, so `--max-files` is unaffected and `--tokens` totals still reflect the full
  subtree; expand with `--full` or `--max-per-node 0`. JSON/MCP carry the breakdown as
  `elided_dirs` / `elided_files` (omitted when zero, so no schema bump).

### Changed
- One invariant annotation format: the three-field `Concern | Non-concern | IO` grammar is
  fixed, not configurable. The only per-language knob is the comment marker.
- Stricter vacuity enforcement: a filler `Concern` (`utils`/`helpers`/…) and an inward
  `Non-concern` (`this file's own …`) now fail.

## [0.1.1] - 2026-07-10

### Added
- Shell script support: `.sh` / `.bash` recognized by the annotation engine (shebang skipped,
  annotation read from the first comment below it).

### Fixed
- `npx annotated-tree` / `npm install` failed: the launcher shim `bin/annotated-tree.js`
  opened with its annotation comment instead of a `#!/usr/bin/env node` shebang, so the
  npm-linked executable could not run. Shebang restored; CI asserts it on line 1.
- Strip a leading UTF-8 BOM when reading a file's annotation head, so a BOM-prefixed shebang
  file is no longer mis-read as lacking a first-line shebang.

## [0.1.0] - 2026-07-10

Initial release.

### Added
- Annotated tree view: every source file shows its first-line responsibility annotation,
  extracted by a configurable per-language engine (structured comment tokens plus a regex
  escape hatch).
- Cross-ecosystem dependency graph in the tree: `pyproject.toml`, `package.json`,
  `Cargo.toml` and `go.mod` cross-referenced into internal deps, external deps, and reverse
  "used by" edges; unresolved workspace/path deps flagged.
- `--strict-check` lint mode: nonzero exit on any code file lacking a conforming annotation.
  Enforces architectural dependency `[rules]` (deny edges, forbid cycles, forbid orphans)
  declared in `.annotated-tree.toml`.
- `--format json` (versioned, stable schema) and `--format md`.
- `--symbols`: per-file top-level definition outline via tree-sitter (build with
  `--features symbols`).
- `--mcp`: serve the map, dependency and strict-check tools over stdio as a Model Context
  Protocol server (build with `--features mcp`).
- `--changed` / `--since <ref>`: restrict the view to files changed versus a git ref plus
  their reverse-dependency blast radius.
- `--tokens` rough per-file/package token estimate, `--age` modification times, `--max-files`
  runaway-scope safety valve (exit 2 before any output).
- Flags: `-L/--max-depth`, `--include-tests`, `--no-gitignore`, `--ascii`, `-I/--ignore`,
  `--config`, `--no-limit`, `--ignore-parsing-errors`.
- Layered configuration: built-in defaults < `~/.config/annotated-tree/config.toml` < repo
  `./.annotated-tree.toml` < CLI flags. Regex-configurable extraction and validation
  convention per language.
- Non-fatal stderr warnings for unparseable manifests (silence with
  `--ignore-parsing-errors`); a corrupt manifest never aborts the run.
- Distribution: crates.io, cargo-binstall, npm/npx, and a checksum-verifying `curl | sh`
  installer.
- Golden-file and integration test suite; CI across Linux, macOS and Windows.

[Unreleased]: https://github.com/fredrikolis/annotated-tree/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/fredrikolis/annotated-tree/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/fredrikolis/annotated-tree/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/fredrikolis/annotated-tree/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/fredrikolis/annotated-tree/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/fredrikolis/annotated-tree/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/fredrikolis/annotated-tree/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/fredrikolis/annotated-tree/releases/tag/v0.1.0
