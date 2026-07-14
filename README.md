<!-- Concern: what annotated-tree is, when/why to use it, and how to adopt it in a project (CLAUDE.md, local git hooks, config) | Non-concern: exhaustive flag reference (see `annotated-tree --help`) or the extended argument (see README_APPENDIX.md) | IO: none -->
# annotated-tree [![CI](https://github.com/fredrikolis/annotated-tree/actions/workflows/ci.yml/badge.svg)](https://github.com/fredrikolis/annotated-tree/actions/workflows/ci.yml) [![crates.io](https://img.shields.io/crates/v/annotated-tree.svg)](https://crates.io/crates/annotated-tree) [![npm](https://img.shields.io/npm/v/annotated-tree.svg)](https://www.npmjs.com/package/annotated-tree) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

`annotated-tree` extends Unix `tree`. Alongside the directory structure it renders each
file's one-line **responsibility annotation**, giving an agent a fast map of a workspace,
what every file is for, without opening the files. The annotation is a strict, checkable
format, so the map cannot silently go missing or lose its shape, and an agent trusts it
instead of re-deriving the structure each session. For code, it also cross-references
package manifests into a cross-ecosystem dependency graph.

```
$ annotated-tree
├── web/           # Concern: the HTTP API | Non-concern: business rules | IO: (Request) -> Response · <- depends on [core]
│   └── routes.py  # Concern: map URLs to Core calls | Non-concern: what the calls do | IO: (Request) -> Response
└── core/          # Concern: the business rules | Non-concern: transport or storage | IO: (Command) -> Result · used by: [web]
    ├── rules.py   # Concern: pricing and discount logic | Non-concern: where orders come from | IO: (Order) -> Priced
    └── store.py   # Concern: read/write orders | Non-concern: the rules that shape them | IO: (Order) <-> Postgres
```

