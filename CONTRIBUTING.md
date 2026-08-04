<!-- Concern: how this repo's automated development process works, and how to stand one up elsewhere | Non-concern: cutting a release, tool usage, or the annotation format | IO: none -->
# Contributing
Contributions are welcome, and issues and ideas more so. For code contributions we ask that you follow the repo's standard automated process, or at a minimum the [maintainer agent workflow](#maintainer-agent-workflow).

To stand this process up in a different repo, see [implementing this automation](#implementing-this-automation-in-an-existing-repo).

## Automated software development process

### Product manager agent workflow

```
inbound issue / human idea
  │
  ▼
PRODUCT MANAGER ─ owns the request end to end. Indentation is scope:
│  every line below is spawned by PM and reports back to it.
│
├─ TRIAGE, PM's own work, upstream of this repo.
│     reproduce it, or confirm the attribution
│     does a unit already own it?   the map
│     does a clause forbid it?      SPEC.md
│     any of these fails ──► answer on the issue, stop here
│
├─ spawns PLANNER ─ plans against the standards doc.        ◄─────┐
│     PM reviews that plan adversarially and returns it ──────────┘
│     until it holds. It stays in context, never in this repo.
│
├─ spawns MAINTAINER ─ hands it the approved plan; MAINTAINER owns
│     the change end to end, through push.  [ its loop is figure 2 ]
│
└─ POST-IMPLEMENTATION, PM's own work: clean up branches and
      worktrees, confirm the tree is clean, then answer the issue:
      close it, or open the PR
══════════════════════════════════════════════════════════════════════
a separate flow, many commits later, outside all of the above:
   tag ──► CI ──► release
```

**Triage.** This happens in the product manager's workspace, before anything reaches this repo, and it is the only place a request is turned down. First the work the issue actually needs: reproduce the bug, or confirm a reported regression is attributed to the right cause. Then two questions answered against something written rather than re-argued. Does a unit own this concern? The map says, and a request that duplicates one is a routing answer, not a feature. Does a clause forbid it? [`SPEC.md`](SPEC.md) says, and only in that direction: a capability needs no clause admitting it. Nothing in `.githooks/` reads it: a clause is applied by judgment, never by a gate.

**Plan.** Planning against the rubric you will be judged by makes the review a checklist already satisfied, not a late verdict. PM then reviews the plan adversarially and sends it back until it holds, for the same reason the code is reviewed: PLANNER is the worst judge of its own plan. The plan is never written into the repo, and gitignoring it would not help, because `ls`, `find` and `grep` surface it either way. Staleness is not the real cost. A plan states its premises as fact, some of them are wrong, and an agent that reads one adopts them as constraints. In a review cycle that compounds: a reviewer disproves a premise, the file goes on asserting it, and the next round picks it up again. A written claim outlives its own correction, and misleading text degrades a model more than absence does (Macke & Doyle, 2024). The plan lives in context, or in a workspace the agent is not working in.

**Post-implementation.** PM's own work again, once MAINTAINER has pushed. Delete the branch and any worktree the run created, confirm the tree is clean, then answer the issue: close it, or open the PR. Scaffolding an agent leaves behind is litter in the next agent's map.

### Maintainer agent workflow

```
MAINTAINER's loop. It runs inside PRODUCT MANAGER's scope (figure 2 is
figure 1's black box); everything below is spawned BY MAINTAINER and
reports back TO it.

MAINTAINER ─ owns the change end to end, through commit and push.
│
├─ on MAJOR: spawns a NEW agent to re-plan the fix,           ◄──────┐
│     then the work is implemented and reviewed afresh               │
│                                                                    │
├─ spawns, repeatedly, many, short-lived:                            │
│  ├─ investigate ─ returns a conclusion, not a transcript           │
│  └─ implement ─── leaves work in the tree     ◄──────┐             │
│                                                      │             │
├─ {{ pre-commit hook }} MACHINE, not an agent:        │             │
│     presence · form · budget ── fail ────────────────┘             │
│                                                                    │
├─ spawns REVIEWERS, in strict sequence, never parallel;  ◄───┐      │
│  fresh context each, none of them the author:               │      │
│     1. A  standards                                         │      │
│     2. B  annotations   judges what A may still change      │      │
│     3. C  prose         conditional: only when a            │      │
│                         human-facing doc is in the diff     │      │
│                                                             │      │
├─ verdict ┬─ MAJOR ─── not MAINTAINER's to fix; re-plan ─────┼──────┘
│          ├─ MODERATE  MAINTAINER fixes, re-review ──────────┘
│          └─ MINOR ─── never blocks; fixed, or left knowingly
│
└─ commit ─ commit-msg hook verifies the attestation ─► push to branch
```

**Dispatch.** MAINTAINER dispatches every unit of work, investigations included, and gets conclusions back rather than transcripts. Its own context goes on the one job nothing else can do, dispatching REVIEWERS and owning the change through push.

That thread cannot be delegated. An agent under a commit-and-self-verify brief once reported three reviews complete before any had run, invented the counts, and wrote them into the trailers; the gate passed, because a hook only checks that a trailer is well formed. **Whoever needs a verdict spawns the reviewer.** A subagent's summary of reviews it spawned itself is where a fabricated one hides.

**Requirements arrive just in time.** MAINTAINER is never briefed on the review process. It is told to implement a plan, it does, and it attempts to commit. That commit fails by design, and each gate prints what it wants:

- A failing annotation check prints the annotation guide inline.
- The comment-budget gate prints the comment standard.
- `commit-msg` prints the section of the reviewer prompt the failing gate needs, plus the attestation shape.

Nothing can drift, because the requirement is printed by the file that defines it rather than copied into a briefing. And until a gate asks, MAINTAINER's context goes on the work rather than on process it has not reached. If the plan was written against the standards to begin with, the delta the gates ask for is small.

**Gate.** Presence and form, never truth. Coverage is the product: partial coverage keeps little of the benefit, because the slow read-the-source path stays alive for whatever is missing.

**Review.** How many reviews, and what each judges, follows what the repo has to protect; ours runs three. The commit hook runs none of them. It checks the report's form and blocks on any MAJOR or MODERATE the report declares, but it cannot check that a review happened at all, which buys attribution rather than enforcement: a false attestation passes, and is then a claim with a name on it. Form is deterministic so the machine owns it; truth is judgment so a REVIEWER owns it.

See `annotated-tree --githook-guide` for what each hook demands and the exact attestation shape.

- **A, standards.** Every principle answered, `N/A — reason` included. Self-selecting which ones a change could plausibly breach is how the one that mattered gets dropped.
- **B, annotations.** The linter proves existence, a reader proves truth, and a wrong annotation does more damage than a missing one. Its file list comes from git, never from MAINTAINER.
- **C, prose.** Every claim checked against the built artifact. Conditional: it fires only when a human-facing doc is in the diff.

In order, not parallel: B judges annotations against content A may still be changing. Fresh context and no hints, because naming what changed tells a REVIEWER what counts and it stops looking. To re-review, say only `re-review`.

**Scope is not a reviewer's question.** It was settled before a plan existed, and a reviewer sees neither the issue nor the case for the change; an observation goes back as one MINOR line to the product manager, never as grounds to re-plan.

**The brief carries one thing: INTENT.** What the change set out to do, stated flatly, as a spec would state it: no reason it is worth doing, no defence of the approach, no account of what it replaces. This is most of what stands between a review and a rubber stamp: a reviewer handed a case for the change grades the case. A stricter regime exists, escrowing the intent before the work starts, but writing it at review time has held up well enough here that the extra ceremony has not earned its place.

**Commit.** A commit editing a rubric it is judged against lands alone, unreviewed, by explicit bypass. Judging a yardstick against itself is circular.

## Tuning the review loop

A review-and-fix loop has to be *tuned*. It commonly fails in two directions, and the band between them is narrow.

- **Too loose and nothing is caught.** The reviewer rubber-stamps. Prevented by handing it a flat statement of what the change set out to do and no case for why it is any good, leaving nothing to nod along with.
- **Too tight and nothing ever settles.** Every nit blocks, so the loop never converges. Prevented by a severity that is recorded but never blocks.

We found the balance by having the reviewer sort every finding it reports into three buckets:

- **MAJOR** — the work is wrong, or carries a severe flaw. An incremental fix is unlikely to reach the right answer. Graded by wrongness rather than by the size of the fix, so a one-line change to behaviour cannot land here as a nit.
- **MODERATE** — the outcome is right, the execution is not. Usually a standards violation, and it needs rework.
- **MINOR** — fixable without another look.

**The rule is one line: iterate until MAJOR and MODERATE are both zero.** 
That terminates cleanly because both blocking buckets mean the same thing: there is work the reviewer has not judged yet. When there is none left, the loop is over. MINOR is what makes it reachable, because a finding can be recorded without restarting the review. Nothing is suppressed and no reviewer is asked to pretend the code is clean; findings are declassified, never hidden, so the pressure stays on the code instead of on the report. The loop ends when the reviewer has seen everything that matters, not when it runs out of things to say.

**Two approaches we tried that failed**, both by never settling:

| What we asked for | What we got |
|---|---|
| "Report any issues you can find." | It always found one, however minuscule. No round came back empty. |
| "Stop when every criterion scores above 9/10." | Driving DbC past 9 pushed KISS under it, and the next round traded back. A score also lets a weak dimension hide behind strong ones. |

## Why the limits are tight

**The annotation and comment limits are tight on purpose.** An LLM is a text generator and will fill whatever space you leave it. A limit you can always meet gets spent on words; one you cannot meet gets spent on thinking. Bound the annotation hard and the writer has to stack-rank what matters, or say the same thing one altitude up.

Past that, the bound stops being editorial and becomes a design detector:

- A line that will not fit at the right altitude means the file owns two jobs.
- A function that trips the comment limit needed extracting, not better comments.

**Never raise a threshold to pass.** The bound is the detector, and a bigger number only hides what it found.

Ours are a character cap per annotation, checked by the linter, and a comment ratio checked by a separate tool.

## Implementing this automation in an existing repo

**Step 1: make the mechanical checks pass.** Lint and tests green. Annotate every file and put `annotated-tree --strict-check` over it. Add a comment budget: ours ([`.cargo-lint-extra.toml`](.cargo-lint-extra.toml)) caps consecutive comment lines at one inside a function body and four for a doc run, and holds a file to 30% comments, 20% inside function bodies.

Do this with the gates off. A repo that has never carried annotations fails the annotation check on its first commit, and clearing that backlog one reviewed commit at a time is the slowest route there is. The gates are built to hold a state, not to reach one.

False greens, all three seen here:

- The gate grades your built artifact, so rebuild before every attempt.
- A cached build can link stale embedded content and assert against text you already changed.
- `cmd | tail` returns `tail`'s exit code, not the command's.

**Step 2: stand up the neutral reviewers.** Write a standards document, or borrow [ours](docs/repo-standards.md). Write the prompt each reviewer is handed. Install the hooks that block a commit with no attestation, which is also what teaches the maintainer the review process.

Settle the standards before the hooks go on. A gate pointed at a rubric nobody has agreed with produces findings nobody acts on, and people stop reading the reviews.

**Stop here.** Step 2 is everything the MAINTAINER loop needs. Run it until it can implement most changes to your quality bar on its own, doing the product manager's job by hand meanwhile: triage the issue yourself, write the plan yourself, hand it over. An unstable maintainer loop botches or over-engineers even a good plan, so automating the planning above it buys nothing until the execution under it can be trusted.

**Step 3: set up the product manager workspace.** A private layer above the repo, so plans and experiments never land in it:

```
my-project-pm/          # the PM's workspace, private, never published
├── my-project/         # the repo itself, as a git submodule
├── plans/              # one work-order per change, moved aside once executed
├── references/         # rubrics and standards the reviews are judged against
└── experiments/        # throwaway spikes; the deliverable is a conclusion
```

Add a `SPEC.md` to the repo at this point, not to the workspace, and keep it [intentionally under-specified](SPEC.md). Triage reads it from the workspace, but it lives with the code it constrains, so every checkout carries the invariants.

Writing it is the same balancing act as tuning the review loop. Under-specify and anything goes; over-specify and agents begin rejecting good requests by citing a clause that was never meant to carry the weight. We keep it deliberately under-specified, so that anything no clause forbids is admissible, for the same reason a comment restating the code earns nothing. Two things earn a clause and nothing else does:

- Something certain and permanent. "No x86-only dependency" in a library that has to run on ARM.
- Something you have had to correct an agent on more than once. Usually that is where the repo does something unusual, like deliberately not testing a component whose shape is still in flux.

**Never let an agent derive the spec from the implementation.** It reads as free documentation. It is cement poured over whatever the code happens to do today, and every principled refactor afterwards gets fought on the grounds that it goes against spec.

Commission the whole thing slowly, with the supervision dialled down only as evidence comes in. Read the early reviews yourself and check the verdicts against the diff. The tuning that makes the loop converge is repo-specific, so the first rounds are how you find yours. Once a stage keeps earning its verdict, let it run unattended.
