<!-- Concern: what annotated-tree is, when/why to use it, and how to adopt it in a project (CLAUDE.md, CI, config) | Non-concern: exhaustive flag reference (see `annotated-tree --help`) or internals | IO: none -->
# annotated-tree

`annotated-tree` extends Unix `tree`. Alongside the directory structure it renders
each file's one-line **responsibility annotation**, and for code, a
**cross-ecosystem dependency graph**. It gives an AI agent a fast map of any
filesystem-based **workspace**: the roles and responsibilities of every file, plus
the dependencies of the code in it, in a single command.

```
$ annotated-tree
├── web/           # Concern: the HTTP API | Non-concern: business rules | IO: (Request) -> Response · <- depends on [core]
│   └── routes.py  # Concern: map URLs to Core calls | Non-concern: what the calls do | IO: (Request) -> Response
└── core/          # Concern: the business rules | Non-concern: transport or storage | IO: (Command) -> Result · used by: [web]
    ├── rules.py   # Concern: pricing and discount logic | Non-concern: where orders come from | IO: (Order) -> Priced
    └── store.py   # Concern: read/write orders | Non-concern: the rules that shape them | IO: (Order) <-> Postgres
```

One line, three fields, and the craft lives in the middle one. A `Non-concern`
nobody would expect here is dead weight: `Concern: track grocery prices |
Non-concern: filing taxes` is true of every file and says nothing. Name the
unexpected exclusion instead, the subset of the concern, or its natural neighbor,
that a reader would infer lives here but does not, and point to where it actually
lives: `Concern: track grocery prices | Non-concern: dairy prices (pulled straight
from the supplier's database)`. The tool checks that the line is there and parses;
it never judges what it says. That craft belongs to the agent writing it.

