<!-- Concern: what annotated-tree is, when and why to use it, and how to adopt it in a project | Non-concern: the exhaustive flag reference (--help owns it) or the extended argument | IO: none -->
# annotated-tree [![CI](https://github.com/fredrikolis/annotated-tree/actions/workflows/ci.yml/badge.svg)](https://github.com/fredrikolis/annotated-tree/actions/workflows/ci.yml) [![crates.io](https://img.shields.io/crates/v/annotated-tree.svg)](https://crates.io/crates/annotated-tree) [![npm](https://img.shields.io/npm/v/annotated-tree.svg)](https://www.npmjs.com/package/annotated-tree) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

`annotated-tree` extends Unix `tree`. Alongside the directory structure it renders each
file's one-line **responsibility annotation**, giving an agent a fast map of a workspace,
what every file is for, without opening the files. The annotation is a strict, checkable
format, so the map cannot silently go missing or lose its shape, and an agent trusts it
instead of re-deriving the structure each session. It can also annotate the results of your
agent's exploration tool calls. For code, it cross-references package manifests into a
cross-ecosystem dependency graph.

```
$ annotated-tree
├── web/                # Concern: the HTTP API | Non-concern: business rules | IO: (Request) -> Response · <- depends on [core]
│   ├── pyproject.toml  # Concern: names the web package and its dependency on core | Non-concern: the routes themselves | IO: none
│   └── routes.py       # Concern: map URLs to Core calls | Non-concern: what the calls do | IO: (Request) -> Response
└── core/               # Concern: the business rules | Non-concern: transport or storage | IO: (Command) -> Result · used by: [web]
    ├── pyproject.toml  # Concern: names the core package | Non-concern: the rules it holds | IO: none
    ├── rules.py        # Concern: pricing and discount logic | Non-concern: where orders come from | IO: (Order) -> Priced
    └── store.py        # Concern: read/write orders | Non-concern: the rules that shape them | IO: (Order) <-> Postgres
```

