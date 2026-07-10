<!-- Covers: What annotated-tree is, when/why to use it, and how to adopt it in a project (CLAUDE.md, CI, config). Not: Exhaustive flag reference (see `annotated-tree --help`) or internals. Use when: Evaluating or adopting the tool. -->
# annotated-tree

**Understand an unfamiliar codebase in one command — without reading a line of
source.** `annotated-tree` renders a whole-repo *map*: the directory structure,
each file's first-line **responsibility annotation**, and a **cross-ecosystem
dependency graph** (Python · npm · Cargo · Go) in a single view. Deterministic,
zero LLM calls, nothing to install — it reads only manifest *declarations*, so it
works on a bare checkout.

```
├── frontend/
│   ├── ui/  # used by: [@acme/web]
│   │   └── src/
│   │       └── index.ts  # Barrel: Re-exports every shared UI component.
│   └── web/  # <- depends on [@acme/ui]
│       └── src/
│           └── index.ts  # Entry: Mounts the React root. NOT concerned with routing. | I/O: (HTMLElement) -> void
├── packages/
│   ├── api/  # <- depends on [acme-core]
│   │   └── acme_api/
│   │       └── main.py  # App factory: Builds the ASGI app. NOT concerned with business logic. | I/O: (Settings) -> ASGIApp
│   └── core/  # used by: [acme-api, acme-worker]
│       └── acme_core/
│           └── engine.py  # Engine: Runs the core computation loop. NOT concerned with transport. | I/O: (Job) -> Result
└── services/
    ├── gateway/  # <- depends on [shared]
    └── shared/  # used by: [gateway]
```

Between file names, folder structure, and one-line annotations, the app's purpose
and organization are clear *without opening a single file*. The same map that
orients a human orients an agent.

## When to reach for it

- **Orienting in an unfamiliar repo** — onboarding, code review, or an AI agent
  starting a task. Get the whole layout, what each file is *for*, and how packages
  depend on each other, before opening files. (For agents this is the neutral
  "map the territory first" pass that prevents tunnel-vision on the wrong files.)
- **"Where does X live? What handles Y?"** — file names plus responsibility
  annotations answer it without grep-and-read.
- **"What depends on this? What breaks if I change it?"** — the dependency graph
  shows internal/external deps and reverse *used-by* edges across ecosystems.
- **PR / change review** — `--changed` shows exactly what a branch touched **plus
  its reverse-dependency blast radius** (everything downstream that could break).
- **Giving an agent or LLM repo context** — a compact, high-signal map instead of
  a dump of file contents, via `--format json` or the built-in **MCP server**.
- **Keeping a codebase self-documenting** — `--strict-check` enforces the
  annotation convention (and optional architectural dependency rules) in CI.

## Why it's different

The "feed a codebase to an LLM" tools (repomix, code2prompt, gitingest,
files-to-prompt) *serialize file contents*. `annotated-tree` produces a lightweight
*annotated map* instead — orders of magnitude fewer tokens, and it adds two things
nothing else does:

1. **A cross-ecosystem dependency graph in the tree view.** `pyproject.toml`,
   `package.json`, `Cargo.toml`, and `go.mod` are cross-referenced into
   internal/external/used-by edges — one graph spanning a polyglot monorepo.
2. **A human-authored annotation convention it both *displays* and *enforces*.**
   The map is only as good as the annotations, so the tool lints them (`--strict-check`,
   nonzero exit for CI) — self-documentation that can't silently rot.

Deterministic, no network, no model calls, no install of the target project's deps.

## Install

Build from source today (needs a Rust toolchain):

```sh
cargo build --release        # binary at target/release/annotated-tree
```

These channels are wired in CI and go live with the **first tagged release**:

```sh
cargo install annotated-tree          # crates.io
cargo binstall annotated-tree         # prebuilt binary, no compile
brew install fredrikolis/tap/annotated-tree # Homebrew
npx annotated-tree                    # via npm, no install
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/fredrikolis/annotated-tree/releases/latest/download/annotated-tree-installer.sh | sh
```

## What it does

`annotated-tree [PATHS]…` prints the annotated tree. Highlights (`--help` for the
full, exact flag reference):

| Capability | Flag |
|---|---|
| Annotated tree + dependency graph | *(default)* |
| Structured output for tooling/agents | `--format json` (versioned schema) · `md` |
| Only what changed + blast radius | `--changed` · `--since <ref>` |
| Top-level definitions per file (tree-sitter) | `--symbols` *(build with `--features symbols`)* |
| Serve to agents/editors as MCP tools | `--mcp` *(build with `--features mcp`)* |
| CI lint: annotations + architectural rules | `--strict-check` |
| Rough per-file/dir token estimate | `--tokens` |
| Runaway-scope guard | `--max-files <N>` |

## Use it in your project

### 1. Make agents reach for it first

Paste this near the top of your **`CLAUDE.md`** / **`AGENTS.md`** so fresh sessions
orient with one command instead of blindly reading files:

```markdown
## Orientation — run this first

Before reading source to answer "how is this repo structured / where does X live /
what depends on Y", run `annotated-tree` to get the whole-repo map (structure +
each file's one-line responsibility + package dependency graph) in a single pass:

- `annotated-tree`                — annotated map of the repo
- `annotated-tree <subdir>`       — scope to a subtree
- `annotated-tree --changed`      — only what changed vs HEAD, plus its reverse-dep blast radius (use for review / impact analysis)
- `annotated-tree --format json`  — the same map as structured data

Every source file's first line is a `# Role: what it does. NOT concerned with Y. | I/O: (in) -> out`
annotation — keep it accurate when you add or change a file.
```

### 2. Enforce the convention in CI

Every source file's first non-shebang line describes its responsibility:

```
# [Role]: [what it does]. Responsible for [X]. NOT concerned with [Y]. | I/O: (inputs) -> outputs
```

`--strict-check` fails (nonzero exit) on any code file missing a conforming
annotation, so the map never rots. The full convention is in
[docs/convention.md](docs/convention.md).

```yaml
# .github/workflows/annotations.yml
name: annotations
on: [push, pull_request]
jobs:
  annotated-tree:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo binstall -y annotated-tree
      - run: annotated-tree --strict-check .
```

Add architectural **dependency rules** to the same lint via a repo
`.annotated-tree.toml`:

```toml
[rules]
deny = [["web", "core"]]  # forbid `web` depending on `core`
forbid_cycles = true      # fail on any dependency cycle
forbid_orphans = true     # fail on internal packages with no edge in or out
```

### 3. Configure languages and the convention

Configuration layers low→high: built-in defaults < `~/.config/annotated-tree/config.toml`
< repo `./.annotated-tree.toml` < CLI flags. The repo file owns the *convention*, so
CI enforcement is a property of the repo, not each contributor's machine. Add a
language or override the convention with no code changes:

```toml
[convention]
require = '\|\s*I/O:'          # regex a code annotation must match
hint = "| I/O:"

[languages.ruby]
extensions = [".rb"]
comment = "#"                  # structured tokens cover most languages

[languages.lua]
extensions = [".lua"]
pattern = '(?m)^--\[\[\s*(?P<annotation>.*?)\s*\]\]'   # regex escape hatch for the rest
```

## License

MIT — see [LICENSE](LICENSE).
