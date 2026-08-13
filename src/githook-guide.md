<!-- Concern: documents this repo's commit gates — what each checks and where its wiring lives | Non-concern: the flags, thresholds and rubrics themselves, held by the tools and their configs | IO: none -->
GITHOOK GUIDE — reproduce the two local hooks that keep the map from rotting.

Enforce at COMMIT, in a LOCAL hook, never CI: the hook blocks the bad commit while the agent
still has the context to fix it; CI only flags it after the session is gone. Ship both hooks
under `.githooks/`, enable per clone with `git config core.hooksPath .githooks`, and bypass a
genuinely trivial, non-code commit with `git commit --no-verify`. Mechanical checks first, review
last: a review is wasted on a file that has no annotation at all.

PRE-COMMIT — mechanical, deterministic (never a judgment about meaning)
  Annotations. Run BOTH invocations over the repo, and fail on either:

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

  Comment budget, after the annotation check. A bound on comment VOLUME. Ours is
  `cargo-lint-extra` from
  https://github.com/fredrikolis/cargo-lint-extra, pinned to a rev and configured by
  `.cargo-lint-extra.toml` — Rust-only, so substitute your own.

COMMIT-MSG — semantic, attestation-based (quality + staleness)
  The hook runs NO reviewer, and neither does the dev agent. `git-agent-verdict` dispatches each
  gate's reviewer, records what it reported, and makes the commit.

  One pointer, and this file carries no other. Install it once:

      cargo install git-agent-verdict

  then read the wiring from the tool itself, which is where it stays current:

      git-agent-verdict --repo-setup-guide

  Reproducing any of it here would give the measure a second copy, free to drift.

  What is repo-specific is which reviews you declare, and in what order. That is the whole of
  what this repo contributes; the setup guide carries the shape.

WHY THIS SHAPE
  Render, don't reason: presence/form is deterministic, so it is HARD-GATED by the tool; truth and
  quality are judgment, so they are ATTESTED by a reviewer — the hook verifies the attestation, it
  never makes the semantic call itself.