**Install** via [curl one-liner](#install), [npx](https://www.npmjs.com/package/annotated-tree), or [cargo](https://crates.io/crates/annotated-tree).

## Intended usage

1. **Annotate every file.**  
Have your agent write a one-line annotation at the top of each file (point it at `annotated-tree --annotation-guide` for editorial guidance). `Non-concern` is often the most valuable field as it documents the architectural/structural boundary the content itself does not speak to (see [the format](#the-format)).

2. **Prevent stale annotations using a git hook.**  
Well-written commit-time githooks will catch: a) **structural** issues mechanically using `annotated-tree --strict-check` and, b) **editorial** issues using a neutral agent reviewer (see ours here [`.githooks/`](.githooks/)). Blocking at commit catches the rot while the agent still has the context to fix it.

3. **Put them in front of the agent during daily work.**  
Most agents did not see `annotated-tree` in their training set and will reflexively explore a folder using traditional `grep`/`find`/`ls`. To make sure that agents still see and use annotations for decision making, we can install a `PreToolUse` hook (see ours here [`.claude/settings.json`](.claude/settings.json)). The tool call still runs unchanged, but each path in the output is printed along with its annotation.

## TL;DR (for humans)

If agents do real work in your repo, you have hit these:

- You **re-teach it your architecture** every session.
- It drops a correct change in the **wrong place**.
- It **litters a duplicate** for something you already had, because it never knew it existed.
- Its **memory and skill files** have drifted into a duplicated mess.

`annotated-tree` fixes the cause, not the symptoms. It **makes your agent write a
one-line annotation at the top of every file** (what the file is for, and what it is
deliberately not), and **a local git hook enforces it**, so the annotations can't go
missing or rot.
That is workspace hygiene your agent keeps up itself.

Once every file carries its annotation, one command renders the whole workspace, each
file's job and how the code connects, so your agent plans against real structure instead
of re-deriving it every session. The overview is only worth trusting *because* the
process guarantees it is complete.

You do not run it, your agent does. Paste this and it sets itself up:

```text
Set up https://github.com/fredrikolis/annotated-tree in this workspace.
First read the raw README so you work from the source, not your priors:
https://raw.githubusercontent.com/fredrikolis/annotated-tree/main/README.md

Then:
- Install it.
- Annotate the workspace in batches, busiest areas first. Editorial guidance:
  `annotated-tree --annotation-guide`.
- Add the commit-time git hooks. Setup guide: `annotated-tree --githook-guide`.
- Put annotations in tool results:
  `annotated-tree bash-annotator --install-claude-hook .claude/settings.json`.
  Naming the file keeps it to this workspace; the default is my global
  settings. Tell me to restart Claude Code afterwards, or it does not take
  effect.
- Wire the map into every session.

Set up only what fits, not everything. Recommend, then do it, and ask me only
where you actually need a call.
```

## Rationale (long read for agents)

### Agentic development is software automation

Structure turns expensive inference into cheap reading. That is the whole idea behind
this tool. Automation has always run on it:

- **Throughput lives in the environment, not the worker.** A warehouse robot doesn't
  recognize a package, it reads the barcode: an annotation fixed to the item so no
  worker ever has to *infer* what it is. Rails, fixtures, labels, barcodes each turn
  inference into reading, which is faster and right every time. The speed is in the
  structure, not the robot (Kirsh, 1995; re-proved for agents by SWE-agent, Yang et al.,
  2024).
- **Coverage is the product.** Barcodes on 60% of the items keep almost none of the
  benefit, because the slow looking-at-things has to stay alive for the other 40%. The
  payoff needs the label to *always* be there, which is why the tool ships with a
  linter.
- **The annotation is the file's barcode.** `# Concern: issue session tokens |
  Non-concern: password checks (see credentials.py) | IO: (Request) -> Session` tells
  an agent enough to route by, and enough to leave alone, without opening the file.
- **Nothing an agent learns survives the session.** Its model evaporates at session
  end, so anything not written into the workspace is re-derived from scratch every
  time, out of the one resource it actually spends, its context window (even humans
  burn ~58% of their time rebuilding this understanding, Xia et al., 2018). The
  workspace is the agent's only long-term memory.
- **Every session start is a takeover.** The hardest moment in any automation is the
  handover, and it lands on whoever is least prepared (Bainbridge, 1983; Endsley,
  2023). For an agent that is every morning: it arrives knowing nothing, seconds to
  onboard.
- **The worker and the operator are the same thing, reading the same document.** A
  factory kept them apart: barcodes for the robot, schematics for
  the technician. An agent is both, so the annotated tree is both at once: the barcode
  it routes by, and the map it rebuilds the system from.
- **Prose context files miss this.** A CLAUDE.md is one page of hand-maintained prose,
  checked by nothing; it can neither route a change nor flag its own drift (no measured
  gain in task success, +20% cost, Gloaguen et al., 2026). The missing layer is
  per-file, structured, and enforced.

The number this moves is **first-pass yield**: the share of the agent's output that
survives review instead of getting caught and redone. Almost every wrong implementation
we have caught is a separation-of-concerns mistake (the term is Dijkstra's, 1974): code
in the wrong place, a boundary crossed, which is exactly what a concern map prevents.

One boundary: `annotated-tree` annotates the thing being worked on, not the process of
working on it (your review and coding standards are a sibling document;
[docs/repo-standards.md](docs/repo-standards.md) holds ours).

### Why first-line annotations

Why one line at the top of each file, instead of a wiki, longer docstrings, or an
on-demand summary? Because it is the simplest thing that survives fast, agent-driven
change:

- **On the file**, so it cannot go stale: it travels in the same diff and is reviewed
  in the same change. Documentation kept anywhere else is out of date on arrival
  (Lethbridge et al., 2003).
- **First line**, so a tool lifts it with no parsing and no model call, and a reader
  sees it the instant the file opens. That is what makes a whole-workspace map
  possible.
- **One line**, so it forces the point: you cannot stretch a single line over five
  responsibilities. A file that resists being described in one is telling you
  something.
- **A contract, checkable both ways**: cheap to check the code still matches the line,
  and the line still matches the code. Drift is a review signal that has surfaced real
  bugs (Tan et al., 2007). One asymmetry, in plain words: a wrong annotation is worse
  than none, because misleading text degrades a model more than absence does (Macke &
  Doyle, 2024). The linter guarantees presence and form; only review guarantees truth,
  which is why drift is a review concern, not a lint concern.
- **Not a repeat of the code**: it states the file's concern and what it excludes,
  which the internals never say. Types and tests capture what a function does; the
  annotation captures what the file is *for*, and its place in the whole.
- **Written, not re-derived**: an on-demand summary is paid for on every read, comes
  out different every time, and can only describe what the code does, never what it is
  supposed to do. Only written intent can be contradicted (the drift signal) or
  reviewed (the consent), and rationale is the record developers most want and least
  often have (LaToza et al., 2006).

One cost: these lines get long, sometimes longer than the code above them, and to a
human skimming the file that is clutter. That is a human cost, and the human is not the
reader we optimize for. An agent reads the whole line in one pass and moves on.

#### The format

One line, three ` | `-delimited fields, behind the file's comment marker:

```
[marker] Concern: the file's one job | Non-concern: a concern it deliberately does not own | IO: (in) -> out, or none [close]
```

`[marker]`/`[close]` are the language's comment delimiters: `#`, `//`, and `--` need
only the opener; Markdown and HTML need both, e.g.
`<!-- Concern: ... | Non-concern: ... | IO: ... -->`.

- **Form.** All three fields must be present and non-empty. `--strict-check`
  enforces that, plus a 200-character bound on the whole annotation by default, and
  it **never checks what a field says**.
- **Content.** Every field states WHAT, never why, how, or when. No mechanism,
  no rationale, no conditions. That one is a writing rule, not a check.
- **`Non-concern`.** Names something an agent would expect here but this file
  does not own. Naming where it lives instead (a sibling, an external system, or
  out of scope) is optional: include it only when the owner is not already
  obvious from the tree.
- **`IO`.** Reads `none` for docs, config, and data.

#### Good vs bad annotations

An annotation exists to let a reader build a mental model without opening the file.
Here is the same small service annotated two ways. Notice how much of the design you
can reconstruct from each.

Vague annotations, present but you still have to read the code:

```
orders/
├── api.py            # Handles the API.
├── service.py        # Business logic and helpers, also some order logic.
├── repository.py     # Database code.
├── models.py         # Models.
└── notifications.py  # Notification utils, also does some order logic.
```

Where do the request rules live, `api` or `service`? Is `notifications` safe to
change, or does it hold order logic too (it hints that it does)? Where does a new
pricing rule go? You cannot tell without opening every file. The annotations are
there. They just carry no map.

Real annotations, each stating its concern, its boundary, and its I/O:

```
orders/
├── api.py            # Concern: validate requests, call OrderService, serialize replies | Non-concern: order rules or storage | IO: (Request) -> Response
├── service.py        # Concern: order rules (pricing, discounts, state transitions) | Non-concern: HTTP or storage | IO: (OrderDraft) -> Order
├── repository.py     # Concern: load/save Order aggregates | Non-concern: order rules | IO: (Order) <-> Postgres
├── models.py         # Concern: Order / OrderLine types + invariants | Non-concern: I/O or rules | IO: (fields) -> Order
└── notifications.py  # Concern: sends order-event emails via the mailer | Non-concern: deciding when events fire | IO: (OrderEvent) -> void
```

Without opening anything, the org chart is obvious. `api` calls `service` calls
`repository`. Order rules live in `service`, not in `api` and not in `repository`.
`models` is pure. `notifications` only sends, it does not decide when. A new pricing
rule goes in `service.py`, and you already know what it must not touch. That mental
model came from the map, not the source, and it is the explicit `Non-concern:`
boundaries, as much as the concerns, that make it work.

There is a sloppy way to write a `Non-concern` and a rich one. `Non-concern:
everything not X` is always true and useless. A good one excludes something
*plausible*, the unexpected subset of the concern, or the neighbor an agent would
assume comes with it. `notifications.py` sends the order emails but does not decide
when they fire: exactly what you would have guessed wrong, which is exactly why it
is written down.

#### It is not just code

The same map works on any workspace: docs, references, data, a worklog, throwaway
experiments. There is no dependency graph and here IO is just `none`, since there is
no callable contract, but the org chart still reads at a glance:

```
research/
├── NOTES.md          # Concern: running worklog (decisions and open questions, newest first) | Non-concern: the spec | IO: none
├── proposal.md       # Concern: the pitch (problem, approach, success criteria) | Non-concern: implementation detail | IO: none
├── sources/
│   ├── prior-art.md  # Concern: annotated bibliography of related work | Non-concern: our own design | IO: none
│   └── trials.csv    # Concern: raw measurements from the runs | Non-concern: interpretation | IO: none
├── findings.md       # Concern: what the trials mean, and the recommendation | Non-concern: the raw numbers | IO: none
└── experiments/
    └── spike-01/     # Concern: throwaway spike testing approach A, kept for the record | Non-concern: production readiness | IO: none
```

You can see it at once. `proposal` and `findings` rest on `sources/`. `NOTES.md` is
the log, not the spec. `experiments/` is disposable by design. You know where a new
source file goes, and what it must not turn into, without opening one.

`trials.csv` is a plain CSV with nowhere to put a comment, so its line lives in a
`trials.csv.annotation` file beside it — a *sidecar*, holding the same bare
`Concern | Non-concern | IO` line a folder's `.annotation` holds. The sidecar is the
opt-in: a file that carries one is listed whatever its extension, and the sidecar
itself never takes a row of its own. Only a file with no comment syntax gets one, so
there is never a second place to look for a source file's annotation.

A directory gets a charter the same way. A folder has one job too, the coarsest
routing call an agent makes (does this change even belong in here), so it can carry
its own `Concern | Non-concern | IO` line, promoted onto the folder's row in the tree
(you saw one on `core/` at the top). Give it one with a `.annotation` file in the
folder, or let its entry file stand in for free (`lib.rs`/`main.rs`, `mod.rs`,
`__init__.py`, an `index.*`, a `doc.go`); the opt-in `require_package_charter` rule
can require every package with a manifest to have one. An `.annotation` carries the
charter line and no prose under it. `--strict-check` fails a note written below (blank
lines are fine); put the note in a README.

### How an agent uses it

Three uses, roughly in the order a task hits them.

**Plan, before writing a line.** Read the tree to find the unit that already *owns*
the concern you are about to touch, and put the change there instead of inventing a
second home for it. The `Non-concern:` lines and directory charters catch a crossing
before you commit it: pricing logic heading for the API layer, storage creeping into
the rules. Designing something new? Write its annotation first; a concern you cannot
state in one line, with a real `Non-concern`, is a design you have not made yet.
*Outcome: the change lands in the right place the first time.*

**Orient, including what you would never grep for.** "Where does X live, what handles
Y" is the easy half. The half only a map can do is the unknown unknowns: grep finds
what you already suspect is there (you search `retry`, `cache`, `auth` because you
know to look), but you cannot grep a capability you do not know exists. The map indexes
concerns, not identifiers. It reaches you two ways: rendered on demand, or appended to
your own `ls` and `find` results when the tool-call annotator is on, so a listing carries
the same concerns the tree does.

```
core/
├── scheduler.py   # Concern: run queued jobs to completion with retries | Non-concern: which backend runs a job | IO: (Job) -> Result
└── planner.py     # Concern: pick the cheapest backend for each job (cost-based optimizer over the provider menu) | Non-concern: which providers exist (see registry.py) | IO: (Job) -> Backend
```

`planner.py` is on the next line, so naming it would be bloat; `registry.py` is not in
this view, so that pointer earns its place.

Asked to "add a new compute provider," you would have grepped `provider`, wired it in
beside the others, and shipped, never learning that `planner.py` already routes every
job through a cost-based optimizer. The map surfaces it and the real task changes
shape: register with the optimizer, do not sit next to it. *Outcome: you reuse what
exists instead of rebuilding a worse copy in the wrong place.* (To hand the map to
another tool instead of reading it yourself, `--format json` serves the same thing as
structured data.)

**Review and impact.** `--changed` shows what a branch touched plus its
reverse-dependency blast radius, the things downstream that could break. *Outcome: you
scope a review, or a change, to exactly what it can break.*

One boundary: the tool renders, it does not reason. It makes structure observable and
leaves every judgment (what to annotate, where a concern belongs, whether the work is
worth doing) to the agent and to you.

## How to install and use it

### Install

Same prebuilt binary on every channel.

- **npx:** `npx annotated-tree`
- **cargo:** `cargo binstall annotated-tree` (prebuilt), `cargo install annotated-tree` (source)
- **curl:**
  ```sh
  curl --proto '=https' --tlsv1.2 -LsSf https://github.com/fredrikolis/annotated-tree/releases/latest/download/annotated-tree-installer.sh | sh
  ```

**annotated-tree makes no network request of its own.** The only program it starts is `git`, for
`--since`, and only with local commands. It reads files and writes to stdout. Two commands write
to disk:

- `bash-annotator --install-claude-hook` and `--uninstall-claude-hook`, which edit one file: your
  Claude Code settings.
- `strip`, which rewrites the files you name, or with `-R` every annotated file in a directory.

### The commands

`annotated-tree [PATHS]...` prints the annotated tree. The main flags are below. Run
`--help` for the full, exact reference.

| Capability | Flag or command |
|---|---|
| Annotated tree + dependency graph | *(default)* |
| Structured output for tooling and agents | `--format json` (versioned schema), `md` |
| Only what changed, plus blast radius | `--changed`, `--since <ref>` |
| Lint annotations + architectural rules (git hook or CI) | `--strict-check` |
| Bound the whole annotation's length *(200 by default)* | `--max-length <N>`, `0` to disable |
| Show dot-files and dot-directories, such as `.github` | `--hidden` |
| Cap entries shown per directory (big corpora) | `--max-per-node <N>`, `--full` |
| Runaway-scope guard | `--max-files <N>` |
| Remove first-line annotations in bulk | `strip [-R] [-y] <PATH>...` |

**An agent shown an existing annotation edits that line instead of reading the code**, so strip the
ones you want rewritten first. `strip` also removes them from a tree that should not carry
them.

- It lists the files it would change, and **writes nothing until you pass `-y`**.
- A directory needs `-R`. Without it, `strip` exits 2.
- Under `-R`, `strip` skips what the tree render skips: gitignored paths, `tests/`, and
  `node_modules`. Dot-directories too, unless you pass `--hidden`.
- `-I` narrows `strip` on files you name as well as files it walks.
- It deletes a line only when the whole line is a conforming annotation.
- `strip` takes the blank line under the annotation with it.
- A line that opens with one delimiter and ends in something else, such as `<!-- ... --><div>`,
  keeps its annotation.
- A `.annotation` charter or sidecar is reported and skipped. The whole file is the annotation, so
  removing it means deleting the file yourself.

### Put the annotations in your agent's tool call results

Your agent starts a task by searching, and what comes back is a list of paths that says
nothing about what any of them is for. A Claude Code hook pipes your agent's own `grep`,
`find` and `ls` output through an annotator, so the paths come back carrying the annotations.
The commands themselves run exactly as written:

```
$ grep -rl "Renderer" src
src/render/mod.rs  # Concern: the renderer seam — the `Renderer` trait, the format -> renderer factory, and the shared el …
src/render/text.rs  # Concern: formats the canonical map as a `tree`-style text view | Non-concern: filesystem reads | IO: …
```

Same command, same files, one line each, and your agent types nothing different. (Two of the
six result lines are shown, sorted and abridged for width here; the annotator itself does
neither.) Switch it on with `annotated-tree bash-annotator --install-claude-hook`, and see
what any command would become with `annotated-tree bash-annotator --check '<cmd>'`.

If you allowlist Bash commands, a `Bash(grep:*)` rule stops matching once the command is
rewritten, and those searches start prompting. Widen the rule, approve the prompts, or leave the
hook off where the allowlist matters more.

### Wire it into every session

Your agent starts every session blind, so the map has to reach it before it guesses.
Several ways to inject it, strongest first:

- **System prompt.** `claude --append-system-prompt-file <(annotated-tree)`. *Pro:* the
  agent reads the map as ground truth every session; the `-file` form fits a large tree
  past shell argument limits. *Con:* you bake it into how the agent launches.
- **Session hook.** Feed `annotated-tree` output through a `SessionStart` hook (its
  `compact` source also covers post-compaction) or `UserPromptSubmit` (refreshed every
  prompt). *Pro:* the map lands exactly when the agent's memory resets. *Con:* hook
  output is often size-capped.
- **AGENTS.md / CLAUDE.md note.** *Pro:* lowest effort, no launch change. *Con:* agents
  skim these files and slide back to grep-and-read.

The mechanics are still shifting; we default to the system prompt.

### Enforce it

Enforce at commit in a **local git hook, not CI**: the hook blocks the bad commit while
the agent can still fix it. `annotated-tree --githook-guide` prints the full setup guide.
Three gates under `.githooks/` (enable with `git config core.hooksPath .githooks`):

1. **Lint enforcement (pre-commit).** `annotated-tree --strict-check .` rejects a missing
   or malformed annotation. Presence and form, no judgment.

2. **Reviewer enforcement (commit-msg).** The lint cannot tell whether a line is still
   *true*. Gate the commit on a neutral reviewer (not the author) who checks, per changed
   file, that the annotation still holds after the diff: `Concern` names what the file now
   does, `Non-concern` still excludes a real boundary the file does not own (not a truism),
   `IO` still matches. Grade each finding by what is WRONG, not by what the fix costs: block on
   MAJOR and MODERATE, which both mean there is work the reviewer has not judged yet, and record
   MINOR, which never blocks. A gate demanding zero findings never converges, because a reviewer
   with nothing at stake always finds one more. `--githook-guide` ships the wiring and names the
   tool that defines the attestation shape. [`.githooks/commit-msg`](.githooks/commit-msg) is
   a working example.

3. **Standards enforcement (optional, recommended, workspace-dependent).** Layer your
   repo's architectural and anti-litter rules on top. Lint-checkable ones go in a repo
   `.annotated-tree.toml`; the rest ride the same neutral review as (2).

   ```toml
   [rules]
   deny = [["web", "core"]]  # forbid `web` depending on `core`
   forbid_cycles = true      # fail on any dependency cycle
   forbid_orphans = true     # fail on internal packages with no edge in or out
   ```

### Configure it

Config layers built-in defaults < `~/.config/annotated-tree/config.toml` < repo
`./.annotated-tree.toml` < CLI flags; the repo file owns the language table and
dependency rules, so enforcement is a property of the repo, not each contributor's
machine. The annotation format is invariant; the only per-language knob is the comment
marker. Teaching it a new language is a few lines of TOML (an extension list + comment
marker, or a regex for exotic comment syntax), no code change. See the shipped
[default_config.toml](src/default_config.toml) for the exact keys.

The crate also exposes its walk, annotation and render primitives as a library. That surface
carries no stability promise: no semver policy, no deprecation cycle, and a breaking change
arrives as a compile error. 0.6.0 is such a change.

## Beyond the codebase

It is not only for code. A sales or product workspace is a worksite too, and the same
annotations make it legible to the agent working it:

```
sales/                   # Concern: work the current lead list | Non-concern: where the leads come from | IO: none
├── customer-list.csv
└── skills/              # Concern: how the sales agent works a lead | Non-concern: the lead data | IO: none
    ├── outreach.md      # Concern: how we contact a lead | Non-concern: which leads are worth it | IO: none
    └── lead-scoring.md  # Concern: how we rank leads | Non-concern: how we reach out | IO: none
```

The skills carry their concern and boundary the way code does, and the split between
scoring and outreach reads at a glance. `customer-list.csv` has no comment line to
hold an annotation (plain CSV has no comment syntax), so it renders as a bare name
until you drop a `customer-list.csv.annotation` sidecar beside it; the `sales/`
charter above it carries the meaning meanwhile.

The layers stack: the code repo, the product workspace that feeds it features and
bugs, the business workspace that decides what to build above that. Each is a
workspace an agent works, fed from the layer above and feeding the one below. Make each
layer legible and the automation scales up the org, not only the codebase.

## Additional Reading
The extended argument (the infinite-context objection, related work, what is still
unproven) and the full references for every citation on this page live in
[README_APPENDIX.md](README_APPENDIX.md).

## About the author

Fredrik Rydén holds a Ph.D. in telerobotics from the University of Washington and has
spent some fifteen years keeping humans in control of machines: teleoperating surgical
robots, subsea systems for the U.S. Navy, and remote-operation R&D with NASA and
defense contractors. He is the founder and CEO of
[Olis Robotics](https://www.olisrobotics.com), which builds software for monitoring
and remotely operating industrial robots.
