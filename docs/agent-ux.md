<!-- Concern: the agent-UX bar this tool's invocation surface is judged against, and the honesty rules for a signal an agent optimizes toward | Non-concern: CLI grammar, or the annotation format | IO: none -->
# Agent UX

`git agent-verdict --standards programming` carries the universal principles. This file carries
only what is specific to a tool whose primary user is an agent, and whose output that agent
optimizes against.

---

**When a tool's primary consumer is an AI agent, agent UX IS the UX. Any commit that touches the invocation surface — command syntax, flags, defaults, output, errors, exit codes, `--help` — is an agent-UX change, and its review carries a severity for whether an agent parses, trusts, and acts on it more reliably.**

An agent invokes the tool programmatically — it parses the output, branches on it, and pays a token/latency cost per call. The human reading the same run is the *dual-render* of one structured object, never a separate code path. Design for the agent first; the human view falls out for free. The test for every surface change: does it convert an act of inference into an act of reading? And the surface only ratchets forward — a regression in agent ergonomics is a blocker, not a tradeoff.

Beyond any single call, the compounding value is *comprehension without reading source* — the agent routes a change or judges a boundary from the rendered map, not by re-deriving it from the code. Optimize the surface so the agent trusts it enough to act *without* opening the files it summarizes; a map that still forces a source-read to resolve a routing question has leaked its job back to the code.

The objective behind all of it: **maximize the share of the agent's work that is productive** — output kept, not code caught in review and redone. Every wasted token — a wrong change, a re-read forced by an opaque surface — is the cost. Good agent UX raises that productive fraction toward the ceiling where nearly everything the agent emits is worth keeping; poor agent UX spends the agent's scarcest resource (context) on re-derivation and its output on work that gets thrown away.

**Core contract**:

- **Parseable** — structured data (JSON envelope) to stdout, progress/debug to stderr, never mixed
- **Unambiguous empties** — empty is a first-class value (`[]`, zero count), distinct from error and from not-found; one null convention, never mixed
- **Stable dispatch keys** — agents branch on a namespaced `code`, never on message prose; prose may change, codes are an API
- **Syntax is an API too** — a flag rename, output reshape, or default flip is a breaking change to unattended callers; it lands with its `--help`/schema/docs update in the same commit
- **Located, fixable diagnostics** — `code` + `location` (byte span and line:col) + `fix`, one object per finding, not one opaque error string that discards count, location, and remedy
- **Non-interactive** — nothing on the default path blocks; gate danger behind `--confirm`/`--yes`
- **Deterministic** — same input → same output; meaningful, consistent exit codes to branch on
- **Verdict-driven exit** — `status`/exit code follow the verdict (input rejected or not), never "any diagnostics present"; a warning on accepted input is not a failure
- **Token-economical** — dense, zero filler; context is the agent's scarcest resource, and noise degrades it, compounding across retry loops
- **Self-correcting `--help`** — usage, examples, output schema, exit codes, so an agent repairs its own call without a human

The contract above is universal; the concrete grammar — envelope shape, exit-code table, verb and flag conventions — is an interface-level concern, out of scope here.

| Pattern | Score | Notes |
|---------|-------|-------|
| New output path: structured, stdout-clean, dispatchable | +10 | Agent parses and branches reliably |
| Diagnostic carries `code` + `location` + `fix` | +9 | Agent applies, doesn't infer |
| Flag renamed, output reshaped, or default flipped without same-commit `--help`/schema/docs | -9 | Breaks unattended agent callers mid-run |
| One canonical object, dual-rendered to human + JSON | +8 | No second code path to drift |
| Human-only, unparseable output ("Done!") from an agent-first tool | -9 | Human-first regression; breaks the primary consumer |
| Agent forced to branch on message text | -8 | Brittle — prose drift breaks callers |
| Interactive prompt on the default path | -10 | Hangs autonomous execution |
| Warning flips the exit code on accepted input | -8 | Every warning halts unattended automation |
| Empty, null, error, and not-found conflated in output | -7 | Agent can't branch; forces a re-run or source-read |
| Progress/debug on stdout, corrupting the parse | -8 | Poisons the data stream |
| Tool makes the agent's semantic call (guesses whether a thing is true / right / dead) | -7 | Non-deterministic; can't be trusted to branch on, and invites scope-creep |

