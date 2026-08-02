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

  Circular-rubric guard. A diff that edits any rubric a review is judged against — your standards
  doc, your annotation guide, your prose-style doc, the review prompt itself — is blocked: it lands
  alone via `--no-verify`. Judging a yardstick against itself is circular.

  ONE SHAPE, three gates. Each requires a named reviewer, one `- <item>: ` line per checklist item
  whose payload is `none`, `N/A — reason`, or a severity plus the finding, and `MAJOR`/`MODERATE`/
  `MINOR` counts, each read from the LAST match so body prose cannot shadow the trailer. A missing
  line, a blocker above 0, or a line whose severity contradicts a declared 0, fails. Iterate fix ->
  re-review until no MAJOR and no MODERATE remains. `MINOR` is required and NEVER gated, so a real
  nit has a home instead of being inflated into a blocker. A severity, not a score: it drives the
  weak dimension to zero directly, where a number lets it hide behind strong ones and invites
  argument that cannot change the outcome.

  Grade the three rungs by what the FIX costs, not by how bad the finding sounds — that is what
  makes the loop converge. MODERATE means the fix produces something the reviewer has not seen, so
  the review runs again; anything an agent can apply verbatim without a second look is MINOR and
  never blocks. MAJOR is the one rung graded by wrongness instead: it is re-planned by a neutral
  task agent, not patched in place, so a cheap fix that changes behaviour cannot land as a nit.

  The prompt each reviewer is handed, and what the three severities mean under it, live in ONE
  place — `docs/review-prompt.md`, which the hook prints on failure, section by gate.

  Gate A — standards review. `Reviewer:` + one line per principle in your standards doc, then
  `MAJOR: <n>`, `MODERATE: <n>`, `MINOR: <n>`.

  Gate B — annotation review. `Annotation-Reviewer:` + one line per file in the diff, then
  `Annotation-MAJOR: <n>`, `Annotation-MODERATE: <n>`, `Annotation-MINOR: <n>`. The reviewer
  confirms every file carries an APPROPRIATE annotation and that the diff did not make it STALE —
  the truth + staleness check a linter cannot make. The hook derives the file list from git, never
  from the author: an author-supplied list decides what gets looked at, and that is where a missed
  file hides.

  Gate C — conditional style review. Only when a human-facing doc is in the diff:
  `Style-Reviewer:` + one line per rule in your prose-style doc, then `Style-MAJOR: <n>`,
  `Style-MODERATE: <n>`, `Style-MINOR: <n>`, from a fresh-context reviewer that did not write the
  change. A reminder gate: a hook cannot stop a determined agent from rubber-stamping, so make it
  print the exact reviewer prompt.

WHY THIS SHAPE
  Render, don't reason: presence/form is deterministic, so it is HARD-GATED by the tool; truth and
  quality are judgment, so they are ATTESTED by a reviewer — the hook verifies the attestation, it
  never makes the semantic call itself. Attestation keys are an API — grep them, don't read prose.
