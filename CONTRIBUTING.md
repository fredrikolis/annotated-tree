<!-- Concern: how this repo's automated development process works, and how to stand one up elsewhere | Non-concern: cutting a release, using annotated-tree itself, or the annotation format | IO: none -->
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
├─ spawns PLANNER ─ plans against the declared standards.   ◄─────┐
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

**Triage.** The only place a request is turned down, and it happens before anything reaches this repo. Both questions are answered against something written rather than re-argued: a request a unit already owns is a routing answer, not a feature, and [`SPEC.md`](SPEC.md) forbids in one direction only, since a capability needs no clause admitting it. No gate reads it, so a clause is applied by judgment.

**Plan.** Planning against the rubric you will be judged by makes the review a checklist already satisfied, not a late verdict. PLANNER is the worst judge of its own plan, so PM returns it until it holds, then approves it.

### Maintainer agent workflow

```
MAINTAINER's loop. It runs inside PRODUCT MANAGER's scope (figure 2 is
figure 1's black box); MAINTAINER spawns everything below it except the
REVIEWERS, which the commit step dispatches.

MAINTAINER ─ owns the change end to end, through commit and push.
│
├─ spawns, repeatedly, many, short-lived:
│  ├─ investigate ─ returns a conclusion, not a transcript
│  └─ implement ─── leaves work in the tree     ◄──────┐
│                                                      │
├─ {{ pre-commit hook }} MACHINE, not an agent:        │
│     presence · form · budget ── fail ────────────────┘
│
├─ commits through `git-agent-verdict`. IT, not MAINTAINER,
│     dispatches the REVIEWERS through the host's runner; fresh
│     context each, none of them the author
│
├─ on a MAJOR: a NEW agent re-plans the fix, which is implemented
│     and reviewed afresh. MAINTAINER fixes lesser findings itself
│
└─ the tool commits ─► push to branch
```

**Requirements arrive just in time.** MAINTAINER is never briefed on the review process. It is told to implement a plan, it does, and it attempts to commit. That commit fails by design, and each gate prints what it wants.

Nothing can drift, because the requirement is printed by the file that defines it rather than copied into a briefing. Until a gate asks, MAINTAINER's context goes on the work.

**Gate.** Presence and form, never truth. Coverage is the product: partial coverage keeps little of the benefit, because the slow read-the-source path stays alive for whatever is missing.

**Review.** Use [git-agent-verdict](https://github.com/fredrikolis/git-agent-verdict). The commit hook runs no reviewer: the tool dispatches each one, records what came back, and makes the commit; the hook checks what was recorded. It owns the severity ladder, the trailer shape and the trust model, and the case for all three.

[`.githooks/commit-msg`](.githooks/commit-msg) declares which reviews this repo runs, and in what order. Run `git agent-verdict --reviewer-prompt <gate>` for a gate's live brief, and `annotated-tree --githook-guide` for the hook wiring. A second copy here would drift, and did.

**Scope is not a REVIEWER's question.** A scope observation comes back to the product manager as one MINOR line, never as grounds to re-plan.

## Why the limits are tight

An LLM is a text generator and will fill whatever space you leave it. A limit you can always meet gets spent on words; one you cannot meet gets spent on thinking. Bound the annotation hard and the writer has to stack-rank what matters, or say the same thing one altitude up.

Past that, the bound stops being editorial and becomes a design detector:

- A line that will not fit at the right altitude means the file owns two jobs.
- A function that trips the comment limit needed extracting, not better comments.

**Never raise a threshold to pass.** The bound is the detector, and a bigger number only hides what it found. This covers the annotation cap as much as the comment budget: the hook hardcodes both, and editing either is how the detector gets switched off.

## Implementing this automation in an existing repo

**Step 1: make the mechanical checks pass.** Lint and tests green. Annotate every file and put `annotated-tree --strict-check` over it. Add a comment budget; ours is [`.cargo-lint-extra.toml`](.cargo-lint-extra.toml).

Do this with the gates off. A repo that has never carried annotations fails the annotation check on its first commit, and clearing that backlog one reviewed commit at a time is the slowest route there is. The gates are built to hold a state, not to reach one.

False greens, all three seen here:

- The gate grades your built artifact, so rebuild before every attempt.
- A cached build can link stale embedded content and assert against text you already changed.
- `cmd | tail` returns `tail`'s exit code, not the command's.

**Step 2: stand up the neutral reviewers.** `git agent-verdict --repo-setup-guide` carries the wiring: declaring gates, the per-host runner, and committing through the tool.

Settle the standards before the hooks go on. A gate pointed at a rubric nobody has agreed with produces findings nobody acts on, and people stop reading the reviews.

**Stop here.** Step 2 is everything the MAINTAINER loop needs. Run it until it can implement most changes to your quality bar on its own, doing the product manager's job by hand meanwhile.

An unstable maintainer loop botches or over-engineers even a good plan, so automating the planning above it buys nothing until the execution under it can be trusted.

**Step 3: set up the product manager workspace.** A private layer above the repo, so plans and experiments never land in it:

```
my-project-pm/          # the PM's workspace, private, never published
├── my-project/         # the repo itself, as a git submodule
├── plans/              # one work-order per change, moved aside once executed
├── references/         # rubrics and standards the reviews are judged against
└── experiments/        # throwaway spikes; the deliverable is a conclusion
```

Keeping plans out is the point. A plan states its premises as fact, some of them are wrong, and a reviewer who disproves one cannot stop the file asserting it to the next agent that reads it. Gitignoring does not help, because `ls`, `find` and `grep` surface it anyway.

Add a `SPEC.md` to the repo at this point, not to the workspace, and keep it [intentionally under-specified](SPEC.md). It lives with the code it constrains, so every checkout carries the invariants. Under-specify and anything goes; over-specify and agents begin rejecting good requests by citing a clause that was never meant to carry the weight. Two things earn a clause and nothing else does:

- Something certain and permanent. "No x86-only dependency" in a library that has to run on ARM.
- Something you have had to correct an agent on more than once. Usually that is where the repo does something unusual, like deliberately not testing a component whose shape is still in flux.

**Never let an agent derive the spec from the implementation.** It reads as free documentation. It is cement poured over whatever the code happens to do today, and every principled refactor afterwards gets fought on the grounds that it goes against spec.

Commission the whole thing slowly, with the supervision dialled down only as evidence comes in. Read the early reviews yourself and check the verdicts against the diff. The tuning that makes the loop converge is repo-specific, so the first rounds are how you find yours. Once a stage keeps earning its verdict, let it run unattended.