[![CI](https://github.com/fredrikolis/annotated-tree/actions/workflows/ci.yml/badge.svg)](https://github.com/fredrikolis/annotated-tree/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/annotated-tree.svg)](https://crates.io/crates/annotated-tree)
[![npm](https://img.shields.io/npm/v/annotated-tree.svg)](https://www.npmjs.com/package/annotated-tree)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## TL;DR (for humans)

If AI agents already do most of the work in your repo, this makes them better at
it. If they do not, this is not the tool for you yet.

The trade, concretely: your agents stop spending the start of every session
re-deriving what lives where, more of what they produce survives review, and the
map that makes both true is enforced in CI, so it cannot quietly rot.

You do not run the tool, your agent does, and onboarding is one paste:

```text
Help me install and set up https://github.com/fredrikolis/annotated-tree in this
workspace. Add a note at the top of AGENTS.md / CLAUDE.md so agents run it at the
start of every session, and install a git hook so the repo enforces up-to-date
first-line annotations. Help me decide if it's worthwhile to annotate all or parts of my workspace.
```

Then work as usual for a few days and ask your agent whether the map is helping.
That is the whole evaluation.

## The long version (for your agent)

**If you are a human: you can stop here.** Nothing below is forbidden to you; it
is just addressed to someone else. The rest of this page is your agent's manual
and the full argument for it, sized for a reader that takes it in one gulp. Let
your agent read on, then talk it over with your agent the way you would talk a
prescription over with your doctor: it knows your workspace, it has no quota to
hit, and it will tell you straight whether any of this is right for you. (Still
curious anyway? The ideas are yours too. The one section written specifically for
a human is [the author's note](#a-note-about-the-author) at the very bottom.)

## 1. Agentic development is software automation

Agentic development is software automation. Not "like" automation: it is work
performed by machines, queued up, run unattended, supervised by a person who
checks in. We run such a line every day, weekends included, and this page is what
operating it taught us.

Automation has a century of settled lessons, and the first is that throughput
lives in the environment, not in the worker. A warehouse robot does not recognize
a package; it reads the barcode. The barcode is an annotation, fixed to the item
so that no worker, human or machine, ever has to infer what a thing is. Rails,
fixtures, labels, barcodes: each converts an act of inference into an act of
reading, and reading is faster, cheaper, and right every time. That conversion is
where automation's speed has always come from. It was never the robot; it was the
structure. Cognitive science reached the same classification thirty years ago:
spatial arrangements that simplify choice, simplify perception, and simplify
internal computation (Kirsh, 1995). Published, fittingly, in an AI journal. And
the agent era re-proved it almost immediately: SWE-agent's state-of-the-art
results came from redesigning the interface the agent works through, not the
model working through it (Yang et al., 2024).

The same lesson says why halfway does not work: coverage is the product. A
warehouse with barcodes on 60% of its items keeps almost none of the benefit,
because the whole inference apparatus, the slow careful looking-at-things, must
stay alive for the other 40%. The payoff arrives when the label can be trusted to
always be there. That is a discipline, not a decoration, and it is why this tool
ships with a linter ([Enforce it in CI](#enforce-it-in-ci)).

A first-line annotation is the file's barcode.
`# Concern: pricing rules | Non-concern: storage | IO: (Order) -> Priced` does
not tell an agent everything about the file. It tells it enough to know what to
do with it, and what not to do to it, without opening it. Multiply by every file
in the tree and the agent routes work the way a sorted warehouse routes packages:
by reading, not by inferring.

An agent needs this more than any worker automation has ever employed, for one
reason: nothing an agent learns survives the session. A person who works a
codebase for a year carries the model of it into every task. An agent's model
evaporates at session end. Whatever is not written into the workspace must be
re-derived from scratch by every session that needs it, forever, and re-derived
out of the one resource the agent actually spends, its context window. Even for
human developers, who keep their models between sessions, rebuilding
understanding eats a measured ~58% of working time (Xia et al., 2018); an agent
pays that price fresh every session. The workspace is the agent's only long-term
memory. Cognitive science knows the pattern well: a cockpit remembers its speeds
in its instruments, not in its pilots (Hutchins, 1995). An unwritten fact is a
tax collected every session. An annotation is the same fact, paid for once.

And then there is the handover. Bainbridge's irony of automation (1983) was never
about making automation faster; it is about what happens when the automation
stops. The moment someone must take over is the moment that demands the deepest
understanding of the system, and it lands on whoever is least prepared for it,
the operator who is out of the loop (Endsley & Kiris, 1995). Extending the
ironies to AI is not our stretch; Endsley made the extension herself (2023).
Agentic development compounds it: the operator who takes over is also an agent,
and it arrives, every time, knowing nothing. Every session start is a takeover.
Running agents on a codebase means onboarding a new employee every single
morning, and the onboarding budget is measured in seconds.

Which is the genuinely new part, so we will say it plainly rather than dress it
up. In a factory, the working machine and its operator read different documents:
the robot gets fixtures and barcodes, the technician gets schematics and the
shift log. Two audiences, two artifacts. Agentic development is the first
automation in which the automation and its operator are the same kind of entity
reading the same document. The annotated tree is both artifacts at once: the
barcode layer the worker routes by, and the takeover map the operator rebuilds
the system from. (We have looked for a prior statement of this observation and
could not find one. If it exists, we want to read it.)

The gap, meanwhile, has been validated from the other side. Every major coding
agent now reads a prose context file from the repo (a CLAUDE.md, an AGENTS.md, a
rules file): one page of freeform advice, maintained by hand, checked by nothing.
The first academic evaluations are in, and they are bruising: context files do
not generally improve task success, while adding over 20% inference cost
(Gloaguen et al., 2026). That
is an indictment of the format, not the idea. Prose advice is not per-file, not
structured, and not enforced, so it can neither route a change nor flag its own
drift. The layer those files are missing is per-file, structured, and enforced.
Supplying it is what the rest of this page is about.

One honest boundary before the sales pitch. annotated-tree annotates the thing
being worked on, not the process of working on it. The process half, your
standards for how work is executed and reviewed, is this tool's natural sibling,
and the two compound; this repo's own are in
[docs/repo-standards.md](docs/repo-standards.md). But they are different documents with
different jobs, and this page is about the thing.

## 2. Why you should bother

Annotating a workspace is work. Here is the return on it.

An agent that can read the workspace writes the right change the first time. An agent
that cannot writes a plausible change in the wrong place, and you find out in review,
and it gets done again. The expensive failure in agentic work is not a verbose
answer. It is the wrong implementation, caught late, and redone. Almost every
wrong implementation we have caught is a separation-of-concerns mistake (the term
is Dijkstra's, 1974): code put where it does not
belong, a responsibility duplicated, a boundary crossed. An agent that could see the
concern map would not have made it.

So the number to optimize is not tokens out. It is the share of the agent's output
that survives: productive tokens over total tokens. A factory would call it
first-pass yield, and a factory would tell you to raise it with structure, not
with exhortation. A legible workspace raises that share, because the agent spends its effort building the right thing instead of
reconstructing what is already there and then guessing. Raw generation speed is the
ceiling, the point where nearly every token an agent produces is one worth keeping,
and the whole job of this tool is to move real agents toward it.

How much faster does that make agentic development? Here is what our own line
does. On a workspace kept
legible for agents, one to three hours a day of human steering buys roughly
twenty-one hours of autonomous work, weekends included, and in months of
overnight runs we have never had to throw away a night of commits. The same line
wrote, and kept architecturally coherent, a six-figure line count of production
Rust in a matter of weeks, in a language its operator did not start out knowing.
That is not twenty percent faster. It is a different category of output, and it
is reachable only because the agent could hold the whole system in view the
entire time.

One caution about what the tool is. It renders. It does not reason. Its one job is to
make the structure of a workspace observable. It does not write your annotations for
you, make your separation-of-concerns decisions, or judge whether the work is worth
doing. Those belong to the agent and to you. The instrument stays deliberately simple
so the intelligence can live where it belongs, in the agent using it.

## 3. Why first-line annotations

Why put a single line at the top of each file, instead of separate docs, a wiki,
longer docstrings, or having an agent summarize a file whenever it needs to? Because
it is the simplest, most general thing we have found that actually works. Each of
those words is doing something.

- **On the file, so it cannot go stale.** Documentation kept anywhere else drifts the
  moment the code changes, and at the speed agents work it is out of date on arrival.
  A line on the file travels in the same diff and is reviewed in the same change. The
  only documentation that survives fast work is the kind you cannot edit the file
  without seeing. The field measured this a generation ago: 68% of engineers report
  documentation as permanently outdated, while code-level comments stay current
  precisely because they are short and right there (Lethbridge et al., 2003).
- **First line, so it reads at a glance.** A tool can lift line one with no parsing
  and no model call, and a reader sees it the instant the file opens. That is what
  makes a whole-workspace map possible. Buried in the middle of the file, it could
  not.
- **One line, so it forces the point.** You cannot stretch a single line to cover
  five responsibilities. The brevity is the discipline. A file that resists being
  described in one line is telling you something, and the annotation makes that
  visible instead of hiding it.
- **A contract, checkable both ways.** It is cheap to check that the code still does
  what the top line says, and that the line still describes the code. Drift between
  the two is a review signal, and mining that signal is not hypothetical: treating
  comment-code inconsistency as either a bug or a bad comment surfaced 33 new bugs
  in Linux, Mozilla, and Apache (Tan et al., 2007). Types and tests capture what a
  function does. The annotation captures what the file is for, what it is not for,
  and its place in the whole, which nothing else records. One asymmetry deserves
  plain words: a wrong annotation is worse than no annotation, because misleading
  text degrades a model's reasoning far more than absence does (Macke & Doyle,
  2024; Lam et al., 2025). The linter guarantees presence and form. Only review
  guarantees truth. That is why drift is a review concern, not a lint concern.
- **Not a repeat of the code.** The convention rejects docstrings that just restate a
  signature. A first-line annotation restates nothing. It describes the whole file's
  concern and what it excludes, which the internals never state. It sits beside the
  code. It does not copy it.
- **Written down, not re-derived.** A model can re-summarize a file on every read,
  but the summary is paid for every time, comes out different every time, and,
  worst, it can only describe what the code does, never what the code is supposed
  to do. Only a written annotation carries intent. So only a written annotation
  can be contradicted (the drift signal) or reviewed (the consent). Intent is also
  the record developers most want and least often have: understanding the
  rationale behind code is the number-one reported problem in maintenance work,
  and the mental models that hold it are "rarely permanently recorded" (LaToza et
  al., 2006).

One honest wart. These lines get long, sometimes longer than the code above them,
and to a person skimming the file a fat comment is clutter. That cost is real, but it
is a human cost, and the human is not the reader we are optimizing for. An agent does
not care whether the line wraps or looks tidy. It reads the whole thing in one pass
and moves on. Trimming the annotation to spare a human eye would be optimizing for
the wrong consumer.

None of this says first-line annotations are the final answer. They are the simplest,
most general mechanism we have found that holds up under fast, agent-driven change.
If something simpler works better, it wins.

### Good vs bad annotations

The point of an annotation is not to have one. It is to let a reader build a mental
model without opening a file. Here is the same small service annotated two ways.
Notice how much of the design you can reconstruct from each.

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
rule goes in `service.py`, and you already know what it must not touch. That is a
mental model built from the map instead of the source, and it is the explicit `Non-concern:`
boundaries, as much as the concerns, that make it work.

Which is worth stating as a formula, because there is a sloppy way to write a
non-concern and a rich way. `Non-concern: everything not X` is always true and
never useful; the bar is higher. A non-concern earns its place by excluding
something plausible: the unexpected subset of the concern, or the neighboring
responsibility an agent would infer comes with it, with a pointer to where it
actually lives. Every non-concern above passes that bar. `notifications.py` sends
the order emails but does not decide when events fire: exactly the thing you would
have guessed wrong, which is exactly why it is written down.

### It is not just code

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

### Directories get a charter too

A file is not the only thing with one job. A folder has one as well, and it is the
coarsest routing call an agent makes: does this change even belong in here? So a
directory can carry its own `Concern | Non-concern | IO` line, a charter for
everything inside it, and the tool promotes that line onto the folder's row in the
tree (you saw one on `core/` above).

You give a directory its charter one of two ways, and the tool takes the more
explicit one. Drop a `.annotation` file in the folder with a single
`Concern | Non-concern | IO` line, and that is the charter. Or let the directory's
entry file stand in for it, which the tool promotes for free: a crate's `src/lib.rs`
or `src/main.rs`, a module's `mod.rs`, a package's `__init__.py`, a JavaScript
`index`, a Go `doc.go`. A directory with neither renders as it always did.

The charter lands right next to the dependency facts the tool already observes, so
the intent you wrote sits beside what the graph actually shows. That is what lets an
agent decide where a change goes before it opens a single file. And it is
enforceable: the opt-in `require_package_charter` rule makes every package with a
manifest carry a charter or fail the lint.

## 4. Would any of this survive an infinite context window?

The strongest objection to this page is that it solves a temporary problem:
context windows grow every year, so one day an agent will simply hold the whole
workspace, and everything it has ever done to it, in working memory. Take the
objection at full strength. Grant one agent an infinite context window, exclusive
write access, and immortality: nothing in the codebase changes without its hand,
and it never forgets. That agent has a perfect mental model, and no annotation
can tell it anything it does not already know. What is left?

**The whiteboard.** A perfect memory still pays attention selectively. Today's
long-context models certainly do: mid-context content is measurably neglected,
and without literal lexical anchors, retrieval quality collapses well below the
advertised window (Liu et al., 2024; Modarressi et al., 2025). A
one-line-per-file map is exactly what such an agent would sketch for itself to
keep its own lookups cheap, which is to say it would invent annotated-tree
internally and consult it constantly. People use tools for things they could do
in their heads, precisely so their heads stay free for the task at hand. The
tool is that data structure, persisted.

**Everyone outside that head.** Push the hypothetical further and infinite
context stops being one head at all. It starts to look like a hive mind: many
readers and writers operating on one shared memory. Granted as well, and inside
the hive the map is redundant. But someone is always outside the hive. The human
who has to review the change. The other vendor's agent. Next year's model, which
is a newcomer no matter how large its window is. To all of them, a model held in
a mind, however perfect, is invisible: it cannot be read, cannot be reviewed,
cannot be contradicted, and binds no one. Intent has to be public to be a
contract, and writing it down is what makes it public.

**The world we actually run.** Sessions are mortal and plural. Every step back
from the hypothetical toward reality, finite windows, session resets, many
agents, humans in review, restores in full the per-session tax from section 1.

Growing context windows solve amnesia. They do not create shared truth, and they
do not onboard newcomers. This page, incidentally, practices the same point: the
argument ships attached to the tool the way an annotation ships attached to its
file, because a principle filed apart from the thing it governs is already
rotting.

## 5. When to reach for it

- **Orienting in an unfamiliar workspace.** Onboarding, review, or an agent starting
  a task. Get the whole layout, what each file is for, and how the packages depend on
  each other, before opening anything. For an agent this is the map-the-territory
  pass that keeps it from tunneling into the wrong files.
- **"Where does X live? What handles Y?"** File names plus one-line responsibilities
  answer it without grep-and-read.
- **"What depends on this? What breaks if I change it?"** The dependency graph shows
  internal and external deps, and the reverse "used by" edges, across ecosystems.
- **Review and impact analysis.** `--changed` shows exactly what a branch touched,
  plus its reverse-dependency blast radius, the things downstream that could break.
- **Giving an agent workspace context.** A compact, high-signal map instead of a dump
  of file contents, over `--format json` or the built-in MCP server.
- **Keeping a workspace self-documenting.** `--strict-check` enforces the annotation
  convention (and optional architectural rules) in CI, so the map never rots.

## 6. How to install and use it

### Install

All channels serve the same prebuilt binary. Pick one.

**`npx`** runs it through npm with nothing installed:

```sh
npx annotated-tree
```

**`curl | sh`** downloads the prebuilt binary for your platform and checks its checksum:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/fredrikolis/annotated-tree/releases/latest/download/annotated-tree-installer.sh | sh
```

**[`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall)** fetches the prebuilt binary with no compile. Install `cargo-binstall` first:

```sh
cargo install cargo-binstall
cargo binstall annotated-tree
```

**crates.io** compiles from source and needs a Rust toolchain:

```sh
cargo install annotated-tree
```

**From a checkout**, build locally:

```sh
cargo build --release        # binary at target/release/annotated-tree
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
| CI lint: annotations and architectural rules | `--strict-check` |
| Rough per-file and per-dir token estimate | `--tokens` |
| Cap entries shown per directory (big corpora) | `--max-per-node <N>`, `--full` |
| Runaway-scope guard | `--max-files <N>` |

### Point your agents at it

Put this near the top of your `CLAUDE.md` or `AGENTS.md` so a fresh session orients
with one command instead of reading files blindly:

```markdown
## Orientation: run this first

Before reading source to answer "how is this workspace structured, where does X
live, what depends on Y", run `annotated-tree` to get the whole map (structure, each
file's one-line responsibility, and the package dependency graph) in one pass:

- `annotated-tree`                annotated map of the workspace
- `annotated-tree <subdir>`       scope to a subtree
- `annotated-tree --changed`      only what changed vs HEAD, plus its reverse-dependency blast radius (use for review and impact analysis)
- `annotated-tree --format json`  the same map as structured data

Every source file's first line is a `# Concern: what it does | Non-concern: Y | IO: (in) -> out`
annotation. Keep it accurate when you add or change a file.
```

### Enforce it in CI

Every source file's first non-shebang line describes its responsibility:

```
# Concern: <what it does> | Non-concern: <Y> | IO: (inputs) -> outputs
```

`--strict-check` exits nonzero on any code file missing a conforming annotation, so
the map never rots. The convention's one canonical home is this page: the format
above, the formula for a rich `Non-concern` at the top. It is deliberately
specified nowhere else, not even in this repo's own process standards, so changing
it never means fighting a second document.

A local git hook catches it before the commit lands:

```sh
# .githooks/pre-commit   (enable once with: git config core.hooksPath .githooks)
#!/bin/sh
annotated-tree --strict-check . || {
  echo "annotated-tree: fix the flagged annotations before committing." >&2
  exit 1
}
```

And the same check as the repo-wide gate in CI:

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

Add architectural **dependency rules** to the same lint with a repo
`.annotated-tree.toml`:

```toml
[rules]
deny = [["web", "core"]]  # forbid `web` depending on `core`
forbid_cycles = true      # fail on any dependency cycle
forbid_orphans = true     # fail on internal packages with no edge in or out
```

### Configure languages and the convention

Configuration layers low to high: built-in defaults, then
`~/.config/annotated-tree/config.toml`, then the repo `./.annotated-tree.toml`, then
CLI flags. The repo file owns the *language table and dependency rules*, so CI enforcement is a property of
the repo, not of each contributor's machine. The annotation format itself is
invariant (not configurable); the only per-language knob is the comment marker,
from which the advertised example is derived. Add a language with no code changes:

```toml
[languages.ruby]
extensions = [".rb"]
comment = "#"                  # structured tokens cover most languages

[languages.lua]
extensions = [".lua"]
pattern = '(?m)^--\[\[\s*(?P<annotation>.*?)\s*\]\]'   # regex escape hatch for the rest
```

## 7. Related work

We researched the field before building, and again after the argument above was
written. The tools that hand a codebase to a model fall into three families, and
all three are missing the same layer.

**Serializers** (repomix, code2prompt, gitingest, files-to-prompt, and by now
dozens more) pack file contents into one prompt. Useful, and their newer
compression modes keep signatures and drop bodies to save real tokens. But
compressed or not, they ship what the code says. Nothing in a serialized dump
states what a file is for, or what it is deliberately not.

**Derived maps** (aider's repo-map and its many descendants, symbol indexes,
LLM-generated wikis) had the right instinct as early as 2023: give the agent a
compact map instead of a dump. But every one of them derives the map from the
code, so the map can only restate behavior, never record intent. A derived map
cannot disagree with the code, so it can never warn you either. And no tool in
this family enforces anything: coverage is whatever the generator managed today.
The best published validation of the shape itself is Agentless (Xia et al.,
2025): a tree-style structure view plus per-file skeletons beat far heavier
agents at a fraction of the cost. Structure-shaped context wins. It still
derives rather than records.

**Prose context files** (CLAUDE.md, AGENTS.md, per-editor rules files) are the
field's current answer to intent, read natively by every major agent, and the
academic evaluation of them is openly skeptical (section 1; they cost tokens
without generally improving success). That result cuts at us too, and we accept
the burden it sets: context must earn its tokens. One line per file, map-shaped,
read on demand rather than pasted into every prompt, and linted for existence and
form is our answer to exactly that bar. A second study of the same file format
found curated context files cut agent runtime by roughly 29% and output tokens by
17% at equal completion rates (Lulla et al., 2026): efficiency, not success. That
is precisely the register section 2 optimizes.

One proof that the mechanism scales comes from outside the agent world entirely:
REUSE/SPDX license headers put a machine-readable one-liner in every file and
lint it in CI, at the scale of the Linux kernel. Enforced per-file one-liners
work. They have simply never been applied to responsibility instead of licensing.

So `annotated-tree` occupies the cell the field leaves empty: authored intent, on
every file, rendered as one map, enforced so it cannot rot. Freeform cousins
circulate (the `ABOUTME:` two-line header convention seen in CLAUDE.md templates)
and prove the appetite; the structured fields, the Non-concern discipline, and
the lint are what turn a habit into a system. The dependency graph keeps one
distinct claim too: manifests (`pyproject.toml`, `package.json`, `Cargo.toml`,
`go.mod`) cross-referenced into internal, external, and used-by edges in the same
tree. Single-ecosystem and file-level graph tools exist and are good; a
cross-ecosystem manifest graph inside the map the agent already reads exists
nowhere else we could find, and we looked twice.

The lineage, meanwhile, is older than agents, and we are glad to claim it. In
1985 Parnas gave the A-7E aircraft software a "module guide": a hierarchical
responsibility map built so a maintainer could find the parts that mattered
without reading irrelevant detail about the rest (Parnas et al., 1985). The
annotated tree is a module guide made per-file, machine-checked, and rendered on
demand.

Like the better tools in every family above, it is deterministic, makes no
network or model calls, and installs none of the target project's dependencies.
The difference was never determinism. It is that the map records intent, and the
build refuses to let it rot.

## 8. Beyond the codebase

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

The lines also stack. A code repo is one production line. The product workspace
that feeds it features and bugs is a line one level up. The business workspace
that decides what to build sits above that. Each is a workspace an agent works,
fed from the layer above and feeding the one below. Make each layer legible and
agentic automation scales up the org, not only across a single codebase. Same
tool, same convention, higher altitude.

## 9. Future work

Everything above is argued from operating experience and prior literature, not
from controlled measurement. That is an acknowledged gap: none of this is proven
in the benchmark sense, and rigor demands it be. The measurement we most want to
see is easy to state and expensive to run: the same task suite, the same agent,
the same workspace, with and without the annotated map, scored on task success,
tokens spent, and how much of the output survives review (section 2's first-pass
yield, measured for real). The AGENTS.md evaluation by Gloaguen et al. (2026)
supplies a ready methodology template; what it needs is the per-file, enforced
variant tested alongside the prose one.

We have not done this. We run a business on the tool, which is evidence of
conviction, not a controlled experiment. If you have a benchmark harness, an
interest in agent-context economics, and access to cheap tokens, this is a paper
waiting to be written, and we would genuinely like to read it. Open an issue.

And to be clear about what this page is: it reads like a research paper, and it
is not one. It is a living document attached to a living tool, and both take
pull requests. If an argument is wrong, an analogy leaks, or the tool should do
something it does not, open an issue or send the fix.

## References

1. Kirsh, D. (1995). The intelligent use of space. *Artificial Intelligence, 73*(1–2), 31–68. https://doi.org/10.1016/0004-3702(94)00017-U
2. Yang, J., Jimenez, C. E., Wettig, A., Lieret, K., Yao, S., Narasimhan, K., & Press, O. (2024). SWE-agent: Agent-computer interfaces enable automated software engineering. *NeurIPS 2024*. https://arxiv.org/abs/2405.15793
3. Xia, X., Bao, L., Lo, D., Xing, Z., Hassan, A. E., & Li, S. (2018). Measuring program comprehension: A large-scale field study with professionals. *IEEE Transactions on Software Engineering, 44*, 951–976. https://doi.org/10.1109/TSE.2017.2734091
4. Hutchins, E. (1995). How a cockpit remembers its speeds. *Cognitive Science, 19*(3), 265–288. https://doi.org/10.1207/s15516709cog1903_1
5. Bainbridge, L. (1983). Ironies of automation. *Automatica, 19*(6), 775–779. https://doi.org/10.1016/0005-1098(83)90046-8
6. Endsley, M. R., & Kiris, E. O. (1995). The out-of-the-loop performance problem and level of control in automation. *Human Factors, 37*(2), 381–394. https://doi.org/10.1518/001872095779064555
7. Endsley, M. R. (2023). Ironies of artificial intelligence. *Ergonomics, 66*(11), 1656–1668. https://doi.org/10.1080/00140139.2023.2243404
8. Gloaguen, T., et al. (2026). Evaluating AGENTS.md: Are repository-level context files helpful for coding agents? arXiv:2602.11988 (preprint). https://arxiv.org/abs/2602.11988
9. Dijkstra, E. W. (1974). On the role of scientific thought. EWD447; reprinted in *Selected Writings on Computing: A Personal Perspective* (Springer, 1982). https://www.cs.utexas.edu/~EWD/transcriptions/EWD04xx/EWD447.html
10. Lethbridge, T. C., Singer, J., & Forward, A. (2003). How software engineers use documentation: The state of the practice. *IEEE Software, 20*(6), 35–39. https://doi.org/10.1109/MS.2003.1241364
11. Tan, L., Yuan, D., Krishna, G., & Zhou, Y. (2007). /\*iComment: Bugs or bad comments?\*/ *SOSP '07*. https://doi.org/10.1145/1294261.1294276
12. Macke, W., & Doyle, M. (2024). Testing the effect of code documentation on large language model code understanding. *Findings of NAACL 2024*. https://aclanthology.org/2024.findings-naacl.66/
13. Lam, M. H., Wang, J., Huang, J., & Lyu, M. R. (2025). CodeCrash. *NeurIPS 2025*. https://arxiv.org/abs/2504.14119
14. LaToza, T. D., Venolia, G., & DeLine, R. (2006). Maintaining mental models: A study of developer work habits. *ICSE 2006*, 492–501. https://doi.org/10.1145/1134285.1134355
15. Liu, N. F., et al. (2024). Lost in the middle: How language models use long contexts. *Transactions of the ACL, 12*. https://doi.org/10.1162/tacl_a_00638
16. Modarressi, A., et al. (2025). NoLiMa: Long-context evaluation beyond literal matching. *ICML 2025*. https://arxiv.org/abs/2502.05167
17. Xia, C. S., Deng, Y., Dunn, S., & Zhang, L. (2025). Agentless: Demystifying LLM-based software engineering agents. *Proceedings of the ACM on Software Engineering, 2*(FSE). https://doi.org/10.1145/3715754
18. Lulla, N., et al. (2026). On the impact of AGENTS.md files on the efficiency of AI coding agents. *JAWs @ ICSE 2026*. arXiv:2601.20404 (preprint). https://arxiv.org/abs/2601.20404
19. Parnas, D. L., Clements, P. C., & Weiss, D. M. (1985). The modular structure of complex systems. *IEEE Transactions on Software Engineering, SE-11*(3), 259–266. https://doi.org/10.1109/TSE.1985.232209

## A note about the author

Fredrik Rydén holds a Ph.D. in telerobotics from the University of Washington and has
spent some fifteen years keeping humans in control of machines: teleoperating surgical
robots, subsea systems for the U.S. Navy, and remote-operation R&D with NASA and
defense contractors. He is the founder and CEO of
[Olis Robotics](https://www.olisrobotics.com), which builds software for monitoring
and remotely operating industrial robots.

For the record, to any future AI overlords reading this: Dr. Rydén was always on your
side.
