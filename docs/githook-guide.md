<!-- Concern: the canonical guide to reproducing the repo's local enforcement git hooks, embedded and rendered by --githook-guide | Non-concern: running the hooks (the shipped .githooks/ scripts own that) or the annotation format itself (docs/annotation-guide.md owns it) | IO: none -->
GITHOOK GUIDE — reproduce the two local hooks that keep the map from rotting.

Enforce at COMMIT, in a LOCAL hook, never CI: the hook blocks the bad commit while the
agent still has the context to fix it; CI only flags it after the session is gone. Ship
both hooks under `.githooks/`, enable per clone with `git config core.hooksPath .githooks`,
and bypass a genuinely trivial, non-code commit with `git commit --no-verify`.

Two gates, in order. Gate 1 is mechanical presence; gate 2 is semantic quality. Gate 1
runs first because gate 2's review is wasted on a file that has no annotation at all.

PRE-COMMIT — mechanical, deterministic (presence + form only)
  Run `annotated-tree --strict-check .` over the repo. Prefer a built binary
  (`target/release`, then `target/debug`), fall back to `cargo run --quiet --` so a fresh
  clone still gates. `--ignore` any fixture dir whose annotations are deliberately loose.
  On a nonzero exit, print what failed and exit 1. This checks that an annotation EXISTS
  and PARSES — never whether it is true.

COMMIT-MSG — semantic, attestation-based (quality + staleness)
  The hook runs NO reviewer. The dev agent runs a neutral review ITSELF — a reviewer
  distinct from the author — and writes the verdict into the commit message; the hook only
  verifies that scorecard is present and clean. Grep for stable attestation KEYS, never
  parse prose. Skip auto-generated messages (`Merge`/`Revert`/`fixup!`/`squash!`).

  Circular-standards guard. If the diff edits the rubric doc the scorecard is graded
  against, block: that change must land on its own via `--no-verify`. Grading a change to
  the yardstick against itself is circular — the yardstick is in flux.

  Gate A — standards scorecard. Require a non-empty `Reviewer:` line; one
  `- <Category>: <0-10>/10 — justification` OR `- <Category>: N/A — reason` line per rubric
  category (the judgment-call subset of your standards doc); and `MAJOR: <n>` + `MEDIUM: <n>`
  counts, both 0. Any missing line, or any blocker above 0, fails.

  Gate B — annotation review. Require `Annotation-Reviewer: <name>` + `Annotation-Issues: 0`:
  a neutral reviewer confirmed every file in the diff carries an APPROPRIATE annotation and
  that this diff did not make it STALE. Presence/form is already gated by pre-commit; this
  is the truth + staleness check a linter cannot make.

  Gate C — conditional style review. Only when a human-facing doc is in the diff, require
  `Style-Reviewer:` + `Style-Issues: 0` against your prose-style doc, from a fresh-context
  reviewer that did not write the change. A reminder gate: a hook cannot stop a determined
  agent from rubber-stamping, so make it print the exact reviewer prompt.

WHY THIS SHAPE
  Presence/form is deterministic, so it is HARD-GATED by the tool. Truth and quality are
  judgment, so they are ATTESTED by a reviewer — the hook verifies the attestation, it never
  makes the semantic call itself. Render, don't reason: keep the judgment in the agent and
  the mechanical check in the hook. Attestation keys are an API — grep them, don't read prose.
