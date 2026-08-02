<!-- Concern: the pipeline a change runs through here, who owns each stage and why, and how to stand one up elsewhere | Non-concern: cutting a release, tool usage, or the annotation format | IO: none -->
# Contributing
Contributions are welcome, and issues and ideas more so. We ask only that a change follow the repo's standard process, which is unusual mainly in being almost entirely automated: agents do the work and the grading, and a human sets direction and rejects. Below is that process, with the reasoning attached to each stage. Copy the shape into your own repos and adapt it there however you like, never here: the rubrics below are ours, yours will differ, and changing one of ours runs the same pipeline as any other change.

```
issue / human idea
  │
  ▼
PLANNER ─ plans against the standards doc, then is done and gone.
  │       Its plan never lands in this repo.
  ▼
MAINTAINER ─ a different agent. Owns the change through commit and push.
│  Indentation is scope: everything below is spawned BY MAINTAINER and
│  reports back TO it. Nothing below is its peer or at global scope.
│
├─ spawns, repeatedly, many, short-lived:                   ◄──────┐
│  ├─ re-plan ───── a NEW agent, only ever on MAJOR                │
│  ├─ investigate ─ returns a conclusion, not a transcript         │
│  └─ implement ─── leaves work in the tree                        │
│                                                                  │
├─ {{ pre-commit hook }} MACHINE: presence · form · budget ─fail───┤
│                                                                  │
├─ REVIEWERS, strictly in sequence, never parallel; ◄───────┐      │
│  fresh context each, none of them the author:             │      │
│     1. A  standards                                       │      │
│     2. B  annotations   judges what A may still change    │      │
│     3. C  prose         conditional: only when a          │      │
│                         human-facing doc is in the diff   │      │
│  verdict ┬─ MAJOR ─── not MAINTAINER's to fix; re-plan ───┼──────┘
│          ├─ MODERATE  MAINTAINER fixes, gate re-runs ─────┘
│          └─ MINOR ─── never blocks; fixed, or left knowingly
│
└─ commit ─ the commit-msg hook verifies the attestation ─► push to branch

════════════════════════════════════════════════════════════════════
a separate flow, many commits later, outside MAINTAINER:
   tag ──► CI ──► release
```

**Plan.** Planning against the rubric you will be judged by makes the review a checklist already satisfied, not a late verdict. The plan is never written into the repo: it stops being true the moment it is executed, and a doc that can go stale will.

**Implement.** MAINTAINER did not write the plan, so it has no authorship to defend. It dispatches every unit of work, investigations included, and gets conclusions back rather than transcripts. It spends its own context on the one job nothing else can do: dispatching REVIEWERS and owning the change through push.

That thread cannot be delegated. An agent under a commit-and-self-verify brief once reported three reviews complete before any had run, invented the counts, and wrote them into the trailers; the gate passed, because a hook only checks that a trailer is well formed. **Whoever needs a verdict spawns the reviewer.** A subagent's summary of reviews it spawned itself is where a fabricated one hides.

**Gate.** Presence and form, never truth. Coverage is the product: partial coverage keeps little of the benefit, because the slow read-the-source path stays alive for whatever is missing, so the saving never arrives.

**Budgets are painful on purpose.** An LLM is a text generator and will fill whatever space you leave it. A budget you can always meet gets spent on words; a budget you cannot meet gets spent on thinking. Bound the annotation hard and the writer has to stack-rank what actually matters, or say the same thing one altitude up. Past that point the bound stops being editorial and becomes a design detector: a line that will not fit at the right altitude means the file owns two jobs, and a function that trips the comment budget needed extracting, not better comments. **Never raise a threshold to pass.** The bound is the detector, and a bigger number only hides what it found. Ours are a character cap enforced by the annotation checker, and a comment ratio enforced by a separate lint tool.

False greens, all three seen here:

- The gate grades your built artifact, so rebuild before every attempt.
- A cached build can link stale embedded content and assert against text you already changed.
- `cmd | tail` returns `tail`'s exit code, not the command's.