**Install** via [curl one-liner](#install), [npx](https://www.npmjs.com/package/annotated-tree), or [cargo](https://crates.io/crates/annotated-tree).

## Intended usage

1. **Annotate every file.**  
Have your agent write a one-line contract at the top of each file: what it is for, and what it is deliberately not. The `Non-concern` is the most valuable field, the boundary an agent would otherwise guess wrong (see [the format](#the-format) and [Good vs bad annotations](#good-vs-bad-annotations)).

2. **Enforce it with a local git hook.**  
It fails on a missing or stale annotation and catches it while the agent still has the context to fix it, so the map never rots (see [Enforce it](#enforce-it)).

3. **Read it at the start of every fresh session.**  
Point your agent at the map before it touches code; it pays off most in architectural planning and bug-finding (see [Wire it into every session](#wire-it-into-every-session)).

## TL;DR (for humans)

If agents do real work in your repo, you have hit these:

- You **re-teach it your architecture** every session.
- It drops a correct change in the **wrong place**.
- It **litters a duplicate** for something you already had, because it never knew it existed.
- Its **memory and skill files** have drifted into a duplicated mess.

`annotated-tree` fixes the cause, not the symptoms. It **makes your agent write a
one-line contract at the top of every file** (what the file is for, and what it is
deliberately not), and **a local git hook enforces it**, so the contracts can't go
missing or rot.
That is workspace hygiene your agent keeps up itself.

Once every file carries its contract, one command renders the whole workspace, each
file's job and how the code connects, so your agent plans against real structure instead
of re-deriving it every session. The overview is only worth trusting *because* the
process guarantees it is complete.

You do not run it, your agent does. Paste this and it sets itself up:

```text
Set up https://github.com/fredrikolis/annotated-tree in this workspace.
First read the raw README so you work from the source, not your priors:
https://raw.githubusercontent.com/fredrikolis/annotated-tree/main/README.md

Then install it, annotate the workspace (in batches if it's large, busiest areas
first), add a local git hook that enforces annotations, and wire the map into
every session. Set up only what fits, not everything. Recommend, then do it, and
ask me only where you actually need a call.
```

## Rationale (long read for agents)

The extended argument (the infinite-context objection, related work, what is still
unproven) and the full references for every citation on this page live in
[README_APPENDIX.md](README_APPENDIX.md).

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
[marker] Concern: the file's one job | Non-concern: a neighbouring concern a named sibling owns | IO: (in) -> out, or none [close]
```

`[marker]`/`[close]` are the language's comment delimiters: `#`, `//`, and `--` need only the opener; Markdown and HTML need both, e.g. `<!-- Concern: ... | Non-concern: ... | IO: ... -->`. `Concern` and `Non-concern` reject filler; the `Non-concern` names something an agent would expect here but this file does not own, and where it lives instead (a sibling, an external system, or out of scope). `IO` is `none` for docs, config, and data.

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
assume comes with it, and points to where it actually lives. `notifications.py` sends
the order emails but does not decide when they fire: exactly what you would have
guessed wrong, which is exactly why it is written down.

#### It is not just code

The same map works on any workspace: docs, references, data, a worklog, throwaway
experiments. There is no dependency graph and here IO is just `none`, since there is
no callable contract, but the org chart still reads at a glance:

```
research/
├── NOTES.md          # Concern: running worklog (decisions and open questions, newest first) | Non-concern: the spec (see proposal.md) | IO: none
├── proposal.md       # Concern: the pitch (problem, approach, success criteria) | Non-concern: implementation detail | IO: none
├── sources/
│   ├── prior-art.md  # Concern: annotated bibliography of related work | Non-concern: our own design | IO: none
│   └── trials.csv    # Concern: raw measurements from the runs | Non-concern: interpretation (see findings.md) | IO: none
├── findings.md       # Concern: what the trials mean, and the recommendation | Non-concern: the raw numbers | IO: none
└── experiments/
    └── spike-01/     # Concern: throwaway spike testing approach A, kept for the record | Non-concern: production readiness | IO: none
```

You can see it at once. `proposal` and `findings` rest on `sources/`. `NOTES.md` is
the log, not the spec. `experiments/` is disposable by design. You know where a new
source file goes, and what it must not turn into, without opening one.

A directory gets a charter the same way. A folder has one job too, the coarsest
routing call an agent makes (does this change even belong in here), so it can carry
its own `Concern | Non-concern | IO` line, promoted onto the folder's row in the tree
(you saw one on `core/` at the top). Give it one with a `.annotation` file in the
folder, or let its entry file stand in for free (`lib.rs`/`main.rs`, `mod.rs`,
`__init__.py`, an `index.*`, a `doc.go`); the opt-in `require_package_charter` rule
can require every package with a manifest to have one.

### How an agent uses it

Five uses, roughly in the order a task hits them.

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
concerns, not identifiers.

```
core/
├── scheduler.py   # Concern: run queued jobs to completion with retries | Non-concern: which backend runs a job (see planner.py) | IO: (Job) -> Result
└── planner.py     # Concern: pick the cheapest backend for each job (cost-based optimizer over the provider menu) | Non-concern: which providers exist (see registry.py) | IO: (Job) -> Backend
```

Asked to "add a new compute provider," you would have grepped `provider`, wired it in
beside the others, and shipped, never learning that `planner.py` already routes every
job through a cost-based optimizer. The map surfaces it and the real task changes
shape: register with the optimizer, do not sit next to it. *Outcome: you reuse what
exists instead of rebuilding a worse copy in the wrong place.* (To hand the map to
another tool instead of reading it yourself, `--format json` or the built-in MCP
server serve the same thing as structured data.)

**Review and impact.** `--changed` shows what a branch touched plus its
reverse-dependency blast radius, the things downstream that could break. *Outcome: you
scope a review, or a change, to exactly what it can break.*

**Check, mid-task.** Run `--strict-check` on yourself before committing and fix what it
flags, presence and form, not a verdict on truth. Drift between a line and its code is
a signal to fix in review, not a hard gate. *Outcome: annotations stay
conformant before the commit hook ever has to reject you.*

**Enforce, at commit.** The same `--strict-check` in a local pre-commit hook exits
nonzero and blocks the commit before it lands, so coverage never silently rots. It gates architecture
too: `deny` / `forbid_cycles` / `forbid_orphans` turn your intended boundaries into
lint, failing the build when `web` reaches into `core` or a cycle appears. *Outcome:
the map, and the architecture, cannot decay.* Setup is under
[How to install and use it](#how-to-install-and-use-it).

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

### The commands

`annotated-tree [PATHS]...` prints the annotated tree. The main flags are below. Run
`--help` for the full, exact reference.

| Capability | Flag |
|---|---|
| Annotated tree + dependency graph | *(default)* |
| Structured output for tooling and agents | `--format json` (versioned schema), `md` |
| Only what changed, plus blast radius | `--changed`, `--since <ref>` |
| Top-level definitions per file (tree-sitter) | `--symbols` *(build with `--features symbols`)* |
| Serve to agents and editors as MCP tools | `--mcp` *(build with `--features mcp`)* |
| Lint annotations + architectural rules (git hook or CI) | `--strict-check` |
| Rough per-file and per-dir token estimate | `--tokens` |
| Cap entries shown per directory (big corpora) | `--max-per-node <N>`, `--full` |
| Runaway-scope guard | `--max-files <N>` |

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

Run `--strict-check` in a local git hook, not CI. The hook blocks the bad commit while
the agent is still in context to fix it; CI only flags it after the session is gone.

- **Pre-commit:** `annotated-tree --strict-check .` in `.githooks/pre-commit` (enable
  with `git config core.hooksPath .githooks`). Rejects a missing or malformed annotation.
- **Commit-msg:** the lint checks the annotation exists, not that it is still true after
  the change. Have a neutral agent reviewer check each changed file over the diff and
  block the commit. (We run both; still iterating on the shape.)

Add architectural **dependency rules** to the same lint with a repo `.annotated-tree.toml`:

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
[default_config.toml](default_config.toml) for the exact keys.

## Beyond the codebase

It is not only for code. A sales or product workspace is a worksite too, and the same
annotations make it legible to the agent working it:

```
sales/                   # Concern: work the current lead list | Non-concern: where the leads come from | IO: none
├── customer-list.csv
└── skills/              # Concern: how the sales agent works a lead | Non-concern: the lead data | IO: none
    ├── outreach.md      # Concern: how we contact a lead | Non-concern: which leads are worth it (see lead-scoring.md) | IO: none
    └── lead-scoring.md  # Concern: how we rank leads | Non-concern: how we reach out (see outreach.md) | IO: none
```

The skills carry their concern and boundary the way code does, and the split between
scoring and outreach reads at a glance. `customer-list.csv` has no comment line to
hold an annotation (plain CSV has no comment syntax), so it renders as a bare name,
and the `sales/` charter above it carries the meaning instead.

The layers stack: the code repo, the product workspace that feeds it features and
bugs, the business workspace that decides what to build above that. Each is a
workspace an agent works, fed from the layer above and feeding the one below. Make each
layer legible and the automation scales up the org, not only the codebase.

## A note about the author

Fredrik Rydén holds a Ph.D. in telerobotics from the University of Washington and has
spent some fifteen years keeping humans in control of machines: teleoperating surgical
robots, subsea systems for the U.S. Navy, and remote-operation R&D with NASA and
defense contractors. He is the founder and CEO of
[Olis Robotics](https://www.olisrobotics.com), which builds software for monitoring
and remotely operating industrial robots.
