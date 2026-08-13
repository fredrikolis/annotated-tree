<!-- Concern: the canonical guide to reproducing the repo's local enforcement git hooks | Non-concern: what a gate checks (git-agent-verdict owns that) or the annotation format | IO: none -->
GITHOOK GUIDE — reproduce the two local hooks that keep the map from rotting.

Enforce at COMMIT, in a LOCAL hook, never CI: the hook blocks the bad commit while the agent
still has the context to fix it; CI only flags it after the session is gone. Ship both hooks
under `.githooks/`, enable per clone with `git config core.hooksPath .githooks`, and bypass a
genuinely trivial, non-code commit with `git commit --no-verify`. Two gates: gate 1 runs first,
because gate 2's review is wasted on a file that has no annotation at all.

PRE-COMMIT — mechanical, deterministic (presence + form only)
  Run BOTH invocations over the repo, and fail on either:

      annotated-tree --strict-check . --hidden --max-length 200
      annotated-tree --strict-check . --hidden --include-tests -I 'tests/fixtures' --max-length 200

  The second exists because a directory named `tests` is pruned unless `--include-tests`, so
  without it `tests/` is invisible to the check and a bare test file still reports all files
  passed. `--hidden` is there for exactly that reason at the other end of the tree: a
  dot-directory — `.github`, `.githooks`, `.claude` — is pruned without it, so nothing beneath one
  is walked at all. With it, the hook you are writing right now is checked like any other file: an
  extensionless script resolves its language from its `#!` line, and a workflow from `.yml`.
  Expect the first run to fail on hooks nothing had ever checked. `.git` is never walked either
  way. `--max-length 200` bounds the whole annotation; `-I`/`--ignore` any fixture dir whose
  annotations are deliberately loose. Prefer a built binary (`target/release`, then
  `target/debug`), fall back to `cargo run --quiet --` so a fresh clone still gates. On a nonzero
  exit, print what failed and exit 1. This checks that an annotation EXISTS and PARSES, never
  that it is true.

COMMIT-MSG — semantic, attestation-based (quality + staleness)
  The hook runs NO reviewer, and neither does the dev agent. The commit is:

      git agent-verdict attest --intent "<the aim, one flat line>"

  That runs each gate's reviewer through the host's configured runner, one gate per run in
  declaration order, and commits once every gate is attested. Nothing is handed back to paste.

  The runner is HOST configuration, not the repo's — maintainers share neither a machine, a budget,
  nor a preferred agent:

      git config --global agent-verdict.runner "<command reading a brief on stdin>"

  This repo implements none of that. `git-agent-verdict` owns it, and its README owns the
  trailer format, the severity ladder, the trust model, and the scoping flags:

      https://github.com/fredrikolis/git-agent-verdict

  Adopters install it once (`cargo install git-agent-verdict`) and name their runner once per
  machine. The hook then pins the LINE its flags are written against, which replaces any
  hand-rolled version check:

      git agent-verdict --require-version <major.minor>

  What stays here is only what is repo-specific. First, the circular-rubric guard, DECLARED and
  not automatic: judging a change to a rubric against that same rubric is circular, so staging one
  of these docs refuses the commit and it lands alone via `--no-verify`. Name in-repo docs only — a
  rubric outside the worktree can never be staged, so it never needs guarding:

      git agent-verdict --rubric-guard --doc <your standards doc> --doc <your annotation guide>

  Second, the gate declarations — one call per review, in review order, and nothing else:

      git agent-verdict "$1" standards --doc <your standards doc> --path .
      git agent-verdict "$1" annotations --doc <your annotation guide> --path .
      git agent-verdict "$1" prose --doc <your prose-style doc> --path README.md

  Substitute your own paths. Every `--doc` must resolve and every literal `--path` must name
  something git tracks, or the gate exits 2 rather than passing. A `--doc` may sit OUTSIDE the
  worktree — a rubric shared across repos from a knowledge base is the case this serves, and
  copying it in to satisfy the gate would give the measure a second copy, free to drift. Add
  `--simple` to a gate to make it advisory: it counts findings and never blocks.

  Line order IS review order, and `set -e` stops at the first unattested gate on purpose: a later
  gate must never be judged against content an earlier one is still changing.

WHY THIS SHAPE
  Render, don't reason: presence/form is deterministic, so it is HARD-GATED by the tool; truth and
  quality are judgment, so they are ATTESTED by a reviewer — the hook verifies the attestation, it
  never makes the semantic call itself.