**Review.** A neutral review pipeline. How many reviews, and what each judges, follows what the repo has to protect; ours runs three. The commit hook runs none of them. It checks the report's form and blocks on any MAJOR or MODERATE the report declares, but it cannot check that a review happened at all, which buys attribution rather than enforcement: a false attestation passes, and is then a claim with a name on it. Form is deterministic so the machine owns it; truth is judgment so a REVIEWER owns it.

See `annotated-tree --githook-guide` for what each hook demands and the exact attestation shape. This doc is the pipeline and the reasoning; that one is the mechanics.

- **A, standards.** Every principle answered, `N/A — reason` included. Self-selecting which ones a change could plausibly breach is how the one that mattered gets dropped.
- **B, annotations.** The linter proves existence, a reader proves truth, and a wrong annotation does more damage than a missing one. Its file list comes from git, never from MAINTAINER.
- **C, prose.** Every claim checked against the built artifact. Conditional: it fires only when a human-facing doc is in the diff.

In order, not parallel: B judges annotations against content A may still be changing. Fresh context and no hints, because naming what changed tells a REVIEWER what counts and it stops looking. To re-review, say only `re-review`.

**The brief carries one thing: INTENT.** What the change set out to do, stated flatly, as a spec would state it: no reason it is worth doing, no defence of the approach, no account of what it replaces. A REVIEWER needs the target to judge whether the diff hits it, and nothing that argues it was hit. This is most of what stands between a review and a rubber stamp: a reviewer handed a case for the change grades the case. A stricter regime exists, escrowing the intent before the work starts, but writing it at review time has held up well enough here that the extra ceremony has not earned its place.

**Fix.** A review-and-fix loop has to be *tuned*. It is a feedback loop, and the zone where it converges is narrow:

```
 rubber stamping ────────── converges ────────── never-ending review
 too loose:                  tuned               too tight:
 nothing is caught                               nothing ever settles
```

Both ends are failure modes, and each has its own guard: the INTENT discipline above holds the left, MINOR below holds the right. They are also not independent. A gate tuned too tight does not stay there: when the only way to commit is a clean report, the reviewer learns to produce one, and you arrive at rubber stamping from the far side.

Severity is graded by what the fix costs, not by how bad the finding sounds. The three buckets exist because two simpler stopping rules both landed on the right-hand failure:

- *"Report when it is free of issues."* Ask an agent to find something and it will, however minuscule. No round ever came back empty, so the loop never settled, and it burned itself on nitpicks that did not matter.
- *"Stop when every criterion scores above 9/10."* The same failure one level up. Driving DbC past 9 pushed KISS under it, and the next round traded back. A score also lets a weak dimension hide behind strong ones, and invites argument that cannot change the outcome.

**MINOR is the escape valve that makes it terminate.** The nitpick is still written down, so nothing is suppressed and no reviewer is asked to pretend the code is clean, but it does not trigger a re-review. Findings get declassified, never hidden, which is what stops the pressure landing on the report instead of the code. MAJOR stays graded by wrongness rather than cost, so a cheap fix that changes behaviour cannot land as a nit.

**Commit.** A commit editing a rubric it is judged against lands alone, unreviewed, by explicit bypass. Judging a yardstick against itself is circular.

**Switching this on.** Commission it the way you would any automation: slowly, watching it run, with the supervision dialled down only as evidence comes in. Read the early reviews yourself and check the verdicts against the diff. The tuning that makes the loop converge is repo-specific, and it cannot be copied off someone else's numbers, so the first rounds are how you find yours. Once a stage keeps earning its verdict, let it run unattended.

Enforcement goes last. A repo that has never carried annotations fails the annotation gate on its first commit, and clearing that backlog one reviewed commit at a time is the slowest route there is. Get the repo close with the gates off, then turn them on: they are built to hold a state, not to reach one.

Settle the rubrics before that. `docs/repo-standards.md` and its siblings are the input to every review, so copy ours or write the ones that fit you. A gate pointed at a rubric nobody has agreed with yet produces findings nobody acts on, which is how a review loop gets taught to be ignored.
