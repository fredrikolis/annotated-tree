<!-- Concern: the canonical guide to reproducing the repo's local enforcement git hooks | Non-concern: running the hooks (.githooks/ owns that) or the annotation format | IO: none -->
GITHOOK GUIDE — reproduce the two local hooks that keep the map from rotting.

Enforce at COMMIT, in a LOCAL hook, never CI: the hook blocks the bad commit while the agent
still has the context to fix it; CI only flags it after the session is gone. Ship both hooks
under `.githooks/`, enable per clone with `git config core.hooksPath .githooks`, and bypass a
genuinely trivial, non-code commit with `git commit --no-verify`. Two gates: gate 1 runs first,
because gate 2's review is wasted on a file that has no annotation at all.

PRE-COMMIT — mechanical, deterministic (presence + form only)
  Run BOTH invocations over the repo, and fail on either:

      annotated-tree --strict-check . --max-length 200
      annotated-tree --strict-check . --include-tests -I 'tests/fixtures' --max-length 200

  The second exists because a directory named `tests` is pruned unless `--include-tests`, so
  without it `tests/` is invisible to the check and a bare test file still reports all files
  passed. `--max-length 200` bounds the whole annotation; `-I`/`--ignore` any fixture dir whose
  annotations are deliberately loose. Prefer a built binary (`target/release`, then
  `target/debug`), fall back to `cargo run --quiet --` so a fresh clone still gates. On a nonzero
  exit, print what failed and exit 1. This checks that an annotation EXISTS and PARSES, never
  that it is true.

COMMIT-MSG — semantic, attestation-based (quality + staleness)
  The hook runs NO reviewer, and skips auto-generated messages (`Merge`/`Revert`/`fixup!`/`squash!`).
  The dev agent runs a neutral review ITSELF — a reviewer distinct from the author — and writes the
  verdict into the commit message; the hook only verifies that attestation is present and clean.

  Trust model: the gate verifies that an attestation is PRESENT and WELL-FORMED; it cannot verify
  that a review happened. What it buys is ATTRIBUTION, not enforcement — a false attestation
  passes, but it is then a claim on the record with a name on it.

  Circular-standards guard. A diff that edits the rubric doc the review is judged against is
  blocked: it lands alone via `--no-verify`. Judging a yardstick against itself is circular.

  Gate A — standards review. Require a non-empty `Reviewer:` line; one `- <Principle>: ` line per
  rubric principle (the judgment-call subset of your standards doc) whose payload is `none`,
  `N/A — reason`, or a severity plus the finding; and `MAJOR: <n>`, `MODERATE: <n>`, `MINOR: <n>`
  counts, each read from the LAST match so body prose cannot shadow the trailer.
    MAJOR — violates an AUTO-REJECT blocker, or breaks a stated contract or invariant.
    MODERATE — a real principle violation that must be fixed, but breaks nothing already shipped.
    MINOR — a nit; fixing it is the author's discretion.
  Iterate fix -> re-review until no MAJOR and no MODERATE remains. A missing line, a blocker above
  0, or a line whose severity contradicts a declared 0, fails. `MINOR:` is required and NEVER
  gated, so a real nit has a home instead of being inflated. A severity, not a score: it drives the
  weak dimension to zero directly, where a number lets it hide behind strong ones and invites
  argument that cannot change the outcome.

  Gate B — annotation review. Require `Annotation-Reviewer: <name>` + `Annotation-Issues: 0`: a
  neutral reviewer confirmed every file in the diff carries an APPROPRIATE annotation and that the
  diff did not make it STALE — the truth + staleness check a linter cannot make. APPROPRIATE: every
  field states WHAT, never why/how/when; Concern is the file's ONE job; Non-concern is a concern it
  does not own, its where-it-lives pointer optional (omit it when the tree shows the owner anyway).

  Gate C — conditional style review. Only when a human-facing doc is in the diff, require
  `Style-Reviewer:` + `Style-Issues: 0` against your prose-style doc, from a fresh-context
  reviewer that did not write the change. A reminder gate: a hook cannot stop a determined
  agent from rubber-stamping, so make it print the exact reviewer prompt.

WHY THIS SHAPE
  Render, don't reason: presence/form is deterministic, so it is HARD-GATED by the tool; truth and
  quality are judgment, so they are ATTESTED by a reviewer — the hook verifies the attestation, it
  never makes the semantic call itself. Attestation keys are an API — grep them, don't read prose.
