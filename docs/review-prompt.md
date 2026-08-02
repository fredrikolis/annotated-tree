<!-- Concern: the canonical prompt a neutral reviewer is handed, and what its severities mean | Non-concern: which gate demands a review, or what a passing attestation looks like | IO: none -->
NEUTRAL REVIEW — hand a reviewer in a FRESH context exactly the block below and nothing else.

The diff is the only signal it gets about where to look. Naming what you changed, or what you
suspect, tells it what counts, and it will find that and stop looking. To re-review, update the
diff and say only `re-review`.

  ----------------------------------------------------------------------
  INTENT: <what the diff sets out to do, under 1000 characters. State the aim flatly, as a
  spec would: no reason it is worth doing, no defence of the approach, no account of what it
  replaces. The reviewer needs the target to judge whether the diff hits it, and nothing that
  argues it was hit.>

  Review the staged diff (git diff --cached) against docs/repo-standards.md.
  Give EVERY principle in its summary table one line, plus its AUTO-REJECT list:
  '- <Principle>: none | N/A — <why> | <SEVERITY> — <finding>'.
  MAJOR    = an unintended consequence, a bug, or a solution that is wrong.
  MODERATE = the solution is right but breaches a standard; it needs rework.
  MINOR    = polish, a quick fix, a nice-to-have.
  End with 'MAJOR: <n>', 'MODERATE: <n>', 'MINOR: <n>'. Do not pad the count.
  ----------------------------------------------------------------------

EVERY principle, never a subset: choosing in advance which ones a change could breach is how the
one that matters gets dropped. `N/A — reason` is the answer where a principle does not apply.

Iterate fix -> re-review until no MAJOR and no MODERATE remains. MINOR never blocks — it is the
author's discretion, fixed or consciously left, so a real nit has a home instead of being inflated.