**Render, don't reason.** The tool's job is to make state *observable* — deterministically and cheaply — not to make the semantic judgments the agent exists to make (is this annotation *true*? does this change *belong*? is this code *dead*?). Keep the tool simple and the intelligence in the agent: a zero-inference, deterministic surface is more trustworthy to branch on than a "smart" one that can be wrong, and it keeps the tool's own scope honest. This is Separation of Concerns at the tool↔agent boundary — the complexity belongs in the agent, not the instrument.

## When the output is an optimization target (an agent's fitness function)

Some agent-first tools do more than report — their signal becomes something an agent *optimizes against*, closing a dev loop around it the way it closes one around tests (behavior) or types (contracts). When the output is a target, three properties decide whether the loop improves the real thing or just games the proxy (Goodhart's law):

- **Observable** — a machine-readable, dispatchable signal (stable `code`s + counts), never a human verdict the agent must interpret.
- **Convergent** — a *gradient*, not a pass/fail gate. Emit a distance-to-done (N of M, a decreasing violation count) so the agent knows it is getting warmer and can recognize *done*. A binary gate is a weak target; a slope is a strong one.
- **Goodhart-resistant** — satisfying the metric must *require* improving (or honestly reporting) the underlying property, as far as a deterministic check reaches. It does not reach meaning: a gate can confirm that a field is present, non-empty, and inside a length bound, never that what it says is worth reading. So scope the gate to form and route the meaning to a reviewer — filler that clears the form is a review finding, not a gate failure, and a gate that claims to catch it is claiming coverage it does not have. Reward **honesty over tidiness** — surface the real state (dead code, cycles, overlapping responsibilities), never a description that conceals a mess; honest overlap between two units is an architecture finding (keep/move/delete), not something to reword away. Anchor the signal in what the tool *observes* about the system and cross-check it against what the code *self-reports*: the discrepancy between claim and reality is the least gameable signal of all.

| Pattern | Score | Notes |
|---------|-------|-------|
| Metric is a gradient with an explicit distance-to-done | +9 | Agent can converge and know when finished |
| Signal anchored in observed facts, cross-checked vs self-report | +10 | Discrepancy is un-gameable |
| Passing requires improving the real property, where a check can observe it | +9 | Optimizer and intent aligned |
| Form checked by the gate, meaning left to a reviewer | +8 | Honest split — deterministic half gated, semantic half attested |
| Metric that rewards a tidy report over an honest one | -10 | Goodhart — optimizes the proxy, corrupts the loop |
| Gate advertised as catching what it cannot detect | -8 | False coverage — the defect passes with the gate vouching for it |
| Binary pass/fail with no convergence signal | -5 | Weak target; agent can't tell it's getting warmer |

**Filler is guidance, not a gate.** A form check — every required part present, non-empty, inside its length bound — is the most a deterministic gate can honestly claim about an annotation. Whether a present field *says* anything is a judgment, so it belongs to the annotation review (`CONTRIBUTING.md`, reviewer B), where a reviewer names it as a finding. Filler is still bad practice and a reviewer should send it back; it is not grounds for a meaning-judging gate, which is the inference this section forbids.

**The rendered map is itself an optimization target, kept honest at a human-authored ceiling by two checks.** A *charter* — a package/repo-scale annotation whose `Non-concern:` clauses are concrete enough to *reject* an ill-fitting feature by naming the sibling that owns it, not a strawman (a repo charter rejecting "add a program executor" because that is a runtime tool; a service charter rejecting "parse rules in the handler" because a named CLI owns that). And a *stress test* — replaying realistic change-requests to confirm each routes to exactly one unit from the map alone. A charter too vague to reject scope-creep is the map's failure mode, not the agent's.

**Applies when** agents are a primary caller (CLIs agents invoke, MCP servers, function-calling tools, batch/CI interfaces). Human-primary tools (interactive TUI, GUI) optimize for the human and treat agent-parseability as secondary. State which consumer is primary; don't split the difference.

---

**A charter is a routing instrument, not a whitelist.** A unit's charter states one job at its own altitude, so it answers *does this change belong in here* — never *should this have been built*. A change that lands where the owning charter's `Non-concern:` denies it is a misrouting: move the change, or move the charter. A `Concern:` that does not enumerate an addition is NOT a finding — a charter names what its files have in common, never their sum (src/annotation-guide.md). Whether a capability should exist at all is decided before a plan exists, and is not a reviewer's question.
