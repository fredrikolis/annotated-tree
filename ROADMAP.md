<!-- Covers: Implementation plan for roadmap items #1-#9, scored against the language-agnostic standards. Not: Feature justification (see research) or release mechanics (see RELEASING.md). Use when: Building v0.1-v0.3 features. -->
# Implementation roadmap

Concrete, standards-scored build plan for the 9 accepted features. Features are
**given**; this document is about *how*. Every item names real modules and
signatures in the current tree:

```
src/{cli,config,annotation,manifest,graph,tree,strict,walk,util,lib,main}.rs
run(cli, out, err) -> Result<i32>          // lib.rs, the one entrypoint
trait ManifestParser { filename; ecosystem; parse -> Result<ParsedManifest> }
Config { display, languages: Vec<Language>, ext_to_lang }   // layered, regex-driven
graph::build(&[PathBuf]) -> (HashMap<PathBuf, DirDeps>, Vec<String>)
tests/{golden.rs, warnings.rs} + tests/golden/*.txt          // the behavioural spec
```

---

## Item 0 (FOUNDATIONAL): Canonical model + renderer split

**Not on the numbered list, but a hard prerequisite for #1, #3, #4, #7, #9.** Do it
first. Everything else is cheap once it lands and expensive without it.

### Problem
`tree.rs::render` builds an in-memory `Node` tree *and* formats text *and* re-reads
annotations/mtime inline (`file_annotation`, `dir_annotation`, `age_suffix`). The
structure exists only for the duration of text formatting. Adding JSON/MD (#1) or an
MCP surface (#7) against this forces either duplicated traversal (DRY violation) or
a second parallel representation (Canonical-Representation violation, score −10).

### Approach
Introduce **one canonical in-memory model**, built once, rendered many ways.

- **New `src/model.rs`** — the single source of truth:
  ```rust
  pub struct CodebaseMap { pub roots: Vec<DirNode> }
  pub struct DirNode {
      pub name: String,
      pub deps: Option<DirDeps>,        // from graph, keyed while building
      pub dirs: Vec<DirNode>,           // sorted: dirs then files (current order)
      pub files: Vec<FileNode>,
  }
  pub struct FileNode {
      pub name: String,
      pub annotation: Option<String>,   // extracted ONCE here
      pub age_secs: Option<i64>,        // computed once when show_age
      pub symbols: Vec<Symbol>,         // empty until #4
      pub tokens: Option<u32>,          // None until #3
  }
  pub fn build(root, files, graph, config, max_depth) -> DirNode
  ```
  `model::build` absorbs the traversal currently inside `tree.rs::insert`/
  `render_node`, plus the annotation/age lookups. It performs **all filesystem
  reads**; renderers are pure `&CodebaseMap -> String/bytes` (Fail-Fast at the read
  boundary, then trusted internally — DbC).

- **New `src/render/` module** (split at a natural seam — File-Size discipline):
  ```rust
  // render/mod.rs
  pub trait Renderer { fn render(&self, map: &CodebaseMap) -> String; }
  // render/text.rs  — glyphs + "# annotation" (MOVED verbatim from tree.rs)
  // render/json.rs  — #1
  // render/md.rs    — #1
  ```
  Open/Closed: new formats = new `impl Renderer`, zero core edits. `lib.rs::run`
  selects a renderer by `cli.format` and writes its output to `out`.

- **`tree.rs` shrinks to `render/text.rs`**; `graph.rs`, `walk.rs`, `annotation.rs`
  unchanged. `strict.rs` stays independent (it needs per-file pass/fail, not the
  map) — do **not** force it through the model (that would be a wrong abstraction).

### Standards scoring
- **Canonical-Representation +10**: one interior form (`CodebaseMap`), converted to
  text/json/md only at the edge.
- **DRY +9 / SoC +9**: traversal lives once; render is a separate concern with one
  reason to change per format.
- **Open/Closed +9**: `Renderer` trait is the extension axis for #1.
- **Remove-then-Replace +10**: this is a *behaviour-preserving refactor*. The
  **existing `tests/golden/*.txt` are the contract** — text output must stay
  byte-identical. No golden file changes in Item 0; if one changes, the refactor is
  wrong. This is the boundary test that lets us gut `tree.rs` safely.
- **File-Size**: pre-empts the monolith `tree.rs` would become once 3 formats +
  symbols + tokens pile in.

### Tests
Zero new golden files. Existing `golden.rs` (default/depth1/ascii/no_gitignore/
strict) must pass unchanged. Add unit tests on `model::build` asserting node counts
/ordering for `sample/`. **Risk**: mtime (`age_secs`) is non-deterministic — keep it
out of golden text (already off by default) and out of JSON golden (see #1).

**Effort: M.** **Risk: low** (pure refactor, golden-gated).

---

## Item 1: `--format json` (+ `md`)  — v0.1

### Approach
- `cli.rs`: add `#[arg(long, value_enum, default_value_t = Format::Text)] format: Format`
  with `enum Format { Text, Json, Md }` (clap `ValueEnum`). Map nothing into
  `CliOverrides` — format is a render-time concern, not config state (Minimal API).
- `render/json.rs`: `serde::Serialize` on the `model` types; `serde_json::to_string_pretty(map)`.
  Derive `Serialize` on `CodebaseMap`/`DirNode`/`FileNode`/`DirDeps`. A stable schema
  is the public contract — version it with a top-level `{"schema": 1, "roots": …}`.
- `render/md.rs`: headings per package + bullet lists; reuses model, no new reads.
- `lib.rs::run`: `let renderer = render::for_format(cli.format); writeln!(out, "{}", renderer.render(&map))`.

### New deps
`serde_json` — **already in `Cargo.toml`** (used by the npm manifest parser). `serde`
derive already present. Zero new deps.

### Standards scoring
- **Canonical-Representation +10 / DRY +10**: JSON/MD are just other renderers over
  the Item 0 model; no traversal duplication.
- **Open/Closed +9**: adding `md` after `json` touched only `render/`.
- **Minimal API Surface**: JSON schema is the *only* new public contract; keep it
  small and explicitly versioned (DbC with external consumers — this is the one place
  docstrings/schema docs are warranted, per the Documentation exception).
- **DbC**: JSON is consumed by *other programs* (#7 MCP, editors) → the schema is a
  boundary contract; validate/version at the edge.

### Tests
Golden `tests/golden/default.json` compared with `serde_json` value-equality (parse
both, assert equal `Value`) — **not** string compare, so field ordering/whitespace
isn't the spec (WHAT-not-HOW). Exclude `age_secs`/`tokens` (null by default) to stay
deterministic. Round-trip test: `serde_json::from_str::<CodebaseMap>` of our own
output.

**Effort: S.** **Risk: low.** Prereq: Item 0.

---

## Item 3: Per-file/dir token-count display — v0.1

(Sequenced before #2 because it is pure-core and rounds out the v0.1 data model.)

### Approach
- **New `src/tokens.rs`**: `pub fn estimate(text: &str) -> u32`. Ship a
  **deterministic heuristic** (e.g. `ceil(bytes / 4)` or a cheap word/punct split) —
  NOT a real BPE tokenizer. YAGNI: exact GPT/Claude BPE needs a big vocab dep
  (`tiktoken-rs`) for a *display* number; the heuristic is within the tool's "map,
  not exact" contract. Document the approximation in `--help`.
- `model::build` fills `FileNode.tokens` when `config.display.show_tokens`; dir totals
  computed by summation in the model (canonical — computed once, not per renderer).
- `cli.rs`: `--tokens` flag → `CliOverrides.show_tokens` → `Display.show_tokens`
  (mirrors the existing `show_age` plumbing exactly — DRY with the established
  pattern).
- `render/text.rs`: append `  [~120 tok]`; `render/json.rs`: the `tokens` field is
  already there.

### New deps
None. (Explicitly reject `tiktoken-rs` — Evidence-Based: a display estimate doesn't
justify a multi-MB vocab + load time. Revisit only if users need exact budgeting.)

### Standards scoring
- **YAGNI +high**: heuristic over exact tokenizer until proven necessary.
- **DRY +9**: reuses the `show_age` config→display→model→render path verbatim.
- **Canonical-Representation +9**: totals summed once in the model; renderers read.
- **Fail-Fast**: unreadable file → `tokens: None`, consistent with `annotation: None`.

### Tests
Unit: `estimate("")==0`, monotonic, deterministic. Golden `--tokens --ascii` over
`sample/` (deterministic because heuristic is pure over committed fixture bytes).

**Effort: S.** **Risk: low** (heuristic accuracy is a documented non-goal).

---

## Item 2: npm / `npx annotated-tree` channel — v0.1

Packaging, not Rust code. Detailed publish flow already in `RELEASING.md`; this is the
architecture.

### Approach
- **`npm/` workspace** (new dir): a thin main package `annotated-tree` with
  `optionalDependencies` on per-platform packages `annotated-tree-{darwin-arm64,
  linux-x64-musl,win32-x64,…}`, each carrying one prebuilt binary + `os`/`cpu` fields
  so npm installs exactly one. A JS shim (`bin/annotated-tree.js`) `execFileSync`s the
  resolved binary, forwarding argv/stdio/exit code.
- **No `postinstall` downloader** (works under `--ignore-scripts`; no install-time
  network — DbC at the package boundary).
- CI: extend `.github/workflows/release.yml` `upload-assets` to also `npm publish`
  each platform package + the shim, reusing the same binaries built for the GitHub
  release (Single-source-of-truth: one binary artifact, many channels).

### New deps
Node tooling only (build-time). Optionally `abemedia/cargo-npm` to generate the
platform packages instead of hand-maintaining them — evaluate vs a ~40-line script.

### Standards scoring
- **DRY +9**: the release binary is built once; npm, Homebrew, binstall all consume
  the same artifact. No parallel build path.
- **Minimal API Surface**: the shim forwards argv untouched — it adds no behaviour,
  so there's nothing to drift from the binary's contract.
- **Fail-Fast**: shim exits with the binary's own exit code; missing-platform-binary
  is an explicit error at launch, not a silent no-op.

### Tests
`npm pack` + `npx ./pkg.tgz --version` in CI on all three OS runners (already added to
the CI matrix). Assert the shim's exit code mirrors the binary.

**Effort: M.** **Risk: medium** (platform-package matrix correctness; validated per
`RELEASING.md` local checklist before publish).

---

## Item 5: Architectural dep-rules in `--strict-check` — v0.2

(Sequenced before #4 in v0.2: reuses two existing pillars, no new deps, best
fit-to-effort.)

### Approach
- **New `src/rules.rs`**: consumes the *existing* `graph::build` output.
  ```rust
  pub struct Rules { forbidden: Vec<(String, String)>, forbid_cycles: bool, forbid_orphans: bool }
  pub struct Violation { pub kind: RuleKind, pub message: String }  // Canonical, renderer-agnostic
  pub fn evaluate(graph: &Graph, rules: &Rules) -> Vec<Violation>
  ```
  Cycle detection = DFS/Tarjan over the internal-dep edges already computed in
  `graph.rs` (reverse edges exist too). Note: `graph::build` currently returns
  `HashMap<dir, DirDeps>`; expose the package-level edge list it builds internally
  (small Minimal-API addition) so `rules` doesn't re-derive it (DRY).
- **`config.rs`**: extend the layered TOML with a `[rules]` table (new
  `RawRules`, merged by the existing `merge()` precedence — Open/Closed on the config
  schema, no new loader). Regex-free; declarative `deny = [["go", "python-worker"]]`,
  `forbid_cycles = true`.
- **`strict.rs`**: `check()` gains a rules pass. Today it returns `(String, i32)`;
  keep that contract, append rule violations to the same report/exit-code path
  (Liskov: strict-check's observable contract — `path:line: message` + nonzero exit —
  is unchanged, just more findings).

### New deps
None. Graph + config layering + strict harness already exist.

### Standards scoring
- **Open/Closed +9**: extends `strict` + `config` + `graph`; core untouched.
- **DRY +9**: reuses the computed graph; cross-ecosystem edges are a *free*
  differentiator (dependency-cruiser/madge are JS-only).
- **SoC +9**: `rules.rs` = policy evaluation, separate from `graph.rs` (edge
  computation) and `strict.rs` (reporting).
- **Fail-Fast**: an unknown package name in a rule is a config error surfaced at load
  (DbC on user config), not a silently-ignored rule.

### Tests
Fixture: extend `sample/.annotated-tree.toml` (or a rules-specific fixture dir) with a
deliberately-violating rule; golden `strict_check_rules.txt` pins the violation output
+ exit 1. Unit: cycle detection on a hand-built 3-node cyclic graph (WHAT: "reports
the cycle", not traversal order).

**Effort: M.** **Risk: medium** (cycle-detection correctness — covered by unit tests
on known graphs).

---

## Item 4: Symbol/definition outline (tree-sitter) — v0.2

The flagship. Largest surface; isolate it hard.

### Approach
- **New `src/symbols/` module**, one seam per grammar (File-Size + SoC):
  ```rust
  // symbols/mod.rs
  pub struct Symbol { pub kind: SymbolKind, pub name: String, pub signature: String, pub line: u32 }
  pub trait SymbolExtractor { fn language(&self) -> &str; fn extract(&self, src: &str) -> Vec<Symbol>; }
  pub fn for_language(name: &str) -> Option<Box<dyn SymbolExtractor>>;
  // symbols/{python,rust,go,typescript}.rs — one tree-sitter query each
  ```
  Extractors run tree-sitter queries (`.scm`) for top-level defs. Open/Closed: a new
  language = a new extractor + query, registered by `config.Language.name` — **reuses
  the existing config-driven language registry** rather than a second language list
  (DRY with `config.rs`).
- `model::build`: when `config.display.show_symbols`, look up an extractor by the
  file's resolved `Language.name` and fill `FileNode.symbols`. Reads the file body
  (already opened for annotation — read once, extract both: annotation from head,
  symbols from full source; one read boundary).
- `cli.rs`: `--symbols` → `Display.show_symbols` (same plumbing as age/tokens).
- Renderers: `text.rs` indents symbols under the file; `json.rs` gets them free.

### New deps
`tree-sitter` + `tree-sitter-{python,rust,go,typescript}` grammar crates. **Justified
(Evidence-Based, not cargo-cult)**: hand-writing multi-language parsers is the wrong
build; tree-sitter is the boring, incremental, battle-tested default (the entire
competitor wave uses it). Scope grammars to the **4 ecosystems we already parse
manifests for** — no speculative languages (YAGNI). Vendored `.scm` queries, no
network.

### Standards scoring
- **Open/Closed +9**: `SymbolExtractor` per grammar; core/model/renderers unchanged
  when adding a language.
- **SoC +9 / File-Size +9**: `symbols/*.rs` isolates the one heavy subsystem; splits
  are per-language natural seams.
- **DbC / Fail-Fast**: a parse failure or unsupported language yields `symbols: []`
  (graceful, like `annotation: None`) — a malformed source file never aborts the map.
- **Composition-over-Inheritance**: extractors composed behind a trait, not a base
  class.
- **Tension**: default output could get noisy → `--symbols` is **opt-in** (KISS for
  the default view); scores well by not bloating the common path.

### Tests
Per-language unit tests: feed a known snippet, assert extracted `Symbol` set (WHAT).
Add a `sample/` file per language already exists (`.py/.rs/.go/.ts`) → golden
`--symbols` output. Extraction is deterministic over committed fixtures.

**Effort: L.** **Risk: medium-high** (grammar version pinning, query maintenance,
binary-size growth — mitigate with a `symbols` cargo feature so the base binary stays
lean).

---

## Item 6: `curl | sh` installer — v0.2

### Approach
- **`installer/install.sh`** (POSIX): detect OS/arch (`uname -sm`), map to a release
  asset name, download the tarball + `.sha256`, **verify checksum (Fail-Fast — abort
  on mismatch)**, extract to `~/.local/bin` (or `$INSTALL_DIR`). A base-URL env var
  override enables the local validation already documented in `RELEASING.md`.
- CI publishes `install.sh` as a stable release asset (`…/releases/latest/download/
  annotated-tree-installer.sh`) — the URL already referenced in `README.md`.

### New deps
None (shell + coreutils).

### Standards scoring
- **DRY +9**: consumes the same release assets as binstall/npm/brew (one artifact
  source).
- **Fail-Fast +10**: checksum verification before install; explicit error on
  unsupported platform (no silent partial install).
- **KISS**: a single POSIX script, no installer framework.

### Tests
Shellcheck in CI; a local test that runs the installer against a `python3 -m
http.server` artifact dir with `INSTALL_DIR=$(mktemp -d)` and asserts the installed
binary runs (per `RELEASING.md` §local validation).

**Effort: S/M.** **Risk: low-medium** (platform detection matrix; covered by the
local harness).

---

## Item 7: MCP server mode (`--mcp`) — v0.3

The one genuinely-async surface. Everything above stays sync; async is contained here.

### Approach
- **New `src/mcp.rs`** — the *only* async module. `#[tokio::main(flavor)]`-style entry
  invoked from `lib.rs::run` when `cli.mcp`:
  ```rust
  pub fn serve(cli: &Cli) -> Result<i32>   // sync outer signature; owns the runtime
  ```
  Inside, a `tokio` runtime + `rmcp` (official Rust MCP SDK) server over **stdio**
  (the correct 2026 local-desktop transport). Tools/resources map 1:1 onto existing
  sync builders:
  - `map(path, max_depth)` → `model::build` → `render/json`
  - `dependents(pkg)` / `dependencies(pkg)` → `graph`
  - `strict_check(path)` → `strict::check`
- **Async boundary ("contain the ugly")**: model/graph/strict are **blocking**
  (filesystem, thread-pool walk). They MUST NOT run on the async request path.
  Wrap each call in `tokio::task::spawn_blocking(move || model::build(...))`. The
  blocking core is isolated behind the async tool handlers; **callers (the MCP client)
  see only async**, exactly as the Async-I/O-Contract standard prescribes.

### New deps
`tokio` (feature-gated), `rmcp`. **Gated behind a `mcp` cargo feature** so the default
CLI binary carries no async runtime (Minimal footprint; the sync-core justification in
`lib.rs`'s header comment stays true for the default build).

### Standards scoring
- **Async-I/O-Contract +10**: `--mcp` is real concurrent network/IPC I/O → async at
  the edge; blocking work wrapped in `spawn_blocking`, never leaking (the doc's
  "Contain the Ugly" exemplar). The CLI core stays sync **with justification** (batch
  disk walk, no concurrent waits) — both scored correctly, no contradiction.
- **DRY +10 / Minimal API**: MCP tools are thin adapters over `model`/`graph`/`strict`
  — no reimplemented logic; the JSON schema from #1 is the wire contract.
- **SoC +9**: all async isolated in `mcp.rs`; deleting the feature removes async
  entirely.
- **Depends on #1** (JSON schema is the serialized tool payload).

### Tests
Integration: drive the server with a scripted MCP stdio session (JSON-RPC request →
assert tool result). Unit-test tool handlers by calling the underlying sync builders
directly (the async wrapper is thin, tested via one round-trip). WHAT: "map tool
returns the sample map", not runtime internals.

**Effort: M.** **Risk: medium** (`rmcp` API churn — pin version; transport choice
confirmed by the running MCP-distribution research).

---

## Item 8: `--changed` / `--since <ref>` — v0.3

### Approach
- **New `src/changed.rs`**: `pub fn changed_files(root, since: &str) -> Result<HashSet<PathBuf>>`.
  Prefer **shelling to `git diff --name-only <ref>`** over the `git2` crate
  (Evidence-Based/KISS: one subprocess vs a large libgit2 binding for a name list;
  git is already assumed present for this feature). Contain it behind the function so
  swapping to `git2` later is a one-module change (Minimal API).
- `cli.rs`: `--since <REF>` (and `--changed` = `--since HEAD`). Passed as a **filter
  predicate** into `model::build` (or a post-filter on the walked file set in `lib.rs`,
  reusing `walk::collect_code_files` then intersecting).
- **Blast radius**: for each changed file, resolve its owning package via `graph`, then
  include that package's **reverse-dep closure** (`DirDeps.used_by`, transitively) —
  this monetizes the unique `used_by` edges. New `graph` helper
  `reverse_closure(pkg) -> Vec<String>`.

### New deps
None (shell `git`). `git2` explicitly deferred until a measured need.

### Standards scoring
- **KISS/Evidence-Based +9**: subprocess over heavy binding for a name-only query.
- **DRY +9**: reuses `walk`, `graph.used_by`, and the model; `--changed` is a filter,
  not a parallel pipeline.
- **Fail-Fast**: not a git repo / bad ref → explicit error from the git boundary, not
  an empty silent result.
- **SoC**: git interaction quarantined in `changed.rs`.

### Tests
Fixture git repo built in a tempdir (like `warnings.rs` builds temp trees): commit,
modify a file, assert `changed_files` + reverse-closure membership. WHAT: "editing
core surfaces api+worker as blast radius."

**Effort: S/M.** **Risk: low-medium** (git invocation portability — already testing on
the 3-OS CI matrix).

---

## Item 9: Mark unresolved internal deps — v0.3 (quick win)

### Approach
- `graph.rs`: today a `Dep` is internal if `dep.local || known.contains(canon)`. A
  `local`-declared dep (npm `workspace:*`, Cargo `path=`) that is **not** in
  `known` names is *declared internal but unresolved*. Split `DirDeps.internal` into
  resolved vs unresolved, or tag entries:
  ```rust
  pub struct DirDeps { pub used_by: Vec<String>, pub internal: Vec<Dep2>, pub external: Vec<String> }
  pub struct Dep2 { pub name: String, pub resolved: bool }
  ```
- `DirDeps::annotation()`: render unresolved as `@acme/ghost (unresolved)`.
- Model/JSON (#1) expose `resolved: bool` per internal dep.

### New deps
None.

### Standards scoring
- **Fail-Fast / Explicit-over-silent +9**: a dangling workspace dep becomes *visible*
  instead of masquerading as a normal edge (today's silent fallback → explicit
  signal, the exact anti-pattern the doc calls out).
- **Canonical-Representation**: resolution status is a model field, rendered at the
  edge — not recomputed per renderer.
- **Minimal change**: contained to `graph.rs` + `DirDeps::annotation` + model.

### Tests
Reuse the existing dangling-dep scenario (already exercised manually): golden the
`(unresolved)` marker. Unit on `graph::build` with a workspace dep absent from the
tree.

**Effort: S.** **Risk: low.**

---

## Sequencing (dependency DAG)

```
              ┌────────────────── Item 0: canonical model + Renderer trait ──────────────────┐
              │                    (golden-gated behaviour-preserving refactor)              │
              ▼                              ▼                         ▼                      ▼
   #1 --format json/md            #3 token display          #9 unresolved deps      (enables #4, #7)
        │  │                            │                         │
        │  └───────────────┐           │                         │
        ▼                  ▼           ▼                         ▼
   #7 MCP (needs #1)   #4 symbols (model.symbols)          (graph field)

   Independent of the model:
   #5 arch-rules  → reuses graph + strict + config   (no model dependency)
   #2 npm channel → packaging, parallel to all code
   #6 curl installer → packaging, parallel
   #8 --changed → reuses walk + graph.used_by (light model touch)
```

**Recommended order:**
1. **Item 0** (refactor; unblocks everything; golden-gated, low risk).
2. **#1 JSON/MD**, **#3 tokens**, **#9 unresolved** — small, all ride the new model;
   land together as the v0.1 data-model release.
3. **#2 npm**, **#6 installer** — packaging, parallelizable with steps 1–2 (different
   surface, no code coupling).
4. **#5 arch-rules** — independent, high fit-to-effort; good first v0.2 win.
5. **#4 symbols** — the big one; behind a `symbols` cargo feature.
6. **#7 MCP** (needs #1) and **#8 --changed** — v0.3 fast-follows.

## Cross-cutting architecture

- **One model, many renderers.** `model.rs` is the canonical interior; `render/*`,
  `mcp.rs`, and JSON consumers are all edges. No feature adds a second traversal.
- **Config is the extension registry.** Languages (#4 symbols), conventions, and rules
  (#5) all layer through the existing `Config`/`merge()` precedence — Open/Closed on
  data, not code.
- **Sync core, async only at the MCP edge.** The `lib.rs` header justification for a
  sync core remains true; `tokio`/`rmcp` are `mcp`-feature-gated and wrap blocking
  builders in `spawn_blocking`. No async leaks into the CLI path.
- **Cargo features keep the base binary lean.** `symbols` (tree-sitter) and `mcp`
  (tokio/rmcp) are opt-in; default `cargo install annotated-tree` stays small and
  dependency-light (Minimal API Surface at the build level).
- **Golden tests are the spec across every change.** Item 0 proves equivalence;
  each feature adds golden files that define WHAT, letting internals change freely
  (Remove-then-Replace discipline maintained).
- **One artifact, many channels.** The single release binary feeds crates.io,
  binstall, npm, Homebrew, the curl installer, and the MCP/npm wrapper — no parallel
  build paths (DRY at the distribution layer).

---

## Item 10: Runaway-scope safety valve — v0.1 hardening

Added after research (repomix/gitingest precedent). We materialize a full in-memory
model, so an accidental `annotated-tree /` or a giant `vendor/` is a real footgun —
unlike streaming tools. Guard it; make it trivially overridable.

### Approach
- **`src/walk.rs`**: a mid-walk counter over the ALREADY-filtered entries (post
  gitignore + `node_modules`/`__pycache__`/`.git`/tests skips). Trip the instant
  file-count > `max_files` OR bytes-read > `max_bytes`, **before** model build /
  graph / render — so no stdout byte is written on trip (this is what makes
  `--format json`/`--mcp` safe for free). Return a typed `LimitExceeded` error, not
  a `process::exit` (Fail-Fast at the boundary; caller decides how to surface).
- **`src/cli.rs`**: `--max-files <N>`, `--max-bytes <SIZE>`, `--no-limit` (alias
  `--force`). **Not** render state — these are walk-scope limits.
- **`src/config.rs` + `default_config.toml`**: `[limits] max_files`, `max_bytes`;
  env `ANNOTATED_TREE_MAX_FILES`. Precedence via the existing layered `merge()`
  (Open/Closed on config).
- **`src/lib.rs::run`**: on `LimitExceeded`, write the diagnostic to `err` (NEVER to
  `out` — no JSON error object on stdout), return exit code 2. Defaults: `max_files
  = 10000`, `max_bytes = 2GiB` (tune against the sample corpus).
- **`--mcp` (#7)**: catch `LimitExceeded` and return a structured MCP tool error
  ("raise max_files"), not a process exit — the server stays alive.

### New deps
None.

### Standards scoring
- **Fail-Fast +10**: explicit abort + nonzero exit + actionable message over silently
  churning through `/`; the anti-pattern (silent truncation) is explicitly avoided.
- **DbC / Minimal API**: `LimitExceeded` is a typed boundary error; `run` decides
  surfacing per output mode. Never emits partial/corrupt output.
- **SoC**: counting lives in `walk`, thresholds in `config`, surfacing in `lib`/`mcp`.
- **Canonical / DRY**: one counter in the single walk; every format benefits.

### Tests
Integration (tempdir like `warnings.rs`): a tree exceeding a low `--max-files 2`
aborts with exit 2, **empty stdout**, stderr names the limit + override; `--no-limit`
completes. Freezes the external abort contract (exit code + empty-stdout guarantee)
that JSON/agent consumers depend on (+8). Unit: the counter trips at the boundary.

### Effort: S/M. Risk: low. Sequenced after #1, before #7.
